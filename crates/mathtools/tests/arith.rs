//! Exact arithmetic, rounding into formats, encodings and text, checked against Rust's own
//! correctly rounded binary32/binary64 arithmetic and against the defining properties.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, missing_docs)]
#![allow(clippy::arithmetic_side_effects, clippy::as_conversions, clippy::indexing_slicing)]

mod common;

use common::{f, Rng};
use liella_mathtools::mp::{correctly_rounded, ulp_error, Approx, Big, Fmt, Round, Rounded};
use std::cmp::Ordering;

const MODES: [Round; 4] = [Round::Nearest, Round::Up, Round::Down, Round::Zero];

fn same_f64(got: u128, want: f64) -> bool {
    let g = f64::from_bits(got as u64);
    (g.is_nan() && want.is_nan()) || g.to_bits() == want.to_bits()
}

#[test]
fn f64_ops_match_rust() {
    // no spec clause: test tooling of the math oracle (D-10.12).
    let mut g = Rng(10);
    for i in 0..200_000 {
        let (a, b) = (g.f64_any(), g.f64_any());
        let (ba, bb) = (f(a), f(b));
        let (s, _) = ba.add(&bb).round_to(Fmt::F64, Round::Nearest);
        assert!(same_f64(s, a + b), "{a:e} + {b:e}");
        let (d, _) = ba.sub(&bb).round_to(Fmt::F64, Round::Nearest);
        assert!(same_f64(d, a - b), "{a:e} - {b:e}");
        let (m, _) = ba.mul(&bb).round_to(Fmt::F64, Round::Nearest);
        assert!(same_f64(m, a * b), "{a:e} * {b:e}");
        if i % 4 == 0 {
            // Division and square root through the Ziv loop; exactness is detected by the
            // directed roundings agreeing.
            let q = correctly_rounded(Fmt::F64, Round::Nearest, 4000, |p| Approx {
                value: ba.div(&bb, p, Round::Nearest),
                exact: ba.div(&bb, p, Round::Down).identical(&ba.div(&bb, p, Round::Up)),
            })
            .unwrap();
            assert!(same_f64(q.0, a / b), "{a:e} / {b:e}");
            let r = correctly_rounded(Fmt::F64, Round::Nearest, 4000, |p| Approx {
                value: ba.sqrt(p, Round::Nearest),
                exact: ba.sqrt(p, Round::Down).identical(&ba.sqrt(p, Round::Up)),
            })
            .unwrap();
            assert!(same_f64(r.0, a.sqrt()), "sqrt {a:e}");
        }
    }
}

#[test]
fn exact_ops_identities() {
    // no spec clause: test tooling of the math oracle (D-10.12).
    let mut g = Rng(11);
    for _ in 0..20_000 {
        let (ea, eb, ec) = ((g.next() % 400) as i64 - 200, (g.next() % 400) as i64 - 200, (g.next() % 40) as i64 - 20);
        let a = g.big113(ea);
        let b = g.big113(eb).neg();
        let c = g.big113(ec);
        // (a + b) - b == a, distributivity, exact zero sum.
        assert!(a.add(&b).sub(&b).identical(&a));
        assert!(a.add(&b).mul(&c).identical(&a.mul(&c).add(&b.mul(&c))));
        let z = a.sub(&a);
        assert!(z.is_zero() && !z.is_neg());
        // Correctly rounded division: q*b - a changes sign across the neighbouring values.
        let q = a.div(&c, 100, Round::Down);
        let up = q.add(&Big::pow2(q.ilogb().unwrap() - 99));
        assert!(q.mul(&c).cmp(&a) != Some(Ordering::Greater));
        assert_eq!(up.mul(&c).cmp(&a), Some(Ordering::Greater));
        let s = c.abs().sqrt(90, Round::Down);
        let s_up = s.add(&Big::pow2(s.ilogb().unwrap() - 89));
        assert!(s.mul(&s).cmp(&c.abs()) != Some(Ordering::Greater));
        assert_eq!(s_up.mul(&s_up).cmp(&c.abs()), Some(Ordering::Greater));
    }
    assert!(Big::zero(true).add(&Big::zero(true)).is_neg());
    assert!(!Big::zero(true).add(&Big::zero(false)).is_neg());
    assert!(Big::inf(false).add(&Big::inf(true)).is_nan());
    assert!(Big::inf(false).mul(&Big::zero(false)).is_nan());
    assert!(Big::zero(true).sqrt(10, Round::Nearest).is_neg());
}

fn f32_next_up(x: f32) -> f32 {
    if x.is_nan() || x == f32::INFINITY {
        return x;
    }
    if x == 0.0 {
        return f32::from_bits(1);
    }
    if x > 0.0 { f32::from_bits(x.to_bits() + 1) } else { f32::from_bits(x.to_bits() - 1) }
}

fn f32_next_down(x: f32) -> f32 {
    -f32_next_up(-x)
}

/// x rounded to f32 in `mode`, from Rust's round-to-nearest conversion.
fn f32_mode(x: f64, mode: Round) -> f32 {
    let r = x as f32;
    if f64::from(r) == x || x.is_nan() {
        return r;
    }
    let down = if f64::from(r) < x { r } else { f32_next_down(r) };
    let up = if f64::from(r) > x { r } else { f32_next_up(r) };
    match mode {
        Round::Nearest => r,
        Round::Down => down,
        Round::Up => up,
        Round::Zero => if x > 0.0 { down } else { up },
    }
}

#[test]
fn f64_to_f32_all_modes_and_flags() {
    // no spec clause: test tooling of the math oracle (D-10.12).
    let mut g = Rng(12);
    for _ in 0..200_000 {
        let x = match g.next() % 4 {
            0 => g.f64_exp(-160, -120),
            1 => g.f64_exp(120, 130),
            _ => g.f64_any(),
        };
        for mode in MODES {
            let (bits, fl) = f(x).round_to(Fmt::F32, mode);
            let want = f32_mode(x, mode);
            let got = f32::from_bits(bits as u32);
            assert!((got.is_nan() && want.is_nan()) || got.to_bits() == want.to_bits(), "{x:e} {mode:?}: {got:e} vs {want:e}");
            if !x.is_finite() {
                assert_eq!(fl, Rounded::default());
                continue;
            }
            // Flags from the definitions, using scaled copies to get the unbounded-exponent
            // rounding: x * 2^-100 and x * 2^100 are exact and land in f32's normal range.
            let inexact = f64::from(want) != x;
            let big = f64::from(f32_mode(x * 2f64.powi(-100), mode)).abs() * 2f64.powi(100);
            let small = f64::from(f32_mode(x * 2f64.powi(100), mode)).abs() * 2f64.powi(-100);
            let overflow = x.abs() > 1e30 && big > f64::from(f32::MAX);
            let tiny = x.abs() < 1e-30 && small < f64::from(f32::MIN_POSITIVE);
            let want_fl = Rounded { inexact: inexact || overflow, overflow, underflow: tiny && inexact };
            assert_eq!(fl, want_fl, "{x:e} {mode:?}");
        }
    }
}

/// Smallest positive step above |r| (r a finite value of `fmt`).
fn ulp_of(fmt: Fmt, r: &Big) -> Big {
    let emin = i64::from(fmt.emin());
    let e = r.ilogb().unwrap_or(emin).max(emin);
    Big::pow2(e - i64::from(fmt.prec) + 1)
}

/// The gap below |r| (half the ulp at a power of two above the subnormal range).
fn ulp_below(fmt: Fmt, r: &Big) -> Big {
    let u = ulp_of(fmt, r);
    let emin = i64::from(fmt.emin());
    if r.sig_bits() == 1 && r.ilogb().is_some_and(|e| e > emin) { u.mul_pow2(-1) } else { u }
}

/// Checks `x.round_to(fmt, mode)` against the defining inequalities, for every mode.
fn check_rounding(fmt: Fmt, x: &Big) {
    let max = Big::from_bits(fmt, fmt.max_bits(false));
    for mode in MODES {
        let (bits, fl) = x.round_to(fmt, mode);
        let r = Big::from_bits(fmt, bits);
        // Symmetry: rounding -x in the mirrored mode gives -r.
        let mirror = match mode {
            Round::Up => Round::Down,
            Round::Down => Round::Up,
            m => m,
        };
        let (nb, nfl) = x.neg().round_to(fmt, mirror);
        assert!(Big::from_bits(fmt, nb).identical(&r.neg()), "{} {mode:?}", x.to_hex());
        assert_eq!(nfl, fl);
        let ax = x.abs();
        let ar = r.abs();
        let toward_up = match mode {
            Round::Up => !x.is_neg(),
            Round::Down => x.is_neg(),
            _ => false,
        };
        assert_eq!(fl.inexact, !r.identical(x) || fl.overflow, "{} {mode:?}", x.to_hex());
        if r.is_inf() {
            assert!(fl.overflow);
            continue;
        }
        if fl.overflow {
            assert!(ar.identical(&max));
            continue;
        }
        match mode {
            Round::Nearest => {
                let d = ar.sub(&ax).abs();
                let half = if ar.cmp(&ax) == Some(Ordering::Greater) { ulp_below(fmt, &ar) } else { ulp_of(fmt, &ar) };
                assert!(d.mul_pow2(1).cmp(&half) != Some(Ordering::Greater), "{} nearest", x.to_hex());
            }
            _ if toward_up => {
                assert!(ar.cmp(&ax) != Some(Ordering::Less));
                assert_eq!(ar.sub(&ulp_below(fmt, &ar)).cmp(&ax), Some(Ordering::Less), "{} {mode:?}", x.to_hex());
            }
            _ => {
                assert!(ar.cmp(&ax) != Some(Ordering::Greater));
                assert_eq!(ar.add(&ulp_of(fmt, &ar)).cmp(&ax), Some(Ordering::Greater), "{} {mode:?}", x.to_hex());
            }
        }
    }
}

#[test]
fn rounding_properties_all_formats() {
    // no spec clause: test tooling of the math oracle (D-10.12).
    let mut g = Rng(13);
    for fmt in [Fmt::F32, Fmt::F64, Fmt::F80, Fmt::F128] {
        let emin = i64::from(fmt.emin());
        let emax = i64::from(fmt.emax);
        let p = i64::from(fmt.prec);
        for _ in 0..20_000 {
            let e = match g.next() % 4 {
                0 => emin - p - 2 + (g.next() % (p as u64 + 4)) as i64,
                1 => emax - 2 + (g.next() % 4) as i64,
                2 => emin - 2 + (g.next() % 4) as i64,
                _ => (g.next() % 200) as i64 - 100,
            };
            // 113 random bits, sometimes truncated to land near halfway cases.
            let mut x = g.big113(e);
            if g.next() % 3 == 0 {
                x = x.round(fmt.prec + 1, Round::Zero);
            }
            check_rounding(fmt, &x);
        }
    }
}

#[test]
fn encodings_round_trip() {
    // no spec clause: test tooling of the math oracle (D-10.12).
    let mut g = Rng(14);
    for fmt in [Fmt::F32, Fmt::F64, Fmt::F80, Fmt::F128] {
        let total = fmt.total_bits();
        let mask = if total == 128 { u128::MAX } else { (1u128 << total) - 1 };
        for _ in 0..50_000 {
            let mut bits = (u128::from(g.next()) << 64 | u128::from(g.next())) & mask;
            if g.next() % 4 == 0 {
                bits &= !(((1u128 << fmt.exp_bits) - 1) << (total - 1 - fmt.exp_bits)); // subnormal
            }
            if fmt.explicit_lead {
                // A valid x87 encoding: the explicit bit matches the exponent class.
                let field = (bits >> 64) & 0x7fff;
                bits = if field == 0 { bits & !(1u128 << 63) } else { bits | (1u128 << 63) };
            }
            let x = Big::from_bits(fmt, bits);
            let (back, fl) = x.round_to(fmt, Round::Nearest);
            if x.is_nan() {
                assert_eq!(back, fmt.nan_bits());
            } else {
                assert_eq!(back, bits, "{fmt:?} {bits:x}");
                assert_eq!(fl, Rounded::default());
            }
        }
    }
    // Known encodings.
    assert_eq!(Big::one().round_to(Fmt::F80, Round::Nearest).0, 0x3fff_8000_0000_0000_0000);
    assert_eq!(Big::one().round_to(Fmt::F128, Round::Nearest).0, 0x3fff_0000_0000_0000_0000_0000_0000_0000);
    assert_eq!(Big::nan().round_to(Fmt::F80, Round::Nearest).0, 0x7fff_c000_0000_0000_0000);
    assert_eq!(Big::nan().round_to(Fmt::F128, Round::Nearest).0, 0x7fff_8000_0000_0000_0000_0000_0000_0000);
    assert_eq!(Big::inf(true).round_to(Fmt::F80, Round::Nearest).0, 0xffff_8000_0000_0000_0000);
    // x87 pseudo-denormal: exponent field 0 with the explicit bit set has the value of the
    // normal with field 1, which is how it re-encodes.
    let pd = Big::from_bits(Fmt::F80, 0x0000_8000_0000_0000_0001);
    assert_eq!(pd.round_to(Fmt::F80, Round::Nearest).0, 0x0001_8000_0000_0000_0001);
    // Unnormal, pseudo-infinity and pseudo-NaN decode as NaN.
    assert!(Big::from_bits(Fmt::F80, 0x3fff_4000_0000_0000_0000).is_nan());
    assert!(Big::from_bits(Fmt::F80, 0x7fff_0000_0000_0000_0000).is_nan());
    assert!(Big::from_bits(Fmt::F80, 0x7fff_4000_0000_0000_0000).is_nan());
    assert!(Big::from_bits(Fmt::F80, 0x7fff_8000_0000_0000_0000).is_inf());
    // Overflow in each mode.
    let huge = Big::pow2(20000);
    assert_eq!(huge.round_to(Fmt::F80, Round::Zero).0, Fmt::F80.max_bits(false));
    assert_eq!(huge.neg().round_to(Fmt::F128, Round::Up).0, Fmt::F128.max_bits(true));
    assert_eq!(huge.neg().round_to(Fmt::F128, Round::Down).0, Fmt::F128.inf_bits(true));
}

#[test]
fn text() {
    // no spec clause: test tooling of the math oracle (D-10.12).
    let mut g = Rng(15);
    for _ in 0..20_000 {
        let x = g.f64_any();
        if !x.is_finite() {
            continue;
        }
        // Rust prints the shortest decimal that rounds back; parse_rounded must agree.
        let s = format!("{x:e}");
        let b = Big::parse_rounded(&s, 53).unwrap();
        if x.abs() >= f64::MIN_POSITIVE {
            assert_eq!(b.to_f64().to_bits(), x.to_bits(), "{s}");
        }
        // Subnormals have fewer bits: parse wide, then round to the format (the shortest
        // string is nowhere near 2^-300 of a rounding boundary).
        let b = Big::parse_rounded(&s, 300).unwrap();
        assert_eq!(b.round_to(Fmt::F64, Round::Nearest).0 as u64, x.to_bits(), "{s}");
        let h = f(x).to_hex();
        assert!(Big::parse(&h).unwrap().identical(&f(x)), "{h}");
        // An exact parse (when the decimal is dyadic, e.g. "1e23") rounds back to x too.
        assert!(Big::parse(&format!("{x:?}")).is_none_or(|p| p.round_to(Fmt::F64, Round::Nearest).0 as u64 == x.to_bits()));
    }
    assert!(Big::parse("0.1").is_none());
    assert_eq!(Big::parse("-0x1.8p-3").unwrap().to_f64(), -0.1875);
    assert!(Big::parse("-1.25e-3").is_none(), "1.25e-3 = 1/800 is not dyadic");
    assert_eq!(Big::parse("-1.25e-1").unwrap().to_f64(), -0.125);
    assert_eq!(Big::parse("1.5e1").unwrap().to_f64(), 15.0);
    assert_eq!(Big::parse("0.375").unwrap().to_hex(), "0x1.8p-2");
    assert_eq!(Big::parse_rounded("0.1", 53).unwrap().to_f64(), 0.1);
    assert_eq!(Big::parse_rounded("0.1", 24).unwrap().round_to(Fmt::F32, Round::Nearest).0 as u32, 0.1f32.to_bits());
    assert!(Big::parse("-inf").unwrap().is_neg());
    assert!(Big::parse("NaN").unwrap().is_nan());
    assert!(Big::parse("-0").unwrap().is_neg());
    assert!(Big::parse("1e").is_none() && Big::parse("").is_none() && Big::parse("0x").is_none());
    assert_eq!(f(-0.0).to_hex(), "-0x0p+0");
    assert_eq!(f(1.0).to_hex(), "0x1p+0");
    assert_eq!(f(f64::MIN_POSITIVE / 4.0).to_hex(), "0x1p-1024");
    assert_eq!(f(0.1).to_decimal(17), "1.0000000000000001e-1");
    assert_eq!(f(-1234.5).to_decimal(3), "-1.23e3");
    assert_eq!(f(9.9996).to_decimal(4), "1.000e1");
    assert_eq!(f(1.0).to_decimal(1), "1e0");
}

#[test]
fn misc() {
    // no spec clause: test tooling of the math oracle (D-10.12).
    let mut g = Rng(16);
    for _ in 0..20_000 {
        let x = g.f64_any();
        let b = f(x);
        if x.is_nan() {
            assert!(b.cmp(&b).is_none());
            continue;
        }
        let y = g.f64_any();
        if !y.is_nan() {
            assert_eq!(b.cmp(&f(y)), x.partial_cmp(&y), "{x:e} {y:e}");
        }
        if x.is_finite() {
            assert_eq!(b.floor().to_f64().to_bits(), x.floor().to_bits(), "{x:e}");
            assert_eq!(b.trunc().to_f64().to_bits(), x.trunc().to_bits(), "{x:e}");
            assert_eq!(b.is_integer(), x.fract() == 0.0);
            if x.abs() >= f64::MIN_POSITIVE {
                assert_eq!(b.ilogb(), Some(((x.to_bits() >> 52) & 0x7ff) as i64 - 1023));
            }
        }
    }
    // ulp_error.
    let one = Big::one();
    let third = one.div(&Big::from_i64(3), 200, Round::Nearest);
    let got = third.round_to(Fmt::F64, Round::Nearest).0;
    let e = ulp_error(Fmt::F64, got, &third);
    assert!(e > 0.0 && e <= 0.5, "{e}");
    let up = got + 1;
    let eu = ulp_error(Fmt::F64, up, &third);
    assert!((eu - (1.0 - e)).abs() < 1e-9 || (eu - (1.0 + e)).abs() < 1e-9, "{eu} {e}");
    assert_eq!(ulp_error(Fmt::F64, Fmt::F64.inf_bits(false), &Big::inf(false)), 0.0);
    assert_eq!(ulp_error(Fmt::F64, Fmt::F64.inf_bits(false), &Big::pow2(5000)), f64::INFINITY);
    assert_eq!(ulp_error(Fmt::F64, Fmt::F64.nan_bits(), &Big::nan()), 0.0);
    // Subnormal ulp is 2^-1074.
    assert_eq!(ulp_error(Fmt::F64, 1, &Big::zero(false)), 1.0);
    // Invalid formats are refused without panicking.
    for bad in [
        Fmt { prec: 0, emax: 5, exp_bits: 200, explicit_lead: false },
        Fmt { prec: 1, emax: 0, exp_bits: 0, explicit_lead: true },
        Fmt { prec: 120, emax: 1023, exp_bits: 11, explicit_lead: false },
    ] {
        assert!(!bad.is_valid());
        assert!(Big::from_bits(bad, 12345).is_nan());
        assert_eq!(Big::one().round_to(bad, Round::Up), (0, Rounded::default()));
        assert_eq!((bad.nan_bits(), bad.inf_bits(true), bad.max_bits(false)), (0, 0, 0));
        let _ = bad.total_bits();
    }
    for good in [Fmt::F32, Fmt::F64, Fmt::F80, Fmt::F128] {
        assert!(good.is_valid());
    }
}
