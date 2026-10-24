//! Async-signal-safe fatal diagnostics and abort, over `Sys` (spec §1.2, §2.4, §2.5).
//!
//! Nothing here allocates or takes a lock: the diagnostics must come out even from a corrupted
//! heap (§2.5 guarantee), so text goes straight to descriptor 2. A message goes out as one
//! gathered write, so that two dying threads do not interleave inside it (D-22.12
//! (8)).

use liella_kernel::errno::Errno;
use liella_kernel::fd::Fd;
use liella_kernel::mem::IoVec;
use liella_kernel::sys::{Disposition, Sys};
use liella_kernel::uapi::{SigSet, SIGABRT, SIG_UNBLOCK};

/// Parts handed to one `writev`.
const GATHER: usize = 16;

/// Write `parts` to standard error, in order (errors are ignored: there is nowhere to report
/// them).
pub fn message<S: Sys>(sys: &S, parts: &[&[u8]]) {
    let mut rest = parts;
    // Bytes of `rest[0]` already written.
    let mut skip = 0usize;
    loop {
        // Step over written and empty parts.
        while let Some(first) = rest.first() {
            if skip < first.len() {
                break;
            }
            skip = skip.saturating_sub(first.len());
            rest = rest.get(1..).unwrap_or_default();
        }
        let Some(first) = rest.first() else { return };
        let head = first.get(skip..).unwrap_or_default();
        let more = rest.get(1..).unwrap_or_default();
        let take = more.len().min(GATHER.saturating_sub(1));
        let iov: [IoVec<'_>; GATHER] = core::array::from_fn(|i| match i.checked_sub(1) {
            None => IoVec::new(head),
            Some(j) => IoVec::new(if j < take { more.get(j).copied().unwrap_or_default() } else { &[] }),
        });
        match sys.writev(Fd::STDERR, iov.get(..take.saturating_add(1)).unwrap_or_default()) {
            // The descriptor takes nothing more: give up quietly.
            Ok(0) => return,
            Ok(n) => skip = skip.saturating_add(n),
            Err(e) if e == Errno::EINTR => {}
            Err(_) => return,
        }
    }
}

/// The decimal digits of `v` in `buf`; returns the used tail.
pub fn decimal(v: u64, buf: &mut [u8; 20]) -> &[u8] {
    let mut at = buf.len();
    let mut v = v;
    loop {
        at = at.saturating_sub(1);
        let digit = u8::try_from(v % 10).unwrap_or(0);
        if let Some(slot) = buf.get_mut(at) {
            *slot = b'0'.saturating_add(digit);
        }
        v /= 10;
        if v == 0 || at == 0 {
            break;
        }
    }
    buf.get(at..).unwrap_or_default()
}

/// `abort()` (spec §1.2): unblock `SIGABRT` and raise it; if a handler returns, reset the
/// disposition to the default and raise it again; if the process still survives, execute the
/// architecture trap instruction (`trap`), then `_exit(127)`. Streams are not flushed.
pub fn abort<S: Sys>(sys: &S, trap: impl FnOnce()) -> ! {
    let abrt = SigSet::empty().with(SIGABRT);
    let raise = || {
        let _ = sys.tgkill(sys.getpid(), sys.gettid(), SIGABRT);
    };
    let _ = sys.sigprocmask(SIG_UNBLOCK, Some(&abrt));
    raise();
    // A handler caught it and returned (or it was ignored): the default action now.
    let _ = sys.sigaction_set(SIGABRT, Disposition::Default, 0, SigSet::empty());
    let _ = sys.sigprocmask(SIG_UNBLOCK, Some(&abrt));
    raise();
    trap();
    sys.exit_group(127)
}

/// A fatal diagnostic, then [`abort`] (`__libc_fatal`).
pub fn fatal<S: Sys>(sys: &S, parts: &[&[u8]], trap: impl FnOnce()) -> ! {
    message(sys, parts);
    abort(sys, trap)
}

/// `prog: file:line: ` and, with a function, `function: `, as message parts; the line's digits
/// live in `digits`.
fn location<'a>(prog: &'a [u8], file: &'a [u8], line: u32, function: Option<&'a [u8]>, digits: &'a mut [u8; 20]) -> [&'a [u8]; 8] {
    let named = !prog.is_empty();
    let (prog, prog_sep): (&[u8], &[u8]) = if named { (prog, b": ") } else { (b"", b"") };
    let (func, func_sep): (&[u8], &[u8]) = match function {
        Some(f) => (f, b": "),
        None => (b"", b""),
    };
    [prog, prog_sep, file, b":", decimal(u64::from(line), digits), b": ", func, func_sep]
}

/// The `__assert_fail` text (spec §2.4, in glibc 2.39's exact form, ERRATA E8):
/// `prog: file:line: function: Assertion `expr' failed.`; the program name and its `: ` are
/// left out when it is empty, the function and its `: ` when there is none.
pub fn assert_message<S: Sys>(sys: &S, prog: &[u8], expr: &[u8], file: &[u8], line: u32, function: Option<&[u8]>) {
    let mut digits = [0u8; 20];
    let [a, b, c, d, e, f, g, h] = location(prog, file, line, function, &mut digits);
    message(sys, &[a, b, c, d, e, f, g, h, b"Assertion `", expr, b"' failed.\n"]);
}

/// The `__assert_perror_fail` text: `prog: file:line: function: Unexpected error: text.`
pub fn assert_perror_message<S: Sys>(sys: &S, prog: &[u8], text: &[u8], file: &[u8], line: u32, function: Option<&[u8]>) {
    let mut digits = [0u8; 20];
    let [a, b, c, d, e, f, g, h] = location(prog, file, line, function, &mut digits);
    message(sys, &[a, b, c, d, e, f, g, h, b"Unexpected error: ", text, b".\n"]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimal_digits() {
        let mut b = [0u8; 20];
        assert_eq!(decimal(0, &mut b), b"0");
        assert_eq!(decimal(12345, &mut b), b"12345");
        assert_eq!(decimal(u64::MAX, &mut b), b"18446744073709551615");
    }
}
