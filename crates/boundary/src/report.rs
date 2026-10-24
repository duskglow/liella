//! Error text and diagnostic lines (spec §2.2, §2.3; D-2.2, D-2.3): `strerror`, `strerror_r`
//! (GNU and XSI), `strerrorname_np`, `strerrordesc_np`, `perror`, and the `err`/`warn` and
//! `error`/`error_at_line` families. The texts are the reference library's C-locale texts:
//! message catalogs arrive with gettext (phase 13).
//!
//! `strerror_r`, `perror` and the families were rewritten under the clean-room rules
//! (implementation plan §1.4, items 6 and 7) from the spec and `spec/behaviour/diagnostics.md`.
//!
//! **Lines.** `warn`: `short: message: error text`, or `short: error text` without a format.
//! `warnx`: `short: message`. `err` and `errx` print as `warn` and `warnx`, then `exit`.
//! `error`: `full: message[: error text]`; `error_at_line`: `full:file:line: message[: error
//! text]` (a NULL file prints as `error`). `short` and `full` are `program_invocation_short_name`
//! and `program_invocation_name`; the error text is `errno`'s at the call for `err`/`warn`,
//! `errnum`'s (when not 0) for `error`. Every line ends with a newline.
//!
//! **`error` and `error_at_line`** flush `stdout` first, call `error_print_progname` (when set)
//! in place of printing the program name, count each message printed in
//! `error_message_count`, and `exit(status)` when `status` is not 0. With `error_one_per_line`
//! set, `error_at_line` skips a message whose file name (compared as text) and line equal those
//! of the previous `error_at_line` call made while it was set; it still exits for a non-zero
//! status, since the headers declare that form `noreturn` (the reference returns).
//!
//! **Output** goes through the `stderr` stream as one piece of text: through its codec when it
//! is wide-oriented; `perror` leaves an unoriented stream unoriented (§2.2), the others make it
//! byte-oriented. `errno` is left as it was, except that a failed write stores its error.

use core::sync::atomic::{AtomicU32, AtomicU8, AtomicUsize, Ordering::Relaxed};

use liella::diag;
use liella::error::{Errno, KResult};
use liella::mem::TryVec;
use liella::text::Encoding;
use liella_legacy::stdio_compat as sc;
use liella_rt::sync::RawMutex;

use crate::stdio::{self, Locking};
use crate::sys::CSys;
use crate::{conv, data, errno, ffi, locale, printf, process};

/// The address of a static NUL-terminated text, 0 for none.
fn text_addr(t: Option<&'static [u8]>) -> usize {
    t.map_or(0, |t| t.as_ptr().expose_provenance())
}

/// `Unknown error N`, NUL-terminated, into `b`; the length without the NUL.
fn unknown(e: i32, b: &mut [u8; 32]) -> usize {
    let n = diag::unknown(e, b);
    if let Some(t) = b.get_mut(n) {
        *t = 0;
    }
    n
}

/// The text for error number `e` (no NUL): its description, or `Unknown error N` in `b`.
fn error_text(e: i32, b: &mut [u8; 32]) -> &[u8] {
    match diag::description(e) {
        Some(d) => d.as_bytes(),
        None => {
            let n = diag::unknown(e, b);
            b.get(..n).unwrap_or(&[])
        }
    }
}

/// `text` into `buf` as a C string, cut to fit (nothing at all for an empty buffer): whether
/// all of it fitted.
fn put_cut(text: &[u8], buf: &mut [u8]) -> bool {
    let Some(room) = buf.len().checked_sub(1) else { return false };
    let n = conv::copy_units(buf.get_mut(..room).unwrap_or(&mut []), text);
    if let Some(t) = buf.get_mut(n) {
        *t = 0;
    }
    n == text.len()
}

/// `strerror(e)` (and `strerror_l`): a static text, or `Unknown error N` in the calling thread's
/// slot. A thread that has no slot and cannot get one (no memory for its descriptor) gets the
/// constant `Unknown error` and `errno` `ENOMEM` (POSIX: no return value is reserved for an
/// error), never a buffer another thread writes (D-C0.2). The address.
pub fn strerror(e: i32) -> usize {
    if let Some(d) = diag::description_c(e) {
        return text_addr(Some(d));
    }
    let mut b = [0u8; 32];
    let n = unknown(e, &mut b);
    let Some(buf) = crate::thread::result_slot(liella_rt::thread::SLOT_STRERROR) else {
        errno::set(Errno::ENOMEM);
        return text_addr(Some(UNKNOWN_ERROR));
    };
    buf.store(b.get(..=n).unwrap_or(&[]));
    buf.addr()
}

/// `strerror`'s text for a thread without a result slot.
const UNKNOWN_ERROR: &[u8] = b"Unknown error\0";

/// `strerrorname_np(e)`, `strerrordesc_np(e)`: static texts, 0 for an unknown number.
pub fn strerrorname(e: i32) -> usize {
    text_addr(diag::name_c(e))
}

/// `strerrordesc_np(e)`.
pub fn strerrordesc(e: i32) -> usize {
    text_addr(diag::description_c(e))
}

/// GNU `strerror_r(e, buf, n)` (`buf` is the caller's buffer, at `addr`): the address of the
/// text. A known number gives its static text and leaves the buffer alone; an unknown one
/// gives `Unknown error N`, cut to fit, in the buffer (untouched when empty). Without a usable
/// buffer (NULL with a non-zero size), the unknown text goes to `strerror`'s buffer instead.
pub fn strerror_r(e: i32, buf: KResult<&mut [u8]>, addr: usize) -> usize {
    if let Some(d) = diag::description_c(e) {
        return text_addr(Some(d));
    }
    let Ok(buf) = buf else { return strerror(e) };
    let mut b = [0u8; 32];
    let n = diag::unknown(e, &mut b);
    put_cut(b.get(..n).unwrap_or(&[]), buf);
    addr
}

/// XSI `strerror_r` (`__xpg_strerror_r`): the text, cut to fit, in `buf` (untouched when
/// empty). 0 when a known number's text fitted, `ERANGE` when it did not, `EINVAL` for an
/// unknown number (whose `Unknown error N` is still stored), `EFAULT` without a usable buffer.
/// `errno` is not changed.
pub fn xpg_strerror_r(e: i32, buf: KResult<&mut [u8]>) -> i32 {
    let Ok(buf) = buf else { return Errno::EFAULT.get() };
    let mut b = [0u8; 32];
    match diag::description(e) {
        Some(d) if put_cut(d.as_bytes(), buf) => 0,
        Some(_) => Errno::ERANGE.get(),
        None => {
            let n = diag::unknown(e, &mut b);
            put_cut(b.get(..n).unwrap_or(&[]), buf);
            Errno::EINVAL.get()
        }
    }
}

/// Write `parts` to the current `stderr` stream as one text (one write when memory allows),
/// in its orientation; an undecided stream becomes byte-oriented only if `orient`. Afterwards
/// `errno` is `saved` (the caller's, whatever the stream layer or a callback did to it),
/// unless the write failed: then it is that error.
pub(crate) fn emit(parts: &[&[u8]], orient: bool, saved: i32) {
    let enc = Encoding::of(locale::current());
    let mut whole = TryVec::new();
    let joined = parts.iter().try_for_each(|p| whole.extend_from_slice(p)).is_ok();
    let r = stdio::with(stdio::stderr(), Locking::Locked, |g, file, ext| {
        if joined {
            sc::write_text(g, file, ext, whole.as_slice(), orient, enc)
        } else {
            parts.iter().try_for_each(|p| sc::write_text(g, file, ext, p, orient, enc))
        }
    });
    match r.and_then(core::convert::identity) {
        Err(e) => errno::set(e),
        Ok(()) if errno::get() != saved => errno::set_raw(saved),
        Ok(()) => {}
    }
}

/// `perror(s)`: `s: text` (just the text when `s` is NULL or empty) and a newline, for the
/// current `errno`.
pub fn perror(s: KResult<Option<&[u8]>>) {
    let saved = errno::get();
    let mut b = [0u8; 32];
    let text = error_text(saved, &mut b);
    match s.ok().flatten().filter(|s| !s.is_empty()) {
        Some(s) => emit(&[s, b": ", text, b"\n"], false, saved),
        None => emit(&[text, b"\n"], false, saved),
    }
}

/// Which diagnostic function: the numbers the C adapters pass (0 `warn`, 1 `warnx`, 2 `err`,
/// 3 `errx`, 4 `error`, 5 `error_at_line`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// `warn`.
    Warn,
    /// `warnx`.
    Warnx,
    /// `err`.
    Err,
    /// `errx`.
    Errx,
    /// `error`.
    Error,
    /// `error_at_line`.
    ErrorAtLine,
}

impl Kind {
    /// The adapter's number.
    pub fn of(n: i32) -> Kind {
        match n {
            1 => Kind::Warnx,
            2 => Kind::Err,
            3 => Kind::Errx,
            4 => Kind::Error,
            5 => Kind::ErrorAtLine,
            _ => Kind::Warn,
        }
    }
}

/// The program name at `addr` (one of the program-name objects); `(null)` for NULL, as `%s`
/// prints it.
fn program_name(addr: usize) -> &'static [u8] {
    if addr == 0 { b"(null)" } else { conv::process_text(addr) }
}

/// `v` in decimal, into the end of `b`.
fn decimal(mut v: u32, b: &mut [u8; 10]) -> &[u8] {
    let mut i = b.len();
    loop {
        i = i.saturating_sub(1);
        if let Some(d) = b.get_mut(i) {
            *d = b'0'.saturating_add(u8::try_from(v % 10).unwrap_or(0));
        }
        v /= 10;
        if v == 0 {
            break;
        }
    }
    b.get(i..).unwrap_or(&[])
}

/// The longest file name `error_one_per_line` remembers; a longer one is never taken as a
/// repeat (the message is printed).
const LAST_MAX: usize = 4096;
/// [`LAST_LEN`] when there is no remembered location.
const NO_LAST: usize = usize::MAX;
/// The location of the previous `error_at_line` call made with `error_one_per_line` set: the
/// file name's bytes, its length ([`NO_LAST`] for none) and the line; under [`LAST_LOCK`].
static LAST_NAME: [AtomicU8; LAST_MAX] = [const { AtomicU8::new(0) }; LAST_MAX];
static LAST_LEN: AtomicUsize = AtomicUsize::new(NO_LAST);
static LAST_LINE: AtomicU32 = AtomicU32::new(0);
/// Serializes the comparison with the remembered location and its replacement.
static LAST_LOCK: RawMutex = RawMutex::new();

/// Whether `file`:`line` repeats the remembered location; it becomes the remembered one (a
/// NULL file name, or one too long to keep, leaves none).
fn repeats(file: Option<&[u8]>, line: u32) -> bool {
    let _g = LAST_LOCK.lock(&CSys);
    let same = file.is_some_and(|f| {
        LAST_LEN.load(Relaxed) == f.len() && LAST_LINE.load(Relaxed) == line && LAST_NAME.iter().zip(f).all(|(a, &b)| a.load(Relaxed) == b)
    });
    match file {
        Some(f) if f.len() <= LAST_MAX => {
            for (a, &b) in LAST_NAME.iter().zip(f) {
                a.store(b, Relaxed);
            }
            LAST_LEN.store(f.len(), Relaxed);
            LAST_LINE.store(line, Relaxed);
        }
        _ => LAST_LEN.store(NO_LAST, Relaxed),
    }
    same
}

/// The `err`, `warn`, `error` and `error_at_line` families (`fmt` with the arguments of the
/// `va_list` at `ap`). The `err` forms, and the `error` forms with a non-zero `status`, end
/// the process.
pub fn report(kind: Kind, status: i32, errnum: i32, file: KResult<Option<&[u8]>>, line: u32, fmt: KResult<Option<&[u8]>>, ap: usize) {
    let saved = errno::get();
    let fmt = fmt.ok().flatten();
    // Formatted first, so that `%m` sees the caller's `errno`. A message that cannot be
    // formatted (no memory) is left out.
    let collected = fmt.map(|f| printf::collect(f, ap));
    let msg = collected.as_ref().and_then(|m| m.as_ref().ok()).map_or(&[][..], |m| m.as_slice());
    let mut b = [0u8; 32];
    match kind {
        Kind::Warn | Kind::Err | Kind::Warnx | Kind::Errx => {
            let short = program_name(ffi::program_invocation_short_name.load(Relaxed));
            match (kind, fmt.is_some()) {
                (Kind::Warn | Kind::Err, true) => emit(&[short, b": ", msg, b": ", error_text(saved, &mut b), b"\n"], true, saved),
                (Kind::Warn | Kind::Err, false) => emit(&[short, b": ", error_text(saved, &mut b), b"\n"], true, saved),
                _ => emit(&[short, b": ", msg, b"\n"], true, saved),
            }
            if matches!(kind, Kind::Err | Kind::Errx) {
                process::exit(status);
            }
        }
        Kind::Error | Kind::ErrorAtLine => {
            let file = if kind == Kind::ErrorAtLine { file.ok().flatten() } else { None };
            let suppressed = kind == Kind::ErrorAtLine && data::error_one_per_line.load(Relaxed) != 0 && repeats(file, line);
            if !suppressed {
                let _ = stdio::fflush(stdio::stdout(), Locking::Locked);
                // The program name, then `:` before a location or `: ` before the message; the
                // hook prints whatever it likes in place of both.
                let hook = data::error_print_progname.load(Relaxed);
                let [n0, n1]: [&[u8]; 2] = if hook == 0 {
                    [program_name(ffi::program_invocation_name.load(Relaxed)), if file.is_some() { b":" } else { b": " }]
                } else {
                    // The program's own `void (*)(void)`, set for exactly this use (§2.3).
                    let _ = ffi::liella_machine_call0(hook);
                    [&[], &[]]
                };
                let mut d = [0u8; 10];
                let [a0, a1, a2, a3]: [&[u8]; 4] = match file {
                    Some(f) => [f, b":", decimal(line, &mut d), b": "],
                    None => [&[]; 4],
                };
                let [t0, t1]: [&[u8]; 2] = if errnum != 0 { [b": ", error_text(errnum, &mut b)] } else { [&[]; 2] };
                emit(&[n0, n1, a0, a1, a2, a3, msg, t0, t1, b"\n"], true, saved);
                data::error_message_count.fetch_add(1, Relaxed);
            }
            if status != 0 {
                process::exit(status);
            }
        }
    }
}
