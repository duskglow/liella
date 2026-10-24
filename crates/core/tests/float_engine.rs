//! Internal tests of the exact float engine (IP-18, spec §8.2/§8.4 guarantees: correctly rounded
//! parsing for any input length, exact formatting). Deterministic pseudo-random inputs, checked
//! against Rust's own correctly rounded `f32`/`f64` parsing and formatting, and by round trips
//! for the formats Rust has no type for.

#![allow(clippy::panic, clippy::approx_constant, clippy::unwrap_used, clippy::as_conversions, clippy::arithmetic_side_effects, clippy::indexing_slicing, missing_docs)]

use liella::num::format::{format_float, Case, Sink, Spec, Style};
use liella::num::parse::{parse_float, Dialect, FloatRange};
use liella::num::{Float, Format, Rounding, Value};

struct S(Vec<u8>);
impl Sink for S {
    fn put(&mut self, b: &[u8]) {
        self.0.extend_from_slice(b);
    }
}

/// SplitMix64: a fixed, documented test-input generator.
struct Gen(u64);
impl Gen {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
}

fn parse(s: &str, fmt: Format, mode: Rounding) -> (u128, usize, FloatRange) {
    let p = parse_float(s.as_bytes(), b".", fmt, mode, Dialect::C99);
    (p.value.pack(fmt), p.used, p.range)
}

fn fmt_e(bits: u128, fmt: Format, prec: usize) -> String {
    let mut s = S(Vec::new());
    format_float(&mut s, Float::unpack(bits, fmt), fmt, Spec { style: Style::Exp, case: Case::Lower, precision: Some(prec), alternate: false }, b".", Rounding::Nearest);
    String::from_utf8(s.0).unwrap()
}

#[test]
fn random_f64_bits_round_trip_through_17_digits() {
    // Clauses: s8_4_g_0, s8_2_g_0, s8_2_c_1 (D-8.5).
    let mut g = Gen(1);
    for _ in 0..100_000 {
        let x = f64::from_bits(g.next());
        if !x.is_finite() {
            continue;
        }
        let text = fmt_e(u128::from(x.to_bits()), Format::F64, 16);
        assert_eq!(text, format!("{x:.16e}").replace("e", "e+").replace("e+-", "e-").pipe_exp(), "{x:e}");
        let (bits, used, _) = parse(&text, Format::F64, Rounding::Nearest);
        assert_eq!(bits as u64, x.to_bits(), "{text}");
        assert_eq!(used, text.len());
    }
}

trait PipeExp {
    fn pipe_exp(self) -> String;
}
impl PipeExp for String {
    /// Rust prints `e5`; C prints `e+05` (at least two exponent digits).
    fn pipe_exp(self) -> String {
        let (m, e) = self.split_once('e').unwrap();
        let (sign, digits) = e.split_at(1);
        let digits = if digits.len() < 2 { format!("0{digits}") } else { digits.to_string() };
        format!("{m}e{sign}{digits}")
    }
}

#[test]
fn random_decimal_strings_parse_like_rust() {
    // Clauses: s8_2_g_0, s8_2_c_0, s8_2_c_1 (D-8.5).
    let mut g = Gen(2);
    for _ in 0..50_000 {
        let ndig = 1 + (g.next() % 40) as usize;
        let mut s = String::new();
        for k in 0..ndig {
            if k == 1 && g.next() % 2 == 0 {
                s.push('.');
            }
            s.push(char::from(b'0' + (g.next() % 10) as u8));
        }
        let e = (g.next() % 700) as i32 - 350;
        s.push_str(&format!("e{e}"));
        let want64: f64 = s.parse().unwrap();
        let want32: f32 = s.parse().unwrap();
        assert_eq!(parse(&s, Format::F64, Rounding::Nearest).0 as u64, want64.to_bits(), "{s}");
        assert_eq!(parse(&s, Format::F32, Rounding::Nearest).0 as u32, want32.to_bits(), "{s} (f32)");
    }
}

/// The exact decimal expansion of the midpoint between `x` and its successor.
fn midpoint_text(x: f64) -> String {
    let mut s = S(Vec::new());
    let next = f64::from_bits(x.to_bits() + 1);
    // Both are exact dyadic rationals; their midpoint has an exact decimal expansion, found by
    // formatting x with enough digits and averaging digit strings is awkward, so use the engine
    // in a format with one more bit of precision: binary128 holds the midpoint exactly.
    let mid_bits = {
        let fx = Float::unpack(u128::from(x.to_bits()), Format::F64);
        let fy = Float::unpack(u128::from(next.to_bits()), Format::F64);
        let (Value::Finite { mant: a, exp: ea }, Value::Finite { mant: b, exp: eb }) = (fx.value, fy.value) else { panic!() };
        // Bring both to the smaller exponent, add, halve by lowering the exponent.
        let e = ea.min(eb);
        let sum = (a << (ea - e)) + (b << (eb - e));
        let r = liella::num::float::round(false, sum, e - 1, false, Format::F128, Rounding::Nearest);
        assert!(!r.inexact);
        r.value.pack(Format::F128)
    };
    format_float(&mut s, Float::unpack(mid_bits, Format::F128), Format::F128, Spec { style: Style::Exp, case: Case::Lower, precision: Some(800), alternate: false }, b".", Rounding::Nearest);
    let t = String::from_utf8(s.0).unwrap();
    // Trim trailing zeros of the mantissa (the expansion is exact well before 800 digits).
    let (m, e) = t.split_once('e').unwrap();
    format!("{}e{}", m.trim_end_matches('0'), e)
}

#[test]
fn halfway_cases_with_long_tails_round_correctly() {
    // Clauses: s8_2_g_0, s8_2_c_0 (D-8.5).
    for x in [1.0f64, 0.1, 1e300, 5e-324, 2.2250738585072014e-308, 123.456] {
        let mid = midpoint_text(x);
        let (m, e) = mid.split_once('e').unwrap();
        let below = x.to_bits();
        let above = below + 1;
        let even = if below % 2 == 0 { below } else { above };
        // Exactly the midpoint: ties to even.
        assert_eq!(parse(&mid, Format::F64, Rounding::Nearest).0 as u64, even, "{mid}");
        // A nonzero digit 100,000 places later: above the midpoint.
        let up = format!("{m}{}1e{e}", "0".repeat(100_000));
        assert_eq!(parse(&up, Format::F64, Rounding::Nearest).0 as u64, above);
        assert_eq!(parse(&up, Format::F64, Rounding::Nearest).1, up.len());
        // The midpoint minus a tiny amount: replace the last digit 5 by 4999...
        let m_dn = format!("{}4{}", &m[..m.len() - 1], "9".repeat(100_000));
        let dn = format!("{m_dn}e{e}");
        assert_eq!(parse(&dn, Format::F64, Rounding::Nearest).0 as u64, below, "below midpoint");
    }
}

#[test]
fn directed_modes_bracket_the_value() {
    // Clauses: s8_2_c_0, s8_2_g_0 (D-8.5).
    let mut g = Gen(3);
    for _ in 0..5_000 {
        let s = format!("{}.{}e{}", g.next() % 1000, g.next() % 1_000_000, (g.next() % 80) as i32 - 40);
        for fmt in [Format::F32, Format::F64, Format::F80, Format::F128] {
            let up = parse(&s, fmt, Rounding::Up).0;
            let dn = parse(&s, fmt, Rounding::Down).0;
            let zr = parse(&s, fmt, Rounding::Zero).0;
            let ne = parse(&s, fmt, Rounding::Nearest).0;
            // Positive finite values: bit patterns order like values.
            assert!(dn <= ne && ne <= up, "{s}");
            assert!(up - dn <= 1, "{s}: up and down are adjacent or equal");
            assert_eq!(zr, dn, "{s}: toward zero equals down for positives");
            let neg = format!("-{s}");
            let sign = 1u128 << (fmt.prec + fmt.exp_bits - u32::from(!fmt.explicit_lead));
            assert_eq!(parse(&neg, fmt, Rounding::Up).0, dn | sign, "{s}: -x up = -(x down)");
        }
    }
}

#[test]
fn extended_formats_round_trip_exactly() {
    // Clauses: s8_2_g_0, s8_4_g_0, s10_0_c_r3, s10_0_c_r8 (D-8.5).
    let mut g = Gen(4);
    for fmt in [Format::F80, Format::F128] {
        for _ in 0..3_000 {
            let raw = (u128::from(g.next()) << 64) | u128::from(g.next());
            let total = fmt.prec - u32::from(!fmt.explicit_lead) + fmt.exp_bits + 1;
            let mut bits = if total >= 128 { raw } else { raw & ((1u128 << total) - 1) };
            if fmt.explicit_lead {
                // Make it a valid x87 encoding: lead bit set unless the exponent field is 0.
                if (bits >> 64) & 0x7fff != 0 {
                    bits |= 1u128 << 63;
                } else {
                    bits &= !(1u128 << 63);
                }
            }
            let f = Float::unpack(bits, fmt);
            if !matches!(f.value, Value::Finite { .. }) {
                continue;
            }
            // 40 significant digits identify any binary128 or x87 value.
            let text = fmt_e(bits, fmt, 40);
            let (back, _, _) = parse(&text, fmt, Rounding::Nearest);
            assert_eq!(back, bits, "{text}");
        }
    }
}

#[test]
fn overflow_and_underflow_per_mode() {
    // Clauses: s8_2_f_1, s8_2_f_2, s8_2_c_0, s8_2_c_2 (D-8.5).
    let max = f64::MAX.to_bits();
    assert_eq!(parse("1e999", Format::F64, Rounding::Nearest), (u128::from(f64::INFINITY.to_bits()), 5, FloatRange::Overflow));
    assert_eq!(parse("1e999", Format::F64, Rounding::Zero).0 as u64, max);
    assert_eq!(parse("1e999", Format::F64, Rounding::Down).0 as u64, max);
    assert_eq!(parse("-1e999", Format::F64, Rounding::Up).0 as u64, (-f64::MAX).to_bits());
    assert_eq!(parse("1e-999", Format::F64, Rounding::Up).0 as u64, 1, "the smallest subnormal");
    assert_eq!(parse("1e-999", Format::F64, Rounding::Nearest), (0, 6, FloatRange::Underflow));
    // Exact subnormal: not an underflow (no inexactness).
    assert_eq!(parse("0x1p-1074", Format::F64, Rounding::Nearest).2, FloatRange::Ok);
}

#[test]
fn hundred_thousand_digit_inputs_round_correctly() {
    // Clauses: s8_2_g_0, s8_2_c_0 (D-8.5).
    // An exact halfway point between 1 and the next double, then 10^5 more digits: the tie
    // is broken only by the very last one.
    let half = "1.00000000000000011102230246251565404236316680908203125";
    let tie = format!("{half}{}", "0".repeat(100_000));
    assert_eq!(parse(&tie, Format::F64, Rounding::Nearest).0, 1.0f64.to_bits() as u128, "a tie rounds to even");
    let above = format!("{tie}1");
    assert_eq!(parse(&above, Format::F64, Rounding::Nearest).0, 1.0000000000000002f64.to_bits() as u128);
    let (_, used, _) = parse(&above, Format::F64, Rounding::Nearest);
    assert_eq!(used, above.len(), "every digit is consumed");
    let nines = format!("0.{}", "9".repeat(100_000));
    assert_eq!(parse(&nines, Format::F64, Rounding::Nearest).0, 1.0f64.to_bits() as u128);
    assert_eq!(parse(&nines, Format::F64, Rounding::Down).0, (1.0f64.to_bits() - 1) as u128, "just below 1 when rounding down");
}
