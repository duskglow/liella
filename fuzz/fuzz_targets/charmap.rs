//! Charmaps (D-6.8; the POSIX charmap format, gzip-compressed as installed): the decompressor,
//! the charmap parser, decoding and encoding, and `iconv` conversions through the parsed
//! charmap (which the library keeps `'static`, so it is leaked within the harness budget).
//!
//! Input: a mode byte (bit 0: the rest is gzip data to inflate, as the installed `.gz` files;
//! otherwise `charmap text NUL sample bytes`).
#![no_main]
#![allow(clippy::disallowed_methods)] // a test tool: S8 (fallible allocation) governs the library crates only

use libfuzzer_sys::fuzz_target;
use liella::charmap::{inflate, Charmap, Step, MAX_BYTES};
use liella::iconv::{Codec, Conv, State};
use liella::text::CharmapRef;
use liella_fuzz::Input;

/// The largest text inflated (the installed charmaps are below 1 MiB).
const INFLATE_LIMIT: usize = 4 << 20;

fn use_charmap(cm: Charmap, sample: &[u8]) {
    let _ = (cm.name(), cm.aliases().count(), cm.ascii(), cm.mb_cur_max());
    let mut at = 0usize;
    while at < sample.len() {
        match cm.decode(&sample[at..]) {
            Step::Char(cp, n) => {
                assert!(n > 0 && n <= MAX_BYTES && n <= sample.len() - at, "decode took {n} bytes");
                let mut out = [0u8; MAX_BYTES];
                if let Some(len) = cm.encode(cp, &mut out) {
                    assert!((1..=MAX_BYTES).contains(&len));
                }
                at += n;
            }
            Step::Incomplete | Step::Invalid => at += 1,
        }
    }
    for (cp, bytes, len) in cm.mappings().take(2048) {
        assert!((1..=MAX_BYTES).contains(&len));
        let _ = cm.decode(&bytes[..len]);
        let mut out = [0u8; MAX_BYTES];
        let _ = cm.encode(cp, &mut out);
    }
    // Conversions through the charmap, both directions.
    let size = sample.len().saturating_mul(16).saturating_add(4096);
    let Some(cm) = liella_fuzz::leak_value(cm, size) else { return };
    let loc = liella_fuzz::locale(true);
    let codec = Codec::Charmap(CharmapRef(cm));
    for (from, to) in [(codec, Codec::Utf8), (Codec::Utf8, codec), (codec, Codec::Utf16(None))] {
        for (translit, ignore) in [(false, false), (true, true)] {
            let mut conv = Conv { from, to, translit, ignore, state: State::default() };
            let mut out = [0u8; 256];
            let mut rest = sample;
            for _ in 0..64 {
                let o = conv.convert(rest, &mut out, &loc);
                assert!(o.read <= rest.len() && o.written <= out.len());
                rest = &rest[o.read..];
                if o.read == 0 || rest.is_empty() {
                    break;
                }
            }
        }
    }
}

fuzz_target!(|data: &[u8]| {
    let mut input = Input::new(data);
    let mode = input.byte();
    if mode & 1 != 0 {
        let gz = input.rest();
        if let Ok(text) = inflate::gunzip(gz, INFLATE_LIMIT) {
            assert!(text.len() <= INFLATE_LIMIT);
            if let Ok(cm) = Charmap::parse(&text) {
                use_charmap(cm, b"abc\x80\xff");
            }
        }
        let _ = inflate::gunzip_prefix(gz, 4096);
        return;
    }
    let text = input.part();
    let sample = input.rest();
    liella::charmap::header_names(text, &mut |n| {
        let _ = n.len();
    });
    if let Ok(cm) = Charmap::parse(text) {
        use_charmap(cm, sample);
    }
});
