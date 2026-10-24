//! iconv (spec §6.6, D-6.8): charset names with their suffixes (`//TRANSLIT`, `//IGNORE`) and
//! conversions between the built-in codecs, fed in pieces with small output buffers as callers
//! do. A conversion to UTF-32 and back to UTF-8 that succeeds must give the input back.
//!
//! Input: an output-size byte, then `from-name NUL to-name NUL data`. A name that is no built-in
//! codec picks one by its first byte.
#![no_main]
#![allow(clippy::disallowed_methods)] // a test tool: S8 (fallible allocation) governs the library crates only

use libfuzzer_sys::fuzz_target;
use liella::iconv::{self, Codec, Conv, Order, State};
use liella_fuzz::Input;

const CODECS: [Codec; 12] = [
    Codec::Utf8,
    Codec::Utf16(None),
    Codec::Utf16(Some(Order::Le)),
    Codec::Utf16(Some(Order::Be)),
    Codec::Utf32(None),
    Codec::Utf32(Some(Order::Le)),
    Codec::Utf32(Some(Order::Be)),
    Codec::Ucs2(Order::Le),
    Codec::Ucs2(Order::Be),
    Codec::Ucs4(Order::Be),
    Codec::Ascii,
    Codec::Latin1,
];

fn codec(full: &[u8]) -> (Codec, bool, bool) {
    let spec = iconv::parse_name(full);
    let c = iconv::builtin(spec.name).unwrap_or(CODECS[usize::from(full.first().copied().unwrap_or(0)) % CODECS.len()]);
    (c, spec.translit, spec.ignore)
}

/// Convert all of `input` with output pieces of `piece` bytes: the output, and whether every
/// call succeeded.
fn convert(conv: &mut Conv, input: &[u8], piece: usize) -> (Vec<u8>, bool) {
    let loc = liella_fuzz::locale(true);
    let mut out = Vec::new();
    let mut buf = vec![0u8; piece];
    let mut rest = input;
    let mut ok = true;
    for _ in 0..input.len().saturating_mul(4).saturating_add(8) {
        if rest.is_empty() {
            break;
        }
        let o = conv.convert(rest, &mut buf, &loc);
        assert!(o.read <= rest.len() && o.written <= buf.len(), "convert stays within its buffers");
        out.extend_from_slice(&buf[..o.written]);
        rest = &rest[o.read..];
        match o.result {
            Ok(_) => {}
            Err(liella::error::Errno::E2BIG) if o.read > 0 || o.written > 0 => {}
            Err(_) => {
                ok = false;
                break;
            }
        }
    }
    (out, ok && rest.is_empty())
}

fuzz_target!(|data: &[u8]| {
    let mut input = Input::new(data);
    let piece = usize::from(input.byte() % 64) + 1;
    let (from, tr1, ig1) = codec(input.part());
    let (to, translit, ignore) = codec(input.part());
    let text = input.rest();
    let _ = (tr1, ig1);

    let mut conv = Conv { from, to, translit, ignore, state: State::default() };
    let _ = convert(&mut conv, text, piece);
    conv.reset();
    let _ = convert(&mut conv, text, 256);
    let _ = Conv::from_words(conv.to_words(), &|_| unreachable!("no charmap codec here"));

    // UTF-8 -> UTF-32 -> UTF-8 gives the input back.
    let mut there = Conv { from: Codec::Utf8, to: Codec::Utf32(Some(Order::Le)), translit: false, ignore: false, state: State::default() };
    let (wide, ok) = convert(&mut there, text, piece);
    if ok {
        let mut back = Conv { from: Codec::Utf32(Some(Order::Le)), to: Codec::Utf8, translit: false, ignore: false, state: State::default() };
        let (again, ok2) = convert(&mut back, &wide, 4096);
        assert!(ok2, "UTF-32 from valid UTF-8 converts back");
        assert_eq!(again, text, "UTF-8 round trip");
    }
});
