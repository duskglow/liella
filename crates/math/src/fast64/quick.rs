//! The quick phase of `double` `exp` (D-10.51, D-10.54; LIBM-F64-EXP-QUICK): a cheap evaluation with
//! a proven relative bound and Ziv's rounding test (Ziv, ACM TOMS 17(3), 1991), in front of today's
//! in-place tier [`super::exp_nearest`] (tier 2, unchanged) and the accurate path. Not wired into
//! the C entry yet (LIBM-F64-EXP-QUICK-WIRE): `None` means "run tier 2".
//!
//! **Method.** `x = (128m + j)·ln2/128 + r` (Tang, "Table-driven implementation of the exponential
//! function in IEEE floating-point arithmetic", ACM TOMS 15(2), 1989; N = 128):
//! * `t = RN(x·INV + SHIFT)` with `INV = RN(128/ln2)`, `SHIFT = 1.5·2^52` (one FMA in the FMA
//!   build); `t` lies in `[2^52, 2^53)`, so `kd = t − SHIFT` is the integer `k` exactly and `k` is
//!   read from `t`'s bits (`bits(t) − bits(SHIFT)`, no conversion instruction): `j = k mod 128`,
//!   `2^m` from `(k + 1023·128) >> 7`.
//! * Cody–Waite (Cody & Waite, Software Manual for the Elementary Functions, 1980):
//!   `ln2/128 = L1 + L2`, `L1` of 36 bits (`k·L1` exact for `|k| < 2^17`). `r1 = x − kd·L1` is
//!   exact: for `k != 0`, `|x| >= 2^-9` makes `x` a multiple of `2^-61`, `kd·L1` is a multiple of
//!   `2^-43`, and `|r1| < 2^-8` (the Sterbenz-type grid argument). `rh = RN(r1 − kd·L2)`,
//!   `rl ≈ (r1 − rh) − kd·L2` (the low part kept).
//! * `T_j = 2^(j/128) = th + tl` within `2^-105·th` (`EXP2_J128`, `cargo xtask tables`, 2 KiB).
//! * `q ≈ e^rh − 1 − rh` by Taylor's degree 6 in Estrin's scheme (Estrin 1960; Knuth, TAOCP 2,
//!   4.6.4): `r2 = rh²`, `pa = rh·C3 + 1/2`, `pb = rh·C5 + C4`, `pc = r2·C6 + pb`,
//!   `q = r2·(r2·pc + pa)`.
//! * `y = T·(1 + r + q)`: `u = RN(th·rh)`; `th + u = yh + e1` by Fast2Sum (`|u| < th`);
//!   `w = th·(rl + q) + tl·(1 + rh)`; `yl = (e1 + ul) + w` with `ul = th·rh − u` exactly by one
//!   FMA in the FMA build and `ul = 0` (dropped) in the SSE2 build; `(zh, zl)` = Fast2Sum of
//!   `(yh, yl)` (Dekker 1971; Muller et al., Handbook of Floating-Point Arithmetic, 2nd ed. 2018).
//! * Rounding: the one-sided test of [`super::near`] (de Dinechin, Lauter & Muller, RAIRO-ITA 41,
//!   2007) with `c` the least double with `c·(1 − 2^54·rho) > 1 + 2^-53`; `zh·2^m` is formed
//!   beside the test, not after it. Its proof needs `e^x` neither a double nor a midpoint (true for
//!   every double `x != 0`: Lindemann–Weierstrass) and `zh·2^m` normal (below). exp's test has
//!   no `w != 0` guard (D-10.77, LIBM-F64-QUICK-PROFILE): `zl = 0` passes it, and then the
//!   error `<= rho·|zh| < 2^-61·|zh|` is below a quarter of `ulp(zh)` (the gap below a power of
//!   two included), so `zh` is still `e^x` rounded to nearest; the guard exists only for the
//!   family's exact results (exp2, exp10 below), which exp has none of.
//! * Domain `2^-60 <= |x| < 708` (today's `exp_near`; one integer subtract and compare on the
//!   bits, NaN and infinities fall outside): `|k| <= 130740 < 2^17`, `-1022 <= m <= 1021`, and
//!   `zh >= 1` whenever `m = -1022` (`j >= 76` there), so `zh·2^m` is normal and finite.
//!
//! **Bound** (relative to `th`, `u = 2^-53`, `|r| <= R = (1/2 + 2^-35)·ln2/128 < 2^-8.5287` since
//! `|x·128/ln2 − kd| <= 1/2 + 2^-35`, `|rh| <= R + 2^-62`, `|rl| <= 2^-62 + 2^-79`):
//! 1. rounding of `th·rh` (SSE2 build only): `u·R = 2^-61.529`;
//! 2. polynomial evaluation in Estrin's order: `pa`, `pb`, `pc` and the inner sum carry at most
//!    `1.002u` absolute on a value `>= 0.4995`, `r2` and the last product `u` each: `q` within
//!    `4.01u·|q|` (SSE2; fewer roundings with FMA), stated `4.05u·qmax`, `qmax = 2^-18.056`:
//!    `2^-69.04`;
//! 3. the low sum: `RN(rl + q)`, `RN(th·a)` (SSE2 only), the sum forming `w` and the one forming
//!    `yl`, each `u·(qmax + 2^-62) = 2^-71.06`: 4 of them `2^-69.06` (SSE2), 3 `2^-69.47` (FMA);
//!    `e1 + ul` and `tl·(1 + rh)`'s roundings act on `2^-52` quantities (`< 2^-104`);
//! 4. neglected `rl·(e^rh − 1)`: `2^-70.53`; neglected `tl·(e^r − 1 − rh)`: `2^-71.06`;
//! 5. truncation `R^7/7!·e^R`: `2^-72.00`;
//! 6. reduction `|kd|·|ln2/128 − L1 − L2| + ` the two roundings forming `rl` (`<= 3·2^-80`, the
//!    terms `kd·L2` below `2^-27`): `2^-78.41`; the table `2^-105`.
//!
//! Divided by `|zh|/th >= e^-R·(1 − 2^-60) = 0.99729`: FMA `2^-67.727`, stated `RHO_FMA =
//! 1.25·2^-68` (`2^-67.678`); SSE2 `2^-61.503`, stated `RHO_SSE2 = 1.4375·2^-62` (`2^-61.476`).
//! The `bounds` tests recompute every term from the constants. Expected fallback `≈ c − 1`: SSE2
//! 0.565%, FMA 0.0076%.
//!
//! **Directed rounding** (LIBM-DIRECTED-INPLACE, D-10.94): [`exp_directed`] runs the same
//! reduction and kernel in the caller's direction (down, up or toward zero; the entry has checked
//! that the rest of MXCSR is the default word), so every rounding below may err by up to one ulp
//! (`2u = 2^-52` relative) instead of half of one:
//! * Reduction: `t = RN(x·INV + h + SHIFT)` would round directed, leaving `|x·128/ln2 − k| < 1`,
//!   which doubles `R` and breaks `r1`'s exactness for `|x| < 2^-9`. The entry passes `h = +1/2`
//!   when the sum rounds down (down, toward zero: the sum is positive) and `h = −1/2` when it
//!   rounds up, so `t = ∘(∘(x·INV + h) + SHIFT)` is `x·128/ln2` rounded to nearest within the
//!   roundings of `x·INV` and `+ h` (each below `2^-36` for `|x·INV| < 2^17`): `|x·128/ln2 − k| <=
//!   1/2 + 2^-34`, so `R` and `|k| <= 130743` keep their values (to `2^-34`) and `r1` is exact
//!   by the same grid argument (`k != 0` forces `|x| >= 0.0027 > 2^-9`; Sterbenz's lemma and the
//!   exact `kd·L1` hold in any direction). `kd = t − SHIFT` is exact in any direction.
//! * The kernel: bound terms 1-4 and 6 are rounding errors, each linear in `u`, so they double
//!   (`rl` grows to `|rl| <= ulp(rh) <= 2^-61`); terms 5 and the table's `2^-105` are not
//!   roundings and stay. Fast2Sum is no longer error-free: its first difference is exact
//!   (Sterbenz: the two heads are within a factor 2), and the error term is that exact difference
//!   rounded once, `<= 2u·ulp(head)`, `2^-103·th` for each of the two (Boldo, Graillat & Muller,
//!   "On the robustness of the 2Sum and Fast2Sum algorithms", ACM TOMS 44(1), 2017); the products'
//!   low parts (`th·rh − u` by FMA) stay exact, a product's error being representable in any
//!   direction. Totals over `|zh|/th >= e^-R·(1 − 2^-50)`: SSE2 `2^-60.50`, FMA `2^-66.84`;
//!   stated `RHO_DIR_SSE2 = 1.5·2^-61` (`2^-60.415`), `RHO_DIR_FMA = 1.25·2^-67` (`2^-66.678`).
//! * Rounding test: with `e = ∘(zh·rho)`, `lo = ∘(zh + ∘(zl − e))` and `hi = ∘(zh + ∘(zl + e))`
//!   in the caller's direction `∘`: the inner roundings (`<= 2^-103·zh`) and `e`'s (`2^-52·e`)
//!   fit in the margin between the totals and the stated `rho`, so `zh + ∘(zl − e) <= y <= zh +
//!   ∘(zl + e)` for the exact `y = e^x/2^m`; `∘` is monotone, so `lo == hi` means `∘(y) = lo`. The test is sound in every direction and needs no property of
//!   `e^x` (no midpoint or exactness argument). `lo·2^m` is exact: `lo >= 1.48` when `m = −1022`
//!   (`j >= 73` there) and `e^708 < 2^1022`. Expected fallback `≈ 2·rho/ulp`: SSE2 about 1.1%,
//!   FMA 0.02%; a decline goes to the switched path (the caller's word restored on exit, so the
//!   attempt's inexact flag is dropped with it). The arithmetic raises inexact only, as in round
//!   to nearest (the magnitudes are those of the nearest proof).
//!
//! **exp2 and exp10** (LIBM-F64-EXP-FAMILY-QUICK, D-10.64): the same kernel (`T_j` on, above)
//! fed by their own reductions, so `2^x` or `10^x` is `2^(k/128)·e^r` with `r = rh + rl`:
//! * exp2, `2^-60 <= |x| < 1021`: `t = RN(x·128 + SHIFT)` (`x·128` exact, so `|x·128 − k| <=
//!   1/2`), `s = x − kd/128` exact (`kd/128` exact; for `k != 0`, `|x| >= 2^-8` puts `x` and
//!   `kd/128` on the grid of `ulp(x) >= 2^-60` and `|s| <= 2^-8`), `rh = RN(s·LN2H)`; FMA build
//!   `rl = RN(s·LN2L + (s·LN2H − rh))` (the bracket exact by one FMA), SSE2 build `rl = s·LN2L`
//!   and the dropped `|s·LN2H − rh| <= ulp(rh)/2 <= 2^-62` is an extra term `u·rh` (bound term
//!   1's size). No `ln2` reduction constant: `|r| <= ln2/256` with no `2^-35` slack.
//! * exp10, `2^-60 <= |x| < 307`: `t = RN(x·INV10 + SHIFT)`, `INV10 = RN(128/log10 2)`, so
//!   `|x·128/log10 2 − k| <= 1/2 + 2^-35` (`|x·INV10| < 2^17`). With `L = log10(2)/128` split as
//!   `E0 + E1 + E2` (`NE*` negated, within `2^-121.2`; Cody–Waite as above): `r0 = x − kd·E0` is exact (for
//!   `k != 0`, `|x| > L/2 > 2^-10` puts both on the grid `2^-62`, and `|r0| < L/2 + 2^-28.7 <
//!   2^-9`); `sh = RN(r0 − kd·E1)` with `sl = RN(((r0 − sh) − kd·E1) − kd·E2)`, the bracket exact
//!   by Fast2Sum (`kd·E1`'s last place divides `2^-81`, which divides `r0`), so `|sh + sl − (x −
//!   kL)| < |k|·2^-121.2 + 2^-116 < 2^-104`, `|sh| <= 0.001176`, `|sl| <= 2^-62.9`. Then `r =
//!   (sh + sl)·ln10`: `rh = RN(sh·LN10H)`, FMA `rl = RN(sh·LN10L + RN(sl·LN10H + (sh·LN10H −
//!   rh)))`, SSE2 the same without the bracket (dropped `<= 2^-62`, the term `u·rh`). `|r| =
//!   |x·128/log10 2 − k|·ln2/128`: exp's `R` holds.
//! * Bound: terms 2, 3, 5 and the table are exp's with `|rl| <= 2^-62 + |s|·|LN2L|` (exp2) or
//!   `2^-62 + |sh|·|LN10L| + 2^-62.9·LN10H < 2^-60.5` (exp10) in place of `2^-62` (the SSE2
//!   builds drop the `2^-62`); term 4 grows to `|rl|·(e^R − 1)`; term 6 becomes the reduction's
//!   `2^-113` (exp2) and `2.31·2^-104 + 2^-112` (exp10); the SSE2 builds add `u·rh·e^R` for the
//!   dropped product. Totals over `0.99729`: exp2 FMA `2^-67.64`, SSE2 `2^-60.51`; exp10 FMA
//!   `2^-67.30`, SSE2 `2^-60.51`; stated for both `RHO_FAM_FMA = 1.6875·2^-68` (`2^-67.25`) and
//!   `RHO_FAM_SSE2 = 1.4375·2^-61` (`2^-60.48`). Expected fallback: FMA 0.010%, SSE2 1.14% (a
//!   Dekker product would halve the SSE2 rate for about six more operations on every call; at
//!   tier 2's cost the 0.57% it saves is worth about one instruction per call).
//! * Exact results: `2^x` for an integer `x` gives `s = rh = rl = q = 0`, `j = 0`, so `zl = 0`
//!   exactly and the test declines without raising inexact (tier 2 then answers it exactly);
//!   `10^x` for the integers `1 ..= 23` (a double or, at 23, a midpoint) is declined before the
//!   kernel. Every other `2^x`, `10^x` for a double `x` is irrational or not dyadic.
//!
//! **expm1** (LIBM-F64-EXP-FAMILY-QUICK-2, D-10.64; Tang, "Table-driven implementation of the expm1
//! function in IEEE floating-point arithmetic", ACM TOMS 18(2), 1992: a table region and a
//! polynomial region), `2^-60 <= |x| < 708`, `x > −37` (one compare more on the bits):
//! * `|x| >= 2^-5`: exp's reduction and kernel, with `th·rh`'s low part kept in both builds (the
//!   FMA's, or Dekker's product in the SSE2 build: the `−1` magnifies the kernel's error by up to
//!   `e^x/|e^x − 1| <= 32.503` here, and term 1 would dominate). `vh = zh·2^m`; `vh − 1 = s + e`
//!   exactly (2Sum, Knuth TAOCP 2, 4.2.2); `lo = RN(e + zl·2^m)`, then Fast2Sum `(s, lo)`. Kernel
//!   bound (exp's terms 2-6, 4 low-sum roundings in SSE2, 3 with FMA): `2^-67.59` (SSE2),
//!   `RHO_FMA` (FMA); times 32.503, plus `lo`'s rounding (`< 2^-100`): `2^-62.57` / `2^-62.66`.
//! * `|x| < 2^-5`: `e^x − 1 = x + x²/2 + x³·P(x)`, `P` Taylor's `1/3! ..= 1/9!` in Estrin's
//!   order (truncation `X^9/10!·1.003 = 2^-66.79` relative, `X = 2^-5`). The head keeps every bit
//!   of `x`: `xh` = `x` with its low 27 significand bits cleared (one AND), `xl = x − xh` exact,
//!   `x²/2 = xh²/2 + xl·(x + xh)/2` with the first product exact (26-bit `xh`) and the second
//!   below `2^-25·x²` (its two roundings `< 2^-82·|x|`). `h0 + e = x + xh²/2` exactly (Fast2Sum,
//!   `|x| >= xh²/2`); `t = (x·s)·P` with `s = RN(x²)` within `9.01u·X²·pmax·|x|` (three
//!   roundings, Estrin's `P` within `3u`, counted twice for margin; `pmax = 0.16797`); `lo = e +
//!   (hl + t)` two roundings `2^-64.57`; Fast2Sum `(h0, lo)`. Over `|e^x − 1| >= 0.98455·|x|`:
//!   `2^-62.04` (SSE2), `2^-62.09` (FMA).
//! * Stated for both builds and regions `RHO_M1 = 1.5·2^-62` (`2^-61.415`); expected fallback
//!   about 0.6%. `e^x − 1` is irrational for every double `x != 0` (Lindemann–Weierstrass), so
//!   no exact case; `zh` is normal and needs no scaling.
//!
//! **cosh, sinh, tanh** (LIBM-F64-HYPERB-QUICK, D-10.70), `a = |x|`, in front of tier 2
//! (`fast64::hyperbolic`, unchanged), all tested with `C_M1` (`RHO_M1 = 1.5·2^-62 = 2^-61.415`
//! relative to the returned head `h`):
//! * `e^a` and `e^-a` from ONE reduction (`exp_pm`, `a < 23`): exp's reduction gives `k`, `rh`,
//!   `rl`; `e^-a = 2^(−k/128)·e^(−r)`, so the second exponential is the kernel's tail at
//!   `(kb' = 2·1023·128 − kb, −rh, −rl)` with the same polynomial split into even and odd parts
//!   `E = r2·(1/2 + r2·(C4 + r2·C6))`, `O = (r2·rh)·(C3 + r2·C5)`, `q± = E ± O` (the same degree-6
//!   Taylor polynomial, so truncation term 5 holds; its evaluation: `E` within `3.01u·E`, `O`
//!   within `4u·|O|` with `|O| <= R³/6·1.01 = 2^-28.17`, the sum `u·|q±|`, at most
//!   `4.01u·E + 5u·|O| < 4.05u·qmax`: term 2 as stated). `e^-a`'s pair is scaled by `2^(m' − m)
//!   >= 2^-67` into `e^a`'s units (exact: parts normal). Each side's bound is the kernel's,
//!   relative to its own head: `RHO_SSE2` with `th·rh`'s low part dropped (`X = false`), and with
//!   it kept (Dekker's product or the FMA, `X = true`) expm1's kernel bound `2^-67.59` (SSE2) or
//!   `RHO_FMA = 2^-67.68`.
//! * cosh, `2^-60 <= a < 2^-5`: `1 + s·P(s)`, `s = RN(a²)`, `P` Taylor's `1/2, 1/4!, 1/6!, 1/8!`
//!   (omitted `s^5/10! < 2^-71.8`); the tail within `3.01u` of itself and at most `2^-11`:
//!   `2^-62.41` absolute, `h >= 1`; Fast2Sum`(1, tail)` exact. (The exponentials' route would
//!   lose `e^±a`'s low parts in `zl + ql` here: harmless to the bound, but `l` comes out 0 and
//!   the test declines.)
//! * cosh, `2^-5 <= a < 23`: `X = false`; `h + l` = Fast2Sum of `zh + qh` (`zh >= qh`: `e^a >=
//!   e^-a` and the kernels' errors are far below `e^(2a) − 1 >= 2^-59`, or both heads are `1`),
//!   its error plus `zl + ql` (roundings on `2^-51`-sized parts: `< 2^-102·h`), a second
//!   Fast2Sum. Error `RHO_SSE2·(zh + qh) + 2^-102·h`: `<= 2^-61.47·h`.
//! * sinh, `2^-5 <= a < 23`: `X = true`, the same with `zh − qh`; the kernels' errors reach the
//!   difference multiplied by `(e^a + e^-a)/(e^a − e^-a) = coth a <= coth 2^-5 = 32.0104`:
//!   `2^-67.59·32.0104 = 2^-62.585`, plus the low sum's `2^-100` (`dh >= 0.06·zh`).
//! * sinh, `2^-60 <= a < 2^-5`: `a + a³·P(a²)`, `P` Taylor's `1/3! ..= 1/9!` (omitted `a^10/11!·1.01
//!   < 2^-75.2` relative); the tail `(a·s)·P` with `s = RN(a²)` within `4.01u` of itself and at
//!   most `a²/6·1.002 <= 2^-12.58·a`: `2^-63.58·a`; Fast2Sum`(a, tail)` exact.
//! * cosh and sinh, `23 <= a < 708`: `e^a/2·(1 ± e^-2a)`, exp's quick parts halved: `RHO_SSE2 +
//!   e^-46 = 2^-61.43` (FMA: `2^-66.2`).
//! * tanh, `2^-60 <= a < 2^-6`: `a + a³·P(a²)` as sinh's series, `P` Taylor's `−1/3, 2/15,
//!   −17/315, 62/2835` (alternating, falling: omitted `< 0.0089·a^11 < 2^-66.8·a`); the tail
//!   within `5.01u` of itself (the four roundings and `RN(−1/3)`'s) and at most `a³/3`:
//!   `2^-64.3·a`, total `2^-64.0·a`, `h >= 0.99·a`. (Through expm1 the correction `a³/3` sinks
//!   below expm1's own error for `a < 2^-30` and the low word can come out 0: a decline.) sinh's
//!   series counts `RN(1/6)`'s rounding the same way (`5.01u`: `2^-63.3`).
//! * tanh, `2^-6 <= a <= 22`: `N/(N + 2)` with `N = nh + nl = e^(2a) − 1` from
//!   `expm1_quick_parts` (relative error `ε <= 2^-62.04`, its proven figure). `D = 2 + N` by 2Sum
//!   (exact) with `dl = RN(de + nl)` (`< 2^-105·D`). As `D` follows `N` exactly, `N/D`'s
//!   relative error is `ε·2/(N + 2) <= ε` (no doubling). One division: `inv = RN(1/dh)`, `q1 =
//!   RN(nh·inv)` (within `3u` of `N/D`, so `nh − RN(q1·dh)` is exact by Sterbenz), the residual
//!   `res = N − q1·D` with `q1·dh` exact (`$lox`), `q2 = RN(res·inv)`: the remaining error below
//!   `2^-100·q`. Total `<= 2^-62.03`.
//! * Every bound above is below `RHO_M1`; the test needs the result neither a double nor a
//!   midpoint, true for every double `a != 0` (Lindemann–Weierstrass: `e^a` is transcendental).
//!   Results stay normal (`h >= 2^-61`) and finite (`sc <= 2^1020`). Outside these domains
//!   (`a < 2^-60`, NaN, infinities, `a >= 708`, tanh above 22) tier 2 and then the accurate path
//!   answer exactly as before, so the overflow thresholds and the saturation of tanh are theirs.
//!   No spurious flag: every product of the small paths stays normal (`a >= 2^-60`).
//! * Targets: FP operations per call (both exponentials, the sum, the test) cosh about 45 (FMA)
//!   and 55 (SSE2), sinh 47 / 75 (Dekker's products), tanh about 40 / 55 plus one division;
//!   critical path the exp kernel's 16 (FMA) plus the Fast2Sums and the test (about 22); the
//!   two sides run in parallel.
//!
//! **Design targets** (D-10.51): instructions per call with the fallback included FMA <= 80,
//! SSE2 <= 95; FP operations 31 (FMA) and 39 (SSE2) including the two compares; critical path
//! 16 dependent FP operations plus the compare (FMA: `t, kd, r1, rh, r2, pc, inner, q, rl + q, w,
//! yl, zh, yh − zh, zl, zl·c, zh + w`), 21 in the SSE2 build (each FMA two operations).
//!
//! Both builds are correctly rounded wherever they answer, so they return the same values; their
//! fallback sets differ.

#![allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)] // bit arithmetic on bounded values (above), indices masked to 0..127

use crate::coeffs::tables as t;

/// `1.5·2^52`: `x·INV + SHIFT` lies in `[2^52, 2^53)`, where every double is an integer.
pub(super) const SHIFT: f64 = 6_755_399_441_055_744.0;
/// `bits(SHIFT) − 1023·128`: `bits(t)` minus this is `k + 1023·128 >= 0`; shifted right by 7, the
/// biased exponent of `2^m`.
pub(super) const KBIAS: u64 = 0x4338_0000_0000_0000 - 1023 * 128;
pub(super) const INV: f64 = f64::from_bits(t::SCALE_128[0]);
pub(super) const NL1: f64 = -f64::from_bits(t::LN2_128_CW[0]);
pub(super) const NL2: f64 = -f64::from_bits(t::LN2_128_CW[1]);
/// `RN(1/3!)` .. `RN(1/6!)` (checked by the `bounds` tests).
pub(super) const C3: f64 = f64::from_bits(0x3fc5_5555_5555_5555);
pub(super) const C4: f64 = f64::from_bits(0x3fa5_5555_5555_5555);
pub(super) const C5: f64 = f64::from_bits(0x3f81_1111_1111_1111);
pub(super) const C6: f64 = f64::from_bits(0x3f56_c16c_16c1_6c17);
/// `bits(2^-60)` and `bits(708) − bits(2^-60)`: the domain as one unsigned compare on `|x|`'s bits.
pub(super) const LO_BITS: u64 = 0x3c30_0000_0000_0000;
pub(super) const SPAN: u64 = 0x4086_2000_0000_0000 - LO_BITS;
pub(super) const ABS: u64 = 0x7fff_ffff_ffff_ffff;

/// The stated bounds (module doc) and their [`super::near`] multipliers: the least doubles with
/// `c·(1 − 2^54·rho) > 1 + 2^-53` (checked exactly by the `bounds` tests).
#[doc(hidden)] // public for the boundary's FMA-build tests only
pub const RHO_SSE2: f64 = f64::from_bits(0x3c17_0000_0000_0000);
#[doc(hidden)] // public for the boundary's FMA-build tests only
pub const RHO_FMA: f64 = f64::from_bits(0x3bb4_0000_0000_0000);
pub(crate) const C_SSE2: f64 = f64::from_bits(0x3ff0_1721_3fcb_b4d5);
pub(crate) const C_FMA: f64 = f64::from_bits(0x3ff0_0050_0190_07d1);
/// exp's stated bounds in a directed rounding direction (module doc, "Directed rounding"): the
/// half-width of [`exp_directed`]'s interval relative to `zh`, `1.5·2^-61` (SSE2) and
/// `1.25·2^-67` (FMA), checked against the recomputed terms by the `directed_bounds` test.
#[doc(hidden)] // public for the boundary's FMA-build tests only
pub const RHO_DIR_SSE2: f64 = f64::from_bits(0x3c28_0000_0000_0000);
#[doc(hidden)] // public for the boundary's FMA-build tests only
pub const RHO_DIR_FMA: f64 = f64::from_bits(0x3bc4_0000_0000_0000);

// exp2 and exp10 on the same kernel (LIBM-F64-EXP-FAMILY-QUICK, D-10.64): their own reductions
// feed `kernel` a `(rh, rl)` with the bound of the exp2/exp10 section of the module doc.
/// `ln2 = LN2H + LN2L` and `ln10 = LN10H + LN10L` (`|LN2L|`, `|LN10L| < 2^-52`, each pair within
/// `2^-106` relative of exact; `mp::ln2`, `mp::ln10`).
const LN2H: f64 = f64::from_bits(t::LN2[0]);
const LN2L: f64 = f64::from_bits(t::LN2[1]);
const LN10H: f64 = f64::from_bits(t::LN10[0]);
const LN10L: f64 = f64::from_bits(t::LN10[1]);
/// `RN(128/log10 2)`: twice `SCALES[1] = RN(64/log10 2)` (doubling is exact).
const INV10: f64 = 2.0 * f64::from_bits(t::SCALES[1]);
/// `−log10(2)/128` in three parts: `LOG10_2_64_CW` halved (exact), negated; `E0` has 32
/// significant bits (lowest `2^-40`), `E1` 36 (lowest `2^-81`), `E2` 36, so `k·E0`, `k·E1`, `k·E2`
/// are exact for `|k| < 2^17`, and the parts are within `2^-121.2` of `log10(2)/128`.
const NE0: f64 = -0.5 * f64::from_bits(t::LOG10_2_64_CW[0]);
const NE1: f64 = -0.5 * f64::from_bits(t::LOG10_2_64_CW[1]);
const NE2: f64 = -0.5 * f64::from_bits(t::LOG10_2_64_CW[2]);
/// `bits(1021) − bits(2^-60)` and `bits(307) − bits(2^-60)`: the exp2 and exp10 domains.
const SPAN2: u64 = 0x408f_e800_0000_0000 - LO_BITS;
const SPAN10: u64 = 0x4073_3000_0000_0000 - LO_BITS;
/// `bits(1)` and `bits(23) − bits(1)`: exp10's exact cases `1 <= x <= 23` (integers) are left
/// to the entry's exact path, which raises no inexact.
const ONE_BITS: u64 = 0x3ff0_0000_0000_0000;
const SPAN_EXACT10: u64 = 0x4037_0000_0000_0000 - ONE_BITS;

/// exp2's and exp10's stated bounds (module doc, "exp2 and exp10") and their [`super::near`]
/// multipliers (checked exactly by the `bounds` tests).
#[doc(hidden)] // public for the boundary's FMA-build tests only
pub const RHO_FAM_SSE2: f64 = f64::from_bits(0x3c27_0000_0000_0000);
#[doc(hidden)] // public for the boundary's FMA-build tests only
pub const RHO_FAM_FMA: f64 = f64::from_bits(0x3bbb_0000_0000_0000);
pub(crate) const C_FAM_SSE2: f64 = f64::from_bits(0x3ff0_2e85_c089_8b72);
pub(crate) const C_FAM_FMA: f64 = f64::from_bits(0x3ff0_006c_02d9_133a);

// expm1 (D-10.64, module doc "expm1"): exp's reduction and kernel for `|x| >= 2^-5`, a series
// with an exact head below.
/// `RN(1/7!)`, `RN(1/8!)`, `RN(1/9!)`: the small region's series beyond exp's `C3 ..= C6`.
const C7: f64 = 1.0 / 5040.0;
const C8: f64 = 1.0 / 40320.0;
const C9: f64 = 1.0 / 362_880.0;
/// Clears the low 27 significand bits: `xh` keeps 26 significant bits, so `xh·xh` is exact.
const MASK27: u64 = !((1 << 27) - 1);
/// `bits(2^-5)`: below it (on `|x|`'s bits) the series region.
const SMALL_M1: u64 = 0x3fa0_0000_0000_0000;
/// `bits(2^-5)`: sinh's series below it (module doc, "cosh, sinh, tanh").
const SINH_POLY: u64 = 0x3fa0_0000_0000_0000;
/// `bits(23)`: from it on, `cosh = sinh = e^a/2` within `e^-46 < 2^-66.3` relative.
const HYP_BIG: u64 = 0x4037_0000_0000_0000;
/// `bits(22) − bits(2^-60)`: tanh's quick domain `[2^-60, 22]` as one unsigned compare.
const TANH_SPAN: u64 = 0x4036_0000_0000_0000 - LO_BITS;
/// `bits(2^-6)`: tanh's series below it, with Taylor's `−1/3, 2/15, −17/315, 62/2835`.
const TANH_POLY: u64 = 0x3f90_0000_0000_0000;
const T3: f64 = -1.0 / 3.0;
const T5: f64 = 2.0 / 15.0;
const T7: f64 = -17.0 / 315.0;
const T9: f64 = 62.0 / 2835.0;
/// `bits(−37)`: a negative `x` with bits at or above it is `x <= −37` (tier 2's limit).
const NEG37_BITS: u64 = 0xc042_8000_0000_0000;
/// expm1's stated bound (both builds, both regions; module doc "expm1") and its [`super::near`]
/// multiplier (checked exactly by the `bounds` tests).
#[doc(hidden)] // public for the boundary's FMA-build tests only
pub const RHO_M1: f64 = f64::from_bits(0x3c18_0000_0000_0000);
pub(crate) const C_M1: f64 = f64::from_bits(0x3ff0_1824_3651_7a38);

/// `a·b + c` in two roundings (the SSE2 build).
#[inline(always)]
fn ma(a: f64, b: f64, c: f64) -> f64 {
    a * b + c
}

/// The low part of `th·rh` the SSE2 build drops (bound term 1).
#[inline(always)]
fn no_lo(_th: f64, _rh: f64, _u: f64) -> f64 {
    0.0
}

/// `a·b − p` exactly for `p = RN(a·b)` without FMA (Dekker's product, Veltkamp's splitting at 27
/// bits; Dekker 1971): expm1's SSE2 build keeps `th·rh`'s low part, which its `−1` would otherwise
/// magnify (module doc "expm1"). `|a| < 2`, `|b| < 2^-8`: no overflow, and the parts' products
/// stay normal or 0 for the `rh` that reach it. tanh's residual uses it with `2^-61 <= a < 1`,
/// `2 <= b < 2^64`: the split's `b·(2^27 + 1)` and every partial product stay finite and normal.
#[inline(always)]
fn dekker_lo(a: f64, b: f64, p: f64) -> f64 {
    const SPLIT: f64 = 134_217_729.0;
    let ca = a * SPLIT;
    let ah = ca - (ca - a);
    let al = a - ah;
    let cb = b * SPLIT;
    let bh = cb - (cb - b);
    let bl = b - bh;
    (((ah * bh - p) + ah * bl) + al * bh) + al * bl
}

// The kernel in two builds (D-10.25, D-10.50): `$ma` is `a·b + c` (two roundings, or one FMA),
// `$lo` the low part of `th·rh` (0, or exact by one FMA), `$lox` that low part exactly (Dekker's
// product, or the FMA) for expm1, `$c` the rounding test's multiplier.
macro_rules! quick {
    ($(#[$a:meta])*; $(#[$i:meta])*; $ma:path; $lo:path; $lox:path; $c:expr; $cf:expr; $cd:expr) => {
/// `e^x = (zh + zl)·sc` within `rho·|zh|·sc` for `2^-60 <= |x| < 708`, else `None` (module doc).
$(#[$a])*
$(#[$i])*
#[doc(hidden)] // public for the boundary's FMA-build tests only
pub fn exp_quick_parts(x: f64) -> Option<(f64, f64, f64)> {
    if (x.to_bits() & ABS).wrapping_sub(LO_BITS) >= SPAN {
        return None;
    }
    let tt = $ma(x, INV, SHIFT);
    let kd = tt - SHIFT;
    let kb = tt.to_bits().wrapping_sub(KBIAS);
    let r1 = $ma(kd, NL1, x);
    let rh = $ma(kd, NL2, r1);
    let rl = $ma(kd, NL2, r1 - rh);
    Some(kernel::<false>(kb, rh, rl))
}

/// `e^x` rounded in the caller's direction (down, up or toward zero, read by nothing here: the
/// arithmetic runs in it) for `2^-60 <= |x| < 708` when the directed test decides it, else `None`
/// (module doc, "Directed rounding", D-10.94). `h` is `−1/2` when the caller rounds up and `+1/2`
/// when it rounds down or toward zero, so that `k` is `x·128/ln2` rounded to nearest within `2^-34`
/// whatever the direction. Not for round to nearest (`h = ±1/2` there breaks the reduction).
$(#[$a])*
$(#[$i])*
#[doc(hidden)] // public for the boundary's FMA-build tests only
pub fn exp_directed(x: f64, h: f64) -> Option<f64> {
    if (x.to_bits() & ABS).wrapping_sub(LO_BITS) >= SPAN {
        return None;
    }
    let tt = $ma(x, INV, h) + SHIFT;
    let kd = tt - SHIFT;
    let kb = tt.to_bits().wrapping_sub(KBIAS);
    let r1 = $ma(kd, NL1, x);
    let rh = $ma(kd, NL2, r1);
    let rl = $ma(kd, NL2, r1 - rh);
    let (zh, zl, sc) = kernel::<false>(kb, rh, rl);
    let e = zh * $cd;
    let lo = zh + (zl - e);
    if lo == zh + (zl + e) { Some(lo * sc) } else { None }
}

/// `2^x = (zh + zl)·sc` within `RHO_FAM_*·|zh|·sc` for `2^-60 <= |x| < 1021`, else `None`
/// (module doc, "exp2 and exp10"). An integer `x` gives `zl = 0` exactly, which the rounding test
/// declines (the entry's exact path answers it).
$(#[$a])*
$(#[$i])*
#[doc(hidden)] // public for the boundary's FMA-build tests only
pub fn exp2_quick_parts(x: f64) -> Option<(f64, f64, f64)> {
    if (x.to_bits() & ABS).wrapping_sub(LO_BITS) >= SPAN2 {
        return None;
    }
    let tt = $ma(x, 128.0, SHIFT);
    let kd = tt - SHIFT;
    let kb = tt.to_bits().wrapping_sub(KBIAS);
    let s = $ma(kd, -1.0 / 128.0, x);
    let rh = s * LN2H;
    let rl = $ma(s, LN2L, $lo(s, LN2H, rh));
    Some(kernel::<false>(kb, rh, rl))
}

/// `10^x = (zh + zl)·sc` within `RHO_FAM_*·|zh|·sc` for `2^-60 <= |x| < 307` other than the
/// integers `1 ..= 23`, else `None` (module doc, "exp2 and exp10").
$(#[$a])*
$(#[$i])*
#[doc(hidden)] // public for the boundary's FMA-build tests only
pub fn exp10_quick_parts(x: f64) -> Option<(f64, f64, f64)> {
    let b = x.to_bits();
    if (b & ABS).wrapping_sub(LO_BITS) >= SPAN10 {
        return None;
    }
    if b.wrapping_sub(ONE_BITS) <= SPAN_EXACT10 && (x + SHIFT) - SHIFT == x {
        return None;
    }
    let tt = $ma(x, INV10, SHIFT);
    let kd = tt - SHIFT;
    let kb = tt.to_bits().wrapping_sub(KBIAS);
    let r0 = $ma(kd, NE0, x);
    let e1 = kd * NE1;
    let sh = r0 + e1;
    let sl = $ma(kd, NE2, (r0 - sh) + e1);
    let rh = sh * LN10H;
    let rl = $ma(sh, LN10L, $ma(sl, LN10H, $lo(sh, LN10H, rh)));
    Some(kernel::<false>(kb, rh, rl))
}

/// The kernel shared by exp, exp2 and exp10: `2^(k/128)·e^(rh + rl)` as `(zh, zl, 2^m)` from
/// `kb = k + 1023·128` (module doc, from `T_j` on).
$(#[$a])*
$(#[$i])*
fn kernel<const X: bool>(kb: u64, rh: f64, rl: f64) -> (f64, f64, f64) {
    let r2 = rh * rh;
    let pa = $ma(rh, C3, 0.5);
    let pb = $ma(rh, C5, C4);
    let pc = $ma(r2, C6, pb);
    let q = r2 * $ma(r2, pc, pa);
    let (zh, zl) = side::<X>(kb, rh, rl, q);
    (zh, zl, f64::from_bits((kb >> 7) << 52))
}

/// The kernel from `T_j` on: `T_j·(1 + rh + rl + q)` as `(zh, zl)` for `j = kb mod 128` and
/// `q ≈ e^rh − 1 − rh` (module doc, from `y = T·(1 + r + q)`).
$(#[$a])*
$(#[$i])*
fn side<const X: bool>(kb: u64, rh: f64, rl: f64, q: f64) -> (f64, f64) {
    let j = usize::try_from(kb & 127).unwrap_or(0);
    let th = f64::from_bits(t::EXP2_J128[j][0]);
    let tl = f64::from_bits(t::EXP2_J128[j][1]);
    let u = th * rh;
    let yh = th + u;
    let e1 = (th - yh) + u;
    let w = $ma(th, rl + q, tl * (1.0 + rh));
    let yl = (e1 + if X { $lox(th, rh, u) } else { $lo(th, rh, u) }) + w;
    let zh = yh + yl;
    (zh, (yh - zh) + yl)
}

/// `e^a` and `e^-a` from one reduction, for `2^-60 <= a < 23` (module doc, "cosh, sinh, tanh"):
/// `(zh, zl, qh, ql, kb)` with `e^a = (zh + zl)·2^m` and `e^-a = (qh + ql)·2^m`, `kb = k +
/// 1023·128` (so `m = (kb >> 7) − 1023`). `e^-a` is the kernel at `(−k, −r)`: the polynomial's
/// even and odd parts `E`, `O` are shared (`q± = E ± O`), and its scaling by `2^(m' − m) >=
/// 2^-67` is exact.
$(#[$a])*
$(#[$i])*
fn exp_pm<const X: bool>(a: f64) -> (f64, f64, f64, f64, u64) {
    let tt = $ma(a, INV, SHIFT);
    let kd = tt - SHIFT;
    let kb = tt.to_bits().wrapping_sub(KBIAS);
    let r1 = $ma(kd, NL1, a);
    let rh = $ma(kd, NL2, r1);
    let rl = $ma(kd, NL2, r1 - rh);
    let r2 = rh * rh;
    let e = r2 * $ma(r2, $ma(r2, C6, C4), 0.5);
    let o = (r2 * rh) * $ma(r2, C5, C3);
    let (zh, zl) = side::<X>(kb, rh, rl, e + o);
    let kn = 2 * 1023 * 128 - kb;
    let (wh, wl) = side::<X>(kn, -rh, -rl, e - o);
    let s = f64::from_bits(((kn >> 7) + 1023 - (kb >> 7)) << 52);
    (zh, zl, wh * s, wl * s, kb)
}

/// `sinh|x| = (h + l)·sc` within `RHO_M1·|h|·sc` for `2^-60 <= |x| < 708`, else `None` (module
/// doc, "cosh, sinh, tanh"): the odd series below `2^-5`, `e^a/2` from `23` on, else
/// `(e^a − e^-a)/2` from [`exp_pm`].
$(#[$a])*
$(#[$i])*
#[doc(hidden)] // public for the boundary's FMA-build tests only
pub fn sinh_quick_parts(x: f64) -> Option<(f64, f64, f64)> {
    let b = x.to_bits() & ABS;
    if b.wrapping_sub(LO_BITS) >= SPAN {
        return None;
    }
    let a = f64::from_bits(b);
    if b < SINH_POLY {
        let s = a * a;
        let p = $ma(s, $ma(s, $ma(s, C9, C7), C5), C3);
        let tl = (a * s) * p;
        let h = a + tl;
        return Some((h, (a - h) + tl, 1.0));
    }
    if b >= HYP_BIG {
        let (zh, zl, sc) = exp_quick_parts(a)?;
        return Some((zh, zl, 0.5 * sc));
    }
    let (zh, zl, qh, ql, kb) = exp_pm::<true>(a);
    let dh = zh - qh;
    let lo = (zl - ql) + ((zh - dh) - qh);
    let h = dh + lo;
    Some((h, (dh - h) + lo, f64::from_bits(((kb >> 7) - 1) << 52)))
}

/// `cosh x = (h + l)·sc` within `RHO_M1·|h|·sc` for `2^-60 <= |x| < 708`, else `None` (as
/// [`sinh_quick_parts`], with the even series below `2^-5`).
$(#[$a])*
$(#[$i])*
#[doc(hidden)] // public for the boundary's FMA-build tests only
pub fn cosh_quick_parts(x: f64) -> Option<(f64, f64, f64)> {
    let b = x.to_bits() & ABS;
    if b.wrapping_sub(LO_BITS) >= SPAN {
        return None;
    }
    let a = f64::from_bits(b);
    if b < SINH_POLY {
        let s = a * a;
        let tl = s * $ma(s, $ma(s, $ma(s, C8, C6), C4), 0.5);
        let h = 1.0 + tl;
        return Some((h, (1.0 - h) + tl, 1.0));
    }
    if b >= HYP_BIG {
        let (zh, zl, sc) = exp_quick_parts(a)?;
        return Some((zh, zl, 0.5 * sc));
    }
    let (zh, zl, qh, ql, kb) = exp_pm::<false>(a);
    let dh = zh + qh;
    let lo = (zl + ql) + ((zh - dh) + qh);
    let h = dh + lo;
    Some((h, (dh - h) + lo, f64::from_bits(((kb >> 7) - 1) << 52)))
}

/// `tanh|x| = h + l` within `RHO_M1·|h|` for `2^-60 <= |x| <= 22`, else `None` (module doc,
/// "cosh, sinh, tanh"): `N/(N + 2)` with `N = e^(2a) − 1` from [`expm1_quick_parts`], one
/// division and one correction.
$(#[$a])*
$(#[$i])*
#[doc(hidden)] // public for the boundary's FMA-build tests only
pub fn tanh_quick_parts(x: f64) -> Option<(f64, f64)> {
    let b = x.to_bits() & ABS;
    if b.wrapping_sub(LO_BITS) > TANH_SPAN {
        return None;
    }
    let a = f64::from_bits(b);
    if b < TANH_POLY {
        let s = a * a;
        let p = $ma(s, $ma(s, $ma(s, T9, T7), T5), T3);
        let tl = (a * s) * p;
        let h = a + tl;
        return Some((h, (a - h) + tl));
    }
    let (nh, nl) = expm1_quick_parts(2.0 * a)?;
    let dh = nh + 2.0;
    let bb = dh - nh;
    let dl = ((nh - (dh - bb)) + (2.0 - bb)) + nl;
    let inv = 1.0 / dh;
    let q1 = nh * inv;
    let p = q1 * dh;
    let res = (((nh - p) - $lox(q1, dh, p)) + nl) - q1 * dl;
    let q2 = res * inv;
    let h = q1 + q2;
    Some((h, (q1 - h) + q2))
}

/// `sinh`'s in-place fast path (as [`expm1_tiered`]): the quick phase, then on a miss or outside
/// its domain tier 2 (`fast64::sinh_nearest` in the same build, out of line).
$(#[$a])*
$(#[$i])*
pub fn sinh_tiered(x: f64) -> Option<f64> {
    let Some((h, l, sc)) = sinh_quick_parts(x) else { return tier2_sinh(x) };
    let w = l * C_M1;
    if h + w == h && w != 0.0 {
        return Some(f64::from_bits((h * sc).to_bits() | (x.to_bits() & !ABS)));
    }
    tier2_sinh(x)
}

/// `cosh`'s in-place fast path (as [`sinh_tiered`]).
$(#[$a])*
$(#[$i])*
pub fn cosh_tiered(x: f64) -> Option<f64> {
    let Some((h, l, sc)) = cosh_quick_parts(x) else { return tier2_cosh(x) };
    let w = l * C_M1;
    if h + w == h && w != 0.0 {
        return Some(h * sc);
    }
    tier2_cosh(x)
}

/// `tanh`'s in-place fast path (as [`sinh_tiered`]).
$(#[$a])*
$(#[$i])*
pub fn tanh_tiered(x: f64) -> Option<f64> {
    let Some((h, l)) = tanh_quick_parts(x) else { return tier2_tanh(x) };
    let w = l * C_M1;
    if h + w == h && w != 0.0 {
        return Some(f64::from_bits(h.to_bits() | (x.to_bits() & !ABS)));
    }
    tier2_tanh(x)
}

/// The quick phase of `sinh`, `cosh` or `tanh` alone (`f` 0, 1, 2): the correctly rounded result,
/// or `None` when the phase declines (tests only).
$(#[$a])*
$(#[$i])*
#[doc(hidden)] // public for the boundary's FMA-build tests only
pub fn hyp_quick(f: u8, x: f64) -> Option<f64> {
    let sign = x.to_bits() & !ABS;
    let (h, l, sc, sign) = match f {
        0 => { let (h, l, sc) = sinh_quick_parts(x)?; (h, l, sc, sign) }
        1 => { let (h, l, sc) = cosh_quick_parts(x)?; (h, l, sc, 0) }
        _ => { let (h, l) = tanh_quick_parts(x)?; (h, l, 1.0, sign) }
    };
    let w = l * C_M1;
    let decided = h + w == h && w != 0.0;
    if decided { Some(f64::from_bits((h * sc).to_bits() | sign)) } else { None }
}

/// The correctly rounded `e^x` (round to nearest) when the quick phase's test decides it, else
/// `None` (the caller runs [`super::exp_nearest`]).
$(#[$a])*
$(#[$i])*
pub fn exp_quick(x: f64) -> Option<f64> {
    let (zh, zl, sc) = exp_quick_parts(x)?;
    let v = zh * sc;
    if zh + zl * $c == zh { Some(v) } else { None }
}

/// `exp`'s in-place fast path (D-10.51): the quick phase, then on a miss tier 2
/// (`fast64::exp_nearest` in the same build, out of line). `None` outside `2^-60 <= |x| < 708`
/// (tier 2's own domain, so it is not tried) or when tier 2 declines too: the caller then runs
/// the accurate path.
$(#[$a])*
$(#[$i])*
pub fn exp_tiered(x: f64) -> Option<f64> {
    let (zh, zl, sc) = exp_quick_parts(x)?;
    let v = zh * sc;
    if zh + zl * $c == zh {
        return Some(v);
    }
    tier2(x)
}

/// The correctly rounded `2^x` (round to nearest) when the quick phase's test decides it, else
/// `None`.
$(#[$a])*
$(#[$i])*
pub fn exp2_quick(x: f64) -> Option<f64> {
    let (zh, zl, sc) = exp2_quick_parts(x)?;
    let v = zh * sc;
    let w = zl * $cf;
    if zh + w == zh && w != 0.0 { Some(v) } else { None }
}

/// `exp2`'s in-place fast path (as [`exp_tiered`]): the quick phase, then on a miss tier 2
/// (`fast64::exp2_nearest` in the same build, out of line; it answers integers exactly).
$(#[$a])*
$(#[$i])*
pub fn exp2_tiered(x: f64) -> Option<f64> {
    let Some((zh, zl, sc)) = exp2_quick_parts(x) else { return tier2_exp2(x) };
    let v = zh * sc;
    let w = zl * $cf;
    if zh + w == zh && w != 0.0 {
        return Some(v);
    }
    tier2_exp2(x)
}

/// The correctly rounded `10^x` (round to nearest) when the quick phase's test decides it, else
/// `None`.
$(#[$a])*
$(#[$i])*
pub fn exp10_quick(x: f64) -> Option<f64> {
    let (zh, zl, sc) = exp10_quick_parts(x)?;
    let v = zh * sc;
    let w = zl * $cf;
    if zh + w == zh && w != 0.0 { Some(v) } else { None }
}

/// `exp10`'s in-place fast path (as [`exp_tiered`]): the quick phase, then on a miss tier 2
/// (`fast64::exp10_nearest` in the same build, out of line).
$(#[$a])*
$(#[$i])*
pub fn exp10_tiered(x: f64) -> Option<f64> {
    let Some((zh, zl, sc)) = exp10_quick_parts(x) else { return tier2_exp10(x) };
    let v = zh * sc;
    let w = zl * $cf;
    if zh + w == zh && w != 0.0 {
        return Some(v);
    }
    tier2_exp10(x)
}

/// `e^x − 1 = zh + zl` within `RHO_M1·|zh|` for `2^-60 <= |x| < 708`, `x > −37`, else `None`
/// (module doc, "expm1"): the series with an exact head below `2^-5`, else exp's kernel with the
/// `−1` folded in before rounding.
$(#[$a])*
$(#[$i])*
#[doc(hidden)] // public for the boundary's FMA-build tests only
pub fn expm1_quick_parts(x: f64) -> Option<(f64, f64)> {
    let b = x.to_bits();
    let a = b & ABS;
    if a.wrapping_sub(LO_BITS) >= SPAN || b >= NEG37_BITS {
        return None;
    }
    if a < SMALL_M1 {
        let xh = f64::from_bits(b & MASK27);
        let xl = x - xh;
        let hh = 0.5 * (xh * xh);
        let hl = (0.5 * xl) * (x + xh);
        let s = x * x;
        let p01 = $ma(x, C4, C3);
        let p23 = $ma(x, C6, C5);
        let p4 = $ma(s, C9, $ma(x, C8, C7));
        let p = $ma(s, $ma(s, p4, p23), p01);
        let tl = (x * s) * p;
        let h0 = x + hh;
        let lo = ((x - h0) + hh) + (hl + tl);
        let zh = h0 + lo;
        return Some((zh, (h0 - zh) + lo));
    }
    let tt = $ma(x, INV, SHIFT);
    let kd = tt - SHIFT;
    let kb = tt.to_bits().wrapping_sub(KBIAS);
    let r1 = $ma(kd, NL1, x);
    let rh = $ma(kd, NL2, r1);
    let rl = $ma(kd, NL2, r1 - rh);
    let (zh, zl, sc) = kernel::<true>(kb, rh, rl);
    let vh = zh * sc;
    let s = vh - 1.0;
    let bb = s - vh;
    let lo = ((vh - (s - bb)) + (-1.0 - bb)) + zl * sc;
    let h = s + lo;
    Some((h, (s - h) + lo))
}

/// The correctly rounded `e^x − 1` (round to nearest) when the quick phase's test decides it,
/// else `None`.
$(#[$a])*
$(#[$i])*
pub fn expm1_quick(x: f64) -> Option<f64> {
    let (zh, zl) = expm1_quick_parts(x)?;
    let w = zl * C_M1;
    if zh + w == zh && w != 0.0 { Some(zh) } else { None }
}

/// `expm1`'s in-place fast path (as [`exp_tiered`]): the quick phase, then on a miss or outside
/// its domain tier 2 (`fast64::expm1_nearest` in the same build, out of line).
$(#[$a])*
$(#[$i])*
pub fn expm1_tiered(x: f64) -> Option<f64> {
    let Some((zh, zl)) = expm1_quick_parts(x) else { return tier2_expm1(x) };
    let w = zl * C_M1;
    if zh + w == zh && w != 0.0 {
        return Some(zh);
    }
    tier2_expm1(x)
}
    };
}

/// Tier 2 of [`expm1_tiered`] (and its answer outside the quick domain), out of line and cold.
#[cold]
#[inline(never)]
fn tier2_expm1(x: f64) -> Option<f64> {
    super::expm1_nearest(x)
}

/// Tier 2 of [`sinh_tiered`] (and its answer outside the quick domain), out of line and cold.
#[cold]
#[inline(never)]
fn tier2_sinh(x: f64) -> Option<f64> {
    super::sinh_nearest(x)
}

/// Tier 2 of [`cosh_tiered`] (as [`tier2_sinh`]).
#[cold]
#[inline(never)]
fn tier2_cosh(x: f64) -> Option<f64> {
    super::cosh_nearest(x)
}

/// Tier 2 of [`tanh_tiered`] (as [`tier2_sinh`]).
#[cold]
#[inline(never)]
fn tier2_tanh(x: f64) -> Option<f64> {
    super::tanh_nearest(x)
}

/// Tier 2 of [`exp2_tiered`] (and its answer outside the quick domain: NaN, infinities, integers),
/// out of line and cold.
#[cold]
#[inline(never)]
fn tier2_exp2(x: f64) -> Option<f64> {
    super::exp2_nearest(x)
}

/// Tier 2 of [`exp10_tiered`] (as [`tier2_exp2`]).
#[cold]
#[inline(never)]
fn tier2_exp10(x: f64) -> Option<f64> {
    super::exp10_nearest(x)
}

/// Tier 2 of [`exp_tiered`] (the SSE2 build of [`super::exp_nearest`]), out of line and cold so
/// the quick phase keeps its registers: about 0.5% of calls reach it.
#[cold]
#[inline(never)]
fn tier2(x: f64) -> Option<f64> {
    super::exp_nearest(x)
}

quick!(; #[inline(always)]; ma; no_lo; dekker_lo; C_SSE2; C_FAM_SSE2; RHO_DIR_SSE2);

/// The quick phase's FMA build (D-10.25, D-10.50): callable only on a CPU with FMA whose AVX state
/// the OS saves; the boundary chooses it once at startup (`dispatch::fma`).
// ARCH(x86_64): the run-time FMA build; FMA is base ISA on aarch64, so one build there
#[cfg(target_arch = "x86_64")]
pub mod fused {
    use super::{
        t, ABS, C3, C4, C5, C6, C7, C8, C9, C_FAM_FMA, C_FMA, C_M1, RHO_DIR_FMA, INV, INV10, KBIAS, LN10H, LN10L, LN2H, LN2L, LO_BITS, MASK27, NE0, NE1, NE2,
        NEG37_BITS, NL1, NL2, ONE_BITS, SHIFT, SMALL_M1, SPAN, SPAN10, SPAN2, SPAN_EXACT10, SINH_POLY, HYP_BIG,
        TANH_SPAN, TANH_POLY, T3, T5, T7, T9,
    };
    // ARCH(x86_64): the scalar FMA intrinsics
    use core::arch::x86_64::{_mm_cvtsd_f64, _mm_fmadd_sd, _mm_fmsub_sd, _mm_set_sd};

    /// `a·b + c` in one rounding.
    #[target_feature(enable = "fma")]
    #[inline]
    fn ma(a: f64, b: f64, c: f64) -> f64 {
        _mm_cvtsd_f64(_mm_fmadd_sd(_mm_set_sd(a), _mm_set_sd(b), _mm_set_sd(c)))
    }

    /// `th·rh − u` exactly for `u = RN(th·rh)` (2MultFMA; the product is far from underflow).
    #[target_feature(enable = "fma")]
    #[inline]
    fn lo(th: f64, rh: f64, u: f64) -> f64 {
        _mm_cvtsd_f64(_mm_fmsub_sd(_mm_set_sd(th), _mm_set_sd(rh), _mm_set_sd(u)))
    }

    /// Tier 2 of [`exp_tiered`] in the FMA build (`fast64::fused::exp_nearest`), out of line and
    /// cold: under 0.01% of calls reach it.
    #[target_feature(enable = "fma")]
    #[cold]
    #[inline(never)]
    fn tier2(x: f64) -> Option<f64> {
        crate::fast64::fused::exp_nearest(x)
    }

    /// Tier 2 of [`exp2_tiered`] in the FMA build (`fast64::fused::exp2_nearest`), out of line
    /// and cold.
    #[target_feature(enable = "fma")]
    #[cold]
    #[inline(never)]
    fn tier2_exp2(x: f64) -> Option<f64> {
        crate::fast64::fused::exp2_nearest(x)
    }

    /// Tier 2 of [`exp10_tiered`] in the FMA build (`fast64::fused::exp10_nearest`), out of
    /// line and cold.
    #[target_feature(enable = "fma")]
    #[cold]
    #[inline(never)]
    fn tier2_exp10(x: f64) -> Option<f64> {
        crate::fast64::fused::exp10_nearest(x)
    }

    /// Tier 2 of [`expm1_tiered`] in the FMA build (`fast64::fused::expm1_nearest`), out of
    /// line and cold.
    #[target_feature(enable = "fma")]
    #[cold]
    #[inline(never)]
    fn tier2_expm1(x: f64) -> Option<f64> {
        crate::fast64::fused::expm1_nearest(x)
    }

    /// Tier 2 of [`sinh_tiered`] in the FMA build (`fast64::fused::sinh_nearest`), out of line
    /// and cold.
    #[target_feature(enable = "fma")]
    #[cold]
    #[inline(never)]
    fn tier2_sinh(x: f64) -> Option<f64> {
        crate::fast64::fused::sinh_nearest(x)
    }

    /// Tier 2 of [`cosh_tiered`] in the FMA build (as [`tier2_sinh`]).
    #[target_feature(enable = "fma")]
    #[cold]
    #[inline(never)]
    fn tier2_cosh(x: f64) -> Option<f64> {
        crate::fast64::fused::cosh_nearest(x)
    }

    /// Tier 2 of [`tanh_tiered`] in the FMA build (as [`tier2_sinh`]).
    #[target_feature(enable = "fma")]
    #[cold]
    #[inline(never)]
    fn tier2_tanh(x: f64) -> Option<f64> {
        crate::fast64::fused::tanh_nearest(x)
    }

    quick!(
        #[doc = ""]
        #[doc = "# Safety"]
        #[doc = ""]
        #[doc = "The CPU must support FMA and the OS must save the AVX state (the `#[target_feature]` contract)."]
        #[target_feature(enable = "fma")]; #[inline]; ma; lo; lo; C_FMA; C_FAM_FMA; RHO_DIR_FMA
    );
}

#[cfg(test)]
mod tests;
