// Specified from glibc-functional-spec.md §8. Generated stubs were replaced by real tests of the
// SAME NAME (see crates/spectest); `cargo xtask stubs` now only checks coverage.

//! Clause tests for spec §8 (44 clauses).
//!
//! The parsers and formatters are tested in `liella::num` and their C shapes in
//! `liella_legacy::num_compat`; §8.8 (the byte-order functions, §15 exports) through the exports.

#![no_builtins] // calls to libc names (labs, strlen, ...) reach Liella's export, not LLVM's folding (OPAQUE-AUDIT)
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, clippy::float_cmp, missing_docs)]

use liella::num::format::{format_float, Bounded, Case, Spec, Style};
use liella::num::parse::{parse_float, parse_i64, parse_i64_short, parse_u64, Dialect, FloatRange, Status};
use liella::num::{self, Float, Format, Rounding, StdBit};
use liella_kernel::Errno;
use liella_legacy::mb_compat::Local;
use liella_legacy::num_compat as nc;

const N: Rounding = Rounding::Nearest;

fn d(s: &str) -> f64 {
    f64::from_bits(parse_float(s.as_bytes(), b".", Format::F64, N, Dialect::C99).value.pack(Format::F64) as u64)
}

fn fmt(x: f64, style: Style, precision: Option<usize>) -> String {
    let mut buf = [0u8; 64];
    let mut b = Bounded::new(&mut buf);
    format_float(&mut b, nc::from_f64(x), Format::F64, Spec { style, case: Case::Lower, precision, alternate: false }, b".", N);
    let n = b.stored();
    String::from_utf8(buf[..n].to_vec()).unwrap()
}

#[test]
fn s8_n_1() {
    // Pure: no Sys anywhere in the conversions.
    assert_eq!(parse_i64(b"42", 10, Dialect::C99).value, 42);
    assert_eq!(fmt(0.5, Style::Fixed, Some(1)), "0.5");
}

#[test]
fn s8_n_2() {
    let comma = parse_float(b"1,5", b",", Format::F64, N, Dialect::C99);
    assert_eq!((comma.used, f64::from_bits(comma.value.pack(Format::F64) as u64)), (3, 1.5), "the radix is a parameter");
    let mut buf = [0u8; 8];
    let mut b = Bounded::new(&mut buf);
    format_float(&mut b, nc::from_f64(1.5), Format::F64, Spec { style: Style::Fixed, case: Case::Lower, precision: Some(1), alternate: false }, b",", N);
    assert_eq!(&buf[..3], b"1,5");
}

#[test]
fn s8_1_c_0() {
    short_path_agrees();
    let p = parse_i64(b"123abc", 10, Dialect::C99);
    assert_eq!((p.value, p.used, p.status), (123, 3, Status::Ok), "the longest initial integer, and where it stopped");
}

#[test]
fn s8_1_i_1() {
    assert_eq!(parse_i64(b" \t\n-17", 10, Dialect::C99).value, -17, "white space, then a sign");
    assert_eq!(parse_i64(b"+17", 10, Dialect::C99).value, 17);
    assert_eq!(parse_i64(b"- 1", 10, Dialect::C99).status, Status::NoDigits, "nothing between the sign and the digits");
}

#[test]
fn s8_1_i_2() {
    assert_eq!(parse_i64(b"zz", 36, Dialect::C99).value, 35 * 36 + 35);
    assert_eq!(parse_i64(b"11", 2, Dialect::C99).value, 3);
    assert_eq!(parse_i64(b"1", 1, Dialect::C99).status, Status::InvalidBase);
    assert_eq!(parse_i64(b"1", 37, Dialect::C99).status, Status::InvalidBase);
}

#[test]
fn s8_1_i_2_1() {
    assert_eq!(parse_i64(b"0x1F", 0, Dialect::C99).value, 31);
    assert_eq!(parse_i64(b"0X1f", 16, Dialect::C99).value, 31, "the prefix is also accepted with base 16");
}

#[test]
fn s8_1_i_2_2() {
    assert_eq!(parse_i64(b"017", 0, Dialect::C99).value, 15);
    assert_eq!(parse_i64(b"019", 0, Dialect::C99).value, 1, "an octal number stops at 9");
}

#[test]
fn s8_1_i_2_3() {
    assert_eq!(parse_i64(b"0b101", 0, Dialect::C23).value, 5);
    assert_eq!(parse_i64(b"0B11", 2, Dialect::C23).value, 3);
    assert_eq!(parse_i64(b"0b101", 0, Dialect::C99).value, 0, "not before C23");
}

#[test]
fn s8_1_i_2_4() {
    assert_eq!(parse_i64(b"0099", 10, Dialect::C99).value, 99);
    assert_eq!(parse_i64(b"99", 0, Dialect::C99).value, 99);
}

#[test]
fn s8_1_i_3() {
    let text = b"  12xyz";
    let mut end = Local(0);
    let (v, e) = nc::strtol(text, 1000, Some(&mut end), 10, Dialect::C99);
    assert_eq!((v, e, end.0), (12, None, 1004), "the address after the last consumed unit");
    let (v, _) = nc::strtol(text, 1000, None::<&mut Local>, 10, Dialect::C99);
    assert_eq!(v, 12, "the end pointer is optional");
    let wide: Vec<i32> = "7z".chars().map(|c| c as i32).collect();
    let mut end = Local(0);
    nc::strtol(&wide[..], 2000, Some(&mut end), 10, Dialect::C99);
    assert_eq!(end.0, 2004, "wide units are 4 bytes");
}

#[test]
fn s8_1_g_1() {
    let mut end = Local(0);
    assert_eq!(nc::strtol(b"-9223372036854775808", 0, Some(&mut end), 10, Dialect::C99), (i64::MIN, None));
    assert_eq!(end.0, 20);
}

#[test]
fn s8_1_g_2() {
    let mut end = Local(7);
    assert_eq!(nc::strtol(b"  x", 100, Some(&mut end), 10, Dialect::C99), (0, None));
    assert_eq!(end.0, 100, "no digits: the end is the input");
}

#[test]
fn s8_1_g_3() {
    assert_eq!(nc::strtol(b"9223372036854775808", 0, None::<&mut Local>, 10, Dialect::C99), (i64::MAX, Some(Errno::ERANGE)));
    assert_eq!(nc::strtol(b"-9223372036854775809", 0, None::<&mut Local>, 10, Dialect::C99), (i64::MIN, Some(Errno::ERANGE)));
    assert_eq!(nc::strtoul(b"18446744073709551616", 0, None::<&mut Local>, 10, Dialect::C99), (u64::MAX, Some(Errno::ERANGE)));
    assert_eq!(nc::strtoul(b"-18446744073709551616", 0, None::<&mut Local>, 10, Dialect::C99), (u64::MAX, Some(Errno::ERANGE)), "overflow stays ULONG_MAX whatever the sign");
}

#[test]
fn s8_1_g_4() {
    assert_eq!(nc::strtoul(b"-1", 0, None::<&mut Local>, 10, Dialect::C99), (u64::MAX, None));
    assert_eq!(nc::strtoul(b"-2", 0, None::<&mut Local>, 10, Dialect::C99).0, u64::MAX - 1);
    assert!(parse_u64(b"-1", 10, Dialect::C99).negated);
}

#[test]
fn s8_1_f_0() {
    let mut end = Local(55);
    assert_eq!(nc::strtol(b"12", 0, Some(&mut end), 1, Dialect::C99), (0, Some(Errno::EINVAL)));
    assert_eq!(end.0, 55, "the end pointer is not touched");
    assert_eq!(nc::strtol(b"12", 0, None::<&mut Local>, -3, Dialect::C99).1, Some(Errno::EINVAL));
    assert_eq!(nc::atoi(b"99999999999"), 99_999_999_999i64 as i32, "atoi: no error reporting");
}

#[test]
fn s8_2_c_0() {
    assert_eq!(d("0.1"), 0.1);
    assert_eq!(d("2.2250738585072011e-308"), f64::from_bits(0x000F_FFFF_FFFF_FFFF), "correctly rounded near the subnormal boundary");
    let up = parse_float(b"0.1", b".", Format::F64, Rounding::Up, Dialect::C99).value.pack(Format::F64) as u64;
    let down = parse_float(b"0.1", b".", Format::F64, Rounding::Down, Dialect::C99).value.pack(Format::F64) as u64;
    assert_eq!(up, down + 1, "in the current rounding mode");
}

#[test]
fn s8_2_c_1() {
    assert_eq!(d("1.5e3"), 1500.0);
    assert_eq!(d("1.5E-3"), 0.0015);
    assert_eq!(d(".5"), 0.5);
    assert_eq!(d("5."), 5.0);
}

#[test]
fn s8_2_c_2() {
    assert_eq!(d("0x1.8p1"), 3.0);
    assert_eq!(d("0X.8P0"), 0.5);
    assert_eq!(d("0x1p-1074"), f64::from_bits(1));
    let p = parse_float(b"0x", b".", Format::F64, N, Dialect::C99);
    assert_eq!(p.used, 1, "\"0x\" alone is the number 0");
}

#[test]
fn s8_2_c_3() {
    assert_eq!(d("INF"), f64::INFINITY);
    assert_eq!(d("-infinity"), f64::NEG_INFINITY);
    let p = parse_float(b"infinit", b".", Format::F64, N, Dialect::C99);
    assert_eq!(p.used, 3, "the longest form that matches");
}

#[test]
fn s8_2_c_4() {
    assert!(d("nan").is_nan());
    let bits = d("nan(0x5)").to_bits();
    assert_eq!(bits & 0xF_FFFF_FFFF_FFFF, 0x8_0000_0000_0005, "the payload sets the mantissa bits (quiet)");
    let p = parse_float(b"nan(abc_9)", b".", Format::F64, N, Dialect::C99);
    assert_eq!(p.used, 10);
}

#[test]
fn s8_2_g_0() {
    let long = format!("0.{}1", "0".repeat(5000));
    assert_eq!(d(&long), 0.0, "any length");
    let half = "1.00000000000000011102230246251565404236316680908203125";
    assert_eq!(d(half), 1.0, "an exact midpoint rounds to even");
    let above = format!("{half}{}1", "0".repeat(400));
    assert_eq!(d(&above), 1.0000000000000002, "a digit far beyond breaks the tie");
}

#[test]
fn s8_2_f_1() {
    let p = parse_float(b"1e400", b".", Format::F64, N, Dialect::C99);
    assert_eq!((p.range, f64::from_bits(p.value.pack(Format::F64) as u64)), (FloatRange::Overflow, f64::INFINITY));
    assert_eq!(nc::strtod(b"-1e400", 0, None::<&mut Local>, N, b"."), (f64::NEG_INFINITY, Some(Errno::ERANGE)));
}

#[test]
fn s8_2_f_2() {
    let (v, e) = nc::strtod(b"4e-320", 0, None::<&mut Local>, N, b".");
    assert_eq!((v.to_bits(), e), (8096, Some(Errno::ERANGE)), "a subnormal result");
    assert_eq!(nc::strtod(b"1e-400", 0, None::<&mut Local>, N, b"."), (0.0, Some(Errno::ERANGE)));
}

#[test]
fn s8_2_f_3() {
    let mut end = Local(9);
    assert_eq!(nc::strtod(b"abc", 300, Some(&mut end), N, b"."), (0.0, None));
    assert_eq!(end.0, 300);
}

#[test]
fn s8_3_c_0() {
    let wide: Vec<i32> = " -0x10".chars().map(|c| c as i32).collect();
    assert_eq!(nc::strtol(&wide[..], 0, None::<&mut Local>, 0, Dialect::C99), (-16, None));
    let wf: Vec<i32> = "2.5".chars().map(|c| c as i32).collect();
    assert_eq!(nc::strtod(&wf[..], 0, None::<&mut Local>, N, &[0x2E]), (2.5, None));
    let bad: Vec<i32> = vec![0x661, 0x662];
    assert_eq!(nc::strtol(&bad[..], 0, None::<&mut Local>, 10, Dialect::C99).0, 0, "only ASCII digits");
}

#[test]
fn s8_4_c_1() {
    let mut b = [0u8; 8];
    assert_eq!(nc::strfrom(&mut b, b"%.2e", nc::from_f64(1234.5), Format::F64, N), Ok(8));
    assert_eq!(&b[..7], b"1.23e+0", "at most n bytes, terminated");
    assert_eq!(b[7], 0);
    assert!(nc::strfrom_spec(b"%10f").is_none(), "no width");
    assert!(nc::strfrom_spec(b"%#g").is_none(), "no flags");
    assert!(nc::strfrom_spec(b"%Lf").is_none(), "no length modifier");
    assert!(nc::strfrom_spec(b"%.f").is_some(), "an empty precision is 0");
}

#[test]
fn s8_4_c_2() {
    let mut out = [0u8; 16];
    let c = nc::ecvt(nc::from_f64(-0.0123), 3, N, &mut out);
    assert_eq!((&out[..3], c.decpt, c.neg), (&b"123"[..], -1, true), "digits only; point and sign apart");
    let c = nc::fcvt(nc::from_f64(2.675), 2, N, &mut out);
    assert_eq!((&out[..3], c.decpt), (&b"267"[..], 1), "ndigit digits after the point, from the exact binary value");
    // ndigit is reduced (by the C entry points) to the limit the precision gives: 17 for double,
    // 21 for long double
    // (spec corrected 2026-10-04: ecvt(3) and the reference agree; the digits stay exact).
    let mut big = [0u8; 64];
    let (mut dp, mut sg) = (0, 0);
    assert_eq!(nc::cvt_r(nc::from_f64(1.0 / 3.0), 30.min(nc::DOUBLE_DIGITS), N, &mut dp, &mut sg, &mut big, false), 0);
    assert_eq!(&big[..18], b"33333333333333331\0", "17 digits, correctly rounded");
    assert_eq!(nc::cvt_r(nc::from_f64(1.0 / 3.0), 30.min(nc::DOUBLE_DIGITS), N, &mut dp, &mut sg, &mut big, true), 0);
    assert_eq!((big.iter().position(|&b| b == 0), dp), (Some(17), 0), "fcvt: 17 places");
    let third = Float::unpack(0x3FFD_AAAA_AAAA_AAAA_AAAB, Format::F80);
    assert_eq!(nc::cvt_r(third, 30.min(nc::LONG_DOUBLE_DIGITS), N, &mut dp, &mut sg, &mut big, false), 0);
    assert_eq!(big.iter().position(|&b| b == 0), Some(21), "long double: 21");
    assert_eq!((nc::DOUBLE_DIGITS, nc::LONG_DOUBLE_DIGITS), (17, 21));
}

#[test]
fn s8_4_c_3() {
    let mut out = [0u8; 32];
    nc::gcvt_into(&mut out[..], nc::from_f64(123456.0), Format::F64, 3, N).unwrap();
    assert_eq!(&out[..9], b"1.23e+05\0");
    assert_eq!(fmt(123456.0, Style::General, Some(3)), "1.23e+05", "the same as %.*g");
}

/// The `#` flag of `%g` after a rounding carry (clause s12_6_c_4, flags; COMPAT-8, differences row "`%#g` after a rounding carry").
/// Liella follows C: the precision P counts significant digits, so `%#.2g` of 99.5 rounds up to `1.0e+02` with the
/// P-1 digits after the point kept. glibc 2.39 prints `1.e+02` there (the point with no digits after it); that is
/// the recorded difference. Pinned here so a change to either shape is deliberate.
#[test]
fn hash_g_carry_keeps_c_digits() {
    // Trace: s12_6_c_4 (the `#` flag), s8_4_c_1 (the same digit core); COMPAT-8, D-8.11 (3).
    let alt = |x: f64, p: usize, case: Case| {
        let mut buf = [0u8; 64];
        let mut b = Bounded::new(&mut buf);
        format_float(&mut b, nc::from_f64(x), Format::F64, Spec { style: Style::General, case, precision: Some(p), alternate: true }, b".", N);
        let n = b.stored();
        String::from_utf8(buf[..n].to_vec()).unwrap()
    };
    assert_eq!(alt(99.5, 2, Case::Lower), "1.0e+02", "C: P-1 digits after the point; glibc prints 1.e+02");
    assert_eq!(alt(99.5, 2, Case::Upper), "1.0E+02", "C; glibc prints 1.E+02");
    assert_eq!(alt(999.5, 3, Case::Lower), "1.00e+03", "C; glibc prints 1.e+03");
    assert_eq!(alt(9.5, 1, Case::Lower), "1.e+01", "P = 1: no digit after the point in either library");
    // No carry: the `#` flag keeps trailing zeros in both libraries.
    assert_eq!(alt(100.0, 3, Case::Lower), "100.", "no carry: the same in glibc");
    assert_eq!(alt(1.0, 4, Case::Lower), "1.000", "no carry: the same in glibc");
}

#[test]
fn s8_4_c_4() {
    let x = Float::unpack(0x3FFF_C000_0000_0000_0000, Format::F80);
    let mut out = [0u8; 8];
    let c = nc::ecvt(x, 2, N, &mut out);
    assert_eq!((&out[..2], c.decpt), (&b"15"[..], 1), "long double: 1.5");
}

#[test]
fn s8_4_c_5() {
    let mut caller = [0u8; 4];
    let (mut dp, mut sg) = (0, 0);
    assert_eq!(nc::cvt_r(nc::from_f64(7.0), 2, N, &mut dp, &mut sg, &mut caller, false), 0);
    assert_eq!(&caller[..3], b"70\0", "the caller's buffer");
}

#[test]
fn s8_4_g_0() {
    assert_eq!(fmt(0.1, Style::Fixed, Some(20)), "0.10000000000000000555", "the exact binary value");
    assert_eq!(fmt(2.5, Style::Fixed, Some(0)), "2", "correctly rounded (ties to even)");
    assert_eq!(fmt(f64::from_bits(1), Style::Exp, Some(16)), "4.9406564584124654e-324");
}

#[test]
fn s8_6_c_1() {
    assert_eq!((nc::abs32(-3), nc::abs64(-3), nc::abs32(i32::MIN)), (3, 3, i32::MIN));
}

#[test]
fn s8_6_c_2() {
    assert_eq!(nc::div32(7, -2), Ok((-3, 1)));
    assert_eq!(nc::div32(-7, 2), Ok((-3, -1)));
    assert!(nc::div32(1, 0).is_err() && nc::div32(i32::MIN, -1).is_err(), "undefined in C: refused");
    assert!(nc::div64(i64::MIN, -1).is_err());
}

#[test]
fn s8_7_c_0() {
    assert_eq!((<u8 as StdBit>::WIDTH, <u16 as StdBit>::WIDTH, <u32 as StdBit>::WIDTH, <u64 as StdBit>::WIDTH), (8, 16, 32, 64));
}

#[test]
fn s8_7_c_1() {
    assert_eq!(0u16.leading_zeros_(), 16, "W for 0");
    assert_eq!(0u32.trailing_zeros_(), 32);
    assert_eq!(0xF0u8.leading_ones_(), 4);
    assert_eq!(0x0Fu8.trailing_ones_(), 4);
}

#[test]
fn s8_7_c_2() {
    assert_eq!((num::first_leading_one(0x10u8), num::first_leading_one(0u8)), (4, 0));
    assert_eq!((num::first_trailing_one(0x10u8), num::first_trailing_zero(0xFFu8)), (5, 0));
    assert_eq!(num::first_leading_zero(0xC0u8), 3);
}

#[test]
fn s8_7_c_3() {
    assert_eq!((num::count_zeros(0x0Fu8), 0xF0F0u16.count_ones_()), (4, 8));
}

#[test]
fn s8_7_c_4() {
    assert!(num::has_single_bit(64u32) && !num::has_single_bit(0u32) && !num::has_single_bit(6u32));
}

#[test]
fn s8_7_c_5() {
    assert_eq!((num::bit_width(0u64), num::bit_width(1u64), num::bit_width(255u8)), (0, 1, 8));
}

#[test]
fn s8_7_c_6() {
    assert_eq!((0u8.bit_floor_(), 1u8.bit_floor_(), 100u8.bit_floor_()), (0, 1, 64));
}

#[test]
fn s8_7_c_7() {
    assert_eq!((0u8.bit_ceil_(), 3u8.bit_ceil_(), 128u8.bit_ceil_()), (1, 4, 128));
    assert_eq!(129u8.bit_ceil_(), 0, "not representable: 0");
}

/// §8.8: on x86_64 `hton*`/`ntoh*` are byte swaps, each its own inverse; only `htonl`, `htons`,
/// `ntohl` and `ntohs` are exported functions (the `htobe*`/`be*toh`/`htole*`/`le*toh` forms are
/// `<endian.h>` macros): the reference's symbol list (Appendix B) and the products agree.
#[test]
fn s8_8_c_0() {
    use liella_boundary::posix::s15_sockets as so;
    assert_eq!(so::htonl(0x0102_0304), 0x0403_0201);
    assert_eq!(so::htons(0x0102), 0x0201);
    assert_eq!(so::ntohl(0x0403_0201), 0x0102_0304);
    assert_eq!(so::ntohs(0x0201), 0x0102);
    for v in [0u32, 1, 0x8000_0000, 0xdead_beef, u32::MAX] {
        assert_eq!(so::ntohl(so::htonl(v)), v);
        assert_eq!(so::htonl(v).to_ne_bytes(), v.to_be_bytes(), "network order is big-endian");
        let h = v as u16;
        assert_eq!(so::ntohs(so::htons(h)), h);
        assert_eq!(so::htons(h).to_ne_bytes(), h.to_be_bytes());
    }
    // The exported names: in the reference's list, exactly the four.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let versions = std::fs::read_to_string(root.join("spec/abi/versions.tsv")).unwrap();
    let order = |n: &str| (n.starts_with("hto") || n.starts_with("ntoh") || ((n.starts_with("be") || n.starts_with("le")) && n.contains("toh"))) && n.bytes().all(|b| b.is_ascii_alphanumeric());
    let mut names: Vec<&str> = versions.lines().filter_map(|l| l.split('\t').nth(1)).filter(|n| order(n)).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names, ["htonl", "htons", "ntohl", "ntohs"]);
    // And in the products (when built): the same four, no macro form defined as a symbol.
    let lib = root.join("target/liella/libc.a");
    if let Ok(o) = std::process::Command::new("nm").args(["-g", "--defined-only"]).arg(&lib).output() {
        if o.status.success() {
            let text = String::from_utf8_lossy(&o.stdout);
            let mut ours: Vec<&str> = text.lines().filter_map(|l| l.split_whitespace().nth(2)).filter(|n| order(n)).collect();
            ours.sort_unstable();
            ours.dedup();
            assert_eq!(ours, ["htonl", "htons", "ntohl", "ntohs"], "libc.a");
        }
    }
}

#[test]
fn s8_9_c_0() {
    assert_eq!((nc::ffs32(0), nc::ffs32(8), nc::ffs32(-1), nc::ffs64(1 << 62)), (0, 4, 1, 63));
}

#[test]
fn s8_10_c_0() {
    let mut out = [0u8; 6];
    let n = num::l64a(0x3F | (2 << 6), &mut out);
    assert_eq!(&out[..n], b"z0", "least significant digit first");
    assert_eq!(num::a64l(b"z0"), 0x3F | (2 << 6));
    assert_eq!(num::l64a(0x1_0000_0000, &mut out), 0, "only the low 32 bits");
}

/// Part of s8_1_c_0 (STRTOIMAX fast path).
fn short_path_agrees() {
    // STRTOIMAX: the one-pass shapes give parse_i64's value and end, from any head of the text.
    let texts: [&[u8]; 24] = [b"12345", b"0", b"7", b"-42x", b"+9 ", b"abc", b"", b" 12", b"-", b"+", b"- 5", b"0x1f", b"012",
        b"999999999999999999", b"9999999999999999999", b"-9223372036854775808", b"123456789012345678z", b"1.5", b"	-3", b"x12",
        b"00000000000000000000001", b"-0", b"5
", b"0b11"];
    for s in texts {
        for base in [0u32, 10, 16, 2] {
            for d in [Dialect::C99, Dialect::C23] {
                let full = parse_i64(s, base, d);
                for cut in 0..=s.len() {
                    let Some((v, used)) = parse_i64_short(&s[..cut], base, cut == s.len()) else { continue };
                    assert!(full.status == Status::Ok || full.status == Status::NoDigits, "{s:?} {base}");
                    assert_eq!((v, used), (full.value, full.used), "{s:?} base {base} cut {cut}");
                }
            }
        }
    }
    assert_eq!(parse_i64_short(b"12345", 10, true), Some((12345, 5)));
    assert_eq!(parse_i64_short(b"-12x", 0, true), Some((-12, 3)));
    assert_eq!(parse_i64_short(b"name", 10, true), Some((0, 0)));
    assert_eq!(parse_i64_short(b"1234567890123456789", 10, true), None);
}
