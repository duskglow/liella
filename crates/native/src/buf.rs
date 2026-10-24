//! The native C ABI's result conventions in Rust terms: a value or a negated error number, and
//! text delivered whole into a caller buffer or not at all (the conventions are stated in
//! `crates/boundary/src/native/mod.rs`). Every service module decodes its results here, so
//! each convention is decided once.

#![forbid(unsafe_code)]

use alloc::vec::Vec;

use liella::error::Errno;
use liella::mem::TryVec;

/// What a call that delivers text into a caller buffer did. The text is delivered whole or not
/// at all.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fit {
    /// The text is the first `n` bytes of the buffer.
    Copied(usize),
    /// The buffer was too small and nothing was copied: the text has this many bytes.
    Needed(usize),
}

/// A result: the value, or the error its negation numbers.
pub(crate) fn value(raw: i64) -> Result<u64, Errno> {
    Errno::decode(raw)
}

/// A length or count.
pub(crate) fn len(raw: i64) -> Result<usize, Errno> {
    value(raw).and_then(|v| usize::try_from(v).map_err(|_| Errno::EOVERFLOW))
}

/// A result with nothing to report (0).
pub(crate) fn done(raw: i64) -> Result<(), Errno> {
    value(raw).map(|_| ())
}

/// A text delivered into a buffer of `buf_len` bytes.
pub(crate) fn fit(raw: i64, buf_len: usize) -> Result<Fit, Errno> {
    len(raw).map(|n| if n <= buf_len { Fit::Copied(n) } else { Fit::Needed(n) })
}

/// A lookup: `ENOENT` is "not found", not an error.
pub(crate) fn found<T>(r: Result<T, Errno>) -> Result<Option<T>, Errno> {
    match r {
        Ok(v) => Ok(Some(v)),
        Err(e) if e == Errno::ENOENT => Ok(None),
        Err(e) => Err(e),
    }
}

/// The size of the first attempt's buffer (on the stack); longer texts take a second call.
const FIRST: usize = 256;

/// A text as an owned vector, from a call that delivers it into a buffer: first into a stack
/// buffer, then, if the text is longer, into one of exactly its length, again while it keeps
/// growing between the calls (another thread changing the environment, say).
pub(crate) fn fetch(mut call: impl FnMut(&mut [u8]) -> i64) -> Result<Vec<u8>, Errno> {
    let mut first = [0u8; FIRST];
    let mut need = len(call(&mut first))?;
    if let Some(text) = first.get(..need) {
        return TryVec::from_slice(text).map(Vec::from);
    }
    let mut buf: TryVec<u8> = TryVec::new();
    loop {
        buf.clear();
        buf.reserve_exact(need)?;
        buf.resize(need, 0)?;
        let n = len(call(&mut buf))?;
        if n <= need {
            buf.truncate(n);
            return Ok(buf.into());
        }
        need = n;
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects)]
mod tests {
    use super::*;

    /// A provider following the C ABI's text convention for `text`.
    fn deliver(text: &[u8], buf: &mut [u8]) -> i64 {
        if let Some(dst) = buf.get_mut(..text.len()) {
            dst.copy_from_slice(text);
        }
        i64::try_from(text.len()).unwrap_or(i64::MAX)
    }

    #[test]
    fn results_decode_the_negated_error() {
        assert_eq!(value(7), Ok(7));
        assert_eq!(value(0), Ok(0));
        assert_eq!(value(-2), Err(Errno::ENOENT));
        assert_eq!(done(-14), Err(Errno::EFAULT));
        assert_eq!(done(0), Ok(()));
        assert_eq!(len(4096), Ok(4096));
        // A number beyond the kernel's range is still an error, never a success.
        assert!(value(-4095).is_err());
    }

    #[test]
    fn fit_reports_copied_or_needed() {
        assert_eq!(fit(3, 3), Ok(Fit::Copied(3)));
        assert_eq!(fit(4, 3), Ok(Fit::Needed(4)));
        assert_eq!(fit(0, 0), Ok(Fit::Copied(0)));
        assert_eq!(fit(-22, 8), Err(Errno::EINVAL));
    }

    #[test]
    fn found_maps_only_enoent_to_none() {
        assert_eq!(found(Ok(1)), Ok(Some(1)));
        assert_eq!(found::<u8>(Err(Errno::ENOENT)), Ok(None));
        assert_eq!(found::<u8>(Err(Errno::EINVAL)), Err(Errno::EINVAL));
    }

    #[test]
    fn fetch_short_text_takes_one_call() {
        let mut calls = 0;
        let got = fetch(|b| {
            calls += 1;
            deliver(b"UTF-8", b)
        });
        assert_eq!(got.as_deref(), Ok(&b"UTF-8"[..]));
        assert_eq!(calls, 1);
    }

    #[test]
    fn fetch_sizes_long_text_exactly() {
        let long = [b'x'; 1000];
        let mut calls = 0;
        let mut lens = Vec::new();
        let got = fetch(|b| {
            calls += 1;
            lens.push(b.len());
            deliver(&long, b)
        });
        assert_eq!(got.as_deref(), Ok(&long[..]));
        assert_eq!(calls, 2);
        assert_eq!(lens, [FIRST, 1000]);
    }

    #[test]
    fn fetch_retries_while_the_text_grows() {
        // The text grows between the calls (a concurrent setenv), then shrinks.
        let texts: [&[u8]; 3] = [&[b'a'; 300], &[b'b'; 400], &[b'c'; 10]];
        let mut i = 0;
        let got = fetch(|b| {
            let t = texts[i.min(2)];
            i += 1;
            deliver(t, b)
        });
        assert_eq!(got.as_deref(), Ok(&[b'c'; 10][..]));
        assert_eq!(i, 3);
    }

    #[test]
    fn fetch_passes_errors_through() {
        assert_eq!(fetch(|_| -2), Err(Errno::ENOENT));
        let mut i = 0;
        let got = fetch(|b| {
            i += 1;
            if i == 1 { deliver(&[0; 500], b) } else { -22 }
        });
        assert_eq!(got, Err(Errno::EINVAL));
    }
}
