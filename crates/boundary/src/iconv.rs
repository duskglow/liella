//! `iconv` glue (spec §6.4; D-6.8, D-6.9): an `iconv_t` is a heap block of
//! [`tb::ICONV_WORDS`] words, a tag and then the descriptor's
//! [`liella::iconv::Conv::to_words`]. Conversions use the calling thread's current
//! `LC_CTYPE` for `//TRANSLIT`, and the empty name is its codeset.

use core::sync::atomic::{AtomicUsize, Ordering::Relaxed};

use liella::error::{Errno, KResult};
use liella::iconv::{self, Conv, State};
use liella_legacy::global_locale as gl;

use crate::conv::IconvArgs;
use crate::locale::{self, Glue};
use crate::sys::CSys;
use crate::{heap, tb};

/// Marks a live descriptor ("liiconv\0"); `iconv_close` clears it.
const TAG: usize = 0x6c69_6963_6f6e_7600;

/// The codec a charset name denotes (as `iconv_open` reads it, without suffixes).
pub fn codec_named(name: &[u8]) -> Option<iconv::Codec> {
    codec(iconv::parse_name(name).name)
}

fn codec(name: &[u8]) -> Option<iconv::Codec> {
    if name.is_empty() {
        return Some(iconv::of_locale(locale::current()));
    }
    gl::codec(&CSys, &Glue, name)
}

/// `iconv_open(to, from)`: the descriptor's address, or `EINVAL` for an unsupported pair.
pub fn open(to: KResult<&[u8]>, from: KResult<&[u8]>) -> KResult<usize> {
    let (to, from) = (iconv::parse_name(to?), iconv::parse_name(from?));
    let conv = Conv {
        from: codec(from.name).ok_or(Errno::EINVAL)?,
        to: codec(to.name).ok_or(Errno::EINVAL)?,
        translit: to.translit,
        ignore: to.ignore,
        state: State::default(),
    };
    let addr = heap::alloc_words(tb::ICONV_WORDS).ok_or(Errno::ENOMEM)?;
    let words = tb::iconv_object(addr).ok_or(Errno::ENOMEM)?;
    store(words, &conv);
    words.first().ok_or(Errno::ENOMEM)?.store(TAG, Relaxed);
    Ok(addr)
}

fn store(words: &[AtomicUsize], conv: &Conv) {
    for (w, v) in words.iter().skip(1).zip(conv.to_words()) {
        w.store(v, Relaxed);
    }
}

/// The live descriptor at `cd`, or `EBADF`.
fn load(cd: usize) -> KResult<(&'static [AtomicUsize], Conv)> {
    let words = tb::iconv_object(cd).ok_or(Errno::EBADF)?;
    if words.first().map(|w| w.load(Relaxed)) != Some(TAG) {
        return Err(Errno::EBADF);
    }
    let mut v = [0usize; 6];
    for (slot, w) in v.iter_mut().zip(words.iter().skip(1)) {
        *slot = w.load(Relaxed);
    }
    let conv = Conv::from_words(v, &tb::charmap_at).ok_or(Errno::EBADF)?;
    Ok((words, conv))
}

/// `iconv(cd, …)`: the irreversible conversions, or the error; the pointers and counts
/// advance past what was converted either way.
pub(crate) fn convert(cd: usize, args: KResult<IconvArgs<'_>>) -> KResult<usize> {
    let (words, mut conv) = load(cd)?;
    let r = match args? {
        IconvArgs::Reset | IconvArgs::Flush => {
            conv.reset();
            Ok(0)
        }
        IconvArgs::Convert(mut input, mut out) => {
            let o = conv.convert(input.bytes(), out.room(), locale::current());
            input.advance(o.read);
            out.advance(o.written);
            o.result
        }
    };
    store(words, &conv);
    r
}

/// `iconv_close(cd)`.
pub fn close(cd: usize) -> KResult<()> {
    let (words, _) = load(cd)?;
    words.first().ok_or(Errno::EBADF)?.store(0, Relaxed);
    heap::free_words(cd);
    Ok(())
}
