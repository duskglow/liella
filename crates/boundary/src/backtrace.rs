//! `backtrace`, `backtrace_symbols` and `backtrace_symbols_fd` (spec §2.7; D-2.7, IP-27),
//! written from the spec and `spec/behaviour/backtrace.md`.
//!
//! A trace is unwound by `liella_rt::unwind` from the registers `getcontext`'s primitive
//! captures. The objects it can unwind through are found without the dynamic linker: the
//! executable (its program headers from `AT_PHDR`) and the image this library is linked into
//! (`__ehdr_start`), the same object in a static program. Their call-frame information is
//! `.eh_frame_hdr` (`PT_GNU_EH_FRAME`), or, for a static executable linked without one, the
//! `.eh_frame` between the start object's label and the terminator `adapter/runtime.c` places
//! after every input section. The stack is read through `process_vm_readv`, so a damaged stack
//! ends a trace instead of faulting. Nothing allocates and nothing takes a lock:
//! `backtrace` and `backtrace_symbols_fd` are async-signal-safe from the first call.
//!
//! Frame texts name the executable by the name it was run as (`program_invocation_name`), as
//! the reference names its main program; an address elsewhere has no object.

use core::ffi::c_int;
use core::sync::atomic::Ordering::Acquire;

use liella::diag::FrameText;
use liella_kernel::errno::{Errno, KResult};
use liella_kernel::fd::Fd;
use liella_kernel::mem::IoVec;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{AT_PHDR, AT_PHNUM};
use liella_rt::start::PHDR_WORDS;
use liella_rt::unwind::{self, Object, OwnMemory, Regs};

use crate::sys::CSys;
use crate::{caller_alloc, conv, ffi, process};

/// Words in a `ucontext_t` (968 bytes).
const UCONTEXT_WORDS: usize = 121;
/// Word indexes of the general registers in a `ucontext_t` (`uc_mcontext.gregs` at byte 40:
/// R12 4, R13 5, R14 6, R15 7, RBP 10, RBX 11, RSP 15, RIP 16; machine/context.S).
const GREGS: [(usize, usize); 8] = [
    (unwind::R12, 9),
    (unwind::R13, 10),
    (unwind::R14, 11),
    (unwind::R15, 12),
    (unwind::RBP, 15),
    (unwind::RBX, 16),
    (unwind::RSP, 20),
    (unwind::RA, 21),
];
/// Bytes per address.
const WORD: usize = 8;
/// Bytes of an ELF header.
const EHDR_LEN: usize = 64;

/// The executable's program headers (at `phdr`) and load bias.
#[derive(Clone, Copy)]
struct Executable {
    phdr: usize,
    words: &'static [u64],
    phnum: usize,
    bias: usize,
}

impl Executable {
    fn find() -> Executable {
        let get = |k: u64| process::auxval(k).and_then(|v| usize::try_from(v).ok()).unwrap_or(0);
        let (phdr, phnum) = (get(AT_PHDR), get(AT_PHNUM));
        let words = conv::program_headers(phdr, phnum.saturating_mul(PHDR_WORDS));
        Executable { phdr, words, phnum, bias: liella_rt::start::program(words, phnum, phdr).bias }
    }

    /// The executable as an object to unwind through.
    fn object(&self) -> Object<'static> {
        let mut o = unwind::object(self.words, self.phnum, self.bias, conv::loaded);
        let (start, end) = (ffi::liella_link_eh_frame_start(), ffi::liella_link_eh_frame_end());
        if !o.has_cfi() && start != 0 && start < end && o.contains(start) {
            o.set_frames(start, end);
        }
        o
    }

    /// `addr`'s offset from the load bias, if it lies in one of the executable's segments.
    fn offset(&self, addr: usize) -> Option<usize> {
        unwind::in_object(self.words, self.phnum, self.bias, addr).then(|| addr.wrapping_sub(self.bias))
    }
}

/// The image this library is linked into, when it is not the executable (the shared product).
fn library(exe: &Executable) -> Option<Object<'static>> {
    let ehdr = ffi::liella_link_ehdr();
    let (phoff, phnum) = unwind::program_headers(conv::loaded(ehdr, EHDR_LEN))?;
    let phdr = ehdr.checked_add(phoff)?;
    if exe.phdr == phdr {
        return None;
    }
    let words = conv::image_program_headers(phdr, phnum.saturating_mul(PHDR_WORDS));
    let bias = unwind::bias_of(words, phnum, ehdr)?;
    Some(unwind::object(words, phnum, bias, conv::loaded))
}

/// The registers `getcontext`'s primitive stored in `uc`.
fn registers(uc: &[u64; UCONTEXT_WORDS]) -> Regs {
    let mut regs = Regs::new();
    for (reg, word) in GREGS {
        regs.set(reg, uc.get(word).copied());
    }
    regs
}

/// The address of the exported entry point `f` (`backtrace` or `__backtrace`), which the trace
/// leaves out when it has a frame of its own.
#[allow(clippy::as_conversions)] // a function's address is its pointer value
pub fn entry(f: extern "C" fn(*mut *mut core::ffi::c_void, c_int) -> c_int) -> usize {
    f as usize
}

/// Bytes for `size` pointers; a negative size is 0.
pub fn bytes(size: c_int) -> usize {
    usize::try_from(size).unwrap_or(0).saturating_mul(WORD)
}

/// `backtrace` into `out` (room for `out.len() / 8` addresses): the return addresses of the
/// calling frames, innermost first, from the caller of `entry`. Never inlined: this function
/// captures the registers in its own frame, which the trace then leaves out.
#[inline(never)]
pub fn capture(out: &mut [u8], entry: usize) -> c_int {
    let max = out.len() / WORD;
    if max == 0 {
        return 0;
    }
    let mut uc = [0u64; UCONTEXT_WORDS];
    let _ = ffi::liella_machine_getcontext(&mut uc);
    let exe = Executable::find();
    let objects = [exe.object(), library(&exe).unwrap_or_default()];
    let mut slots = out.chunks_exact_mut(WORD);
    let n = unwind::trace(&objects, &OwnMemory::new(&CSys), registers(&uc), entry, max, |pc| {
        if let Some(s) = slots.next() {
            s.copy_from_slice(&pc.to_ne_bytes());
        }
    });
    c_int::try_from(n).unwrap_or(c_int::MAX)
}

/// The caller in an `mtrace` line, written into `out` (its length returned): the return
/// address into the first frame whose function is not in `inside` (the C family's entry points
/// and the debugging paths below them), as `<program>:[0x<offset from the load bias>]` when it
/// lies in the executable (so `addr2line` takes it as it is), `[0x<address>]` elsewhere, `??`
/// when the stack cannot be followed. Never inlined: the registers are captured in this frame.
#[inline(never)]
pub(crate) fn caller_text(inside: &[usize], out: &mut [u8]) -> usize {
    let mut uc = [0u64; UCONTEXT_WORDS];
    let _ = ffi::liella_machine_getcontext(&mut uc);
    let exe = Executable::find();
    let objects = [exe.object(), library(&exe).unwrap_or_default()];
    let pc = unwind::caller(&objects, &OwnMemory::new(&CSys), registers(&uc), inside, 32);
    let mut b = liella::num::format::Bounded::new(out);
    match pc {
        Some(pc) => {
            let (name, value): (&[u8], usize) = match exe.offset(pc) {
                Some(off) => (exe_name(), off),
                None => (b"", pc),
            };
            if !name.is_empty() {
                liella::num::format::Sink::put(&mut b, name);
                liella::num::format::Sink::put(&mut b, b":");
            }
            liella::num::format::Sink::put(&mut b, b"[0x");
            liella::num::format::format_u64(&mut b, u64::try_from(value).unwrap_or(0), 16, liella::num::format::Case::Lower);
            liella::num::format::Sink::put(&mut b, b"]");
        }
        None => liella::num::format::Sink::put(&mut b, b"??"),
    }
    b.stored()
}

/// The addresses in `array` (native words).
fn addresses(array: &[u8]) -> impl Iterator<Item = usize> + '_ {
    array.chunks_exact(WORD).map(|w| <[u8; WORD]>::try_from(w).map_or(0, usize::from_ne_bytes))
}

/// The text of the frame at `addr`.
fn text(exe: &Executable, name: &'static [u8], addr: usize, fd: bool) -> FrameText<'static> {
    FrameText::new(addr, exe.offset(addr).map(|off| (name, off)), fd)
}

/// The executable's name as it was run.
fn exe_name() -> &'static [u8] {
    conv::process_text(ffi::program_invocation_name.load(Acquire))
}

/// `backtrace_symbols(array, n)`: one caller-owned block holding the `n` pointers and, after
/// them, the `n` NUL-terminated texts they point to (§2.7 HIST: the caller frees only the
/// block). Its address, or `ENOMEM`.
pub fn symbols(array: &[u8]) -> KResult<usize> {
    let (exe, name) = (Executable::find(), exe_name());
    let table = array.len();
    let mut total = table;
    for addr in addresses(array) {
        total = total.checked_add(text(&exe, name, addr, false).len()).and_then(|t| t.checked_add(1)).ok_or(Errno::ENOMEM)?;
    }
    let size = total.max(1);
    let p = caller_alloc::calloc(size, 1);
    let block = conv::fresh::<u8>(p, size).ok_or(Errno::ENOMEM)?;
    let base = p.expose_provenance();
    let (pointers, texts) = block.split_at_mut_checked(table).ok_or(Errno::ENOMEM)?;
    let mut at = 0usize;
    for (slot, addr) in pointers.chunks_exact_mut(WORD).zip(addresses(array)) {
        slot.copy_from_slice(&base.wrapping_add(table).wrapping_add(at).to_ne_bytes());
        for part in text(&exe, name, addr, false).parts() {
            for &b in part {
                if let Some(d) = texts.get_mut(at) {
                    *d = b;
                }
                at = at.saturating_add(1);
            }
        }
        // The terminating NUL: the block is zeroed.
        at = at.saturating_add(1);
    }
    Ok(base)
}

/// `backtrace_symbols_fd(array, n, fd)`: each text and a newline written to `fd`, one `writev`
/// per frame, without allocating. Stops at the first failed write.
pub fn symbols_fd(array: &[u8], fd: Fd) -> KResult<()> {
    let (exe, name) = (Executable::find(), exe_name());
    for addr in addresses(array) {
        let t = text(&exe, name, addr, true);
        let [head, tail] = t.parts();
        CSys.writev(fd, &[IoVec::new(head), IoVec::new(tail)])?;
    }
    Ok(())
}
