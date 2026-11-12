# handoff: libm-f128-explog (LIBM-F128-EXPLOG, run 1: design only, no code)

**State.** Branch `task/libm-f128-explog` (from origin/next d1b3a596c). No product code yet: this run stopped at
its 180k context checkpoint after reading the predecessors and working the design below. No jobs ran.
Read: D-10.52, D-10.73, `crates/math/src/wide_int/quad.rs` (W3, mulhi3, `round`, tests), `wide_int_el.rs`
(x87 exp/log tier, module doc), `crates/mathtools/src/ld_explog_tables.rs` (generator), `wide.rs` `tiers()`.

**Where it plugs in.** New `crates/math/src/wide_int/quad/explog.rs` (`pub(crate) mod explog;` in quad.rs; a
child module sees W3's private fields and `mulhi3`). `wide.rs::tiers()` gains it (`F::PREC == 113 && !EXPLICIT`,
Kernel::Exp/Log only; other kernels None for -2). Callers already settle: exp `|x| < 2^-116` (linear_tiny),
specials; log `x = 1`, `x <= 0`, specials. Test helpers: `super::super::tests::{MODES, Rng, Tally}`,
quad's `tests::{fixed3, value}`; oracle `mp::exp`, `mp::ln_atanh`, compare to `mp::round::<F128>` (bits + Status).

**Tables (extend `ld_explog_tables.rs`, regenerate with `cargo xtask tables` as a job; never hand-edit).**
Reuse WI_EXP2T, WI_L1, WI_L2, WI_C1, WI_C2, WI_E10/E9/E8 (u64 2^-85/-82/-79), WI_E4/E3 (u128 2^-132/-130),
WI_G3 (u128 2^-129), WI_C_E (>>64 = 128/ln2 in 2^-56 for k), WI_L, WI_LN2_113. New, f128 only (~2.6 KiB):
third limbs `lo = round(v·2^(u+64)) − hi·2^64` (i64, assert fits): WQ_EXP2T_LO[128] (2^(j/128), units 2^-191),
WQ_L1_LO[128] (−ln c1, 2^-191), WQ_L2_LO[65] (−ln c2, 2^-197), WQ_L_LO (ln2/128, 2^-199), WQ_LN2_LO (ln2,
2^-177); coefficients WQ_E12 (u64 2^-92), WQ_E11 (u64 2^-89), WQ_E7/E6/E5 (u128 2^-140/-137/-134), WQ_G11/G10/G9
(u64 2^-67), WQ_G7/G6/G5 (u128 2^-130). Exact: 1/2 = 1<<127 (2^-128), 1/4 = 1<<127 (2^-129), 1/8 = 1<<63 (2^-66).
Signed W3 from (i128 hi, i64 lo) must use wrapping ops (W3::with_lo's `h - 1` panics in debug for hi = 0).

**exp (Tang 1989 table method; Brent-Zimmermann fixed point). Proved budget ≈ 2^-141.8; test B = 140.**
X1 `m` = sig normalised to [2^112, 2^113), `|x| = m·2^e`; `e + 112 >= 14` -> None (exp_of settles).
X2 `k = round(|x|·128/ln2)`: `p = (m>>49)·(WI_C_E>>64)`, `sh = 7 − e` (>= 106); `sh >= 129` -> 0, else
`((p >> (sh−1)) + 1) >> 1`; `k < 2^21.53`, `|f| <= 1/2 + 2^-41`, so `|r| <= ln2/256·(1+2^-40) < 2^-8.528`.
X3 `r = |x| − k·ln2/128` in units 2^-199 mod 2^192: `X = m·2^(e+199)` mod 2^192 (`e+199 < 0`: truncate,
k = 0 there), `LQ = WI_L·2^64 + WQ_L_LO`, `R = X − k·LQ` wrapping (k·LQ: three muls, low 192 bits); R signed,
`|R| < 2^190.48`, error `<= k/2 + 1` units = 2^-178.8. `a = |R|`, `rneg = (R<0) xor x<0`, `k_s = ±k`.
X4 `e^r = 1 ± a + a²·g`, `g = h_2`, `h_k = 1/k! ± a·h_(k+1)`, `h_12 = 1/12!` (series rest a^13/13! = 2^-143.41).
`a1 = a>>127` (u64 2^-72), `ah = a>>63` (u128 2^-136). 64-bit steps h11..h8: `s64(E_k, mulhi64(a1,h)>>11|12|11|11)`;
h7 `mul_wide(ah,h8)>>11`; h6,h5 `mulhi(ah,h)>>11`; h4,h3,g `>>10` (g units 2^-128, error <= 1.51 units ->
2^-144.45 after a²). `a2 = ⌊ah²/2^127⌋` exact square (3 muls; units 2^-145, <= 3.8 units low incl. dropped
cross term -> 2^-144.07 after g). `Q = mulhi(a2,g)` units 2^-145 (< 3 units: 2^-143.42). `P = a ± (Q<<54)`.
X5 `T = WI_EXP2T[j]·2^64 + WQ_EXP2T_LO[j]` (units 2^-191, j = k_s & 127, K = k_s >> 7);
`Rv = T ± mulhi3(T,P)>>7` (2^-190); `Rv < 2^191` only for j = 0, rneg: shift 1. Result `Rv·2^(K−191[−1])`.
Sum: 2^-143.41·(1 + 0.487 + 0.633 + 1.0) = 2^-141.77 < 2^-140.
X6 rounding `round_exp`: `be = e + 191 + EMAX`; `1 <= be < 2·EMAX`: quad-style pack with ERR = 2^52 + 16.
Else `s = be <= 0 ? 80 − be : 79`; None if `s >= 192` or (be == 0 and m>>79 == 2^113 − 1: tininess-after-
rounding band); `m2 = m >> (s−79)` (error stays <= ERR), same 78-bit test, then
`float::round(Exact{neg:false, sig: m2>>78, exp: e+(s−79)+78, sticky:true})` + `float::range_errno` (identical
flags/errno to `mp::round`, which goes through the same `float::round`). Covers subnormal and overflow.

**log (Tang 1990; Wong-Goto two-step reduction, the x87 tier's C1/C2). Budget ≈ 2^-146, B = 140.**
L1 `m` in [2^112, 2^113) (y'' units 2^-112), `q = exp − shift + 112`, `i = (m>>105)&127`, `E = q + [i>=64]`,
`u1 = (m.wrapping_mul(C1[i])) as i128` (units 2^-129, exact; Y = 2^129(1+u1) mod 2^128).
L2 `i2 = (u1 + 2^116) >> 117` in [-32, 32], `c2 = C2[i2+32]`; `u2 = u1·c2 + (c2 − 2^63)·2^129` in units 2^-192,
EXACT (|u1|·c2 two muls into W3, sign, then `h += (c2−2^63) << 65` wrapping); `|u2| < 2^-12.98`, w = |u2|.
L3 `h_1 = 1 ∓ w/2 + w²·h_3` in W3 units 2^-191 (∓: minus for u2 > 0); `h_k = 1/k ∓ w·h_(k+1)`, h_11 = 1/11
(rest w^11/12 = 2^-146.4 relative). `w1 = w>>116` (u64 2^-76), `wh = w>>52` (u128 2^-140). h10,h9:
`s64(G, mulhi64(w1,h)>>12)` (2^-67), h8 `>>13` (1/8, 2^-66), h7 `mul_wide(wh,h8)>>12`, h6,h5 `mulhi>>12`
(2^-130), h4 `>>13` (2^-129), h3 `>>12` (2^-129). `w2 = mulhi(wh,wh)` (2^-152, <= 4 units), `t = mulhi(w2,h3)`
(2^-153), `h1 = ONE ∓ w.shr(2) + (t<<38)`: absolute error ~2^-150.
L4 normalise w (192-bit lzcnt; w = 0 -> P = 0), `P = mulhi3(wn, h1)` units 2^-(191+lzw), renormalise 1-2 bits.
(a) E = 0, L1[i] = 0, i2 = 0: result ±P (relative 2^-146). (b) E = 0 otherwise: S = L1·2^64+L1_LO
+ (L2·2^64+L2_LO)>>6 ± P>>lzw in units 2^-191 (|ln x| >= 2^-13.02: relative <= 2^-146). (c) E != 0: units
2^-177, `E·(WI_LN2_113·2^64 + WQ_LN2_LO)` + L1>>14 + L2>>20 ± P>>(lzw+14); |ln x| >= 0.288|E|: 2^-162.
Results always normal: a copy of quad's `round` with this module's ERR.

**Tests to write.** `constants_are_the_rounded_values` (third limbs vs `fixed3` of mp values; coefficients),
`tier_agrees_with_the_wide_route` (bands of x; exp: |x| in [2^-116, 2^14] incl. [11355, 11357] overflow edge,
[-11433, -11355] subnormal, [-11433.5, -11432.7] bottom; log: x near 1 as 1 ± d·2^-113 d odd, subnormal x,
huge x), kernel error < 2^-B asserted per operand, undecided counted; `_long` #[ignore] for long-sweeps;
a rounding-test boundary test for `round_exp` (subnormal s, overflow carry). Plants: Horner shift, third-limb
sign, `s` formula, ERR, i2 rounding, the (a)/(b)/(c) unit shifts.

**Next step.** Write the generator additions and explog.rs as above, `cargo xtask tables` (job), wire tiers(),
clippy/static, direct gate, plants, percall (`validation/percall/` f128 exp/log rows; base next, glibc), and a
pending record `records new decision D-NEW --series D-10 --story LIBM-F128-EXPLOG` naming the method and bounds.
Estimate: exp ~350, log ~300 instructions (glibc 6673 exp; next 33k).
