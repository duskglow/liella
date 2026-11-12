# LIBM-CPLX-POW handoff: cpow/cpowf quick + middle tiers (envelope path landed on the branch)

State: branch task/libm-cplx-pow-2 (stacks on task/libm-cplx-pow), code head 0b8e2fd51; role keke:~/w/n2.
Run 2 (LIBM-CPLX-POW-2) stopped at the 180k context checkpoint after the code, its tests and the jobs below.
Decisions in force: D-10.53, D-10.74, D-10.99, D-10.25, D-10.17. No decision record for this tier yet (next step).

## What is in

* `crates/math/src/complex_fast_pow.rs` (child module `complex_fast::pow`, reaches the private helpers by
  `super::`): quick tier (`clog_parts` + `fast64::exp_kernel(reduce_pow)` + trig kernels with `Φ.lo` folded),
  a per-part strict test (`|P.lo| > eps`, then `round`: the interval holds no double and no midpoint), a cold
  middle tier near 2^-99 per declined part (`exp_log::log_dd`, `trig::atan_dd`, `wide_dd::exp_dd` with `E.lo`
  folded, `sin_or_cos_dd`), `cpowf` by the 25-bit-grid test `round32` (all modes), `cpow_any`. The module
  comment holds the error analysis (E/Φ absolute errors carried through exp; part bound).
* `complex.rs`: `cpow` tries `cpow_any` first; `cpow_route` = the old ziv ladder (tests' reference).
* `fast64::reduce_pow` made `pub(crate)`.
* Tests `complex_fast_pow_tests.rs`: tiers = route bit for bit (values + flags) in 4 modes, double and float,
  5 bands (typical, |z| within 1e-6 of 1, branch cut, |w| up to 64, real exponents); the middle tier must
  decide every sampled part; exact parts ((3+4i)^1.5, ^2, ^0.5) decline in every mode.
* Plants: handoff/LIBM-CPLX-POW-2-plants.txt (strict test dropped; arg bound loosened to 2^-60).

## Decline rates (from the bounds; NOT yet measured)

Quick: about 2^-10 per part (atan2's 2^-65 dominates). Middle: bound about 2^-97 relative for |w log z| <= 2^4,
so about 2^-44 per quick decline (2^-54 per part) to the route; it grows with |c|, |d| (absolute ℓ errors times
|w|): about 2^-37 at |w| = 2^10. The coordinator's target (<= 2^-30 per part to the 384-bit route) holds by the
bound in every band; the measured per-band table is the next step (sweep below).

## Open / not verified

* Middle-tier constants LR_REL/LR_ABS/LI_REL rest on an a-priori estimate of `log_dd` and `atan_dd` (no
  stated bound in their files) with a factor 2+ margin; the tests compare against the route only. A test that
  measures their errors against `crate::mp` is still to write.
* The ignored decline-rate sweep was dropped: `xtask static` (long-sweeps) needs a runner row for every
  ignored test. Re-add it with that row (see how `cexp_quick_tier_sweep` is registered).

## Next step (continuation)

1. Read the gate and plants results (JOBS in the hand-back); fix any red.
2. Decision record: `cargo xtask records new decision D-NEW --series D-10 --story LIBM-CPLX-POW-2` (method,
   bounds, strict test, rates; cite Joldes-Muller-Popescu 2017, Lindemann-Weierstrass caveat).
3. Decline-rate sweep with a long-sweeps row; per-band table here; mp-based bound tests for log_dd/atan_dd.
4. In-place entry: `cplx2_fast` in crates/boundary/src/mathret.rs (mirror `cplx_fast`), rows `cpow f`/`d` in
   posix/s10_math.tsv, `cargo xtask math-gen`; an in-place `cpow_nearest` needs every intermediate normal.
5. percall typical and hard rows (with and without `--tunables glibc.cpu.hwcaps=-FMA`) vs next and glibc.
