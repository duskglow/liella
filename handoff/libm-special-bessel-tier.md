# LIBM-SPECIAL-BESSEL-TIER handoff (run 1: checkpoint, INCOMPLETE)

State: branch `task/libm-special-bessel-tier` (stacked on `task/libm-special-fast-tier-2` 3856eefe), head = the
handoff commit on top of e950f5aa. Role shiki:~/w/sh1 (Haswell E5-2697 v3); sync with
`--base origin/task/libm-special-fast-tier-2` (the dispatcher's base). Accuracy contract unchanged (correct rounding,
ruling #2760 A); no differences row.

## Done (code in place, tests RED, see below)
* Generator `crates/mathtools/src/bessel_tables.rs` (called from `tables.rs` after `special_tables::emit`; module doc =
  derivation + bounds): Taylor rows at slot centres on [2^-5, 32) (16 slots a binade, 160 per function) from the
  Bessel ODE recurrence (640 bits, checked against 960), absolute per-slot bound stored as the row's last entry
  (truncation to k=60 + Cauchy tail on |z-c|<=c/2 + `special_tables::bound`'s rounding model with fmin=1);
  small-x series A,B,C,D in z=x^2 (DLMF 10.2.2, 10.8.1/2); Hankel P,Q (DLMF 10.17.3, remainder <= first omitted,
  10.17(iii)). `special_tables` now exposes `Slot`, `bound`, `choose_for(target)`.
* Generated (commit e950f5aa): one layout for the 4 tables, D 18, M 8 (!), worst slot 2^-76.9 (target 2^-74);
  HANKEL_N 15 (first omitted at 32: 2^-78.8).
* Evaluator `crates/math/src/fast_bessel.rs` (proof in module doc), `fast64::log_dd` (log_kernel + log_eps),
  wiring in `special::{j0,j1,y0,y1}` (+ `*_accurate` = old bodies), tests `crates/math/tests/fast_bessel.rs`.

## Test run (libm-special-bessel-tier-tabtest-2, rc 101): what it found
1. `j1 kernel(4.97e-299)`: ratio NaN: `eps = |y|*2^-90` UNDERFLOWS to 0 for |y| < 2^-984 (unsound bound). Fix:
   j1 tiny cut: for |x| < 2^-27 return `wide::beside(Mp::from_f64(|x|/2), false, mode')` (x/2 moved toward 0 by
   < 1/4 ulp; mirrored mode + negate for x<0), and decline below 2^-1021.
2. j1 settled only 4416 of 6000 (random, 4 modes): same region: for x < ~2^-358 `x^3` underflows, y is exactly x/2
   and the directed modes refuse. Fix 1 covers it.
3. j0 fallback 40 in 10^5 (ceiling 10): NOT diagnosed. Print the fallback operands (as fast_special's test does) and
   classify by region before any change. Suspects: Hankel at huge x (Payne-Hanek path of `trig::sin_cos`),
   or the [2^-27, 2^-5) series in directed modes.
4. Oracle checks of y0/y1 and Hankel j0/j1 did not run yet (tests stop at the first failure per test).

## Next steps (in order)
1. Fix 1 above; run `tabtest.sh` (~/work/libm-special-bessel-tier/ on shiki: `cargo xtask tables` + the test).
2. Diagnose 3; then `cargo xtask clippy`, `cargo xtask static`, the direct-tier gate, plants (file to write:
   bound row entry, a table coefficient, the HANKEL switch, the near-zero refusal `!(eps < |y|)`), counts after.
3. Cost: M=8 double-double steps is driven by Y1/Y0 near 2^-5 (values ~20, steep coefficients). Cheaper: a separate
   layout for the first binades or for the Y tables (choose per table, generic `taylor<M, D>`), or bound Y1 relative.
   Profile one call first (rule: profile before design).
4. `trig::sin_cos`'s 2^-90 absolute is argued in the module doc, not proved in trig.rs: either prove it there or
   keep the kernel-bound test over Hankel's range (it is in the test).

## Counts before (libm-special-bessel-tier-counts-before, Liella / reference instr per call)
j0 121,247.3 / 415.1; j1 121,324.6 / 420.4; y0 472,291.5 / 435.9; y1 472,301.1 / 435.6. Target 1.05x: ~440.

## Not to redo
* The ODE recurrence is stable at 640 bits (generator asserts agreement with 960 to 2^-300 on every slot).
