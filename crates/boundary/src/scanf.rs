//! Formatted input glue (spec §12.7; D-12.6, D-12.13): the scan engine (`liella::scanf`) over a
//! C string, a wide string or a stream, its pointer arguments read from the C `va_list`
//! (`liella_legacy::scanf_va`), the calling thread's locale and rounding mode.

use liella::error::{Errno, KResult};
use liella::scanf::{self as engine, Ctx, FmtUnit, Input, Outcome, Set, Sink};
use liella::text::Encoding;
use liella_kernel::machine::Machine;
use liella_legacy::scanf_va::Pointers;
use liella_legacy::stdio_compat as sc;

use crate::stdio::{self, Locking};
use crate::sys::CSys;
use crate::va_source::CVa;
use crate::{caller_alloc, conv, errno, locale};

/// The symbol set an export belongs to (0 plain, 1 `__isoc99_`, 2 `__isoc23_`).
pub fn flavour(n: i32) -> Set {
    match n {
        1 => Set::C99,
        2 => Set::C23,
        _ => Set::Gnu,
    }
}

/// The caller's objects, through the pointers of the `va_list`.
struct Args<'a>(Pointers<'a>);

fn word(v: usize) -> i64 {
    i64::from_ne_bytes(u64::try_from(v).unwrap_or(0).to_ne_bytes())
}

impl Sink for Args<'_> {
    fn pointer(&mut self, n: usize) -> usize {
        self.0.get(n).unwrap_or(0)
    }

    fn int(&mut self, addr: usize, size: usize, value: u64) {
        conv::store_int(addr, size, i64::from_ne_bytes(value.to_ne_bytes()));
    }

    fn bytes(&mut self, addr: usize, data: &[u8]) {
        for (d, s) in conv::out_bytes(addr, data.len()).iter_mut().zip(data) {
            *d = *s;
        }
    }

    fn wide(&mut self, addr: usize, data: &[u32]) {
        for (d, w) in conv::out_bytes(addr, data.len().saturating_mul(4)).chunks_exact_mut(4).zip(data) {
            d.copy_from_slice(&w.to_ne_bytes());
        }
    }

    fn alloc_bytes(&mut self, addr: usize, data: &[u8]) -> KResult<()> {
        let p = caller_alloc::dup_bytes(data)?;
        conv::store_int(addr, 8, word(p));
        Ok(())
    }

    fn alloc_wide(&mut self, addr: usize, data: &[u32]) -> KResult<()> {
        let mut bytes = liella::mem::TryVec::with_capacity(data.len().saturating_mul(4))?;
        for w in data {
            bytes.extend_from_slice(&w.to_ne_bytes())?;
        }
        self.alloc_bytes(addr, &bytes)
    }

    fn null(&mut self, addr: usize) {
        conv::store_int(addr, 8, 0);
    }
}

/// Scan `input` with `fmt` and the arguments of the `va_list` at `ap`: the assignments made,
/// `None` (`EOF`) for an input failure before the first conversion; `errno` is set for a read
/// or encoding error whatever the result.
fn run<F: FmtUnit>(fmt: &[F], set: Set, ap: usize, input: &mut dyn Input) -> KResult<Option<i32>> {
    let loc = locale::current();
    // ARCH(x86_64): rounding mode read from MXCSR; FPCR on aarch64 (A-9)
    let ctx = Ctx { loc, enc: Encoding::of(loc), rounding: liella_legacy::num_compat::rounding(CSys.mxcsr()), set };
    let mut va = CVa(ap);
    let mut args = Args(Pointers::new(&mut va));
    let r = engine::scan(fmt, input, &mut args, ctx)?;
    if let Some(e) = r.error {
        errno::set(e);
    }
    Ok(match r.outcome {
        Outcome::InputFailure => None,
        Outcome::Assigned(n) => Some(i32::try_from(n).unwrap_or(i32::MAX)),
    })
}

/// A C string (bytes, or `wchar_t` units).
struct Text<'a, T> {
    s: &'a [T],
    pos: usize,
}

impl<T: FmtUnit> Input for Text<'_, T> {
    fn next(&mut self) -> KResult<Option<u32>> {
        let c = self.s.get(self.pos).map(|u| u.code());
        if c.is_some() {
            self.pos = self.pos.saturating_add(1);
        }
        Ok(c)
    }

    fn back(&mut self, _: u32) {
        self.pos = self.pos.saturating_sub(1);
    }
}

/// A byte stream whose lock is held.
struct Bytes<'a> {
    file: &'static [core::sync::atomic::AtomicUsize],
    ext: &'a mut sc::Ext,
}

impl Input for Bytes<'_> {
    fn next(&mut self) -> KResult<Option<u32>> {
        sc::getc(&stdio::Glue, self.file, self.ext).map(|c| c.map(u32::from))
    }

    fn back(&mut self, unit: u32) {
        // The unit was just read, so the pushback room has it.
        let _ = sc::ungetc(&stdio::Glue, self.file, self.ext, u8::try_from(unit).unwrap_or(0));
    }
}

/// A wide-oriented stream whose lock is held, decoded by its codec.
struct Wide<'a> {
    file: &'static [core::sync::atomic::AtomicUsize],
    ext: &'a mut sc::Ext,
    enc: Encoding,
}

impl Input for Wide<'_> {
    fn next(&mut self) -> KResult<Option<u32>> {
        sc::getwc(&stdio::Glue, self.file, self.ext, self.enc)
    }

    fn back(&mut self, unit: u32) {
        // One character given back; the stream keeps up to four.
        let _ = sc::ungetwc(self.file, self.ext, unit, self.enc);
    }
}

/// `vfscanf(fp, fmt, ap)` (`fp` 0: `stdin`; `ap` the address of the C `va_list`), with the
/// stream locked for the whole call.
pub fn vfscanf(fp: usize, set: Set, fmt: KResult<&[u8]>, ap: usize) -> KResult<Option<i32>> {
    let fmt = fmt?;
    let fp = if fp == 0 { stdio::stdin() } else { fp };
    stdio::with(fp, Locking::Locked, |_, file, ext| run(fmt, set, ap, &mut Bytes { file, ext }))?
}

/// `vsscanf(s, fmt, ap)`.
pub fn vsscanf(s: KResult<&[u8]>, set: Set, fmt: KResult<&[u8]>, ap: usize) -> KResult<Option<i32>> {
    let (s, fmt) = (s?, fmt?);
    run(fmt, set, ap, &mut Text { s, pos: 0 })
}

/// `vfwscanf(fp, fmt, ap)` (`fp` 0: `stdin`): the stream becomes wide-oriented; a
/// byte-oriented one is `EINVAL` (D-12.14).
pub fn vfwscanf(fp: usize, set: Set, fmt: KResult<&[i32]>, ap: usize) -> KResult<Option<i32>> {
    let fmt = fmt?;
    let fp = if fp == 0 { stdio::stdin() } else { fp };
    let enc = Encoding::of(locale::current());
    stdio::with(fp, Locking::Locked, |_, file, ext| {
        if !sc::wide_oriented(file, ext, enc) {
            return Err(Errno::EINVAL);
        }
        run(fmt, set, ap, &mut Wide { file, ext, enc })
    })?
}

/// `vswscanf(s, fmt, ap)`.
pub fn vswscanf(s: KResult<&[i32]>, set: Set, fmt: KResult<&[i32]>, ap: usize) -> KResult<Option<i32>> {
    let (s, fmt) = (s?, fmt?);
    run(fmt, set, ap, &mut Text { s, pos: 0 })
}
