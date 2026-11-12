//! The constants and tables of the x87 `long double` exponentials' and logarithms' integer
//! fixed-point tier (`liella_math::wide_int_el`, D-10.73), written to
//! `crates/math/src/coeffs/wide_int_el.rs` by `cargo xtask tables`.
//!
//! Every value is a fixed-point word "in units `2^-u`" (`v·2^u` rounded to the nearest integer;
//! none is a tie: the irrational values cannot be, and the rationals `1/k`, `1/k!` and `2^63/d`
//! have odd factors in their denominators). The exponentials (Tang, ACM TOMS 15(2), 1989):
//! `128/ln 2`, `128·log2 10`, `ln2/128`, the series `1/k!` and `2^(j/128)`. The logarithms
//! (Tang, ACM TOMS 16(4), 1990, with the two-step reduction of Wong and Goto, IEEE TC 43(3),
//! 1994): the first step's 17-bit reciprocals `c1'` of `1 + (i + 1/2)/128` and `−ln c1`, the
//! second step's reciprocals `c2` of `1 + i2/4096` and `−ln c2`, the series `1/k`, `ln 2`,
//! `1/ln 2` and `1/ln 10`. The tier's module doc holds the error analysis that fixes each unit.
//! The binary128 tier (`liella_math::wide_int::quad::explog`) reuses these words and adds a
//! signed third limb below `2^(j/128)`, `−ln c1`, `−ln c2`, `ln2/128` and `ln 2` (192 bits), and the
//! coefficients of its longer series (`1/k!` to `k = 12`, `1/k` to `k = 11`).

use crate::mp::{self, Big, Round};

/// Working precision (bits): far beyond the 128 bits stored.
const P: u32 = 400;

/// `|v|·2^units` rounded to the nearest integer, as a `u128` (it must fit).
fn ufixed(v: &Big, units: i64) -> u128 {
    let r = v.abs().mul_pow2(units).round_int();
    let mut out = 0u128;
    for k in (0..4).rev() {
        let q = r.mul_pow2(-32 * k).floor();
        let chunk = q.sub(&q.mul_pow2(-32).floor().mul_pow2(32));
        out = (out << 32) | u128::from(chunk.to_i64().unwrap_or(0) as u32);
    }
    assert!(r.mul_pow2(-128).floor().to_i64() == Some(0), "a fixed-point word does not fit in 128 bits");
    out
}

/// `v·2^units` rounded to the nearest integer with the sign of `v`.
fn ifixed(v: &Big, units: i64) -> i128 {
    let r = i128::try_from(ufixed(v, units)).unwrap_or(i128::MAX);
    assert!(r != i128::MAX, "a signed fixed-point word does not fit in 127 bits");
    if v.is_neg() { -r } else { r }
}

fn hex128(v: u128) -> String {
    format!("{v:#034x}")
}

fn shex128(v: i128) -> String {
    if v < 0 { format!("-{}", hex128(v.unsigned_abs())) } else { hex128(v.unsigned_abs()) }
}

fn shex64(v: i64) -> String {
    if v < 0 { format!("-{:#018x}", v.unsigned_abs()) } else { format!("{v:#018x}") }
}

/// The third limb of `v` in units `2^-units`: `round(v·2^(units + 64)) − round(v·2^units)·2^64`,
/// the signed word below the `ufixed`/`ifixed` word, so that `word·2^64 + limb` is `v` in units
/// `2^-(units + 64)` within half a unit (subtracting an integer commutes with rounding).
fn third(v: &Big, units: i64) -> i64 {
    let r = v.mul_pow2(units);
    let lo = r.sub(&r.round_int()).mul_pow2(64).round_int().to_i64().unwrap_or(i64::MIN);
    assert!(lo != i64::MIN, "a third limb does not fit in 63 bits");
    lo
}

/// `1/k!` as a `Big`.
fn inv_fact(k: i64) -> Big {
    let f = (1..=k).fold(Big::one(), |a, j| a.mul(&Big::from_i64(j)));
    Big::one().div(&f, P, Round::Nearest)
}

/// `C1[i]`: `c1'` in units `2^-17`, the 17-bit reciprocal of the interval `[1 + i/128, 1 + (i +
/// 1)/128)`'s midpoint, `round(2^17/(1 + (i + 1/2)/128)) = round(2^25/(2i + 257))` (an odd
/// divisor: no tie); exactly 1 and 1/2 for the intervals around 1 (`i = 0`, `i = 127`), where
/// `u1 = x − 1` is then exact.
fn c1(i: u64) -> u64 {
    match i {
        0 => 1 << 17,
        127 => 1 << 16,
        _ => ((1u64 << 26) / (2 * i + 257)).div_ceil(2),
    }
}

/// `C2[ii]`: `c2` in units `2^-63`, `round(2^63/(1 + i2/4096))` for `i2 = ii − 32` (`d = 4096 +
/// i2`, never a power of two but at `i2 = 0`, where `c2` is exactly 1).
fn c2(ii: u64) -> u64 {
    let d = 4096 + u128::from(ii) - 32;
    u64::try_from(((1u128 << 64) * 4096 / d).div_ceil(2)).unwrap_or(0)
}

/// Writes the family's file body into `src`.
pub(crate) fn emit(src: &mut String) {
    let one = Big::one();
    let ln2 = mp::ln2(P);
    let ln10 = mp::ln10(P);
    let mut w = |s: String| src.push_str(&s);

    w("\n// The exponentials (steps E1-E4 of the tier's proof).\n".into());
    let c_e = Big::from_i64(128).div(&ln2, P, Round::Nearest);
    let c_10 = Big::from_i64(128).mul(&ln10).div(&ln2, P, Round::Nearest);
    for (name, doc, v, units) in [
        ("WI_C_E", "`128/ln 2`, units 2^-120.", c_e, 120),
        ("WI_C_10", "`128·log2(10)`, units 2^-119.", c_10, 119),
        ("WI_L", "`ln2/128`, units 2^-135.", ln2.mul_pow2(-7), 135),
        ("WI_E4", "`1/4!`, units 2^-132.", inv_fact(4), 132),
        ("WI_E3", "`1/3!`, units 2^-130.", inv_fact(3), 130),
    ] {
        w(format!("\n/// {doc}\npub(crate) const {name}: u128 = {};\n", hex128(ufixed(&v, units))));
    }
    for (k, units) in [(10, 85), (9, 82), (8, 79), (7, 76), (6, 73), (5, 70)] {
        let v = ufixed(&inv_fact(k), units);
        let v = u64::try_from(v).unwrap_or(0);
        w(format!("\n/// `1/{k}!`, units 2^-{units}.\npub(crate) const WI_E{k}: u64 = {v:#018x};\n"));
    }
    w("\n/// `2^(j/128)` for j = 0..127, units 2^-127.\npub(crate) static WI_EXP2T: [u128; 128] = [\n".into());
    for j in 0..128 {
        let t = mp::exp2(&Big::from_i64(j).mul_pow2(-7), P).value;
        w(format!("    {},\n", hex128(ufixed(&t, 127))));
    }
    w("];\n".into());

    w("\n// The logarithms (steps L1-L4 of the tier's proof).\n".into());
    for (name, doc, v, units) in [
        ("WI_LN2_113", "`ln 2`, units 2^-113.", ln2.clone(), 113),
        ("WI_LOG2E", "`1/ln 2`, units 2^-127.", one.div(&ln2, P, Round::Nearest), 127),
        ("WI_LOG10E", "`1/ln 10`, units 2^-129.", one.div(&ln10, P, Round::Nearest), 129),
        ("WI_G3", "`1/3`, units 2^-129.", one.div(&Big::from_i64(3), P, Round::Nearest), 129),
    ] {
        w(format!("\n/// {doc}\npub(crate) const {name}: u128 = {};\n", hex128(ufixed(&v, units))));
    }
    for k in [7, 6, 5] {
        let v = u64::try_from(ufixed(&one.div(&Big::from_i64(k), P, Round::Nearest), 66)).unwrap_or(0);
        w(format!("\n/// `1/{k}`, units 2^-66.\npub(crate) const WI_G{k}: u64 = {v:#018x};\n"));
    }

    // L1: every interval's |u1| = |y''·c1' − 1| stays within 2^-7 at both ends (exact integers:
    // (128 + i)·C1 − 2^24 in units 2^-24).
    let c1s: Vec<u64> = (0..128).map(c1).collect();
    for (i, &c) in c1s.iter().enumerate() {
        let i = i as i128;
        for end in [128 + i, 129 + i] {
            let u = end * i128::from(c) - (1 << 24);
            assert!(u.abs() <= 1 << 17, "WI_C1[{i}]: |u1| exceeds 2^-7 at the interval's end {end}/128");
        }
    }
    w("\n/// `c1'` in units 2^-17: `round(2^25/(2i + 257))`, about `1/(1 + (i + 1/2)/128)` (17 bits); `2^17` and `2^16` for\n/// `i` = 0 and 127 (the intervals around 1). Every interval keeps `|y''·c1' − 1| <= 2^-7` (checked here).\npub(crate) static WI_C1: [u32; 128] = [\n".into());
    for row in c1s.chunks(8) {
        let s: Vec<String> = row.iter().map(u64::to_string).collect();
        w(format!("    {},\n", s.join(", ")));
    }
    w("];\n".into());
    w("\n/// `−ln c1` in units 2^-127, `c1 = c1'` (`i < 64`) or `2·c1'` (`i >= 64`, with the exponent one higher).\npub(crate) static WI_L1: [i128; 128] = [\n".into());
    for (i, &c) in c1s.iter().enumerate() {
        let c = Big::from_i64(c as i64).mul_pow2(if i >= 64 { -16 } else { -17 });
        let l = if c.sub(&one).is_zero() { Big::zero(false) } else { mp::log(&c, P).value.neg() };
        w(format!("    {},\n", shex128(ifixed(&l, 127))));
    }
    w("];\n".into());
    let c2s: Vec<u64> = (0..65).map(c2).collect();
    w("\n/// `c2` in units 2^-63: `round(2^63/(1 + i2/4096))` for i2 = -32..32 (index i2 + 32; exactly 1 at i2 = 0).\npub(crate) static WI_C2: [u64; 65] = [\n".into());
    for &c in &c2s {
        w(format!("    {c:#018x},\n"));
    }
    w("];\n".into());
    w("\n/// `−ln c2` in units 2^-133 for the `c2` of `WI_C2` (0 at i2 = 0).\npub(crate) static WI_L2: [i128; 65] = [\n".into());
    for &c in &c2s {
        // c2 can exceed i64::MAX: its top 63 bits and its last bit.
        let c = Big::from_i64((c >> 1) as i64).mul_pow2(-62).add(&Big::from_i64((c & 1) as i64).mul_pow2(-63));
        let l = if c.sub(&one).is_zero() { Big::zero(false) } else { mp::log(&c, P).value.neg() };
        w(format!("    {},\n", shex128(ifixed(&l, 133))));
    }
    w("];\n".into());

    // The binary128 tier (`wide_int::quad::explog`) carries the same tables to 192 bits: a third
    // limb below each word, and the longer series' coefficients.
    w("\n// The binary128 tier (`wide_int::quad::explog`, steps X1-X6 and L1-L4 of its proof).\n".into());
    for (name, doc, v, units) in [
        ("WQ_L_LO", "`WI_L`'s third limb: `WI_L·2^64 + WQ_L_LO` is `ln2/128` in units 2^-199.", ln2.mul_pow2(-7), 135),
        ("WQ_LN2_LO", "`WI_LN2_113`'s third limb: `WI_LN2_113·2^64 + WQ_LN2_LO` is `ln 2` in units 2^-177.", ln2.clone(), 113),
    ] {
        w(format!("\n/// {doc}\npub(crate) const {name}: i64 = {};\n", shex64(third(&v, units))));
    }
    for (k, units) in [(12, 92), (11, 89)] {
        let v = u64::try_from(ufixed(&inv_fact(k), units)).unwrap_or(0);
        w(format!("\n/// `1/{k}!`, units 2^-{units}.\npub(crate) const WQ_E{k}: u64 = {v:#018x};\n"));
    }
    for (k, units) in [(7, 140), (6, 137), (5, 134)] {
        let v = hex128(ufixed(&inv_fact(k), units));
        w(format!("\n/// `1/{k}!`, units 2^-{units}.\npub(crate) const WQ_E{k}: u128 = {v};\n"));
    }
    for k in [11, 10, 9] {
        let v = u64::try_from(ufixed(&one.div(&Big::from_i64(k), P, Round::Nearest), 67)).unwrap_or(0);
        w(format!("\n/// `1/{k}`, units 2^-67.\npub(crate) const WQ_G{k}: u64 = {v:#018x};\n"));
    }
    for k in [7, 6, 5] {
        let v = hex128(ufixed(&one.div(&Big::from_i64(k), P, Round::Nearest), 130));
        w(format!("\n/// `1/{k}`, units 2^-130.\npub(crate) const WQ_G{k}: u128 = {v};\n"));
    }
    w("\n/// `WI_EXP2T`'s third limbs: `WI_EXP2T·2^64 + WQ_EXP2T_LO` is `2^(j/128)` in units 2^-191.\npub(crate) static WQ_EXP2T_LO: [i64; 128] = [\n".into());
    for j in 0..128 {
        let t = mp::exp2(&Big::from_i64(j).mul_pow2(-7), P).value;
        w(format!("    {},\n", shex64(third(&t, 127))));
    }
    w("];\n".into());
    w("\n/// `WI_L1`'s third limbs: `WI_L1·2^64 + WQ_L1_LO` is `−ln c1` in units 2^-191.\npub(crate) static WQ_L1_LO: [i64; 128] = [\n".into());
    for (i, &c) in c1s.iter().enumerate() {
        let c = Big::from_i64(c as i64).mul_pow2(if i >= 64 { -16 } else { -17 });
        let l = if c.sub(&one).is_zero() { Big::zero(false) } else { mp::log(&c, P).value.neg() };
        w(format!("    {},\n", shex64(third(&l, 127))));
    }
    w("];\n".into());
    w("\n/// `WI_L2`'s third limbs: `WI_L2·2^64 + WQ_L2_LO` is `−ln c2` in units 2^-197.\npub(crate) static WQ_L2_LO: [i64; 65] = [\n".into());
    for &c in &c2s {
        let c = Big::from_i64((c >> 1) as i64).mul_pow2(-62).add(&Big::from_i64((c & 1) as i64).mul_pow2(-63));
        let l = if c.sub(&one).is_zero() { Big::zero(false) } else { mp::log(&c, P).value.neg() };
        w(format!("    {},\n", shex64(third(&l, 133))));
    }
    w("];\n".into());
}
