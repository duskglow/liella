//! Formatted output glue (spec §12.6, §12.11; D-12.4, D-12.5, D-12.8, D-12.12): a format's
//! arguments read from the C `va_list`, the calling thread's locale and `errno`, the
//! destinations (a stream, a descriptor, a caller buffer, a fresh allocation, an obstack), and
//! the registered conversions: their argument functions, handlers and types are C functions,
//! called here.
//!
//! A handler writes to a `FILE *`: it is given a memory stream of the destination's orientation
//! (`open_memstream`, or `open_wmemstream` for wide output), and what it wrote there becomes the
//! conversion's output, counted as written. While anything is registered, output to a stream is
//! collected first and written at the end, so a handler never runs while the destination's
//! record is in use.

use core::sync::atomic::{AtomicI32, AtomicU64, AtomicUsize, Ordering::Relaxed};

use liella::error::{Errno, KResult};
use liella::locale::file::Cat;
use liella::mem::TryVec;
use liella::num::float::{Float, Format, Rounding};
use liella::printf::{self as pf, Arg, Class, Conv, Len, Out, Parsed, Registered, Unit};
use liella::text::{self, Decoded, Encoding, MbState};
use liella_abi::layout::struct_printf_info;
use liella_kernel::fd::Fd;
use liella_kernel::machine::Machine;
use liella_kernel::sys::Sys;
use liella_legacy::printf_registry::{self as reg, Info};
use liella_legacy::stdio_compat::PlanSlot;
use liella_legacy::stdio_compat::{self as sc, Env as _};

use crate::stdio::{self, Locking};
use crate::sys::CSys;
use crate::va_source::CVa;
use crate::{caller_alloc, conv, errno, ffi, locale, tb};

/// The library's side of rendering: argument memory, the locale, `errno` at entry.
struct Env {
    loc: &'static liella::locale::Locale,
    enc: Encoding,
    errno_at_entry: i32,
    rounding: Rounding,
}

impl Env {
    fn new() -> Env {
        let loc = locale::current();
        // ARCH(x86_64): rounding mode read from MXCSR; FPCR on aarch64 (A-9)
        Env { loc, enc: Encoding::of(loc), errno_at_entry: errno::get(), rounding: liella_legacy::num_compat::rounding(CSys.mxcsr()) }
    }

    /// For `pf::render_simple`, which asks only `text`: the thread's locale, `errno` and the
    /// rounding mode are not read (about 7% of a short `snprintf`, measured).
    fn text_only() -> Env {
        static C: liella::locale::Locale = liella::locale::Locale::C;
        Env { loc: &C, enc: Encoding::of(&C), errno_at_entry: 0, rounding: Rounding::Nearest }
    }
}

/// The size in bytes of the object `%n` stores into.
fn count_size(len: Len) -> usize {
    match len {
        Len::Char => 1,
        Len::Short => 2,
        Len::None => 4,
        Len::Bits(n) | Len::Fast(n) => usize::from(n / 8).clamp(1, 8),
        _ => 8,
    }
}

impl pf::Env for Env {
    fn text(&mut self, addr: usize, max: Option<usize>) -> &'static [u8] {
        conv::printf_text(addr, max)
    }

    fn wide(&mut self, addr: usize, max: Option<usize>) -> &'static [i32] {
        conv::printf_wide(addr, max)
    }

    fn store_count(&mut self, addr: usize, len: Len, count: usize) {
        conv::store_int(addr, count_size(len), i64::try_from(count).unwrap_or(i64::MAX));
    }

    fn error_text(&mut self, out: &mut TryVec<u8>) -> KResult<()> {
        match liella::diag::description(self.errno_at_entry) {
            Some(d) => out.extend_from_slice(d.as_bytes()),
            None => {
                let mut b = [0u8; 32];
                let n = liella::diag::unknown(self.errno_at_entry, &mut b);
                out.extend_from_slice(b.get(..n).unwrap_or(&[]))
            }
        }
    }

    fn radix(&self) -> &[u8] {
        self.loc.decimal_point()
    }

    fn thousands(&self) -> &[u8] {
        self.loc.thousands_sep()
    }

    fn grouping(&self) -> &[u8] {
        self.loc.grouping()
    }

    fn digit(&self, d: u8) -> &[u8] {
        let s = self.loc.string(Cat::Ctype, 41 + usize::from(d.min(9)));
        const ASCII: &[u8; 10] = b"0123456789";
        if s.is_empty() { ASCII.get(usize::from(d)..usize::from(d).saturating_add(1)).unwrap_or(b"0") } else { s }
    }

    fn encode(&self, wc: u32, out: &mut [u8; 16]) -> Option<usize> {
        text::encode(wc, self.enc, out)
    }

    fn decode(&self, input: &[u8]) -> Option<(u32, usize)> {
        match text::decode(&mut { MbState::INITIAL }, input, self.enc) {
            Decoded::Char(c, n) => Some((c, n)),
            Decoded::Null => Some((0, 1)),
            _ => None,
        }
    }

    fn rounding(&self) -> Rounding {
        self.rounding
    }

    fn custom(&mut self, call: &pf::Call<'_>) -> KResult<pf::Custom> {
        let Some((handler, _)) = reg::handler(call.conv.letter) else { return Ok(pf::Custom::Declined(TryVec::new())) };
        let info = Info::call(call).to_c();
        // Each argument in a slot of its own, and the array of their addresses.
        let mut slots: TryVec<Slot> = TryVec::with_capacity(call.args.len().max(1))?;
        for a in call.args {
            slots.push(Slot(match *a {
                Arg::Int(v) | Arg::Double(v) => [v, 0],
                Arg::LongDouble(b) => [u64::try_from(b & u128::from(u64::MAX)).unwrap_or(0), u64::try_from(b >> 64).unwrap_or(0)],
                Arg::User(addr) => [u64::try_from(addr).unwrap_or(0), 0],
            }))?;
        }
        let mut ptrs: TryVec<usize> = TryVec::with_capacity(slots.len().max(1))?;
        for s in slots.iter() {
            ptrs.push(addr_of(s))?;
        }
        // The handler writes to a memory stream of the destination's orientation.
        let vars = [AtomicUsize::new(0), AtomicUsize::new(0)];
        let fp = stdio::open_memstream(Ok(addr_of(&vars[0])), Ok(addr_of(&vars[1])), call.wide)?;
        if !call.wide {
            // Byte-oriented from the start, as the destination is: wide output to it fails.
            let _ = stdio::fwide(fp, -1);
        }
        let r = c_int(ffi::liella_machine_call3(handler, word(fp), word(addr_of(&info)), word(ptrs.as_slice().as_ptr().expose_provenance())));
        let failure = Errno::new(errno::get()).unwrap_or(Errno::EINVAL);
        let closed = stdio::fclose(fp);
        let (buf, len) = (vars[0].load(Relaxed), vars[1].load(Relaxed));
        let written = memstream_units(buf, len, call.wide);
        stdio::Glue.free(buf);
        closed?;
        let written = written?;
        Ok(match r {
            -2 => pf::Custom::Declined(written),
            r if r < 0 => pf::Custom::Failed(written, failure),
            _ => pf::Custom::Done(written),
        })
    }
}

/// A handler's argument: the value's bytes, 16-aligned for a `long double`.
#[repr(C, align(16))]
#[derive(Clone, Copy)]
struct Slot([u64; 2]);

/// A registered type's object, which the type's function writes (16-aligned).
#[repr(C, align(16))]
#[derive(Default)]
struct Object([AtomicU64; 2]);

fn addr_of<T>(r: &T) -> usize {
    core::ptr::from_ref(r).expose_provenance()
}

/// An address or a count as a C call's integer argument.
fn word(v: usize) -> i64 {
    i64::from_ne_bytes(v.to_ne_bytes())
}

/// The `int` a C function returned (the low half of the register).
fn c_int(r: i64) -> i32 {
    i32::from_ne_bytes(u32::try_from(r & 0xFFFF_FFFF).unwrap_or(0).to_ne_bytes())
}

/// The `len` units of a closed memory stream's buffer at `buf` (bytes, or `wchar_t`s).
fn memstream_units(buf: usize, len: usize, wide: bool) -> KResult<TryVec<u32>> {
    let mut units: TryVec<u32> = TryVec::with_capacity(len)?;
    if buf == 0 {
        return Ok(units);
    }
    let bytes = stdio::Glue.bytes(buf, len.saturating_mul(if wide { 4 } else { 1 }));
    if wide {
        for c in bytes.chunks_exact(4) {
            units.push(u32::from_ne_bytes([c.first().copied().unwrap_or(0), c.get(1).copied().unwrap_or(0), c.get(2).copied().unwrap_or(0), c.get(3).copied().unwrap_or(0)]))?;
        }
    } else {
        for &b in bytes.iter() {
            units.push(u32::from(b))?;
        }
    }
    Ok(units)
}

/// More arguments than this from one argument function is an error (`NL_ARGMAX`).
const ARGS_MAX: usize = 4096;

/// Call the argument function `f` with room for `n` types: its result, the types and the
/// object sizes it stored.
fn arginfo(f: usize, info: &Info, n: usize) -> KResult<(i32, TryVec<i32>, TryVec<i32>)> {
    let mut types: TryVec<AtomicI32> = TryVec::with_capacity(n.max(1))?;
    let mut sizes: TryVec<AtomicI32> = TryVec::with_capacity(n.max(1))?;
    for _ in 0..n.max(1) {
        types.push(AtomicI32::new(0))?;
        sizes.push(AtomicI32::new(0))?;
    }
    let c = info.to_c();
    let r = c_int(ffi::liella_machine_call4(f, word(addr_of(&c)), word(n), word(types.as_slice().as_ptr().expose_provenance()), word(sizes.as_slice().as_ptr().expose_provenance())));
    let mut t: TryVec<i32> = TryVec::with_capacity(n)?;
    let mut s: TryVec<i32> = TryVec::with_capacity(n)?;
    for (a, b) in types.iter().zip(sizes.iter()).take(n) {
        t.push(a.load(Relaxed))?;
        s.push(b.load(Relaxed))?;
    }
    Ok((r, t, s))
}

/// The registered conversions while a format for output is parsed: an argument function is
/// called with room for one type, then, if it wants more, with room for all of them.
struct Hooks {
    wide: bool,
}

impl pf::Registry for Hooks {
    fn modifier(&self, at: &dyn Fn(usize) -> u32) -> Option<(usize, u16)> {
        reg::modifier(at)
    }

    fn conversion(&mut self, conv: &Conv, _first: usize) -> KResult<Registered> {
        let Some((_, f)) = reg::handler(conv.letter) else { return Ok(Registered::No) };
        if f == 0 {
            return Ok(Registered::Builtin);
        }
        let info = Info::parsed(conv, self.wide);
        let (r, mut types, mut sizes) = arginfo(f, &info, 1)?;
        let Ok(n) = usize::try_from(r) else { return Ok(Registered::Builtin) };
        if n > ARGS_MAX {
            return Err(Errno::EINVAL);
        }
        if n > 1 {
            (_, types, sizes) = arginfo(f, &info, n)?;
        }
        let mut classes: TryVec<Class> = TryVec::with_capacity(n)?;
        for k in 0..n {
            classes.push(reg::class_of(types.get(k).copied().unwrap_or(0), sizes.get(k).copied().unwrap_or(0)))?;
        }
        Ok(Registered::Args(classes))
    }
}

/// Parse `fmt`, read its arguments from `ap`, check the fortified rules, render into `out`.
/// A simple format (`pf::simple`: integers, strings and characters with at most a width)
/// without registered conversions is rendered directly, its arguments read as it goes; the
/// output is that of the general path.
fn format<O: Out<u8>>(fmt: &[u8], flag: i32, ap: usize, out: &mut O) -> KResult<usize> {
    format_routed(fmt, flag, ap, pf::Planned::Unknown, None, out)
}

/// [`format`] when a stream's plan cache (D-12.21) may already know `fmt`: `kept` is its plan
/// (lent by the cache, not copied; TAR-TV) when `route` is [`pf::Planned::Simple`];
/// [`pf::Planned::General`] is a format known not to be simple; otherwise the format is planned here.
#[inline]
fn format_routed<O: Out<u8>>(fmt: &[u8], flag: i32, ap: usize, route: pf::Planned, kept: Option<&pf::SimpleFormat>, out: &mut O) -> KResult<usize> {
    if let Some(plan) = kept {
        if plan.is_lone() {
            if let Some(r) = render_lone(fmt, plan, ap, out) {
                return r;
            }
        }
        return render_plan(fmt, plan, ap, out);
    }
    match route {
        pf::Planned::Simple => {}
        pf::Planned::General => return format_at(fmt, fmt.as_ptr().expose_provenance(), flag, ap, out),
        pf::Planned::Unknown => {
            // A format that is exactly `%s` (the commonest one for a string copy) has nothing to parse or
            // pad: one argument, `(null)` for a null pointer, written as is. Same bytes as the general path.
            if fmt == b"%s" && !reg::active() {
                let addr = usize::try_from(liella_abi::va::VaSource::int(&mut CVa(ap))).unwrap_or(0);
                let body: &[u8] = if addr == 0 { b"(null)" } else { conv::printf_text(addr, None) };
                out.put(body);
                if body.len() > usize::try_from(i32::MAX).unwrap_or(usize::MAX) {
                    return Err(Errno::EOVERFLOW);
                }
                return Ok(body.len());
            }
        }
    }
    // The plan is filled in place: returned by value it would be copied.
    let mut plan = pf::SimpleFormat::new();
    if reg::active() || !pf::simple_into(fmt, &mut plan) {
        return format_at(fmt, fmt.as_ptr().expose_provenance(), flag, ap, out);
    }
    // One conversion and its text (gawk's `"*.*%c"`, `"%lu"`): the pieces written directly, as
    // for a kept plan, without render_simple's loop (SPRINTF-RENDER).
    if plan.is_lone() {
        if let Some(r) = render_lone(fmt, &plan, ap, out) {
            return r;
        }
    }
    render_plan(fmt, &plan, ap, out)
}

/// A [`pf::SimpleFormat::is_lone`] plan of text around one plain `%s`, one right-justified
/// integer or one plain `%c`: the text before, the converted argument and the text after,
/// written as render_simple would write them; `None` for any other plan.
#[inline(always)]
fn render_lone<O: Out<u8>>(fmt: &[u8], plan: &pf::SimpleFormat, ap: usize, out: &mut O) -> Option<KResult<usize>> {
    // Text around one plain `%s` (tar -tv's `" %s"`): the three pieces render_simple would
    // write, without its conversion loop.
    if let Some((at, after)) = plan.lone_s() {
        let addr = usize::try_from(liella_abi::va::VaSource::int(&mut CVa(ap))).unwrap_or(0);
        let body: &[u8] = if addr == 0 { b"(null)" } else { conv::printf_text(addr, None) };
        let (head, tail) = (fmt.get(..at).unwrap_or(&[]), fmt.get(after..).unwrap_or(&[]));
        for piece in [head, body, tail] {
            if !piece.is_empty() {
                out.put(piece);
            }
        }
        let n = head.len().saturating_add(body.len()).saturating_add(tail.len());
        // The result is an `int` (D-12.5).
        if n > usize::try_from(i32::MAX).unwrap_or(usize::MAX) {
            return Some(Err(Errno::EOVERFLOW));
        }
        return Some(Ok(n));
    }
    // Text around one right-justified integer (`%02x`, coreutils printf's `%jd`): its field
    // built whole, the three pieces written, without render_simple's loop (PRINTF-LAYERS).
    if let Some(li) = plan.lone_int() {
        let mut buf = [0u8; pf::INT_FIELD];
        let field = li.field(liella_abi::va::VaSource::int(&mut CVa(ap)), &mut buf);
        let (head, tail) = (fmt.get(..li.at).unwrap_or(&[]), fmt.get(li.after..).unwrap_or(&[]));
        for piece in [head, field, tail] {
            if !piece.is_empty() {
                out.put(piece);
            }
        }
        let n = head.len().saturating_add(field.len()).saturating_add(tail.len());
        // The result is an `int` (D-12.5).
        if n > usize::try_from(i32::MAX).unwrap_or(usize::MAX) {
            return Some(Err(Errno::EOVERFLOW));
        }
        return Some(Ok(n));
    }
    // Text around one plain `%c`: its argument's low byte (gawk builds each float format with
    // `"*.*%c"`).
    if let Some((at, after)) = plan.lone_c() {
        let one = [u8::try_from(liella_abi::va::VaSource::int(&mut CVa(ap)) & 0xFF).unwrap_or(0)];
        let (head, tail) = (fmt.get(..at).unwrap_or(&[]), fmt.get(after..).unwrap_or(&[]));
        for piece in [head, &one[..], tail] {
            if !piece.is_empty() {
                out.put(piece);
            }
        }
        let n = head.len().saturating_add(1).saturating_add(tail.len());
        // The result is an `int` (D-12.5).
        if n > usize::try_from(i32::MAX).unwrap_or(usize::MAX) {
            return Some(Err(Errno::EOVERFLOW));
        }
        return Some(Ok(n));
    }
    None
}

/// Render the simple format `fmt`, `plan` being what `pf::simple_into` made of it, its
/// arguments read from `ap` as it goes.
#[inline(always)]
fn render_plan<O: Out<u8>>(fmt: &[u8], plan: &pf::SimpleFormat, ap: usize, out: &mut O) -> KResult<usize> {
    // `CVa` is only the address of the caller's `va_list`, which holds the position: one
    // handle per kind of argument.
    let (mut ints, mut doubles) = (CVa(ap), CVa(ap));
    let mut int_arg = || liella_abi::va::VaSource::int(&mut ints);
    let mut double_arg = || liella_abi::va::VaSource::double(&mut doubles);
    // Floating conversions need the locale's radix and the rounding mode; a format without
    // one is rendered by the instance with no floating code.
    // A format without a `*` width runs the instance with no width-argument code.
    let n = match (plan.has_float(), plan.has_star()) {
        (true, true) => pf::render_simple::<true, true, _>(fmt, plan, &mut int_arg, &mut double_arg, &mut Env::new(), out)?,
        (true, false) => pf::render_simple::<true, false, _>(fmt, plan, &mut int_arg, &mut double_arg, &mut Env::new(), out)?,
        (false, true) => pf::render_simple::<false, true, _>(fmt, plan, &mut int_arg, &mut double_arg, &mut Env::text_only(), out)?,
        (false, false) => pf::render_simple::<false, false, _>(fmt, plan, &mut int_arg, &mut double_arg, &mut Env::text_only(), out)?,
    };
    // The result is an `int` (D-12.5).
    if n > usize::try_from(i32::MAX).unwrap_or(usize::MAX) {
        return Err(Errno::EOVERFLOW);
    }
    Ok(n)
}

/// [`format`] of a copy of the caller's format, which lies at `origin` (the fortified `%n` rule
/// asks where the caller's format is, not the copy).
fn format_at<U: Unit, O: Out<U>>(fmt: &[U], origin: usize, flag: i32, ap: usize, out: &mut O) -> KResult<usize> {
    let mut env = Env::new();
    let mut parsed = Parsed::new();
    if reg::active() {
        pf::parse_into(fmt, &mut Hooks { wide: U::WIDE }, &mut parsed)?;
    } else {
        pf::parse_into(fmt, &mut pf::Plain, &mut parsed)?;
    }
    if flag > 0 && parsed.pieces.iter().any(|p| matches!(p, pf::Piece::Conv(c) if c.letter == u32::from(b'n'))) && !crate::process::is_read_only(origin) {
        crate::process::fortify_fail(b"%n in writable segment detected");
    }
    // Registered types are read by their own functions into objects that live until the
    // handlers ran.
    let mut objects: TryVec<TryVec<Object>> = TryVec::new();
    let mut user = |ty: u32, size: usize| -> KResult<Arg> {
        let f = reg::type_function(ty);
        if f == 0 {
            return Ok(Arg::Int(liella_abi::va::VaSource::int(&mut CVa(ap))));
        }
        let n = size.div_ceil(16).max(1);
        let mut mem: TryVec<Object> = TryVec::with_capacity(n)?;
        for _ in 0..n {
            mem.push(Object::default())?;
        }
        let addr = mem.as_slice().as_ptr().expose_provenance();
        let _ = ffi::liella_machine_call3(f, word(addr), word(ap), 0);
        objects.push(mem)?;
        Ok(Arg::User(addr))
    };
    let mut args = liella::mem::InlineVec::new();
    liella_legacy::printf_va::read(&parsed.classes, &mut CVa(ap), &mut user, &mut args)?;
    let n = pf::render(fmt, &parsed, &args, &mut env, out)?;
    // The result is an `int` (D-12.5).
    if n > usize::try_from(i32::MAX).unwrap_or(usize::MAX) {
        return Err(Errno::EOVERFLOW);
    }
    Ok(n)
}

// ------------------------------------------------------------------------------ destinations

/// Bytes collected in memory.
struct Collect(TryVec<u8>, Option<Errno>);

impl Out<u8> for Collect {
    fn put(&mut self, s: &[u8]) {
        if self.1.is_none() && self.0.extend_from_slice(s).is_err() {
            self.1 = Some(Errno::ENOMEM);
        }
    }
}

/// Wide characters collected in memory.
struct CollectWide(TryVec<u32>, Option<Errno>);

impl Out<u32> for CollectWide {
    fn put(&mut self, s: &[u32]) {
        if self.1.is_none() && self.0.extend_from_slice(s).is_err() {
            self.1 = Some(Errno::ENOMEM);
        }
    }
}

/// Bytes staged per call for an unbuffered stream (AUD-M2, D-12.20): the output goes to the
/// stream in pieces of this size, the last one at the end of the call, so a short call is one
/// `write(2)` instead of one per conversion. The reference stages 128 bytes the same way (strace:
/// `fprintf(stderr, ...)` writes 128-byte pieces, then the rest).
const STAGE: usize = 128;

/// An unbuffered stream's staging buffer and the bytes in it.
type Stage = ([u8; STAGE], usize);

/// Bytes written to a stream whose lock is held. An unbuffered stream's bytes are staged in
/// `stage` and reach it in [`STAGE`]-byte pieces; other streams buffer by themselves.
struct ToStream<'a> {
    file: &'static [core::sync::atomic::AtomicUsize],
    ext: &'a mut sc::Ext,
    error: Option<Errno>,
    stage: Option<&'a mut Stage>,
    /// The stream's write window, held for the call (STDIO-HOT, D-12.19): a piece that fits is
    /// one bound check and a copy; `_IO_write_ptr` is published before any other write.
    window: sc::Window,
}

impl<'a> ToStream<'a> {
    /// `stage` is given for an unbuffered stream only (no 128-byte fill per call otherwise),
    /// by reference: the struct stays small, so building it moves no buffer (STDIO-HOT).
    fn new(file: &'static [core::sync::atomic::AtomicUsize], ext: &'a mut sc::Ext, stage: Option<&'a mut Stage>) -> ToStream<'a> {
        let window = sc::Window::open(file, ext);
        ToStream { file, ext, error: None, stage, window }
    }

    /// Write what is staged (the end of the call, on an error too: the bytes produced so far
    /// reach the stream as they did unstaged).
    fn finish(&mut self) {
        self.window.publish(self.file);
        if let Some((buf, len)) = self.stage.as_deref() {
            send(self.file, self.ext, &mut self.error, &mut self.window, buf.get(..*len).unwrap_or(&[]));
        }
        self.stage = None;
    }

    /// [`Out::put`] past the held window: the general write (or the unbuffered staging).
    #[inline(never)]
    fn put_general(&mut self, mut s: &[u8]) {
        let ToStream { file, ext, error, stage, window } = self;
        let Some((buf, len)) = stage.as_deref_mut() else {
            send(file, ext, error, window, s);
            return;
        };
        while !s.is_empty() && error.is_none() {
            let (now, rest) = s.split_at(STAGE.saturating_sub(*len).min(s.len()));
            if let Some(d) = buf.get_mut(*len..len.saturating_add(now.len())) {
                d.copy_from_slice(now);
            }
            *len = len.saturating_add(now.len());
            s = rest;
            if *len == STAGE {
                send(file, ext, error, window, buf);
                *len = 0;
            }
        }
    }
}

/// `s` to the stream, unless an earlier piece failed. The held window is published first and
/// opened again after (closed once a write failed: later pieces are not written).
fn send(file: &'static [core::sync::atomic::AtomicUsize], ext: &mut sc::Ext, error: &mut Option<Errno>, window: &mut sc::Window, s: &[u8]) {
    if error.is_none() && !s.is_empty() {
        window.publish(file);
        *error = sc::write(&stdio::Glue, file, ext, s).error;
        *window = if error.is_none() { sc::Window::open(file, ext) } else { sc::Window::closed() };
    }
}

impl Out<u8> for ToStream<'_> {
    #[inline]
    fn put(&mut self, s: &[u8]) {
        if !self.window.put(&stdio::Glue, s) {
            self.put_general(s);
        }
    }
}

/// Wide characters written to a wide-oriented stream whose lock is held, through its codec.
struct WideToStream<'a> {
    file: &'static [core::sync::atomic::AtomicUsize],
    ext: &'a mut sc::Ext,
    enc: Encoding,
    error: Option<Errno>,
}

impl Out<u32> for WideToStream<'_> {
    fn put(&mut self, s: &[u32]) {
        for &c in s {
            if self.error.is_none() {
                self.error = sc::putwc(&stdio::Glue, self.file, self.ext, c, self.enc).err();
            }
        }
    }
}

/// `vfprintf(fp, fmt, ap)` (`fp` 0: `stdout`), with the stream locked for the whole call.
pub fn vfprintf(fp: usize, flag: i32, fmt: KResult<&[u8]>, ap: usize) -> KResult<usize> {
    let fmt = fmt?;
    let fp = if fp == 0 { stdio::stdout() } else { fp };
    if reg::active() {
        // Handlers may run: collect, then write what was produced (on an error too, as a
        // direct write would have).
        let mut out = Collect(TryVec::new(), None);
        let r = format(fmt, flag, ap, &mut out);
        let written = stdio::fwrite(Ok(&out.0), fp, Locking::Locked).error;
        let n = r?;
        return match out.1.or(written) {
            Some(e) => Err(e),
            None => Ok(n),
        };
    }
    stdio::with(fp, Locking::Locked, |_, file, ext| {
        // The stream's plan cache (D-12.21), under its lock; `%s` alone has its own path.
        // A hit lends the kept plan (no copy; TAR-TV).
        let (route, kept) = if fmt == b"%s" { (pf::Planned::Unknown, None) } else { plan_cache(&mut ext.printf_plans).map_or((pf::Planned::Unknown, None), |c| c.route(fmt)) };
        if ext.state.policy == liella::stream::Policy::Unbuffered {
            let mut stage = ([0u8; STAGE], 0);
            to_stream(fmt, flag, ap, route, kept, ToStream::new(file, ext, Some(&mut stage)))
        } else {
            to_stream(fmt, flag, ap, route, kept, ToStream::new(file, ext, None))
        }
    })?
}

/// The stream's plan cache (D-12.21), made on its [`PlanSlot::AFTER`]th formatted call (none
/// before, or when it cannot be allocated: the caller parses as usual). Under the stream's lock.
#[inline]
fn plan_cache(slot: &mut PlanSlot) -> Option<&'static mut pf::PlanCache> {
    if slot.addr == 0 {
        if slot.calls < PlanSlot::AFTER - 1 {
            slot.calls = slot.calls.saturating_add(1);
            return None;
        }
        // Never freed: the cache stays with the stream's slot (tb::leaked_at).
        slot.addr = liella::mem::TryBox::new(pf::PlanCache::new()).ok().map_or(0, |b| core::ptr::from_mut(b.leak()).expose_provenance());
        if slot.addr == 0 {
            slot.calls = 0;
            return None;
        }
    }
    Some(tb::leaked_at::<pf::PlanCache>(slot.addr))
}

/// [`vfprintf`] into `out`, under the stream's lock (`route` and `kept`: [`format_routed`]).
#[inline]
fn to_stream(fmt: &[u8], flag: i32, ap: usize, route: pf::Planned, kept: Option<&pf::SimpleFormat>, mut out: ToStream<'_>) -> KResult<usize> {
    let r = format_routed(fmt, flag, ap, route, kept, &mut out);
    out.finish();
    let n = r?;
    out.error.map_or(Ok(n), Err)
}

/// `vfwprintf(fp, fmt, ap)` (`fp` 0: `stdout`): the stream becomes wide-oriented.
pub fn vfwprintf(fp: usize, flag: i32, fmt: KResult<&[i32]>, ap: usize) -> KResult<usize> {
    let fmt = fmt?;
    let units: TryVec<u32> = fmt.iter().map(|&w| u32::from_ne_bytes(w.to_ne_bytes())).try_fold(TryVec::new(), |mut v, u| v.push(u).map(|()| v))?;
    let fp = if fp == 0 { stdio::stdout() } else { fp };
    let enc = Encoding::of(locale::current());
    if reg::active() {
        // Handlers may run: collect, then write (as `vfprintf`).
        stdio::with(fp, Locking::Locked, |_, file, ext| if sc::wide_oriented(file, ext, enc) { Ok(()) } else { Err(Errno::EINVAL) })??;
        let mut out = CollectWide(TryVec::new(), None);
        let r = format_at(&units, fmt.as_ptr().expose_provenance(), flag, ap, &mut out);
        let written = stdio::with(fp, Locking::Locked, |_, file, ext| {
            let mut w = WideToStream { file, ext, enc, error: None };
            w.put(&out.0);
            w.error
        })?;
        let n = r?;
        return match out.1.or(written) {
            Some(e) => Err(e),
            None => Ok(n),
        };
    }
    stdio::with(fp, Locking::Locked, |_, file, ext| {
        // A byte-oriented stream takes no wide output (D-12.14).
        if !sc::wide_oriented(file, ext, enc) {
            return Err(Errno::EINVAL);
        }
        let mut out = WideToStream { file, ext, enc, error: None };
        let n = format_at(&units, fmt.as_ptr().expose_provenance(), flag, ap, &mut out)?;
        out.error.map_or(Ok(n), Err)
    })?
}

/// `fmt` with the arguments of the `va_list` at `ap`, into memory (the diagnostic families).
pub fn collect(fmt: &[u8], ap: usize) -> KResult<TryVec<u8>> {
    collect_chk(fmt, 0, ap)
}

/// [`collect`] with the fortified forms' check level `flag` (`syslog`'s checking forms).
pub fn collect_chk(fmt: &[u8], flag: i32, ap: usize) -> KResult<TryVec<u8>> {
    let mut out = Collect(TryVec::new(), None);
    format(fmt, flag, ap, &mut out)?;
    out.1.map_or(Ok(out.0), Err)
}

/// `vdprintf(fd, fmt, ap)`.
pub fn vdprintf(fd: i32, flag: i32, fmt: KResult<&[u8]>, ap: usize) -> KResult<usize> {
    let fmt = fmt?;
    let mut out = ToFd { fd: Fd::new(fd), buf: [0; TO_FD], len: 0, error: None };
    let n = format(fmt, flag, ap, &mut out)?;
    out.flush();
    match out.error {
        Some(e) => Err(e),
        None => Ok(n),
    }
}

/// `dprintf`'s buffer: output up to this size reaches the descriptor in one `write`.
const TO_FD: usize = 4096;

/// Bytes written to a descriptor through a buffer on the stack (T22): no heap, so `dprintf`
/// still reports when the heap is exhausted. A full buffer is written out and refilled.
struct ToFd {
    fd: Fd,
    buf: [u8; TO_FD],
    len: usize,
    error: Option<Errno>,
}

impl ToFd {
    fn flush(&mut self) {
        let mut done = 0usize;
        while self.error.is_none() && done < self.len {
            match CSys.write(self.fd, self.buf.get(done..self.len).unwrap_or(&[])) {
                Ok(0) => self.error = Some(Errno::EIO),
                Ok(k) => done = done.saturating_add(k),
                Err(e) => self.error = Some(e),
            }
        }
        self.len = 0;
    }
}

impl Out<u8> for ToFd {
    fn put(&mut self, mut s: &[u8]) {
        while self.error.is_none() && !s.is_empty() {
            if self.len == TO_FD {
                self.flush();
                continue;
            }
            let n = s.len().min(TO_FD.saturating_sub(self.len));
            let end = self.len.saturating_add(n);
            if let (Some(d), Some(src)) = (self.buf.get_mut(self.len..end), s.get(..n)) {
                liella::mem::copy(d, src);
            }
            self.len = end;
            s = s.get(n..).unwrap_or(&[]);
        }
    }
}

/// Bytes into a caller's buffer of `max` bytes (`usize::MAX`: unbounded), keeping room for the
/// terminator, aborting when a byte would be written beyond the fortified size `slen` (C0.5: a
/// bound larger than the object alone does not abort, D-C0.5).
struct ToBuffer {
    addr: usize,
    max: usize,
    slen: usize,
    pos: usize,
}

impl Out<u8> for ToBuffer {
    #[inline]
    fn put(&mut self, s: &[u8]) {
        // The common case (SPRINTF-CHK-LS): the piece fits before the terminator's byte and
        // within the fortified size - one compare pair and the copy, inlined into the renderer
        // (the general code below cost about 33 instructions per piece of ls's `"%s %*s "`).
        let end = self.pos.saturating_add(s.len());
        if end < self.max && end <= self.slen {
            liella::mem::copy(conv::out_bytes(self.addr.wrapping_add(self.pos), s.len()), s);
            self.pos = end;
            return;
        }
        self.put_rest(s);
    }
}

impl ToBuffer {
    /// [`Out::put`] when the piece may not fit: what fits before the terminator, and the
    /// fortified check on it.
    #[inline(never)]
    #[cold]
    fn put_rest(&mut self, s: &[u8]) {
        let room = self.max.saturating_sub(1).saturating_sub(self.pos);
        let n = room.min(s.len());
        if n > 0 && self.pos.saturating_add(n) > self.slen {
            crate::ret::chk_fail();
        }
        if n > 0 {
            liella::mem::copy(conv::out_bytes(self.addr.wrapping_add(self.pos), n), s.get(..n).unwrap_or(&[]));
            self.pos = self.pos.saturating_add(n);
        }
    }
}

/// `vsnprintf(s, maxlen, fmt, ap)` and `vsprintf` (`maxlen` `usize::MAX`); `slen` is the
/// fortified destination size (`usize::MAX`: none). The full length is returned.
pub fn vsnprintf(s: usize, maxlen: usize, flag: i32, slen: usize, fmt: KResult<&[u8]>, ap: usize) -> KResult<usize> {
    let fmt = fmt?;
    let mut out = ToBuffer { addr: s, max: maxlen, slen, pos: 0 };
    let r = format(fmt, flag, ap, &mut out);
    // The output so far is terminated, on an error too.
    if maxlen > 0 {
        if out.pos >= slen {
            crate::ret::chk_fail();
        }
        // One indexed store, not a fill of a length the compiler cannot see (PJ-20).
        if let Some(nul) = conv::out_bytes(s.wrapping_add(out.pos), 1).first_mut() {
            *nul = 0;
        }
    }
    r
}

/// Wide characters into a caller's buffer of `max` units (`vswprintf`): `-1` (`EOVERFLOW`)
/// when the result does not fit, as the contract requires.
struct ToWideBuffer {
    addr: usize,
    max: usize,
    slen: usize,
    pos: usize,
    truncated: bool,
}

impl Out<u32> for ToWideBuffer {
    fn put(&mut self, s: &[u32]) {
        for &c in s {
            if self.pos.saturating_add(1) >= self.max {
                self.truncated = true;
                return;
            }
            if self.pos >= self.slen {
                crate::ret::chk_fail();
            }
            conv::out_bytes(self.addr.wrapping_add(self.pos.saturating_mul(4)), 4).copy_from_slice(&c.to_ne_bytes());
            self.pos = self.pos.saturating_add(1);
        }
    }
}

/// `vswprintf(s, maxlen, fmt, ap)`.
pub fn vswprintf(s: usize, maxlen: usize, flag: i32, slen: usize, fmt: KResult<&[i32]>, ap: usize) -> KResult<usize> {
    let fmt = fmt?;
    let units: TryVec<u32> = fmt.iter().map(|&w| u32::from_ne_bytes(w.to_ne_bytes())).try_fold(TryVec::new(), |mut v, u| v.push(u).map(|()| v))?;
    let mut out = ToWideBuffer { addr: s, max: maxlen, slen, pos: 0, truncated: false };
    let r = format_at(&units, fmt.as_ptr().expose_provenance(), flag, ap, &mut out);
    if maxlen > 0 {
        if out.pos >= slen {
            crate::ret::chk_fail();
        }
        if let Ok(nul) = <&mut [u8; 4]>::try_from(conv::out_bytes(s.wrapping_add(out.pos.saturating_mul(4)), 4)) {
            *nul = [0; 4];
        }
    }
    let n = r?;
    if out.truncated {
        return Err(Errno::EOVERFLOW);
    }
    Ok(n)
}

/// `vasprintf(&s, fmt, ap)`: a fresh `malloc` buffer; `*strp` is set only on success.
pub fn vasprintf(strp: KResult<usize>, flag: i32, fmt: KResult<&[u8]>, ap: usize) -> KResult<usize> {
    let (strp, fmt) = (strp?, fmt?);
    let mut out = Collect(TryVec::new(), None);
    let n = format(fmt, flag, ap, &mut out)?;
    if let Some(e) = out.1 {
        return Err(e);
    }
    let len = out.0.len();
    let buf = caller_alloc::malloc(len.saturating_add(1)).expose_provenance();
    if buf == 0 {
        return Err(Errno::ENOMEM);
    }
    let dst = conv::out_bytes(buf, len.saturating_add(1));
    if let Some((last, body)) = dst.split_last_mut() {
        body.copy_from_slice(&out.0);
        *last = 0;
    }
    if let Some(w) = crate::tb::c_word(strp) {
        w.store(buf, core::sync::atomic::Ordering::Relaxed);
    }
    Ok(n)
}

/// `obstack_vprintf(obs, fmt, ap)`: the result grows the obstack's current object (not
/// terminated, as the reference library leaves it).
pub fn obstack_vprintf(obs: Option<&mut liella_abi::layout::struct_obstack>, flag: i32, fmt: KResult<&[u8]>, ap: usize) -> KResult<usize> {
    let (o, fmt) = (obs.ok_or(Errno::EINVAL)?, fmt?);
    let mut out = Collect(TryVec::new(), None);
    let n = format(fmt, flag, ap, &mut out)?;
    if let Some(e) = out.1 {
        return Err(e);
    }
    let len = out.0.len();
    if o.chunk_limit.saturating_sub(o.next_free) < len {
        crate::heap::obstack_grow_room(o, len);
    }
    conv::out_bytes(o.next_free, len).copy_from_slice(&out.0);
    o.next_free = o.next_free.saturating_add(len);
    Ok(n)
}

// ------------------------------------------------------------------------------ §12.11

/// The registered conversions while `parse_printf_format` parses: an argument function is
/// called with room for one type, then, if it wants more and the caller's array has room
/// from its position, with that room; the types go to the caller's array.
struct Survey<'a> {
    out: &'a mut [u32],
    /// Where the conversions whose types an argument function gave start in the format.
    handled: TryVec<usize>,
}

impl pf::Registry for Survey<'_> {
    fn modifier(&self, at: &dyn Fn(usize) -> u32) -> Option<(usize, u16)> {
        reg::modifier(at)
    }

    fn conversion(&mut self, conv: &Conv, first: usize) -> KResult<Registered> {
        let Some((_, f)) = reg::handler(conv.letter) else { return Ok(Registered::No) };
        if f == 0 {
            return Ok(Registered::Builtin);
        }
        let info = Info::parsed(conv, false);
        let (r, types, _) = arginfo(f, &info, 1)?;
        let Ok(n) = usize::try_from(r) else { return Ok(Registered::Builtin) };
        if n > ARGS_MAX {
            return Err(Errno::EINVAL);
        }
        let room = self.out.len().saturating_sub(first);
        let types = if n > 1 && room > 0 { arginfo(f, &info, room)?.1 } else { types };
        for (slot, t) in self.out.iter_mut().skip(first).zip(types.iter().take(n)) {
            *slot = u32::from_ne_bytes(t.to_ne_bytes());
        }
        self.handled.push(conv.span.0)?;
        let mut classes: TryVec<Class> = TryVec::with_capacity(n)?;
        for _ in 0..n {
            classes.push(Class::Int)?;
        }
        Ok(Registered::Args(classes))
    }
}

/// `parse_printf_format(fmt, n, argtypes)`: the number of arguments `fmt` takes (the highest
/// position for a positional format); the type of each argument up to `n` is stored, the
/// others are left.
pub fn parse_format(fmt: KResult<&[u8]>, types: KResult<&mut [u32]>) -> KResult<usize> {
    let (fmt, out) = (fmt?, types?);
    let mut survey = Survey { out, handled: TryVec::new() };
    let parsed = pf::parse(fmt, &mut survey)?;
    let Survey { out, handled } = survey;
    let mut set = |i: usize, t: i32| {
        if let Some(slot) = out.get_mut(i) {
            *slot = u32::from_ne_bytes(t.to_ne_bytes());
        }
    };
    for piece in parsed.pieces.iter() {
        if let pf::Piece::Conv(c) = piece {
            for count in [c.width, c.prec] {
                if let pf::Count::Arg(a) = count {
                    set(a, reg::PA_INT);
                }
            }
            if c.nargs == 1 && !handled.contains(&c.span.0) {
                if let Some(t) = reg::builtin_type(c) {
                    set(c.arg, t);
                }
            }
        }
    }
    Ok(parsed.classes.len())
}

/// Which ready-made formatter.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Formatter {
    /// `printf_size`.
    Size,
    /// `__printf_fp`.
    Fp,
}

/// `printf_size(fp, info, args)` and `__printf_fp(fp, info, args)`: the value `args[0]` points
/// at (a `long double` with `is_long_double`) formatted as `info` says, written to `fp` (as
/// wide characters with `info.wide`); the units written.
pub fn formatter(fp: usize, info: Option<&struct_printf_info>, args: usize, which: Formatter) -> KResult<usize> {
    let info = Info::from_c(info.ok_or(Errno::EINVAL)?);
    let ld = info.has(reg::LONG_DOUBLE);
    let kind = if ld { Format::F80 } else { Format::F64 };
    let x = Float::unpack(conv::printf_float_arg(args, ld), kind);
    let env = Env::new();
    if info.has(reg::WIDE) {
        let units: TryVec<u32> = match which {
            Formatter::Size => reg::printf_size(&env, &info, x, kind)?,
            Formatter::Fp => reg::printf_fp(&env, &info, x, kind)?,
        };
        for &u in units.iter() {
            stdio::putwc(fp, Locking::Locked, u)?;
        }
        Ok(units.len())
    } else {
        let bytes: TryVec<u8> = match which {
            Formatter::Size => reg::printf_size(&env, &info, x, kind)?,
            Formatter::Fp => reg::printf_fp(&env, &info, x, kind)?,
        };
        stdio::fwrite(Ok(&bytes), fp, Locking::Locked).error.map_or(Ok(bytes.len()), Err)
    }
}

/// `printf_size_info(info, n, argtypes)`: one argument, a `double` (`long double` with
/// `is_long_double`), stored when there is room.
pub fn size_info(info: Option<&struct_printf_info>, n: usize, argtypes: Option<&mut i32>) -> KResult<i64> {
    let info = Info::from_c(info.ok_or(Errno::EINVAL)?);
    if n > 0 {
        if let Some(t) = argtypes {
            *t = reg::printf_size_type(&info);
        }
    }
    Ok(1)
}
