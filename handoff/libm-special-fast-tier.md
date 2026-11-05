# LIBM-SPECIAL-FAST-TIER handoff (run 1, checkpoint at about 140k of 200k context)

State: branch `task/libm-special-fast-tier`, cut from origin/next 984a2ddc. Role mei:~/w/s2. Scratch:
~/w/.scratch/LIBM-SPECIAL-FAST-TIER/ (est.py: degree estimator; msg files).
Commits: generator `crates/mathtools/src/special_tables.rs` (WIP), regenerated `crates/math/src/coeffs/tables.rs` (WIP).
Nothing in liella-math uses the tables yet. Accuracy contract unchanged: the design keeps correct rounding
(Ziv first step + mp fallback), so no differences row changes (ruling #2760 option A).

## Profile (base, Zen 1 host, cbench_libm counters, job mei:libm-special-fast-tier-counts-before2)
Instructions per call, Liella / reference: erf 174,293 / 153.8; erff 174,292 / 135.3; erfc 174,773 / 157.8;
erfcf 174,769 / 138.7; lgamma 428,321 / 370.6; lgammaf 429,635 / 277.3; tgamma 460,393 / 609.5;
tgammaf 461,766 / 476.7; j0 121,228 / 415.0; j1 121,322 / 420.1; y0 472,279 / 435.8; y1 472,292 / 435.4;
j0f/j1f/y0f/y1f the same as double. Where it goes: 100% in the mp evaluator (report §3.6: erf profile 48% div_u64
before MP-CORE-SPEED, then series terms, `mp::exp`). The float rows run the same generic mp path: a double fast tier
must also serve `F::PREC == 24` (round the double-double result to float with a float rounding test).

## Design (erf/erfc, double; the generated tables implement it)
Slots: 16 per binade, `i = (x.to_bits() >> 48) - base`; centre bits `(x.to_bits() & !0xffff_ffff_ffff) | 1 << 47`;
`t = x - c` exact (Sterbenz). Row layout: `[P0h, P0l, .., P3h, P3l, p4, .., p14]` (`*_DD` = 4, `*_DEG` = 14).
Evaluation: f64 Horner `r = p14; r = r*t + p_k` down to k = 4; then four double-double steps
`(ph,pl) = two_prod(sh,t); (h,e) = two_sum(Pkh, ph); sl = e + (Pkl + (pl + sl*t)); sh = h`; final
`fast_two_sum(sh, sl)`. Bounds in special_tables.rs module doc (generator evaluates them per slot):
* `|x| < 2^-5`: `ERF_SMALL`: `z = two_prod(x,x)`; `R = q2 + zh(q3 + ... q6)` in f64; `S0 = q0 + q1*z + z2*R` with
  `q1*z = two_prod(q1h, zh) + (q1h*zl + q1l*zh)`; lo summed small parts first, `z2*R` added last; `fast_two_sum`;
  result `x*S0` (two_prod + x*S0l). Scale x by 2^600 (m = -600) below 2^-900 so settle's tiny path sees the subnormal.
  Bound `ERF_SMALL_EPS` 2^-73.5 (+3u^2 for the last product).
* erf, `2^-5 <= |x| < 6`: `ERF_T` (base 0x3fa0 = exponent of 2^-5 with 4 fraction bits), bound `ERF_T_EPS` 2^-71.3.
* erf, `|x| >= 6` (double only: erfc 6 = 2.15e-17 < 2^-54): `near_one`.
* erfc, `x < 1`: `1 - erf` in double-double (absolute error `ERF_T_EPS*erf <= ERF_T_EPS`; pass that absolute eps).
  `x <= -6`: `beside(2)`.
* erfc, `1 <= x < 27.3`: `e^(-x^2) * Y(x)`, `Y` from `ERFCX_T` (base 0x3ff0), bound 2^-71.3. `e^(-x^2)`: add
  to fast64 an `exp_dd(z: Dd)`: `kf = nearest_int(z.hi * SCALES[0]); r = reduce_pow(z, kf); exp_kernel(r, k)` for
  `z = -two_prod(x,x)` (pow's conditions hold: |z.hi| < 746, |z.lo| <= 2^-44). Its bound from exp_kernel's proof:
  `|y|(2^-49|tail| + 2^-73)` + reduction 2^-96.9 + Dd::mul 2^-100: eps = `|y.hi|(ERFCX_T_EPS + 2^-49|tail| + 2^-72)`.
  Then `settle(E*Y, eps, m, mode)` (tiny path covers the subnormal results down to m = -1077; else None).
* Rounding test: `fast64::settle`. Fallback: the existing mp code unchanged. Expected fallback rate about
  `2*eps/2^-52.5` = 2^-18 (erf), 2^-17 (erfc above 1): ceiling for the fallback-rate test from that.
* Wiring: in `special::erf/erfc`, `if F::PREC == 53` call the fast tier on `x.widen()` (needs `F: Small`, or decode
  through `float::decode` and build the f64), record `stats::record("erf", hit)`. erf goes through `mret::elem`
  (s10_math_gen.rs, generated): if the entry overhead dominates after the kernel, the `elem_fast` route (as exp) is
  the next step; measure first.

## Not done (exact next steps, in order)
1. `crates/math/src/fast_special.rs` (erf/erfc as above, proof in module docs) + wiring + tests: random 10^6 sweep
   per mode vs the mp path (bit-identical results and status), fallback-rate ceiling test, kernel-bound test against
   the oracle (`mathtools` erf/erfc at 200 bits: measured error <= *_EPS on 10^5 points). Then float via the same
   kernel with a float rounding test.
2. Plants (`cargo xtask plants`, file in handoff/): slot base off by one (interval boundary), one ERF_T coefficient,
   the `settle` directed-mode branch (rounding test), small/table switch point. Zero-table and reflection-sign plants
   belong to the Bessel and gamma parts.
3. Counts after (job as counts-before2: `CBENCH_ROWS=<rows> cargo xtask bench --compare --counters cbench_libm`;
   `bench --rows` is not accepted together with `--compare`).
4. lgamma/tgamma and Bessel: separate continuations (split per family, as the brief allows).

## Things not to redo
* Degree/slot choice: 16 slots a binade, D 14, M 4 is the generator's cheapest layout meeting 2^-71 (2^-66 as the
  brief suggests would fall back at about 2^-12.5: 230k x 2^-12.5 = +40 instructions a call on average).
* uniform 1/8-wide slots or 32 a binade: more slots (176) or more dd steps (5); estimator est.py in scratch.
