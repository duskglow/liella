# LIBM-CPLX-EXP-TIER handoff: cexp/cexpf quick tier in front of the 384-bit route

State: branch task/libm-cplx-exp-tier (cut from origin/next bb7f6ae1c), HEAD in handoff/reports/LIBM-CPLX-EXP-TIER.json.
Role mei:~/w/s2. Decisions in force: D-10.53 (design), D-10.54 (this story's local details, new), D-10.13, D-10.25.
Run 1 stopped at the context checkpoint (~155k of 200k) with the code written and lint/unit green; the
jobs below were started on the final code and not yet read.

## What is done

* `crates/math/src/complex_fast.rs` (new): `double` tier. e^x from `fast64::exp_kernel` (+ `exp_eps`), sin/cos y
  from `trig::kernel::{reduce, poly, assemble}` + `trig::eps` (one reduction for both), each part a `Dd::mul`
  with bound `(Ee·B + A·Et + 2^-99·A·B)(1 + 2^-48)` (proof in the module comment), rounding test `fast64::near`
  (rho 2^-62, `C_PART = 1 + 2^-8 + 2^-14`) then `fast64::settle` (all modes), scale by exponent add only into
  biased exponent 2..2046 (near overflow/underflow: fallback). `cexp` (any mode), `cexp_nearest` (in place),
  `cexp_any<F>` (f32/f64 dispatch). Ranges: `2^-60 <= |x| <= 746`, `2^-30 <= |y| < 2^20`.
* `crates/math/src/fast32/cexp.rs` (new): `float` tier, `exp_core × trig_core` in double, margin 512 ulp,
  `fast32::round` (online test, normal float results only).
* `complex.rs`: `cexp` = quick tier then `cexp_route` (the old ziv ladder, unchanged, `#[doc(hidden)] pub`).
* Visibility only: `trig::eps`, `trig::kernel::{reduce,poly,assemble}` pub(crate); `fast64::{near, exp_eps}` pub(crate).
* Boundary: `mret::cplx_fast` (in place in the default MXCSR, cold fallback restores MXCSR then `cplx`);
  s10_math.tsv `cexp` split f / d (cplx_fast) / lq (cplx); math-gen + kernel-index regenerated (committed).
* Tests (`complex_fast_tests.rs`): route comparison in 4 modes × f32/f64 × 5 regimes (general, y near π/2+kπ,
  |y| up to 1e7, x near ±overflow/underflow) with answer-rate asserts; bound test vs 384-bit `mp` values;
  float decline-near-boundary test; `#[ignore]` 10^7-per-type sweep (8 threads).
* Plants: handoff/LIBM-CPLX-EXP-TIER-plants.txt (eps_trig dropped, sin/cos swapped, scale twice, float no test).

## Jobs (mei, chain ~/work/libm-cplx-exp-tier/chain.sh, sequential)

libm-cplx-exp-tier-check5 (rc 0: math-gen, kernel-index, xtask clippy, xtask static, unit tests 3 pass),
then libm-cplx-exp-tier-gate (direct tier, --base origin/next), -plants (dir /mnt/zram/claude/libm-cplx-exp-tier/plants-1),
-sweep (10^7 per type, release), -counts (cbench_libm --compare --counters; cexp rows grepped into the log,
full output ~/work/libm-cplx-exp-tier/counts.txt). Superseded: check1-4 (lint fixes, test regime fix).

## Not done / next step

1. Read the four jobs (`~/bin/jobwait libm-cplx-exp-tier-gate libm-cplx-exp-tier-plants libm-cplx-exp-tier-sweep
   libm-cplx-exp-tier-counts` on mei); fix a red, run only the failing suite.
2. Compare the cexp/cexpf counts with the targets (<= 300 / <= 120; glibc 239 / 162).
3. Timing (brief): book kotori or nico (Ivy Bridge, the less busy) and nozomi (N100) with `ops timing-book`.
4. Follow-ups: FMA build of the double tier (both kernels have one); `|y| < 2^-30` and `|x| < 2^-60` in the
   tier; csqrt/clog per D-10.53.

Do not redo: the bound derivation, the visibility changes, the boundary form.
