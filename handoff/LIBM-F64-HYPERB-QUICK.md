# LIBM-F64-HYPERB-QUICK: double cosh/sinh/tanh quick tier on the exp quick kernel

State: branch `task/libm-f64-hyperb-quick` (from origin/next b14f639cc), role keke:~/w/n1. Decision D-10.70
(`records/decisions/D-10/D-10.70.toml`); also D-10.51 (two-phase double), D-10.64 (expm1 quick), D-10.25/D-10.50 (builds).

## What is in

* `crates/math/src/fast64/quick.rs` (module doc "cosh, sinh, tanh" has every bound): `side` (the kernel's tail, now
  shared by `kernel`), `exp_pm` (e^a and e^-a from ONE reduction, even/odd polynomial parts shared),
  `sinh_quick_parts` (odd Taylor series < 2^-5, `exp_pm` with the kernel's low product kept up to 23, `e^a/2` up to 708),
  `cosh_quick_parts` (even series < 2^-5, `exp_pm` without the low product, `e^a/2`), `tanh_quick_parts` (odd series
  < 2^-6, else `N/(N+2)` on `expm1_quick_parts(2a)`, one division + residual, domain <= 22), `*_tiered` (quick, then
  tier 2 `fast64::hyperbolic` cold), `hyp_quick` (test hook). All tested with `C_M1` (`RHO_M1 = 1.5·2^-62`); worst
  stated composition 2^-61.43 (cosh SSE2, big path), sinh mid 2^-62.585, tanh 2^-62.03, series <= 2^-62.4.
* Wiring `crates/boundary/src/hot/hyperbolic.rs` (sinh/cosh/tanh_nearest -> `quick::*_tiered`), `selftest::hyp_quick`,
  tests `crates/boundary/tests/hyperb_quick.rs` (clause s10_0_f_0; `LIELLA_HYPERB_N` per region, default 10000).
* Outside the quick domains (|x| < 2^-60, >= 708, tanh > 22, NaN/inf) tier 2 and the accurate path answer as before:
  overflow thresholds, tanh saturation and subnormal tanh results are theirs (test covers them through the entry).

## Jobs (keke)

* `-lint4` (code = final except the test hook's `decided` line): clippy, static, `hyperb_quick` rc 0. Fallback
  (10000/region): SSE2 sinh 0.50%, cosh 0.57%, tanh 0.41%; FMA 0.49%, 0.56%, 0.41%. No misrounding.
* `-counts` (same code as -lint4; entries identical to final), Zen 1 build host, instructions Liella vs glibc:
  FMA sinh 144.3 vs 145.2, cosh 141.0 vs 84.6, tanh 162.5 vs 131.9; SSE2 (`hwcaps=-FMA`, reached both sides) sinh 209.1
  vs 162.0, cosh 162.8 vs 94.7, tanh 215.3 vs 148.9. Before (brief): cosh 279 / 404. Time ratios on that busy host
  (not a verdict): FMA 0.67 / 1.85 / 1.39, SSE2 0.91 / 1.91 / 1.54.
* `-gate` (lint + `cargo xtask affected --run --base origin/next`) and `-plants2` (6 plants: 5 msg:, 1 equivalent) on
  the final head: see the hand-back. Superseded: `-lint1..3` (fallback fixes, record bar), `-plants` (REFUSED: a ` | `
  inside a lit: pattern).

## Open / next

1. Booked timing (Ivy Bridge, N100) NOT run (context budget): book and time cosh/sinh/tanh; target <= 1.4x.
2. cosh is still 1.67x glibc by instructions: profile one call (wl_percall) before changing; candidates: cosh needs
   neither the second table lookup nor Fast2Sums at full precision (`e^-a` only to 2^-60 relative times e^-2a), an
   even-polynomial region up to ~0.35, a cheaper `e^-a` as `RN(1/zh)`-free scale; sinh likewise above ~1.
3. A minimax odd polynomial (not Taylor) could drop one term in the sinh/tanh series (not tried).
4. tanh above 22 still goes to the accurate path (saturation in the quick phase not done: flags need a runtime op).

## Not to redo

The tiny-argument declines (cosh through `exp_pm` and tanh through expm1 lose the low word below ~2^-26/2^-30: hence
the series), the plants' ` | ` field rule.
