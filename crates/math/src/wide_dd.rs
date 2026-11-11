//! The double-double first tier of the wide elementary functions (spec §10.3, D-10.19, ruling
//! #2760 option A): an approximation of `f(x)` in double-double arithmetic ([`crate::dd`]) with a
//! proved relative error bound `2^-b`, and the rounding test of Ziv's strategy ([`decide`]) on
//! it. When every value within the bound rounds alike in the caller's direction, that rounding is
//! the correctly rounded result; otherwise (about `2^(PREC + 2 − b)` of the operands) the caller
//! takes the 384-bit route of [`crate::wide`], so results stay correctly rounded in every mode.
//!
//! The tier serves formats of at most 64 significand bits (x87 `long double`): [`decide`] is
//! generic over the format and refuses whenever `b` cannot decide its precision, so `_Float128`
//! runs the same entry points and falls through until a triple-double kernel gives it a bound
//! near `2^-140` (the kernels below return the approximation, a power-of-two scale and `b`,
//! the shape a wider tier fills in).
//!
//! Kernels (constants from Python `decimal` at 80 digits, each pinned against the 384-bit
//! arithmetic by `constants_are_the_rounded_values`):
//! * `exp`, `exp2`, `exp10`, `expm1`: `x·c = k + f` with `c = 64/ln 2`, `64` or `64·log2 10` held
//!   as a 128-bit fixed-point integer, the product taken exactly in integers (no cancellation),
//!   `|f| <= 1/2`; `r = f·ln2/64`, `e^r − 1` by a degree-11 polynomial, `2^(k/64)` from a
//!   64-entry table. Bound (exp, exp2, exp10): `r` within `2^-110` absolute (fixed-point
//!   truncation `2^-128`, constant `2^-109` at `|x| < 2^14`, conversion `2^-106` of `|f|`, the
//!   product with `ln2/64` `2^-104` of `|r| <= 2^-7.5`); the polynomial's truncation below
//!   `2^-107`, its double-double steps `2^-104` relative each on terms below `|r|`; the table
//!   entry `2^-106`, the final product and sum `2^-104`: below `2^-103` relative, `b = 100`.
//!   `expm1` adds the subtraction of 1: relative growth at most `e^x/|e^x − 1| < 2^7.6` for
//!   `|x| >= ln2/128` (estimate `2^-95.9`, measured `2^-95`: `b = 92`); below `2^-8` the polynomial runs on `x` itself (`b = 100`).
//! * `log`, `log2`, `log10`, `log1p`: `x = 2^e·m`, `m` in `[0.75, 1.5)` with 113 bits, a
//!   13-bit reciprocal `c` of `m`'s 1/128-interval, `u = m·c − 1` exact in integers
//!   (`|u| <= 2^-7`), `ln(1 + u) = 2·atanh(u/(2 + u))` to `s^13`, `ln x = e·ln 2 − ln c +
//!   ln(1 + u)`. Near 1 (`e = 0`, `c = 1`) only the series runs (relative). Elsewhere
//!   `|ln x| >= 2^-8` and each term is within `2^-104` of itself and at most 2.5 times the
//!   result: below `2^-102` relative, `b = 100` (`log2`, `log10`: one more product, still within).

#![allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::as_conversions,
    clippy::indexing_slicing
)] // fixed-point words, exponents and table indices of bounded range (each noted where it is formed)

use crate::dd::{self, fast_two_sum, Dd};
use crate::float::{self, Exact, Float, Num};
use crate::status::{Rounding, Status};

/// The relative bound exponent of the exponentials and logarithms.
const B_MAIN: u32 = 100;
/// The relative bound exponent of `expm1` where it subtracts 1 (`|x| >= ln2/128`).
const B_EXPM1: u32 = 92;
/// Below `2^RAW` in magnitude `exp`, `expm1` and `log1p` take `x` itself as the reduced
/// argument (`|x| < ln2/128` keeps the polynomial's bound; the series needs `|x| <= 2^-7`).
const RAW: i32 = -8;
/// Mask of an `f64` significand field.
const MANT: u64 = (1 << 52) - 1;

/// The functions of the tier.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Kernel {
    /// `e^x`.
    Exp,
    /// `2^x`.
    Exp2,
    /// `10^x`.
    Exp10,
    /// `e^x − 1`.
    Expm1,
    /// `ln x`.
    Log,
    /// `log2 x`.
    Log2,
    /// `log10 x`.
    Log10,
    /// `ln(1 + x)`.
    Log1p,
}

/// `f(x)` rounded into `F` in `mode` when the tier decides it; `None` sends the caller to the
/// 384-bit route. `n` is finite, nonzero, inside the function's domain and outside the cases the
/// caller settles itself (exact results, tiny arguments, overflow and underflow limits).
pub(crate) fn eval<F: Float>(k: Kernel, n: &Num, mode: Rounding) -> Option<(F, Status)> {
    if F::PREC > 64 {
        return None;
    }
    let (v, scale, bits) = kernel(k, n)?;
    decide::<F>(v, scale, bits, mode)
}

/// The approximation `v·2^scale` of `f(x)` and its bound exponent `b`: `|f(x) − v·2^scale| <
/// 2^-b·|v·2^scale|`. `None` outside the ranges the reductions are proved for.
fn kernel(k: Kernel, n: &Num) -> Option<(Dd, i32, u32)> {
    match k {
        Kernel::Exp | Kernel::Exp2 | Kernel::Exp10 => {
            let (v, scale) = exp_kernel(k, n)?;
            Some((v, scale, B_MAIN))
        }
        Kernel::Expm1 => expm1_kernel(n),
        Kernel::Log => Some((ln_kernel(n.sig, n.exp), 0, B_MAIN)),
        Kernel::Log2 => Some((ln_kernel(n.sig, n.exp).mul(Dd::of(LOG2E)), 0, B_MAIN)),
        Kernel::Log10 => Some((ln_kernel(n.sig, n.exp).mul(Dd::of(LOG10E)), 0, B_MAIN)),
        Kernel::Log1p => log1p_kernel(n).map(|v| (v, 0, B_MAIN)),
    }
}

/// The rounding test: `v·2^scale` rounded into `F` in `mode` when every value within `2^-bits`
/// of it (relative) rounds alike, else `None`. Also `None` for a result outside `F`'s normal
/// range (the subnormal grid and the overflow threshold are the 384-bit route's).
///
/// Proof. `hi = mh·2^eh` (53-bit `mh`) and `|lo| <= ulp(hi)/2` (every double-double the kernels
/// return ends in a `fast_two_sum`; larger `lo` is refused). In units of `2^(eh − 74)`, `S =
/// mh·2^74 ± ⌊|lo|⌋` is within 1 unit of `V = hi + lo` and lies in `[2^125, 2^127)`; after the
/// shift that puts its top bit at 127, within 4 units. The true value `T` has `|T − V| <
/// 2^-bits·|V| < 2^(128 − bits)` units, so `|T − S| < err = 2^(128 − bits) + 4`. With
/// `L = 128 − PREC` bits below `F`'s last place, `f = S mod 2^L` is `S`'s distance above the
/// representable value `g` below it; `half = 2^(L − 1)`. If `err <= f <= half − err` (or the
/// same above the midpoint), `T` and `S + sticky` lie strictly between `g` and the midpoint (or
/// the midpoint and the next value), where every rounding direction maps them to the same value
/// and neither is exact: the rounding of `S` with sticky set is the rounding of `T`, inexact. The
/// range check keeps the result and its neighbours normal: no underflow, no overflow.
pub(crate) fn decide<F: Float>(v: Dd, scale: i32, bits: u32, mode: Rounding) -> Option<(F, Status)> {
    if bits <= F::PREC + 2 {
        return None;
    }
    let hb = v.hi.to_bits();
    let be = ((hb >> 52) & 0x7ff) as i32;
    if be == 0 || be == 0x7ff {
        return None;
    }
    let neg = hb >> 63 != 0;
    let eh = be - 1075;
    let mut s = u128::from((hb & MANT) | (1 << 52)) << 74;
    let lb = v.lo.to_bits();
    if lb << 1 != 0 {
        let le = ((lb >> 52) & 0x7ff) as i32;
        if le == 0x7ff {
            return None;
        }
        let ml = if le == 0 { lb & MANT } else { (lb & MANT) | (1 << 52) };
        // lo = ml·2^(max(le, 1) − 1075): its offset in units of 2^(eh − 74).
        let d = le.max(1) - 1075 - (eh - 74);
        let part = if d >= 0 {
            if d > 21 {
                return None;
            }
            u128::from(ml) << d
        } else if d > -64 {
            u128::from(ml >> -d)
        } else {
            0
        };
        if part > 1 << 73 {
            return None;
        }
        if (lb >> 63 != 0) == neg {
            s += part;
        } else {
            s -= part;
        }
    }
    let lz = s.leading_zeros();
    let s = s << lz;
    let q = eh - 74 - lz as i32 + scale;
    let top = q + 127;
    if top < 2 - F::EMAX || top > F::EMAX - 1 {
        return None;
    }
    let err = (1u128 << (128 - bits)) + 4;
    let low = 128 - F::PREC;
    let mask = (1u128 << low) - 1;
    let half = 1u128 << (low - 1);
    let f = s & mask;
    let clear = if f < half { f >= err && f + err <= half } else { f >= half + err && f + err <= mask };
    if !clear {
        return None;
    }
    let (r, flags) = float::round::<F>(Exact { neg, sig: s, exp: q, sticky: true }, mode);
    Some((r, float::range_errno(r, Status::flags(flags))))
}

/// `v·2^scale` as a double-double, within `2^-105` relative (the bits below the leading 106 of
/// `|v|` are truncated). Every caller keeps `scale − 127 >= −1000`, so both parts are normal.
fn dd_of(v: i128, scale: i32) -> Dd {
    let m = v.unsigned_abs();
    if m == 0 {
        return Dd::from(0.0);
    }
    let lz = m.leading_zeros() as i32;
    let m = m << lz;
    let e = scale - lz;
    // Two 53-bit integers: exact in i64 and in f64.
    let hi = ((m >> 75) as i64) as f64 * dd::pow2(e + 75);
    let lo = ((((m >> 22) as u64) & ((1 << 53) - 1)) as i64) as f64 * dd::pow2(e + 22);
    let r = fast_two_sum(hi, lo);
    if v < 0 { r.neg() } else { r }
}

/// A finite operand of at most 64 significand bits, exactly.
fn dd_of_num(n: &Num) -> Option<Dd> {
    let v = i128::try_from(n.sig).ok()?;
    Some(dd_of(if n.neg { -v } else { v }, n.exp))
}

/// `x·c·2^-cs = k + f` with integral `k` and `|f| <= 1/2`: `k` and `r = f·ln2/64`, `r` within
/// `2^-110` absolute. The product is exact in integers (192 bits); `sh = cs − exp(x)` must leave
/// a 128-bit fraction and an integer part below 2^63 (`129 <= sh <= 255`).
fn reduce(n: &Num, c: u128, cs: i32) -> Option<(i64, Dd)> {
    let sig = u64::try_from(n.sig).ok()?;
    let sh = cs - n.exp;
    if !(129..=255).contains(&sh) {
        return None;
    }
    let a = u128::from(sig) * u128::from(c as u64);
    let b = u128::from(sig) * (c >> 64);
    let mid = (a >> 64) + u128::from(b as u64);
    let p2 = ((b >> 64) + (mid >> 64)) as u64;
    let plo = (mid << 64) | u128::from(a as u64);
    let s1 = (sh - 128) as u32; // 1..=127
    let frac = (plo >> s1) | (u128::from(p2) << (128 - s1));
    let int = if s1 < 64 { p2 >> s1 } else { 0 };
    // Round to nearest: a fraction of 1/2 or more carries; as i128 it is then f − 1.
    let k = int as i64 + (frac >> 127) as i64;
    let f = dd_of(frac as i128, -128);
    let (k, f) = if n.neg { (-k, f.neg()) } else { (k, f) };
    Some((k, f.mul(Dd::of(LN2_64))))
}

/// `e^r − 1` for `|r| <= 2^-7.5`: degree 11, the terms from `r^6` (below `2^-54`) in `f64`.
fn expm1_poly(r: Dd) -> Dd {
    let x = r.hi;
    let tail = f64::from_bits(E6)
        + x * (f64::from_bits(E7)
            + x * (f64::from_bits(E8) + x * (f64::from_bits(E9) + x * (f64::from_bits(E10) + x * f64::from_bits(E11)))));
    let mut acc = Dd::of(E5).add_f64(x * tail);
    acc = acc.mul(r).add(Dd::of(E4));
    acc = acc.mul(r).add(Dd::of(E3));
    acc = acc.mul(r).add(Dd::of(E2));
    r.add(acc.mul(r).mul(r))
}

/// `e^x`, `2^x` or `10^x` as `v·2^scale` (`|x| < 2^14`).
fn exp_kernel(k: Kernel, n: &Num) -> Option<(Dd, i32)> {
    if n.top() >= 14 {
        return None;
    }
    let (k, r) = match k {
        Kernel::Exp if n.top() < RAW => (0, dd_of_num(n)?),
        Kernel::Exp2 => reduce(n, 1 << 127, 121)?,
        Kernel::Exp10 => reduce(n, C_10, 120)?,
        _ => reduce(n, C_E, 121)?,
    };
    let t = Dd::of(EXP2_64[(k & 63) as usize]);
    Some((t.add(t.mul(expm1_poly(r))), (k >> 6) as i32))
}

/// `e^x − 1` (`x >= −51`: the caller settles more negative `x` as −1 plus a tiny amount).
fn expm1_kernel(n: &Num) -> Option<(Dd, i32, u32)> {
    if n.top() >= 14 {
        return None;
    }
    if n.top() < RAW {
        return Some((expm1_poly(dd_of_num(n)?), 0, B_MAIN));
    }
    let (k, r) = reduce(n, C_E, 121)?;
    let p = expm1_poly(r);
    if k == 0 {
        // |x| <= ln2/128 and >= 2^-8: r is within 2^-102 relative of x, no subtraction.
        return Some((p, 0, B_MAIN));
    }
    let t = Dd::of(EXP2_64[(k & 63) as usize]);
    let w = t.add(t.mul(p));
    let big = (k >> 6) as i32;
    if big >= 110 {
        // The 1 lies below 2^-109 of the result.
        return Some((w, big, B_MAIN));
    }
    if big < -80 {
        return None;
    }
    Some((w.scale(dd::pow2(big)).add_f64(-1.0), 0, B_EXPM1))
}

/// `2·atanh(u/(2 + u)) = ln(1 + u)` for `|u| <= 2^-7`, relative `2^-103`: `|s| <= 2^-8`, the
/// series to `s^13` (the next term below `2^-112` of `s`), `s^9` onward in `f64`.
fn ln1p_series(u: Dd) -> Dd {
    let s = u.div(Dd::from(2.0).add(u));
    let z = s.sqr();
    let tail = f64::from_bits(A9) + z.hi * (f64::from_bits(A11) + z.hi * f64::from_bits(A13));
    let mut acc = Dd::of(A7).add_f64(z.hi * tail);
    acc = acc.mul(z).add(Dd::of(A5));
    acc = acc.mul(z).add(Dd::of(A3));
    s.add(s.mul(z).mul(acc)).scale(2.0)
}

/// `ln(y·2^q)` for `y > 0` (relative `2^-102`; `y` truncated to 114 bits, `2^-113`).
fn ln_kernel(y: u128, q: i32) -> Dd {
    let lz = y.leading_zeros() as i32;
    let m = if lz >= 14 { y << (lz - 14) } else { y >> (14 - lz) };
    // y·2^q = (m/2^113)·2^e, m in [2^113, 2^114).
    let mut e = q + 127 - lz;
    let i = ((m >> 106) & 0x7f) as usize;
    let c = u128::from(RCP[i]);
    // m·c < 2^114·2^12.5: exact in u128 and i128; u = m·c/2^125 − 1 (or with m/2 and e + 1).
    let (one, sc) = if i < 64 {
        (1u128 << 125, -125)
    } else {
        e += 1;
        (1u128 << 126, -126)
    };
    let u = dd_of((m * c) as i128 - one as i128, sc);
    let l1 = ln1p_series(u);
    if e == 0 && c == 4096 {
        return l1;
    }
    Dd::of(LN2).mul_f64(f64::from(e)).add(Dd::of(LOGC[i])).add(l1)
}

/// `ln(1 + x)` for `x > −1`, `|x| >= 2^-67`: the series on `x` below `2^-8`, else `1 + x` exactly
/// in integers (to 114 bits when `x > 2^113`) through [`ln_kernel`].
fn log1p_kernel(n: &Num) -> Option<Dd> {
    if n.top() < RAW {
        return Some(ln1p_series(dd_of_num(n)?));
    }
    let sig = u64::try_from(n.sig).ok()?;
    let sig = u128::from(sig);
    let (y, q) = if n.exp > 0 {
        if n.neg {
            return None;
        }
        let one = if n.exp <= 50 { 1u128 << (50 - n.exp) } else { 0 };
        ((sig << 50) + one, n.exp - 50)
    } else {
        // |x| >= 2^-8 with 64 bits: −exp(x) <= 71.
        let one = 1u128.checked_shl(n.exp.unsigned_abs()).filter(|_| n.exp >= -72)?;
        if n.neg {
            (one.checked_sub(sig).filter(|&y| y != 0)?, n.exp)
        } else {
            (one + sig, n.exp)
        }
    };
    Some(ln_kernel(y, q))
}

// ------------------------------------------------------------------------------------------
// Constants (Python `decimal`, 80 digits; checked by `constants_are_the_rounded_values`).
// ------------------------------------------------------------------------------------------

// 2^(j/64), j = 0..63
const EXP2_64: [[u64; 2]; 64] = [
    [0x3ff0000000000000, 0x0000000000000000],
    [0x3ff02c9a3e778061, 0xbc719083535b085d],
    [0x3ff059b0d3158574, 0x3c8d73e2a475b465],
    [0x3ff0874518759bc8, 0x3c6186be4bb284ff],
    [0x3ff0b5586cf9890f, 0x3c98a62e4adc610b],
    [0x3ff0e3ec32d3d1a2, 0x3c403a1727c57b53],
    [0x3ff11301d0125b51, 0xbc96c51039449b3a],
    [0x3ff1429aaea92de0, 0xbc932fbf9af1369e],
    [0x3ff172b83c7d517b, 0xbc819041b9d78a76],
    [0x3ff1a35beb6fcb75, 0x3c8e5b4c7b4968e4],
    [0x3ff1d4873168b9aa, 0x3c9e016e00a2643c],
    [0x3ff2063b88628cd6, 0x3c8dc775814a8495],
    [0x3ff2387a6e756238, 0x3c99b07eb6c70573],
    [0x3ff26b4565e27cdd, 0x3c82bd339940e9d9],
    [0x3ff29e9df51fdee1, 0x3c8612e8afad1255],
    [0x3ff2d285a6e4030b, 0x3c90024754db41d5],
    [0x3ff306fe0a31b715, 0x3c86f46ad23182e4],
    [0x3ff33c08b26416ff, 0x3c932721843659a6],
    [0x3ff371a7373aa9cb, 0xbc963aeabf42eae2],
    [0x3ff3a7db34e59ff7, 0xbc75e436d661f5e3],
    [0x3ff3dea64c123422, 0x3c8ada0911f09ebc],
    [0x3ff4160a21f72e2a, 0xbc5ef3691c309278],
    [0x3ff44e086061892d, 0x3c489b7a04ef80d0],
    [0x3ff486a2b5c13cd0, 0x3c73c1a3b69062f0],
    [0x3ff4bfdad5362a27, 0x3c7d4397afec42e2],
    [0x3ff4f9b2769d2ca7, 0xbc94b309d25957e3],
    [0x3ff5342b569d4f82, 0xbc807abe1db13cad],
    [0x3ff56f4736b527da, 0x3c99bb2c011d93ad],
    [0x3ff5ab07dd485429, 0x3c96324c054647ad],
    [0x3ff5e76f15ad2148, 0x3c9ba6f93080e65e],
    [0x3ff6247eb03a5585, 0xbc9383c17e40b497],
    [0x3ff6623882552225, 0xbc9bb60987591c34],
    [0x3ff6a09e667f3bcd, 0xbc9bdd3413b26456],
    [0x3ff6dfb23c651a2f, 0xbc6bbe3a683c88ab],
    [0x3ff71f75e8ec5f74, 0xbc816e4786887a99],
    [0x3ff75feb564267c9, 0xbc90245957316dd3],
    [0x3ff7a11473eb0187, 0xbc841577ee04992f],
    [0x3ff7e2f336cf4e62, 0x3c705d02ba15797e],
    [0x3ff82589994cce13, 0xbc9d4c1dd41532d8],
    [0x3ff868d99b4492ed, 0xbc9fc6f89bd4f6ba],
    [0x3ff8ace5422aa0db, 0x3c96e9f156864b27],
    [0x3ff8f1ae99157736, 0x3c85cc13a2e3976c],
    [0x3ff93737b0cdc5e5, 0xbc675fc781b57ebc],
    [0x3ff97d829fde4e50, 0xbc9d185b7c1b85d1],
    [0x3ff9c49182a3f090, 0x3c7c7c46b071f2be],
    [0x3ffa0c667b5de565, 0xbc9359495d1cd533],
    [0x3ffa5503b23e255d, 0xbc9d2f6edb8d41e1],
    [0x3ffa9e6b5579fdbf, 0x3c90fac90ef7fd31],
    [0x3ffae89f995ad3ad, 0x3c97a1cd345dcc81],
    [0x3ffb33a2b84f15fb, 0xbc62805e3084d708],
    [0x3ffb7f76f2fb5e47, 0xbc75584f7e54ac3b],
    [0x3ffbcc1e904bc1d2, 0x3c823dd07a2d9e84],
    [0x3ffc199bdd85529c, 0x3c811065895048dd],
    [0x3ffc67f12e57d14b, 0x3c92884dff483cad],
    [0x3ffcb720dcef9069, 0x3c7503cbd1e949db],
    [0x3ffd072d4a07897c, 0xbc9cbc3743797a9c],
    [0x3ffd5818dcfba487, 0x3c82ed02d75b3707],
    [0x3ffda9e603db3285, 0x3c9c2300696db532],
    [0x3ffdfc97337b9b5f, 0xbc91a5cd4f184b5c],
    [0x3ffe502ee78b3ff6, 0x3c839e8980a9cc8f],
    [0x3ffea4afa2a490da, 0xbc9e9c23179c2893],
    [0x3ffefa1bee615a27, 0x3c9dc7f486a4b6b0],
    [0x3fff50765b6e4540, 0x3c99d3e12dd8a18b],
    [0x3fffa7c1819e90d8, 0x3c874853f3a5931e],
];
const C_E: u128 = 0xb8aa3b295c17f0bbbe87fed0691d3e89; // 64/ln 2 * 2^121
const C_10: u128 = 0xd49a784bcd1b8afe492bf6ff4dafdb4d; // 64 log2(10) * 2^120
const LN2_64: [u64; 2] = [0x3f862e42fefa39ef, 0x3c1abc9e3b39803f];
const LN2: [u64; 2] = [0x3fe62e42fefa39ef, 0x3c7abc9e3b39803f];
const LOG2E: [u64; 2] = [0x3ff71547652b82fe, 0x3c7777d0ffda0d24];
const LOG10E: [u64; 2] = [0x3fdbcb7b1526e50e, 0x3c695355baaafad3];
const E2: [u64; 2] = [0x3fe0000000000000, 0x0000000000000000]; // 1/2!
const E3: [u64; 2] = [0x3fc5555555555555, 0x3c65555555555555]; // 1/3!
const E4: [u64; 2] = [0x3fa5555555555555, 0x3c45555555555555]; // 1/4!
const E5: [u64; 2] = [0x3f81111111111111, 0x3c01111111111111]; // 1/5!
const E6: u64 = 0x3f56c16c16c16c17; // 1/6!
const E7: u64 = 0x3f2a01a01a01a01a; // 1/7!
const E8: u64 = 0x3efa01a01a01a01a; // 1/8!
const E9: u64 = 0x3ec71de3a556c734; // 1/9!
const E10: u64 = 0x3e927e4fb7789f5c; // 1/10!
const E11: u64 = 0x3e5ae64567f544e4; // 1/11!
const A3: [u64; 2] = [0x3fd5555555555555, 0x3c75555555555555]; // 1/3
const A5: [u64; 2] = [0x3fc999999999999a, 0xbc6999999999999a]; // 1/5
const A7: [u64; 2] = [0x3fc2492492492492, 0x3c62492492492492]; // 1/7
const A9: u64 = 0x3fbc71c71c71c71c; // 1/9
const A11: u64 = 0x3fb745d1745d1746; // 1/11
const A13: u64 = 0x3fb3b13b13b13b14; // 1/13
// c_i * 4096: m * c_i near 1 for m in the i-th interval (i < 64: [1 + i/128, 1 + (i+1)/128);
// i >= 64: half of it)
const RCP: [u16; 128] = [
    4096, 4049, 4018, 3987, 3957, 3927, 3898, 3869, 3841, 3813, 3785, 3758, 3732, 3705, 3679, 3654,
    3628, 3603, 3579, 3554, 3531, 3507, 3484, 3461, 3438, 3416, 3393, 3372, 3350, 3329, 3308, 3287,
    3267, 3246, 3226, 3207, 3187, 3168, 3149, 3130, 3112, 3093, 3075, 3057, 3039, 3022, 3005, 2987,
    2970, 2954, 2937, 2921, 2905, 2889, 2873, 2857, 2842, 2826, 2811, 2796, 2781, 2767, 2752, 2738,
    5447, 5419, 5391, 5364, 5336, 5309, 5282, 5256, 5230, 5204, 5178, 5153, 5128, 5103, 5078, 5053,
    5029, 5005, 4981, 4958, 4934, 4911, 4888, 4866, 4843, 4821, 4799, 4777, 4755, 4734, 4713, 4692,
    4671, 4650, 4629, 4609, 4589, 4569, 4549, 4529, 4510, 4491, 4472, 4453, 4434, 4415, 4397, 4378,
    4360, 4342, 4324, 4306, 4289, 4271, 4254, 4237, 4220, 4203, 4186, 4169, 4153, 4136, 4120, 4096,
];
// -ln(c_i)
const LOGC: [[u64; 2]; 128] = [
    [0x0000000000000000, 0x0000000000000000],
    [0x3f87a2c82e212c65, 0xbc2d1c95731568a4],
    [0x3f93b024b78c5669, 0x3c3e23a02f82a1d4],
    [0x3f9b9e8027e1918e, 0xbc3bb4f4fcfb9727],
    [0x3fa1ad398c6cd588, 0xbc4b49716ef271a6],
    [0x3fa592bbc15215c9, 0xbc1e5634e6c1fbfc],
    [0x3fa95e430f8ce45e, 0xbc367bb43a6e5d7f],
    [0x3fad310ba20455a1, 0x3c44dbdae98f9f4c],
    [0x3fb074883629640b, 0xbc451ee824c30c1f],
    [0x3fb254062f0a9417, 0xbbfaf40c3a9bab6d],
    [0x3fb4370ce02b7de8, 0xbc4308315b2d0329],
    [0x3fb60c38ba79945d, 0xbc53bc513ed6a1c8],
    [0x3fb7d33687c293c9, 0xbc5cf063e63e7075],
    [0x3fb9af124d64c626, 0xbc44f5f8c466d77a],
    [0x3fbb7c9832f5801a, 0x3c0358893be169bf],
    [0x3fbd3b73f37e1f9b, 0xbc5fd984b5ff12ef],
    [0x3fbf0f70cdd992e3, 0x3c3f6c272c1dca71],
    [0x3fc06a4d1d26c5e6, 0xbc6b22efa3b4dedf],
    [0x3fc1454d8953741c, 0x3c66f103ed5fdcea],
    [0x3fc22aff2ddbd971, 0xbc3535834b0ffc28],
    [0x3fc2ffbf29a6645c, 0xbc6b4621a2bc5451],
    [0x3fc3df3ab13505f7, 0xbc68a4f7c9ebdc82],
    [0x3fc4b6d6fefe22a4, 0x3c6767ab73ca8d5e],
    [0x3fc58fe0e4c62eae, 0xbc40111e0128a1b8],
    [0x3fc66a5d42a3ad34, 0x3c6267540052ff1d],
    [0x3fc73cb9074fd14d, 0xbc6521a000b4cf01],
    [0x3fc81a18b4220535, 0xbc575d551b2a6857],
    [0x3fc8e588ebac2dbf, 0xbc646a9a5dd7ff12],
    [0x3fc9bc062f26fc3a, 0x3c6b03013cda9bfc],
    [0x3fca8a14ffee66bd, 0x3c6f2ba95e8bb64b],
    [0x3fcb5971a213acdb, 0xbc6e2f8aadc42f8f],
    [0x3fcc2a205610593f, 0x3c6839904bfa522d],
    [0x3fccf21d5ecbaa65, 0xbc5163340c0236e7],
    [0x3fcdc56cae452f5a, 0xbc60abb63cfd2336],
    [0x3fce8ff2622babc7, 0x3c33d33981e51981],
    [0x3fcf518262c38082, 0x3c40b8a15d088ef6],
    [0x3fd00f40470c7324, 0x3c6a5f3a45f05206],
    [0x3fd07138604d5862, 0x3c7cdb16ed4e9138],
    [0x3fd0d3c7586cd5e4, 0x3c6642610bcbfdce],
    [0x3fd136ef02e8290c, 0xbc560c396093faf8],
    [0x3fd1956d3b9bc2fa, 0x3c77b9d68d50a15d],
    [0x3fd1f9c39f74c557, 0x3c7515541d5d6c35],
    [0x3fd2596410df963a, 0xbc4f442de36410f7],
    [0x3fd2b9943b06bd76, 0xbc04c4833124d84e],
    [0x3fd31a55d07a8591, 0xbc75dfb4b1118495],
    [0x3fd3763e64645463, 0xbc6c1adc46953834],
    [0x3fd3d2abb3b3b4df, 0xbc50479718ca1525],
    [0x3fd4351b33743eb9, 0xbc7340f4b656e1c0],
    [0x3fd4929e8db4e6e4, 0x3c55955b1c3785b0],
    [0x3fd4eb1f36b07184, 0x3c31d1b95e5ecebe],
    [0x3fd549aec5def881, 0x3c77166af2b67691],
    [0x3fd5a32eb2e4eacb, 0x3c75d5a4b18b2a7f],
    [0x3fd5fd2c78c78828, 0x3c6242ad6f292541],
    [0x3fd657a97b64d437, 0xbc69fdb506def5eb],
    [0x3fd6b2a72488ad8f, 0xbc7740ebada20556],
    [0x3fd70e26e40eb5fa, 0x3c7aedeca7617041],
    [0x3fd76466197e36de, 0x3c4375c5d5ef4c50],
    [0x3fd7c0e612785c73, 0x3c78f713852c0d24],
    [0x3fd81818203afc80, 0xbc4bbc2080a7a682],
    [0x3fd86fc19d05148e, 0x3c7fc8edbd999eff],
    [0x3fd8c7e3d1c80b30, 0xbc7eb27842f61e52],
    [0x3fd91a93f1f4a7a8, 0x3c7f9e3c8bcf8d6a],
    [0x3fd973a3431356ae, 0xbc689d2816cf838f],
    [0x3fd9c73305d47ebb, 0x3c5eec9c7be40a02],
    [0xbfd23e5398b931d0, 0xbc7d3331000bcafe],
    [0xbfd1e9e3678891f4, 0xbc751d6e1f04c8fb],
    [0xbfd195033b43fa99, 0xbc7c666783d93b26],
    [0xbfd142bfeb9a0474, 0x3c79e7a4a75619ee],
    [0xbfd0ed005f657da4, 0xbc7c56bd2abfe82a],
    [0xbfd099e371c0bc82, 0x3c49f2684d93ab38],
    [0xbfd0465a08154ffa, 0x3c705f0ad83878e2],
    [0xbfcfeb0233e607cc, 0xbc66e32d5e8c707f],
    [0xbfcf488311d1b493, 0x3c6058a0d0c0c448],
    [0xbfcea5349e23ac0e, 0x3c5b2ce30cd2d061],
    [0xbfce0114c533197f, 0x3c44990bcaac412f],
    [0xbfcd627d7c1b2da7, 0x3c641d2fa017c9e1],
    [0xbfccc320c0176502, 0xbc6039a653793a85],
    [0xbfcc22fca31887dd, 0xbbf58817bad161d4],
    [0xbfcb820f2fc7e508, 0xbc677bcc3821db0f],
    [0xbfcae05669628825, 0xbc5b09879b60e908],
    [0xbfca4454820d19fa, 0x3c589c33fab39712],
    [0xbfc9a7938d209947, 0xbc663756352dcf6d],
    [0xbfc90a11b46b9b56, 0xbc001375545b6e9a],
    [0xbfc8726935acac62, 0xbc6764c6465f6264],
    [0xbfc7d36832b8f0e3, 0x3c574cf74e521faa],
    [0xbfc73a4d01722e5c, 0x3c69403e4d3a5052],
    [0xbfc6a079d0f7aad2, 0x3c1eedcbac2a7f18],
    [0xbfc60ca8fe8858af, 0xbc62287fa61504f0],
    [0xbfc57168c4aff0e6, 0x3c52e7b6fdae802c],
    [0xbfc4dc378969b1c1, 0xbc5ad9bb79187618],
    [0xbfc446579dc223de, 0x3c6c2a5423d9a9f7],
    [0xbfc3afc766c1e210, 0x3c52bf469fab63ca],
    [0xbfc3188543c098a1, 0xbc551c0051368273],
    [0xbfc2877bbc0b6ba6, 0x3c37205e9247dde8],
    [0xbfc1f5cd21a0d938, 0xbc614d5ddc4da66e],
    [0xbfc16377fb124192, 0x3c4e540be89c1eaa],
    [0xbfc0d07ac9dcbcf8, 0x3c63bdf113483a79],
    [0xbfc03cd40a51ac0d, 0xbc62f3828ce0d1ff],
    [0xbfbf510466fd80bf, 0xbc5b95ae75fde97e],
    [0xbfbe353fec9a2699, 0x3c184447930b489e],
    [0xbfbd183f87ea737c, 0xbc546a84404a2c41],
    [0xbfbbfa007672aa23, 0x3c5745a21d1323b7],
    [0xbfbada7fec6ac801, 0xbc401a5d7c4daf48],
    [0xbfb9b9bb14949167, 0x3c4cdb2f79dfe743],
    [0xbfb8a6377a915c29, 0x3c51296e6f9d7a43],
    [0xbfb7918a1bf981d6, 0xbc411d0f40e35710],
    [0xbfb67bb0726ec0fc, 0x3c5b692c214ddbec],
    [0xbfb564a7ef514311, 0xbc49517cbf107190],
    [0xbfb44c6dfb9b7606, 0xbc575f0688514f9b],
    [0xbfb332fff7bd1f18, 0xbc441527a4ec5456],
    [0xbfb22743457a9c8e, 0x3c54bdf5146181b1],
    [0xbfb10b75afd660c6, 0xbc37330e591f5790],
    [0xbfaffae9119b9303, 0xbc3ba13162a9c446],
    [0xbfaddcaadb46ef1b, 0xbc109ab6f79cf161],
    [0xbfabbc2bfc44f417, 0xbc3e5bafa0943c21],
    [0xbfa99967a4f2b1c8, 0xbc4976b97544edd3],
    [0xbfa792e93e5199ed, 0x3c4a883781558c79],
    [0xbfa56bac43ed7f9a, 0x3c25c8202e48debe],
    [0xbfa360ebf5d83765, 0xbc49281d2d2c97b2],
    [0xbfa15413ca0e72a8, 0x3c4b1d2c1c45ef5e],
    [0xbf9e8a3ee30cdcac, 0xbc07086b1c00b395],
    [0xbf9a68151fb5c04a, 0x3bf080ed414e3611],
    [0xbf9641a176270d6f, 0xbc38ca45dca6d9e5],
    [0xbf9216daf6d9321a, 0x3c387ca4f815f331],
    [0xbf8c4db352e6d472, 0x3c2f0d97312e07a1],
    [0xbf83e7295d25a7d9, 0x3beff29a11443a06],
    [0xbf77ee11ebd82e94, 0x3c161e96e2fc5d90],
    [0x0000000000000000, 0x0000000000000000],
];
// max |u| = 0.0078125 = 2^-7.000
// max c = 5447

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp)]
mod tests {
    use super::*;
    use crate::float::F80;
    use crate::mp::{self, Mp};

    const MODES: [Rounding; 4] = [Rounding::Nearest, Rounding::Down, Rounding::Up, Rounding::Zero];

    fn mp_of(v: Dd) -> Mp {
        Mp::from_f64(v.hi).add(Mp::from_f64(v.lo))
    }

    /// `|got − want| <= ulp(last)/2·(1 + 2^-20)`: `got` is the rounded value (`last` its last
    /// part: the double-double's `lo` or the `f64` itself).
    fn rounded(got: Mp, last: f64, want: Mp, what: &str) {
        let e = ((last.to_bits() >> 52) & 0x7ff) as i32 - 1075;
        let half_ulp = Mp::from_f64(dd::pow2(e - 1)).add(Mp::from_f64(dd::pow2(e - 21)));
        let d = got.sub(want);
        assert!(d.cmp_abs(half_ulp) != core::cmp::Ordering::Greater, "{what}: not the rounded value");
    }

    fn dd_rounded(c: [u64; 2], want: Mp, what: &str) {
        let v = Dd::of(c);
        rounded(mp_of(v), if v.lo == 0.0 { v.hi * f64::EPSILON } else { v.lo }, want, what);
    }

    /// Spec §10.3 (correct rounding of the wide functions rests on these constants).
    #[test]
    fn constants_are_the_rounded_values() {
        let ln2 = mp::ln2();
        for j in 0..64u64 {
            dd_rounded(EXP2_64[j as usize], mp::exp(ln2.mul_u64(j).div_u64(64)), "EXP2_64");
        }
        for (i, &c) in RCP.iter().enumerate() {
            let want = mp::ln(Mp::from_i64(i64::from(c)).ldexp(-12)).neg();
            if c == 4096 {
                assert_eq!(LOGC[i], [0, 0], "LOGC[{i}]");
            } else {
                dd_rounded(LOGC[i], want, "LOGC");
            }
        }
        dd_rounded(LN2_64, ln2.div_u64(64), "LN2_64");
        dd_rounded(LN2, ln2, "LN2");
        dd_rounded(LOG2E, ln2.recip(), "LOG2E");
        dd_rounded(LOG10E, mp::ln10().recip(), "LOG10E");
        let mut fact = 1u64;
        let dds = [(2, E2), (3, E3), (4, E4), (5, E5)];
        let f64s = [(6, E6), (7, E7), (8, E8), (9, E9), (10, E10), (11, E11)];
        for n in 2..=11u64 {
            fact *= n;
            let want = Mp::from_i64(1).div_u64(fact);
            if let Some(&(_, c)) = dds.iter().find(|p| p.0 == n) {
                dd_rounded(c, want, "1/n!");
            }
            if let Some(&(_, c)) = f64s.iter().find(|p| p.0 == n) {
                rounded(Mp::from_f64(f64::from_bits(c)), f64::from_bits(c), want, "1/n!");
            }
        }
        for (n, c) in [(3, A3), (5, A5), (7, A7)] {
            dd_rounded(c, Mp::from_i64(1).div_u64(n), "1/n");
        }
        for (n, c) in [(9, A9), (11, A11), (13, A13)] {
            rounded(Mp::from_f64(f64::from_bits(c)), f64::from_bits(c), Mp::from_i64(1).div_u64(n), "1/n");
        }
        // The fixed-point factors within half a unit.
        let half = Mp::from_i64(1).ldexp(-1);
        let ce = Mp::from_u128(false, C_E, 0).sub(ln2.recip().ldexp(127));
        assert!(ce.cmp_abs(half) != core::cmp::Ordering::Greater, "C_E");
        let c10 = Mp::from_u128(false, C_10, 0).sub(mp::ln10().div(ln2).ldexp(126));
        assert!(c10.cmp_abs(half) != core::cmp::Ordering::Greater, "C_10");
    }

    /// Spec §10.3, correct rounding in every mode: the rounding test refuses an approximation
    /// whose bound reaches a rounding boundary (a midpoint for to-nearest, a representable value
    /// for the directed modes) and decides one clear of it.
    #[test]
    fn rounding_test_refuses_near_a_boundary() {
        let ulp = dd::pow2(-63);
        let near = dd::pow2(-(B_MAIN as i32)) * 0.75;
        let clear = dd::pow2(-(B_MAIN as i32)) * 16.0;
        for base in [ulp / 2.0, ulp] {
            for d in [near, -near] {
                for m in MODES {
                    assert_eq!(decide::<F80>(Dd { hi: 1.0, lo: base + d }, 0, B_MAIN, m), None, "{base:e} {d:e} {m:?}");
                }
            }
            for d in [clear, -clear] {
                for m in MODES {
                    let want = mp::round::<F80>(Mp::from_f64(1.0).add(Mp::from_f64(base + d)), m);
                    assert_eq!(decide::<F80>(Dd { hi: 1.0, lo: base + d }, 0, B_MAIN, m), Some(want), "{base:e} {d:e} {m:?}");
                }
            }
        }
        // A bound too wide for the format never decides (binary128 waits for its own tier).
        assert_eq!(decide::<crate::float::F128>(Dd { hi: 1.0, lo: ulp * 0.3 }, 0, B_MAIN, Rounding::Nearest), None);
    }

    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
    }

    /// A random x87 operand with its leading bit at `2^top`, `top` in `lo..=hi`.
    fn operand(g: &mut Rng, lo: i32, hi: i32, neg: bool) -> Num {
        let span = (hi - lo + 1) as u64;
        let top = lo + (g.next() % span) as i32;
        let sig = u128::from(g.next() | (1 << 63));
        Num { neg, kind: crate::float::Kind::Finite, sig, exp: top - 63 }
    }

    /// The 384-bit value of each function (the route [`crate::wide`] falls back to).
    fn reference(k: Kernel, n: &Num) -> Mp {
        let x = Mp::from_u128(n.neg, n.sig, i64::from(n.exp));
        match k {
            Kernel::Exp => mp::exp(x),
            Kernel::Exp2 => mp::exp(x.mul(mp::ln2())),
            Kernel::Exp10 => mp::exp(x.mul(mp::ln10())),
            Kernel::Expm1 => mp::exp(x).sub(Mp::from_i64(1)),
            Kernel::Log => mp::ln_atanh(x),
            Kernel::Log2 => mp::ln_atanh(x).div(mp::ln2()),
            Kernel::Log10 => mp::ln_atanh(x).div(mp::ln10()),
            Kernel::Log1p => mp::ln1p(x),
        }
    }

    /// The operand ranges each kernel is exercised on (inside the callers' limits).
    fn draw(k: Kernel, g: &mut Rng) -> Num {
        let neg = g.next() & 1 == 1;
        match k {
            Kernel::Exp | Kernel::Exp2 => operand(g, -66, 12, neg),
            Kernel::Exp10 => operand(g, -66, 11, neg),
            Kernel::Expm1 if neg => operand(g, -66, 4, true),
            Kernel::Expm1 => operand(g, -66, 12, false),
            Kernel::Log | Kernel::Log2 | Kernel::Log10 => operand(g, -16382, 16383, false),
            Kernel::Log1p if neg => operand(g, -66, -1, true),
            Kernel::Log1p => operand(g, -66, 16383, false),
        }
    }

    const ALL: [Kernel; 8] =
        [Kernel::Exp, Kernel::Exp2, Kernel::Exp10, Kernel::Expm1, Kernel::Log, Kernel::Log2, Kernel::Log10, Kernel::Log1p];

    /// Each kernel within its bound (one bit of margin), and every result the tier decides equal
    /// to the 384-bit route's in all four modes. Returns the refusals.
    fn sweep(per_kernel: usize, seed: u64) -> usize {
        let mut g = Rng(seed);
        let mut refused = 0;
        for k in ALL {
            let mut worst = i64::MIN;
            for _ in 0..per_kernel {
                let n = draw(k, &mut g);
                let want = reference(k, &n);
                let (v, scale, bits) = kernel(k, &n).unwrap();
                let d = mp_of(v).ldexp(i64::from(scale)).sub(want);
                // |d|/|want| < 2^(ex(d) − ex(want) + 1).
                let rel = mp::ex(d) - mp::ex(want) + 1;
                worst = worst.max(rel + i64::from(bits));
                assert!(rel < -i64::from(bits), "{k:?} {n:?}: error 2^{rel} against the bound 2^-{bits}");
                for m in MODES {
                    match eval::<F80>(k, &n, m) {
                        Some(r) => assert_eq!(r, mp::round::<F80>(want, m), "{k:?} {n:?} {m:?}"),
                        None => refused += 1,
                    }
                }
            }
            println!("{k:?}: worst error 2^{worst} of its bound");
        }
        refused
    }

    /// Spec §10.3 (correctly rounded wide functions): the tier against the 384-bit route.
    #[test]
    fn tier_agrees_with_the_wide_route() {
        let refused = sweep(300, 0x9e37_79b9_7f4a_7c15);
        // Out-of-range results (subnormal, overflow) and a rare undecided case only.
        assert!(refused < 300, "refused {refused}");
    }

    /// The long sweep (`cargo xtask long-sweeps`, release build): more operands, every mode.
    #[test]
    #[ignore = "long: cargo xtask long-sweeps runs it in release (nightly and release runs)"]
    fn tier_agrees_with_the_wide_route_long() {
        let refused = sweep(40_000, 0x2545_f491_4f6c_dd1d);
        println!("refused {refused}");
    }
}
