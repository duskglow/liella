//! The integer fixed-point first tier of the binary128 (`_Float128`) `atan` and `atan2` (spec
//! §10.3, D-10.96): the arc kernel of the x87 tier ([`crate::wide_int::arc`], D-10.52, D-10.75)
//! carried to three 64-bit limbs, with a proved relative error below `2^-160` and the parent's
//! integer rounding test ([`super::round`], bound `2^-152`). What the test cannot decide (about
//! `2^-37` of the operands) takes the 384-bit route of [`crate::wide`].
//!
//! Method. `atan(y/x)` for `0 < y <= x` ([`atan_le`]): Tang's table method (P. T. P. Tang,
//! "Table-lookup algorithms for elementary functions and their error analysis", ARITH 10, 1991)
//! with `c = j/128`, `atan(y/x) = atan(c) + atan(r)`, `r = (y − cx)/(x + cy)`, and the odd series
//! of `atan r` in fixed point (R. Brent, P. Zimmermann, Modern Computer Arithmetic, 2010, ch. 4).
//! The quotient is the parent's [`super::div3`] (Newton's reciprocal and one residual
//! correction, one `divsd`, no integer `div`). `atan x = A(|x|, 1)` or `π/2 − A(1, |x|)`;
//! `atan2` the same on `(|y|, |x|)`, then `π − θ` for `x < 0`. The table `atan(j/128)` is the x87
//! tier's 128-bit `ATAN_J` and a signed third limb [`ATAN_J_LO`]. Constants from Python `decimal`
//! at 220 digits (π by Machin's formula), each pinned against the 384-bit arithmetic by
//! `constants_are_the_rounded_values`.
//!
//! Proof of the bound (`W3` values are 192-bit integers; "λ" is a step's own error).
//! 1. Operands. `|y| = Y·2^ey`, `|x| = X·2^ex`, `Y`, `X` in `[2^112, 2^113)` (binary128
//!    significands, subnormals normalised), `y <= x`, `k = ex − ey >= 0`, `t = y/x`.
//! 2. Index. For `k <= 8`, `h = ⌊dbl(⌊Y/2^60⌋)·(256/dbl(⌊X/2^60⌋))⌋` (two 53-bit integers, exact
//!    in `double`; the truncations, the `divsd` and the product round within `2^-48.7` together)
//!    and `j = ⌊(⌊h/2^k⌋ + 1)/2⌋`: `⌊h/2^k⌋ = ⌊256t(1 + η)⌋`, `|η| < 2^-48.7`, so `|t − j/128| <=
//!    2^-8 + j·2^-55.7 <= 2^-8(1 + 2^-40)` and `j <= 128` (`t <= 1`); `k > 8` gives `t < 2^-7` and
//!    `j = 0` is taken (`h` lies in `[128, 512)`: the conversion is in range). So `|r| <= 2^-8(1 +
//!    2^-40)` and `u = r² <= 2^-16(1 + 2^-39)`.
//! 3. Quotient. `j = 0`: `r = t`, exact when `X = 2^112` (`atan`'s `x = 1`), else `div3` of `Y·2^79`
//!    by `X·2^79` within `2^-162.2` relative. `j >= 1` (so `k <= 8`): in units of `2^(ey − 7)`
//!    the numerator `y − cx = Y·2^7 − j·X·2^k` and the denominator `x + cy = X·2^(k + 7) + j·Y`
//!    are exact integers (the numerator below `2^121` in magnitude, the denominator below `2^129`:
//!    its carry is kept as a 129th bit, normalised exactly into 192 bits), and `div3` gives `|r|`
//!    within `2^-162.2` relative. A relative change `ε` of `r` changes `atan r` by at most `ε`
//!    relative (`r/(1 + r²) <= atan r`).
//! 4. Series. `atan r = r·P(u)`, `P(u) = Σ_{k<=9} (−1)^k u^k/(2k+1)`; the series alternates with
//!    decreasing terms, remainder below `u^10/21 < 2^-164.3`. Horner from the innermost term, every
//!    partial sum positive: steps `k = 8..6` in 64-bit words, `5..2` in 128 bits, `1, 0` in 192
//!    (`u` in units `2^-206`, at most 7 units low; its top 128 bits and top word in units
//!    `2^-142`, `2^-78`). A step's error λ_k (half a unit for the constant, one for the truncated
//!    product, and the truncation of `u`'s word times the inner sum) reaches `P` multiplied by
//!    `u^k`: λ_6 <= `1.5·2^-67 + 2^-81` gives `2^-162.4`, λ_2 <= `1.5·2^-130 + 2^-142` gives
//!    `2^-161.4`, every other step is below `2^-178`: `P` (at least `1 − u/3`) within
//!    `2^-160.69` relative. `r·P` (one 192-bit product, at most 6 units of `2^-(191 + kr)` low,
//!    against a value of at least `2^189.99` units) is `atan r` within `2^-160.68` given `r`;
//!    with step 3's quotient, within `2^-160.25`.
//! 5. Kernel. `j = 0`: `A = atan r`, within `2^-160.25`. `j >= 1`: `A = atan(c) ± atan|r|`,
//!    `atan(c)` within half a unit of `2^-192`, `atan|r| <= 2^-8(1 + 2^-40)` within `2^-160.25`
//!    relative and one unit of `2^-192` for its shift: at most `2^-168.24` absolute against `A >=
//!    atan(2^-8(1 − 2^-40)) > 2^-8.01`: within `2^-160.23`.
//! 6. Compositions in units `2^-191` (`π/2 − A`, `A <= π/4`, result `>= π/4`) and `2^-190` (`π −
//!    θ`, `θ < π/2`, result `> π/2`): the argument's absolute error is at most its own relative
//!    error times the result; the shift and the constant add `1.5` units: below `2^-189.9`
//!    relative. Every result is within `2^-160.2` relative; the rounding test assumes `2^-152`.
//!    Results lie in `[2^-122, π]` (the callers settle `|x| < 2^-58` for `atan` and `|y/x| <
//!    2^-121` with `x > 0` for `atan2`; a smaller `atan2` is refused by the exponent check).

use super::super::arc::ATAN_J;
use super::super::{mul_wide, mulhi, mulhi64};
use super::{ONE, Scaled, W3, div3, mul32, mulhi3, norm, round};
use crate::float::{Float, Kind, Num};
use crate::status::{Rounding, Status};

// The series of `atan r / r` in `u = r²` (step 4), each coefficient `1/(2k+1)` rounded to nearest
// in the units noted.
const A19: u64 = 0xd79435e50d79435e; // 1/19, units 2^-68
const A17: u64 = 0xf0f0f0f0f0f0f0f1; // 1/17, units 2^-68
const A15: u64 = 0x8888888888888889; // 1/15, units 2^-67
const A13: u64 = 0x9d89d89d89d89d8a; // 1/13, units 2^-67
const A11: u128 = 0xba2e8ba2_e8ba2e8b_a2e8ba2e_8ba2e8ba; // 1/11, units 2^-131
const A9: u128 = 0xe38e38e3_8e38e38e_38e38e38_e38e38e4; // 1/9, units 2^-131
const A7: u128 = 0x92492492_49249249_24924924_92492492; // 1/7, units 2^-130
const A5: u128 = 0xcccccccc_cccccccc_cccccccc_cccccccd; // 1/5, units 2^-130
const A3: W3 = W3 { h: 0xaaaaaaaa_aaaaaaaa_aaaaaaaa_aaaaaaaa, l: 0xaaaaaaaaaaaaaaab }; // 1/3, units 2^-193
/// π/2 in units `2^-191`, which is π in units `2^-190` (step 6).
const PI3: W3 = W3 { h: 0xc90fdaa2_2168c234_c4c6628b_80dc1cd1, l: 0x29024e088a67cc74 };

/// A positive binary128 magnitude `m·2^e`, `m` in `[2^112, 2^113)`.
#[derive(Clone, Copy, Debug)]
struct Q {
    m: u128,
    e: i32,
}

const UNIT: Q = Q { m: 1 << 112, e: -112 };

/// `atan x` rounded into `F` (binary128) in `mode` when the tier decides it; `None` sends the
/// caller to the 384-bit route. `x` finite and nonzero (the caller settles `|x| < 2^-58`).
pub(crate) fn atan<F: Float>(n: &Num, mode: Rounding) -> Option<(F, Status)> {
    if F::PREC != 113 || F::EXPLICIT {
        return None;
    }
    let y = mag(n)?;
    let (m, e) = if (y.e, y.m) <= (UNIT.e, UNIT.m) { atan_le(y, UNIT) } else { sub_from(191, atan_le(UNIT, y)) };
    round::<F>(n.neg, m, e, mode)
}

/// `atan2(y, x)` rounded into `F` (binary128) for finite nonzero `y`, `x` (the caller settles
/// zeros, infinities and `|y/x| < 2^-121` with `x > 0`).
pub(crate) fn atan2<F: Float>(y: &Num, x: &Num, mode: Rounding) -> Option<(F, Status)> {
    if F::PREC != 113 || F::EXPLICIT {
        return None;
    }
    let (b, a) = (mag(y)?, mag(x)?);
    let t = if (b.e, b.m) <= (a.e, a.m) { atan_le(b, a) } else { sub_from(191, atan_le(a, b)) };
    let (m, e) = if x.neg { sub_from(190, t) } else { t };
    round::<F>(y.neg, m, e, mode)
}

/// The magnitude of a finite nonzero binary128 operand, normalised (step 1).
fn mag(n: &Num) -> Option<Q> {
    if n.kind != Kind::Finite || n.sig == 0 || n.sig >> 113 != 0 {
        return None;
    }
    let lz = n.sig.leading_zeros() - 15;
    Some(Q { m: n.sig << lz, e: n.exp - lz as i32 })
}

/// `PI3·2^-units − v` (step 6): `π/2 − v` for `units = 191`, `π − v` for 190; `v` below `π/4`
/// (`π/2`), so its exponent is at most `−units − 1`.
fn sub_from(units: u32, (m, e): Scaled) -> Scaled {
    norm(PI3.sub(m.shr((-(units as i32) - e) as u32)), units)
}

fn dbl(v: u64) -> f64 {
    v as i64 as f64 // below 2^53 at every call: exact
}

/// Steps 2 to 5: `atan(y/x)` for `0 < y <= x` as `(M, E)`, `M` in `[2^191, 2^192)`.
fn atan_le(y: Q, x: Q) -> Scaled {
    let k = x.e - y.e; // >= 0: y <= x, both normalised
    let mut j = 0;
    if k <= 8 {
        let h = (dbl((y.m >> 60) as u64) * (256.0 / dbl((x.m >> 60) as u64))) as usize; // in [128, 512)
        j = ((h >> k) + 1) >> 1;
    }
    if j == 0 {
        let (rm, kr) = if x.m == 1 << 112 {
            (W3 { h: y.m << 15, l: 0 }, k - 1) // t = (Y·2^79)·2^(-191-k), exact
        } else {
            let q = div3(W3 { h: y.m << 15, l: 0 }, W3 { h: x.m << 15, l: 0 }); // (Y/X)·2^190
            let l = q.h.leading_zeros();
            (q.shl(l), k + l as i32 - 2)
        };
        return norm(atan_small(rm, kr), (191 + kr) as u32);
    }
    // Units 2^(ey − 7): y − cx and x + cy exact (step 3).
    let num = ((y.m << 7) as i128).wrapping_sub(((j as u128 * x.m) << k) as i128);
    let (s, carry) = (x.m << (k + 7)).overflowing_add(j as u128 * y.m);
    let (den, ld) = if carry {
        (W3 { h: (1 << 127) | (s >> 1), l: (s as u64) << 63 }, -1)
    } else {
        let l = s.leading_zeros();
        (W3 { h: s << l, l: 0 }, l as i32)
    };
    let tj = W3::with_lo(ATAN_J[j - 1], ATAN_J_LO[j - 1]); // atan(j/128), units 2^-192
    let a = if num == 0 {
        tj
    } else {
        let an = num.unsigned_abs();
        let ln = an.leading_zeros();
        let q = div3(W3 { h: an << ln, l: 0 }, den); // |r|·2^(190 + ln − ld)
        let lq = q.h.leading_zeros();
        let kr = (lq + ln) as i32 - ld - 2; // |r| = (q << lq)·2^(-192-kr), kr >= 7
        let p = atan_small(q.shl(lq), kr).shr((kr - 1) as u32); // units 2^-192
        if num < 0 { tj.sub(p) } else { tj.add(p) }
    };
    norm(a, 192) // a >= 2^183.99
}

/// `atan|r|·2^(191 + kr)` for `|r| = rm·2^(-192-kr)`, `rm` in `[2^191, 2^192)`, `kr >= 7`; at
/// least `2^189.99` (step 4).
#[inline(always)]
fn atan_small(rm: W3, kr: i32) -> W3 {
    let u = mulhi3(rm, rm).shr((2 * kr - 14) as u32); // r², units 2^-206
    mulhi3(rm, atan_poly(u))
}

/// Step 4: `P(u)` in units of `2^-191` (`u = r²` in units of `2^-206`, below `2^190`).
#[inline(always)]
fn atan_poly(u: W3) -> W3 {
    let (uh, u1) = (u.h, (u.h >> 64) as u64); // units 2^-142, 2^-78
    let a = A17 - (mulhi64(u1, A19) >> 14); // units 2^-68
    let b = A15 - (mulhi64(u1, a) >> 15); // units 2^-67
    let c = A13 - (mulhi64(u1, b) >> 14); // units 2^-67
    let d = A11 - (mul_wide(uh, c) >> 14); // units 2^-131
    let e = A9 - (mulhi(uh, d) >> 14); // units 2^-131
    let f = A7 - (mulhi(uh, e) >> 15); // units 2^-130
    let g = A5 - (mulhi(uh, f) >> 14); // units 2^-130
    let h = A3.sub(mul32(u, g).shr(15)); // units 2^-193
    ONE.sub(mulhi3(u, h).shr(16)) // units 2^-191
}

/// The third limb of the x87 tier's `ATAN_J` (`round(atan(j/128)·2^192) − ATAN_J·2^64`, row
/// `j − 1`): `ATAN_J·2^64 + ATAN_J_LO` is `atan(j/128)` in units of `2^-192`, within half a unit.
const ATAN_J_LO: [i64; 128] = [
    -8155828434003272192, 2342924687471262674, -8226674086039856477, -7204800540968559551,
    1751562271757023372, 8204957521866650121, -7326808355825729968, 1376343559981845294,
    5953008342742180981, -3896967196689860339, 8313540205588708034, -3476408861431840328,
    -6942003458769687477, -3307053204403633538, -7995271858704131287, -1051824729709389020,
    5586626796868670505, 8694294397370499876, -5471923636569087687, 6761237015359343159,
    -8793242398088849585, 815040985577925939, -5404397856657753026, -1951395414458332469,
    -6370384501464104770, -6125940497305264761, 7568793151201578598, -6804582093791509355,
    -7593216569334106765, -1419976341208695807, 3728819616147804623, 7683117053710754011,
    1341229490992458991, 6274223811059417816, -7521686371647740866, -8653260629717385752,
    -959846512754400095, 2467428129512823062, 5737092185697524831, 311390225032522496,
    -7429146222705450133, -6394763672565136634, 92507247735085343, -8028197496281207830,
    5681908015491754763, 6486360418868967096, -5366755069442502309, -7007006477140743450,
    -2028685236578654575, 1360260873371712416, -9118205826126521325, -2135861867965492997,
    4022595333016182619, -3945997516976108790, -7395111206006486218, 3963203331332035486,
    739531572403431875, -6150781792084590042, -4500929322431974573, -2663211124545070062,
    -4644593411810663066, 3627974146198684122, 7615186843931583632, -5257377779899535488,
    -4069207326710691996, 1697945416662697764, 3172888493167548963, 578230346571950786,
    -1822561002420361100, 7576328024327643596, -3943095948220319908, -1074850851273711684,
    -5862435400335305530, -7878677173386268015, -7215492306738023636, -8088891954762484097,
    -110628544354454569, -512772327192015353, -2323091729462928477, -7959435177956264099,
    -7354197303552704857, -2768050407372147657, -4261725130627892866, -897657835073806630,
    5309527982141309798, -7523022215891101810, -158733677921058598, -2673890206800110244,
    -4189847323515852901, 6497223818810915135, 4129654384206909759, -7100983522927465302,
    4136044761356580612, -3421587357171360796, -8973525696063777061, -2021968305716020761,
    5062719731762915561, 2429717789581915813, -2199246844313483679, -461875878626345188,
    -423978385725070735, 1746751518788421261, 5953564383111542329, 248871460199726630,
    9097573724425766837, 712275234488181191, -5574403237073433366, -1987777213117499650,
    8369644944387423861, 8440149530414489096, 781271097297599766, 4430113497493772929,
    431756449107660586, 2531731440410075971, 1387916823853100134, 1439665475719310218,
    -8504170319460240621, 91060736720985146, -3593725308415294120, -3058251564923472138,
    1734652237142033394, -1625539920367211929, -3581867880964804683, 5595196652625953615,
    8031243985732616596, 1052884014057877547, 4286628319686304077, 2955010104097229940,
];

#[cfg(test)]
mod tests {
    // Spec clause: §10.3 (_Float128 atanf128/atan2f128 correctly rounded in the four directions, D-10.19).
    use super::super::tests::{fixed3, value};
    use super::*;
    use crate::float::F128;
    use crate::mp::{self, Mp};
    use crate::wide_int::tests::{MODES, Rng, Tally};

    /// The proved bound of step 6.
    const PROVED: f64 = -160.0;

    #[test]
    fn constants_are_the_rounded_values() {
        let one = Mp::from_i64(1);
        let x1 = |v: u64| (0, v);
        let x2 = |v: u128| (v >> 64, v as u64);
        let pinned: [((u128, u64), u64, i64); 9] = [
            (x1(A19), 19, 68),
            (x1(A17), 17, 68),
            (x1(A15), 15, 67),
            (x1(A13), 13, 67),
            (x2(A11), 11, 131),
            (x2(A9), 9, 131),
            (x2(A7), 7, 130),
            (x2(A5), 5, 130),
            ((A3.h, A3.l), 3, 193),
        ];
        for (c, k, units) in pinned {
            assert_eq!(c, fixed3(one.div_u64(k), units), "1/{k}");
        }
        assert_eq!((PI3.h, PI3.l), fixed3(mp::pi(), 190), "π");
        for j in 1..=128u64 {
            let t = W3::with_lo(ATAN_J[j as usize - 1], ATAN_J_LO[j as usize - 1]);
            assert_eq!((t.h, t.l), fixed3(mp::atan(one.mul_u64(j).ldexp(-7)), 192), "atan({j}/128)");
        }
    }

    fn num(neg: bool, sig: u128, exp: i32) -> Num {
        Num { neg, kind: Kind::Finite, sig, exp }
    }

    fn exact(n: &Num) -> Mp {
        Mp::from_u128(n.neg, n.sig, i64::from(n.exp))
    }

    /// A random binary128 operand with `floor(log2 |x|)` in `[lo, hi]` and a random sign.
    fn operand(g: &mut Rng, (lo, hi): (i32, i32)) -> Num {
        let top = lo + (g.next() % (hi - lo + 1) as u64) as i32;
        let sig = (((u128::from(g.next()) << 64) | u128::from(g.next())) >> 15) | 1 << 112;
        num(g.next() & 1 != 0, sig, top - 112)
    }

    /// `atan x`: bands of `floor(log2 |x|)` from the smallest operand the tier takes through
    /// `|x|` near 1 to the largest binary128 value.
    const ATAN_BANDS: [(i32, i32); 4] = [(-58, -9), (-8, 0), (1, 120), (121, 16383)];
    /// `atan2`: bands of `floor(log2 |y|) − floor(log2 |x|)` (the index range, the series alone,
    /// the far quotients), each around a random `x` exponent.
    const ATAN2_BANDS: [(i32, i32); 4] = [(-9, 9), (-40, -9), (9, 40), (-200, 200)];

    fn atan2_operands(g: &mut Rng, (lo, hi): (i32, i32)) -> (Num, Num) {
        let x = operand(g, (-16000, 16000));
        let mut y = operand(g, (lo, hi));
        y.exp += x.exp + 112;
        if y.exp < -16494 {
            y.exp = -16494; // keep it in the format: the least exponent with a normal significand
        }
        (y, x)
    }

    /// The kernel's error against the proved bound, and the tier against the 384-bit route in
    /// the four modes.
    fn check(want: Mp, approx: Option<Scaled>, neg: bool, tier: impl Fn(Rounding) -> Option<(F128, Status)>, tally: &mut Tally, what: &str) {
        assert!(approx.is_some(), "{what}: refused by the kernel");
        if let Some((m, e)) = approx {
            // In the 384-bit arithmetic: a far `atan2` quotient lies below the `f64` range.
            let rel = value(neg, m, e).sub(want).div(want).to_f64().abs();
            assert!(rel < PROVED.exp2(), "{what}: error {rel:e}");
            tally.worst = tally.worst.max(rel);
        }
        for mode in MODES {
            tally.calls += 1;
            match tier(mode) {
                Some(got) => assert_eq!(got, mp::round::<F128>(want, mode), "{what} {mode:?}"),
                None => tally.undecided += 1,
            }
        }
    }

    fn check_atan(n: &Num, tally: &mut Tally) {
        let a = mp::atan(exact(n).abs()); // the 384-bit atan takes a magnitude
        let want = if n.neg { a.neg() } else { a };
        let k = mag(n).map(|y| if (y.e, y.m) <= (UNIT.e, UNIT.m) { atan_le(y, UNIT) } else { sub_from(191, atan_le(UNIT, y)) });
        check(want, k, n.neg, |mode| atan::<F128>(n, mode), tally, &format!("atan {n:?}"));
    }

    fn check_atan2(y: &Num, x: &Num, tally: &mut Tally) {
        let want = mp::atan2(exact(y), exact(x));
        let k = mag(y).zip(mag(x)).map(|(b, a)| {
            let t = if (b.e, b.m) <= (a.e, a.m) { atan_le(b, a) } else { sub_from(191, atan_le(a, b)) };
            if x.neg { sub_from(190, t) } else { t }
        });
        // Results below the normal range are refused by the rounding step: the kernel still meets its bound.
        check(want, k, y.neg, |mode| atan2::<F128>(y, x, mode), tally, &format!("atan2 {y:?} {x:?}"));
    }

    /// The edges of the index and the composition: `|x| = 1` and its neighbours, `t` on a
    /// rounding boundary of `j` (`(2j + 1)/256`) and one place either side, `j = 128` (`t` just
    /// below 1), the least `j >= 1` (`t` near `2^-8`), the smallest and largest operands, and
    /// `atan2` on equal and adjacent magnitudes, subnormals and `x < 0` with tiny `y`.
    fn hard_cases(tally: &mut Tally) {
        let top = (1u128 << 113) - 1;
        let mut ops = vec![
            num(false, 1 << 112, -112),
            num(false, top, -113),
            num(false, (1 << 112) + 1, -112),
            num(false, 1 << 112, -170),
            num(false, top, 16271),
            num(false, 1 << 112, -120),
            num(false, top, -121),
        ];
        for j in 0..128u128 {
            // (2j + 1)/256 = (2j + 1)·2^104·2^-112: normalise to 113 bits.
            let v = (2 * j + 1) << 104;
            let lz = v.leading_zeros() - 15;
            for d in [-1i128, 0, 1] {
                ops.push(num(false, (v << lz).wrapping_add_signed(d), -112 - lz as i32));
            }
        }
        for n in &ops {
            for neg in [false, true] {
                check_atan(&Num { neg, ..*n }, tally);
                // The reciprocal operand too: atan(1/t) = π/2 − atan(t) for t = 2^e·m.
                if (-16494..=16271).contains(&(-224 - n.exp)) {
                    check_atan(&num(neg, n.sig, -224 - n.exp), tally);
                }
            }
        }
        let pairs = [
            (num(false, 1 << 112, -112), num(false, 1 << 112, -112)),
            (num(false, top, -112), num(false, top, -112)),
            (num(false, top - 1, -112), num(true, top, -112)),
            (num(true, 1, -16494), num(true, 3, -16494)),
            (num(false, 1, -16494), num(true, 1 << 112, 16271)),
            (num(false, top, 16271), num(false, 1, -16494)),
            (num(false, 1 << 112, -120), num(true, 1 << 112, -112)),
            (num(false, 1 << 112, -234), num(false, 1 << 112, -112)),
        ];
        for (y, x) in pairs {
            check_atan2(&y, &x, tally);
            check_atan2(&x, &y, tally);
        }
        // y/x = 2^-8 with X within 2^98 of the top: where the index gives j = 1, x + cy carries
        // into a 129th bit (step 3).
        for d in 0..8u128 {
            let m = top - (d << 60);
            check_atan2(&num(false, m, -120), &num(d & 1 != 0, m, -112), tally);
        }
    }

    #[test]
    fn tier_agrees_with_the_wide_route() {
        let mut g = Rng(0x6a09_e667_f3bc_c908);
        let mut tally = Tally::default();
        for i in 0..800 {
            check_atan(&operand(&mut g, ATAN_BANDS[i % 4]), &mut tally);
            let (y, x) = atan2_operands(&mut g, ATAN2_BANDS[i % 4]);
            check_atan2(&y, &x, &mut tally);
        }
        assert!(tally.undecided * 1000 <= tally.calls, "{} of {} undecided", tally.undecided, tally.calls);
        let mut hard = Tally::default();
        hard_cases(&mut hard);
        eprintln!(
            "wide_int::quad::arc: {} calls, {} undecided, worst error 2^{:.2}; hard cases: {} calls, {} undecided, worst 2^{:.2}",
            tally.calls,
            tally.undecided,
            tally.worst.log2(),
            hard.calls,
            hard.undecided,
            hard.worst.log2()
        );
    }

    /// The brief's sweep: 10^7 seeded operands (half `atan`, half `atan2`) over the bands in the
    /// four modes, and the hard cases.
    /// `cargo test --release -p liella-math --lib wide_int::quad::arc -- --ignored`.
    #[test]
    #[ignore = "10^7 operands against the 384-bit route: cargo xtask long-sweeps runs it in release (nightly and release runs)"]
    fn tier_agrees_with_the_wide_route_long() {
        const TOTAL: usize = 10_000_000;
        let threads = std::thread::available_parallelism().map_or(4, std::num::NonZero::get);
        let tallies: Vec<Tally> = std::thread::scope(|s| {
            let handles: Vec<_> = (0..threads)
                .map(|i| {
                    s.spawn(move || {
                        let mut g = Rng(0xbb67_ae85_84ca_a73b ^ (i as u64 + 1).wrapping_mul(0x9e37_79b9_7f4a_7c15));
                        let mut tally = Tally::default();
                        for k in (i..TOTAL).step_by(threads) {
                            if k % 2 == 0 {
                                check_atan(&operand(&mut g, ATAN_BANDS[(k / 2) % 4]), &mut tally);
                            } else {
                                let (y, x) = atan2_operands(&mut g, ATAN2_BANDS[(k / 2) % 4]);
                                check_atan2(&y, &x, &mut tally);
                            }
                        }
                        tally
                    })
                })
                .collect();
            handles.into_iter().map(|h| h.join().unwrap_or_else(|p| std::panic::resume_unwind(p))).collect()
        });
        let mut all = Tally::default();
        for t in &tallies {
            all.calls += t.calls;
            all.undecided += t.undecided;
            all.worst = all.worst.max(t.worst);
        }
        let mut hard = Tally::default();
        hard_cases(&mut hard);
        eprintln!(
            "wide_int::quad::arc long: {} operands, {} calls, {} undecided, worst error 2^{:.2}; hard cases: {} calls, {} undecided, worst 2^{:.2}",
            TOTAL,
            all.calls,
            all.undecided,
            all.worst.log2(),
            hard.calls,
            hard.undecided,
            hard.worst.log2()
        );
        assert!(all.undecided * 100_000 <= all.calls, "{} of {} undecided", all.undecided, all.calls);
    }
}
