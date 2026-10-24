// Specified from glibc-functional-spec.md §6. Generated stubs were replaced by real tests of the
// SAME NAME (see crates/spectest); `cargo xtask stubs` now only checks coverage.

//! Clause tests for spec §6 (37 clauses).
//!
//! The conversions are tested in the built-in encodings: ASCII (the C locale, D-6.7) and UTF-8
//! (`C.UTF-8`). `iconv` (§6.4) is tested here through `liella::iconv` with the built-in codecs;
//! the charmap codecs and the exports are compared with the reference library by
//! `iconv_products.rs` and `charmap_data.rs`.

#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, missing_docs)]

use liella::charmap::Charmap;
use liella::error::Errno;
use liella::iconv::{self, Conv, Outcome, State as IconvState};
use liella::locale::Locale;
use liella::text::{self, Decoded, Encoding, Fed, MbState};
use liella_abi::ctypes::{c_char, mbstate_t};
use liella_boundary::posix::s06_multibyte as x;
use liella_legacy::mb_compat::{self, State, INCOMPLETE};
use liella_legacy::mb_state::{self, Slot};

const U: Encoding = Encoding::Utf8;
const A: Encoding = Encoding::Ascii;

fn init() -> MbState {
    MbState::INITIAL
}

fn dec(st: &mut MbState, b: &[u8], e: Encoding) -> Decoded {
    text::decode(st, b, e)
}

#[test]
fn s6_n_1() {
    // Pure: the same input in the same state always gives the same answer, with no Sys.
    for _ in 0..3 {
        assert_eq!(dec(&mut init(), "é".as_bytes(), U), Decoded::Char(0xE9, 2));
    }
}

#[test]
fn s6_n_2() {
    assert_eq!(dec(&mut init(), "😀".as_bytes(), U), Decoded::Char(0x1F600, 4), "one code point per wide unit");
    let mut out = [0u32; 4];
    let r = text::mbs_to_wcs(&mut init(), "a😀".as_bytes(), Some(&mut out), U);
    assert_eq!((r.written, &out[..2]), (2, &[0x61, 0x1F600][..]));
    assert_eq!(Encoding::of(&Locale::C), A, "the C locale's codeset");
}

#[test]
fn s6_1_c_0() {
    assert!(MbState::INITIAL.is_initial());
    assert!(MbState::from_bytes([0; 8]).is_initial(), "zero-initialised is initial");
    let mut st = MbState::INITIAL;
    assert_eq!(dec(&mut st, &[0xE2, 0x82], U), Decoded::Incomplete);
    assert!(!st.is_initial(), "a partial character is held");
    assert_eq!(MbState::from_bytes(st.to_bytes()), st, "the 8 ABI bytes carry it");
    assert_eq!(dec(&mut st, &[0xAC], U), Decoded::Char(0x20AC, 1));
    assert!(mb_compat::mbsinit(None), "NULL is initial");
    assert_eq!(size_of::<mbstate_t>(), 8);
}

#[test]
fn s6_2_c_1() {
    let mut st = MbState::INITIAL;
    assert_eq!(dec(&mut st, "€x".as_bytes(), U), Decoded::Char(0x20AC, 3), "reads only what one character needs");
}

#[test]
fn s6_2_c_1_1() {
    assert_eq!(dec(&mut init(), b"a", A), Decoded::Char(0x61, 1));
    assert_eq!(dec(&mut init(), "ß".as_bytes(), U), Decoded::Char(0xDF, 2));
}

#[test]
fn s6_2_c_1_2() {
    let mut out = 7i32;
    assert_eq!(mb_compat::mbrtowc(Some(&mut out), Some((&[][..], true)), State::Fresh(MbState::INITIAL), &Locale::C), Ok(0));
    assert_eq!(out, 0);
}

#[test]
fn s6_2_c_1_3() {
    let mut raw = [0u8; 8];
    assert_eq!(mb_compat::mbrtowc::<i32>(None, Some((&[][..], false)), State::User(&mut raw), &Locale::C), Ok(INCOMPLETE), "n = 0");
    let mut st = MbState::INITIAL;
    assert_eq!(dec(&mut st, &[0xF0, 0x9F], U), Decoded::Incomplete, "all n bytes consumed into the state");
    assert_eq!(dec(&mut st, &[0x98, 0x80], U), Decoded::Char(0x1F600, 2), "completion counts only the new bytes");
}

#[test]
fn s6_2_c_1_4() {
    let bad = [i8::from_le_bytes([0x80]), 0 as c_char];
    let r = x::mbrtowc(None, bad.as_ptr(), 1, None);
    assert_eq!((r, liella_boundary::errno::get()), (usize::MAX, 84));
    assert_eq!(dec(&mut init(), &[0xC3, 0x28], U), Decoded::Invalid);
}

#[test]
fn s6_2_c_2() {
    let mut buf = [0u8; text::MAX_ENCODED];
    assert_eq!(text::encode(0x20AC, U, &mut buf), Some(3));
    assert_eq!(&buf[..3], "€".as_bytes());
    assert_eq!(text::encode(0x20AC, A, &mut buf), None, "unrepresentable: EILSEQ");
    assert!(text::encode(0x41, A, &mut buf).unwrap() <= Locale::C.mb_cur_max());
}

#[test]
fn s6_2_c_3() {
    let input = &[0x41u8][..];
    let r1 = mb_compat::mbrlen(Some((input, false)), State::Fresh(MbState::INITIAL), &Locale::C);
    let mut out = 0i32;
    let r2 = mb_compat::mbrtowc(Some(&mut out), Some((input, false)), State::Fresh(MbState::INITIAL), &Locale::C);
    assert_eq!(r1, r2);
}

#[test]
fn s6_2_c_4() {
    // mbtowc keeps its state in the hidden slot: a NULL input resets that slot.
    mb_state::store(Slot::Mbtowc, MbState::from_bytes([1, 1, 1, 0, 1, 0, 0, 0]));
    assert!(!mb_state::load(Slot::Mbtowc).is_initial());
    assert_eq!(mb_compat::mbtowc::<i32>(None, None::<(&[u8], bool)>, Slot::Mbtowc, &Locale::C), Ok(0));
    assert!(mb_state::load(Slot::Mbtowc).is_initial());
}

#[test]
fn s6_2_i_0() {
    assert_eq!(x::__ctype_get_mb_cur_max(), Locale::C.mb_cur_max());
    for c in 0..0x80u32 {
        let mut buf = [0u8; text::MAX_ENCODED];
        assert!(text::encode(c, A, &mut buf).unwrap() <= Locale::C.mb_cur_max());
    }
    const { assert!(text::MAX_ENCODED <= 16, "within MB_LEN_MAX") };
}

#[test]
fn s6_2_g_0() {
    let mut out = [0u32; 4];
    let mut st = MbState::INITIAL;
    let r = text::mbs_to_wcs(&mut st, &[0x61, 0xE2, 0x82], Some(&mut out), U);
    assert_eq!((r.written, out[1]), (1, 0), "the partial character is not written");
    let mut b = [0u8; 2];
    let r = text::wcs_to_mbs(&[0x61u32, 0x20AC], Some(&mut b), U);
    assert_eq!((r.written, b), (1, [0x61, 0]), "a character that does not fit is not split");
}

#[test]
fn s6_2_f_1() {
    for bad in [&[0xC0u8, 0x80][..], &[0xE0, 0x80, 0x80], &[0xED, 0xA0, 0x80], &[0xF4, 0x90, 0x80, 0x80], &[0xFF], &[0x80]] {
        assert_eq!(dec(&mut init(), bad, U), Decoded::Invalid, "{bad:02x?}");
    }
    let mut b = [0u8; 4];
    assert_eq!(text::encode(0xD800, U, &mut b), None, "surrogates");
    assert_eq!(text::encode(0x110000, U, &mut b), None, "above U+10FFFF");
}

#[test]
fn s6_2_f_2() {
    let mut raw = [0u8; 8];
    let mut sink = [0u8; 4];
    assert_eq!(mb_compat::c16rtomb(Some(&mut sink[..]), 0xD83D, State::User(&mut raw), &Locale::C), Ok(0));
    assert!(!mb_compat::mbsinit(Some(&raw)));
    let none: Option<&mut [u8]> = None;
    let _ = mb_compat::c16rtomb(none, 0, State::User(&mut raw), &Locale::C);
    assert!(mb_compat::mbsinit(Some(&raw)), "a NULL output resets the state");
    let mut st = mbstate_t::default();
    assert_eq!(x::mbrtowc(None, core::ptr::null(), 0, Some(&mut st)), 0);
}

#[test]
fn s6_2_f_3() {
    assert_eq!(mb_compat::INCOMPLETE, usize::MAX - 1);
    assert_eq!(mb_compat::CONTINUED, usize::MAX - 2);
    let mut st = MbState::INITIAL;
    assert_eq!(text::decode_utf16(&mut st, "😀".as_bytes(), U), Decoded::Char(0xD83D, 4));
    assert_eq!(text::decode_utf16(&mut st, &[], U), Decoded::Continuation(0xDE00), "the (size_t)-3 step");
    let mut st = MbState::INITIAL;
    assert_eq!(text::encode_utf16(&mut st, 0xD83D, U, &mut [0; 4]), Fed::Stored);
}

#[test]
fn s6_3_c_0() {
    let input = "añb".as_bytes();
    let mut out = [0u32; 8];
    let r = text::mbs_to_wcs(&mut init(), input, Some(&mut out), U);
    assert_eq!((r.written, r.consumed), (3, 4));
    let mut two = [0u32; 2];
    let r = text::mbs_to_wcs(&mut init(), input, Some(&mut two), U);
    assert_eq!((r.written, r.consumed), (2, 3), "at most len units; the position is reported");
    let r = text::mbs_to_wcs::<u32>(&mut init(), input, None, U);
    assert_eq!(r.written, 3, "measuring");
    let r = text::mbs_to_wcs(&mut init(), &input[..1], Some(&mut out), U);
    assert_eq!(r.written, 1, "an input limit");
}

#[test]
fn s6_5_c_0() {
    assert_eq!(text::display_width(u32::from('a'), &Locale::C), 1);
    assert_eq!(mb_compat::wcswidth(&[0x61, 0x62, 0x63], &Locale::C), 3);
}

#[test]
fn s6_5_c_3() {
    for c in 0x20..0x7f {
        assert_eq!(mb_compat::wcwidth(c, &Locale::C), 1);
    }
}

#[test]
fn s6_5_c_4() {
    assert_eq!(mb_compat::wcwidth(0x1b, &Locale::C), -1);
    assert_eq!(mb_compat::wcswidth(&[0x61, 0x7f], &Locale::C), -1, "any non-printable character");
    assert_eq!(mb_compat::wcwidth(0, &Locale::C), 0, "but the null character is 0");
}

/// One conversion of `input` with a fresh descriptor `to <- from` under `loc`: the outcome and
/// the output bytes.
fn iconv_once(to: &str, from: &str, input: &[u8], room: usize, loc: &Locale) -> (Outcome, Vec<u8>) {
    let mut c = open(to, from).unwrap();
    let mut out = vec![0u8; room];
    let o = c.convert(input, &mut out, loc);
    out.truncate(o.written);
    (o, out)
}

/// A descriptor for built-in names (the charmaps are tested through the products, D-6.8).
fn open(to: &str, from: &str) -> Option<Conv> {
    let (t, f) = (iconv::parse_name(to.as_bytes()), iconv::parse_name(from.as_bytes()));
    Some(Conv { from: iconv::builtin(f.name)?, to: iconv::builtin(t.name)?, translit: t.translit, ignore: t.ignore, state: IconvState::default() })
}

const C_UTF8: Locale = Locale::c_utf8(b"C.UTF-8");

#[test]
fn s6_4_c_1() {
    for (a, b) in [("UTF-8", "utf-8"), ("UTF-16LE", "utf-16le"), ("ISO-8859-1", "latin1"), ("ISO-8859-1", "L1"), ("ASCII", "ANSI_X3.4-1968"), ("UCS-4", "ISO-10646/UCS4")] {
        assert_eq!(iconv::builtin(a.as_bytes()), iconv::builtin(b.as_bytes()), "{b} names {a}");
        assert!(iconv::builtin(b.as_bytes()).is_some());
    }
    let s = iconv::parse_name(b"ascii//translit");
    assert_eq!((s.name, s.translit, s.ignore), (&b"ascii"[..], true, false), "suffixes on the target, any case");
    let s = iconv::parse_name(b"ASCII//BOGUS");
    assert_eq!((s.name, s.translit, s.ignore), (&b"ASCII"[..], false, false), "unknown suffixes are ignored");
    assert!(iconv::builtin(b"NO-SUCH").is_none());
}

#[test]
fn s6_4_c_1_1() {
    let t = "x€œ“q”一y".as_bytes();
    let (o, out) = iconv_once("ASCII//TRANSLIT", "UTF-8", t, 64, &Locale::C);
    assert_eq!((o.result, out.as_slice()), (Ok(5), &b"xEURoe\"q\"?y"[..]), "approximations, then '?'");
    let (_, out) = iconv_once("ASCII//TRANSLIT", "UTF-8", "ä½ﬁ".as_bytes(), 64, &C_UTF8);
    assert_eq!(out, b"a 1/2 fi", "C.UTF-8: decompositions without marks (D-6.9)");
    let (_, out) = iconv_once("ASCII//TRANSLIT", "UTF-8", "äe\u{301}".as_bytes(), 64, &Locale::C);
    assert_eq!(out, b"?e?", "C: compatibility decompositions only, a mark is '?'");
}

#[test]
fn s6_4_c_1_2() {
    let (o, out) = iconv_once("ASCII//IGNORE", "UTF-8", "xé一y".as_bytes(), 64, &Locale::C);
    assert_eq!((o.result, out.as_slice(), o.read), (Err(Errno::EILSEQ), &b"xy"[..], 7), "dropped, all input read");
}

#[test]
fn s6_4_c_1_3() {
    for to in ["ASCII//TRANSLIT//IGNORE", "ASCII//IGNORE//TRANSLIT", "ASCII//TRANSLIT,IGNORE"] {
        let (o, out) = iconv_once(to, "UTF-8", "x\u{FF}é\u{4E00}y".as_bytes(), 64, &Locale::C);
        assert_eq!((o.result, out.as_slice()), (Ok(3), &b"x???y"[..]), "{to}: transliterated, nothing left to drop");
        let (o, out) = iconv_once(to, "UTF-8", b"a\xffz", 64, &Locale::C);
        assert_eq!((o.result, out.as_slice()), (Err(Errno::EILSEQ), &b"az"[..]), "{to}: invalid input dropped");
    }
}

#[test]
fn s6_4_c_2() {
    let (o, out) = iconv_once("UTF-16LE", "UTF-8", "Aé😀".as_bytes(), 64, &Locale::C);
    assert_eq!((o.result, o.read, o.written), (Ok(0), 7, 8), "everything converted, nothing irreversible");
    assert_eq!(out, [0x41, 0, 0xE9, 0, 0x3D, 0xD8, 0, 0xDE]);
    let (o, _) = iconv_once("ASCII//TRANSLIT", "UTF-8", "é€".as_bytes(), 64, &Locale::C);
    assert_eq!(o.result, Ok(2), "two irreversible conversions");
}

#[test]
fn s6_4_c_3() {
    // No built-in codec has shift states: the flush writes nothing, and resets (the BOM again).
    let mut c = open("UTF-16", "UTF-8").unwrap();
    let mut out = [0u8; 16];
    assert_eq!(c.convert(b"A", &mut out, &Locale::C).written, 4, "BOM and A");
    assert_eq!(c.convert(b"A", &mut out, &Locale::C).written, 2, "no BOM the second time");
    c.reset();
    let o = c.convert(b"A", &mut out, &Locale::C);
    assert_eq!((o.written, &out[..4]), (4, &[0xFF, 0xFE, 0x41, 0][..]), "after the reset, the BOM again");
}

#[test]
fn s6_4_c_4() {
    let mut c = open("UTF-8", "UTF-16").unwrap();
    let mut out = [0u8; 16];
    assert_eq!(c.convert(&[0xFE, 0xFF, 0, 0x41], &mut out, &Locale::C).written, 1, "a BE BOM selects big-endian");
    assert_eq!(&out[..1], b"A");
    assert_eq!(c.state.order, Some(iconv::Order::Be));
    c.reset();
    assert_eq!(c.state, IconvState::default(), "the initial state");
    let o = c.convert(&[0x41, 0], &mut out, &Locale::C);
    assert_eq!(&out[..o.written], b"A", "no BOM: little-endian (the reference library's default)");
}

#[test]
fn s6_4_c_5() {
    // The descriptor is plain data here; the exports free its heap block (symbol tests).
    let cm: &'static Charmap = Box::leak(Box::new(Charmap::parse(b"CHARMAP\n<U0041> /x41\nEND CHARMAP\n").unwrap()));
    let c = Conv { from: iconv::Codec::Charmap(text::CharmapRef(cm)), ..open("UTF-8", "ASCII").unwrap() };
    let w = c.to_words();
    assert_eq!(Conv::from_words(w, &|_| cm), Some(c), "the words carry the descriptor");
    assert_eq!(Conv::from_words([99, 0, 1, 0, 0, 0], &|_| cm), None, "a bad tag");
}

#[test]
fn s6_4_f_1() {
    assert!(open("UTF-8", "NO-SUCH").is_none());
    assert!(iconv::builtin(b"").is_none(), "the empty name is the locale's codeset, resolved by the exports");
    assert_eq!(iconv::of_locale(&Locale::C), iconv::Codec::Ascii);
    assert_eq!(iconv::of_locale(&C_UTF8), iconv::Codec::Utf8);
}

#[test]
fn s6_4_f_2() {
    let (o, _) = iconv_once("ASCII", "UTF-8", "é".as_bytes(), 64, &Locale::C);
    assert_eq!(o.result, Err(Errno::EILSEQ), "(size_t)-1 with an errno");
}

#[test]
fn s6_4_f_2_1() {
    let (o, out) = iconv_once("UTF-16LE", "UTF-8", b"ABC", 5, &Locale::C);
    assert_eq!((o.result, o.read, out.as_slice()), (Err(Errno::E2BIG), 2, &[0x41, 0, 0x42, 0][..]), "stops before the unit that does not fit");
    let (o, _) = iconv_once("UTF-16", "UTF-8", b"A", 1, &Locale::C);
    assert_eq!((o.result, o.written), (Err(Errno::E2BIG), 0), "not even the BOM fits");
}

#[test]
fn s6_4_f_2_2() {
    let (o, out) = iconv_once("UTF-16LE", "UTF-8", b"a\xffb", 64, &Locale::C);
    assert_eq!((o.result, o.read, out.as_slice()), (Err(Errno::EILSEQ), 1, &[0x61, 0][..]), "*in left at the invalid byte");
    let (o, _) = iconv_once("UTF-8", "UTF-16BE", &[0xDC, 0, 0, 0x41], 64, &Locale::C);
    assert_eq!((o.result, o.read), (Err(Errno::EILSEQ), 0), "a lone low surrogate");
    let (o, _) = iconv_once("UTF-8", "UCS-2BE", &[0xD8, 0x3D, 0xDE, 0], 64, &Locale::C);
    assert_eq!(o.result, Err(Errno::EILSEQ), "UCS-2 has no surrogates");
    let (o, _) = iconv_once("UTF-16LE", "UCS-4", &[0, 0x11, 0, 0], 64, &Locale::C);
    assert_eq!(o.result, Err(Errno::EILSEQ), "above U+10FFFF");
    let (o, out) = iconv_once("UTF-8", "UCS-4", &[0, 0x11, 0, 0], 64, &Locale::C);
    assert_eq!((o.result, out.as_slice()), (Ok(0), &[0xF4, 0x90, 0x80, 0x80][..]), "UTF-8 writes 31-bit values (D-6.9)");
}

#[test]
fn s6_4_f_2_3() {
    let (o, out) = iconv_once("UTF-16LE", "UTF-8", b"a\xc3", 64, &Locale::C);
    assert_eq!((o.result, o.read, out.as_slice()), (Err(Errno::EINVAL), 1, &[0x61, 0][..]), "the partial character stays in the input");
    let (o, _) = iconv_once("UTF-8", "UTF-16BE", &[0xD8, 0x3D], 64, &Locale::C);
    assert_eq!(o.result, Err(Errno::EINVAL), "a high surrogate at the end");
}

#[test]
fn s6_4_f_3() {
    let (o, out) = iconv_once("ASCII//IGNORE", "UTF-8", b"x\xffy", 64, &Locale::C);
    assert_eq!((o.result, o.read, out.as_slice()), (Err(Errno::EILSEQ), 3, &b"xy"[..]), "skipped, -1/EILSEQ at the end");
    let (o, _) = iconv_once("ASCII//IGNORE", "UTF-8", "x一yz".as_bytes(), 2, &Locale::C);
    assert_eq!((o.result, o.read), (Err(Errno::E2BIG), 5), "E2BIG still reports first");
    let (o, _) = iconv_once("ASCII//IGNORE", "UTF-8", "x一y\u{e9}".as_bytes()[..6].as_ref(), 64, &Locale::C);
    assert_eq!((o.result, o.read), (Err(Errno::EILSEQ), 5), "an incomplete end after a drop is EILSEQ");
}

#[test]
fn s6_4_g_0() {
    // Byte by byte gives what one call gives.
    let input = "Aé😀z".as_bytes();
    let whole = iconv_once("UTF-16", "UTF-8", input, 64, &Locale::C).1;
    let mut c = open("UTF-16", "UTF-8").unwrap();
    let (mut out, mut pos) = (Vec::new(), 0);
    for fed in 1..=input.len() {
        let mut buf = [0u8; 16];
        let o = c.convert(&input[pos..fed], &mut buf, &Locale::C);
        out.extend_from_slice(&buf[..o.written]);
        pos += o.read;
        assert!(o.result.is_ok() || o.result == Err(Errno::EINVAL));
    }
    assert_eq!((out, pos), (whole, input.len()));
}

#[test]
fn s6_5_c_1() {
    assert_eq!(mb_compat::wcwidth(0x301, &C_UTF8), 0, "a combining mark");
    assert_eq!(mb_compat::wcwidth(0x200B, &C_UTF8), 0, "zero width space");
    assert_eq!(mb_compat::wcwidth(0, &C_UTF8), 0);
}

#[test]
fn s6_5_c_2() {
    assert_eq!(mb_compat::wcwidth(0x4E00, &C_UTF8), 2, "CJK");
    assert_eq!(mb_compat::wcwidth(0xFF21, &C_UTF8), 2, "fullwidth A");
    assert_eq!(mb_compat::wcwidth(0x1F600, &C_UTF8), 2, "emoji");
    assert_eq!(mb_compat::wcswidth(&[0x61, 0x4E00], &C_UTF8), 3);
}

// ------------------------------------------------------------------------------ clauses after a flag line (2026-10-04)

#[test]
fn s6_2_c_5() {
    // btowc/wctob: single bytes of the locale's encoding (the C locale: ASCII), else WEOF/EOF.
    assert_eq!(x::btowc(i32::from(b'A')), 65);
    assert_eq!(x::btowc(-1), u32::MAX, "EOF gives WEOF");
    assert_eq!(x::wctob(65), i32::from(b'A'));
    assert_eq!(x::wctob(0x20AC), -1, "not a single byte: EOF");
}

fn mbs() -> mbstate_t {
    mbstate_t { bytes: [0; 8] }
}

#[test]
fn s6_2_c_6() {
    // mbrtoc16/c16rtomb convert to and from UTF-16 code units (C.UTF-8 here: a C test locale
    // with UTF-8; in the C locale the bytes are ASCII and the units the same).
    let mut st = mbs();
    let mut u = 0u16;
    assert_eq!(x::mbrtoc16(Some(&mut u), c"A".as_ptr(), 2, Some(&mut st)), 1);
    assert_eq!(u, 65);
    let mut out = [0 as c_char; 8];
    assert_eq!(x::c16rtomb(out.as_mut_ptr(), 66, Some(&mut st)), 1);
    assert_eq!(out[0] as u8, b'B');
}

/// In a child with the C.UTF-8 locale: the result of `f`.
fn utf8<T: std::fmt::Debug + PartialEq>(f: impl FnOnce() -> T, want: T) {
    use liella_boundary::posix::s07_locale as lx;
    let c = std::ffi::CString::new("C.UTF-8").unwrap();
    let loc = lx::newlocale(0x1fbf, c.as_ptr(), core::ptr::null_mut());
    assert!(!loc.is_null());
    let old = lx::uselocale(loc);
    let got = f();
    lx::uselocale(old);
    lx::freelocale(loc);
    assert_eq!(got, want);
}

#[test]
fn s6_2_c_6_1() {
    // A character outside the BMP: two calls; the second gives the low surrogate and returns
    // (size_t)-3, consuming nothing.
    utf8(
        || {
            let mut st = mbs();
            let (mut hi, mut lo) = (0u16, 0u16);
            let s = "\u{1F600}".as_bytes();
            let a = x::mbrtoc16(Some(&mut hi), s.as_ptr().cast(), s.len(), Some(&mut st));
            let b = x::mbrtoc16(Some(&mut lo), s.as_ptr().cast(), s.len(), Some(&mut st));
            (a, hi, b, lo)
        },
        (4, 0xD83D, usize::MAX - 2, 0xDE00),
    );
}

#[test]
fn s6_2_c_6_2() {
    // c16rtomb keeps a high surrogate in the state and writes the character with the low one.
    utf8(
        || {
            let mut st = mbs();
            let mut out = [0 as c_char; 8];
            let a = x::c16rtomb(out.as_mut_ptr(), 0xD83D, Some(&mut st));
            let b = x::c16rtomb(out.as_mut_ptr(), 0xDE00, Some(&mut st));
            (a, b, out[..4].iter().map(|&c| c as u8).collect::<Vec<_>>())
        },
        (0, 4, "\u{1F600}".as_bytes().to_vec()),
    );
}

#[test]
fn s6_2_c_7() {
    // mbrtoc32/c32rtomb: code points; mbrtoc8/c8rtomb: one UTF-8 unit per call.
    utf8(
        || {
            let mut st = mbs();
            let mut c = 0u32;
            let s = "é".as_bytes();
            let n = x::mbrtoc32(Some(&mut c), s.as_ptr().cast(), s.len(), Some(&mut st));
            let mut out = [0 as c_char; 8];
            let w = x::c32rtomb(out.as_mut_ptr(), 0x20AC, Some(&mut st));
            let mut u = 0u8;
            let mut st8 = mbs();
            let m1 = x::mbrtoc8(Some(&mut u), s.as_ptr().cast(), s.len(), Some(&mut st8));
            let first = u;
            let m2 = x::mbrtoc8(Some(&mut u), s.as_ptr().cast(), s.len(), Some(&mut st8));
            (n, c, w, out[..3].iter().map(|&b| b as u8).collect::<Vec<_>>(), m1, first, m2, u)
        },
        (2, 0xE9, 3, "€".as_bytes().to_vec(), 2, 0xC3, usize::MAX - 2, 0xA9),
    );
}

/// AUD-M4: a single-character form reads its input one byte at a time and stops where the
/// character does: the largest index read is the character's last byte, however long the text.
#[test]
fn single_character_forms_read_only_the_character() {
    // Trace: s6_2_c_1 ("at most `n` bytes": only the character's bytes are read); D-6.10.
    struct Counted<'a>(&'a [u8], core::cell::Cell<usize>);
    impl mb_compat::MbInput for Counted<'_> {
        fn byte(&self, i: usize) -> Option<u8> {
            self.1.set(self.1.get().max(i + 1));
            self.0.get(i).copied()
        }
    }
    for (loc, ch, want) in [(&Locale::C, &b"A"[..], 1), (&C_UTF8, b"A", 1), (&C_UTF8, "\u{e9}".as_bytes(), 2), (&C_UTF8, "\u{20ac}".as_bytes(), 3), (&C_UTF8, "\u{1f600}".as_bytes(), 4)] {
        let mut text = ch.to_vec();
        text.resize(1 << 16, b'x');
        let input = Counted(&text, core::cell::Cell::new(0));
        let mut out = 0u32;
        assert_eq!(mb_compat::mbrtowc(Some(&mut out), Some(&input), State::Fresh(MbState::INITIAL), loc), Ok(want), "{ch:?}");
        assert_eq!(input.1.get(), want, "{ch:?}: bytes read");
    }
}

/// AUD-FIX: the UTF-8 fast path (lead byte gives the length, one decode) answers and reads
/// exactly as the per-byte definition does: decode a window grown one byte at a time until the
/// decoder stops asking for more. Every lead byte, continuation candidates at every range
/// boundary, NUL, and every limit `n` = 0..=4; all four single-character forms.
#[test]
fn utf8_fast_path_matches_the_per_byte_definition() {
    // Trace: s6_2_c_1 (decodes one character from at most `n` bytes), s6_2_c_3; D-6.10.
    type Dec = fn(&mut MbState, &[u8], Encoding) -> Decoded;
    struct Counted<'a>(&'a [u8], core::cell::Cell<usize>);
    impl mb_compat::MbInput for Counted<'_> {
        fn byte(&self, i: usize) -> Option<u8> {
            self.1.set(self.1.get().max(i + 1));
            self.0.get(i).copied()
        }
    }
    // The definition: (answer, value, bytes asked for, state left).
    fn per_byte(text: &[u8], dec: Dec) -> (Result<usize, Errno>, Option<u32>, usize, [u8; 8]) {
        let (mut k, mut asked, mut open) = (0usize, 0usize, true);
        loop {
            if open {
                asked = asked.max(k + 1);
                match text.get(k) {
                    Some(&b) if k < text::MAX_ENCODED || b == 0 => {
                        k += 1;
                        open = b != 0 && k <= text::MAX_ENCODED;
                    }
                    _ => open = false,
                }
            }
            let mut m = MbState::INITIAL;
            let d = dec(&mut m, &text[..k], U);
            if !(open && d == Decoded::Incomplete) {
                let (r, v) = match d {
                    Decoded::Char(c, used) => (Ok(used), Some(c)),
                    Decoded::Null => (Ok(0), Some(0)),
                    Decoded::Incomplete => (Ok(INCOMPLETE), None),
                    Decoded::Invalid => (Err(Errno::EILSEQ), None),
                    Decoded::Continuation(u) => (Ok(usize::MAX - 2), Some(u)),
                };
                return (r, v, asked, m.to_bytes());
            }
        }
    }
    let set = [0x00u8, 0x41, 0x7f, 0x80, 0x8f, 0x90, 0x9f, 0xa0, 0xbf, 0xc0, 0xff];
    let mut cases = 0usize;
    for b0 in 0..=255u8 {
        for &b1 in &set {
            for &b2 in &set {
                for &b3 in &set {
                    let mut text = vec![b0, b1, b2, b3];
                    text.resize(16, b'x');
                    let limits: &[usize] = if (0xc2..=0xf4).contains(&b0) { &[0, 1, 2, 3, 4, 16] } else { &[16] };
                    for &n in limits {
                        let t = &text[..n];
                        let decs: [(Dec, &str); 3] = [(text::decode, "mbrtowc"), (text::decode_utf16, "mbrtoc16"), (text::decode_utf8_units, "mbrtoc8")];
                        for (dec, name) in decs {
                            let (want, wv, wasked, wstate) = per_byte(t, dec);
                            let mut st = [0u8; 8];
                            let input = Counted(t, core::cell::Cell::new(0));
                            let (got, gv) = match name {
                                "mbrtowc" => {
                                    let mut o = 0u32;
                                    let r = mb_compat::mbrtowc(Some(&mut o), Some(&input), State::User(&mut st), &C_UTF8);
                                    (r, r.ok().filter(|&r| r != INCOMPLETE).map(|_| o))
                                }
                                "mbrtoc16" => {
                                    let mut o = 0u16;
                                    let r = mb_compat::mbrtoc16(Some(&mut o), Some(&input), State::User(&mut st), &C_UTF8);
                                    (r, r.ok().filter(|&r| r != INCOMPLETE).map(|_| u32::from(o)))
                                }
                                _ => {
                                    let mut o = 0u8;
                                    let r = mb_compat::mbrtoc8(Some(&mut o), Some(&input), State::User(&mut st), &C_UTF8);
                                    (r, r.ok().filter(|&r| r != INCOMPLETE).map(|_| u32::from(o)))
                                }
                            };
                            assert_eq!(got, want, "{name} {t:02x?}");
                            assert_eq!(gv, wv, "{name} {t:02x?}: value");
                            assert_eq!(input.1.get(), wasked, "{name} {t:02x?}: bytes read");
                            assert_eq!(st, wstate, "{name} {t:02x?}: state");
                            if name == "mbrtowc" {
                                let len = Counted(t, core::cell::Cell::new(0));
                                assert_eq!(mb_compat::mbrlen(Some(&len), State::Fresh(MbState::INITIAL), &C_UTF8), want, "mbrlen {t:02x?}");
                                assert_eq!(len.1.get(), wasked, "mbrlen {t:02x?}: bytes read");
                            }
                            cases += 1;
                        }
                    }
                }
            }
        }
    }
    assert!(cases > 1_000_000, "{cases}");
}
