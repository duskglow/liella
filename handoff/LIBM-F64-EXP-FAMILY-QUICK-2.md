# LIBM-F64-EXP-FAMILY-QUICK-2: expm1 on the exp quick kernel (continues LIBM-F64-EXP-FAMILY-QUICK)

State: branch `task/libm-f64-exp-family-quick-2` (on LIBM-F64-EXP-FAMILY-QUICK's head be7afcb64, which had not
landed on origin/next when this run started). Role keke:~/w/n1. Decisions in force: D-10.64 (extended with expm1),
D-10.51, D-10.54, D-10.25/D-10.50, D-10.3. Previous record: `handoff/LIBM-F64-EXP-FAMILY-QUICK.md`.

## Done this run

* Step 0: static on the inherited head was RED (`tables-fresh`: `crates/math/KERNELS.md` stale; the previous run
  regenerated it on its role but never committed it). Fixed by committing the regenerated file (with this change).
* expm1 quick phase (`crates/math/src/fast64/quick.rs`, module doc section "expm1", D-10.64): both builds,
  `expm1_tiered` wired in `crates/boundary/src/hot/exp_log.rs`, tier 2 today's `fast64::expm1_nearest`.
  - `|x| >= 2^-5`: exp's reduction and kernel, `vh − 1` by 2Sum, the kernel's low part added before one
    Fast2Sum. The kernel takes a const generic `X` that keeps `th·rh`'s low part (FMA, or Dekker's product in
    SSE2 only for expm1): the `−1` magnifies the kernel error by up to 32.5.
  - `|x| < 2^-5` (the brief's "x low bits survive"): `x + x²/2 + x³·P`, Taylor 1/3!..1/9! in Estrin; the head
    uses a mask split (`xh` = x with low 27 significand bits cleared, `xh²` exact, `xl·(x + xh)/2` small), so
    every bit of x reaches `h0 + e` exactly, without a two-product.
  - Bound `RHO_M1 = 1.5·2^-62` for both builds/regions (computed 2^-62.57 / 2^-62.66 table, 2^-62.04 series),
    `C_M1` the least multiplier; expected fallback about 0.6%.
* `near()` doc (`fast64.rs`, #4010 as I read it; the issue text was not reachable from here): the proof's
  "other side `eps < 2^-15·H`" only holds for `rho <= 2^-69`; now states the precondition `rho < 2^-55` and the
  step `eps <= rho·2^54·H < H/2`, which covers the quick phases' `2^-61` bounds.
* Tests (clause s10_0_f_0, D-10.3): `quick/tests.rs` `expm1_constants_and_bounds`, `expm1_error_within_bound`
  (both regions, 8000 operands at the 2^-5 edge), `expm1_matches_correctly_rounded` (4·10^6, fallback 0.1-1.2%
  on [-1, 1], domain edges); `crates/boundary/tests/exp_quick.rs` `expm1_quick_fma_build`,
  `expm1_quick_sse2_build` (600k each: quick and entry `m::expm1` bit for bit, bound every 16th).
* Plants `handoff/LIBM-F64-EXP-FAMILY-QUICK-2-plants.txt`: small-region boundary moved, x low bits lost, SSE2
  Dekker low part dropped, near always true.

## Jobs (keke)

lint `libm-f64-exp-family-quick-2-lint2` (kernel-index, clippy, static, quick unit tests, boundary exp_quick),
plants `libm-f64-exp-family-quick-2-plants`, gate `libm-f64-exp-family-quick-2-gate` (direct tier, base
origin/next; starts after lint2). SUPERSEDED: lint0 (static red on the inherited head: stale KERNELS.md), lint1
(type error in my new test).

## Not done / next step

1. Counts after for expm1 (cbench_libm `--compare --counters`, FMA): NOT RUN (budget). Before: expm1 156.5 vs
   glibc 86.5; exp2 90.2 vs 51.1 and exp10 106.1 vs 56.1 are the previous story's after-counts.
2. 10^8-operand accuracy jobs for exp2/exp10/expm1 (copy `exp_quick_accuracy_job`): NOT DONE.
3. SSE2 counts: depends on XTASK-BENCH-TUNABLES-AND-MATHTOOLS (not checked this run).
4. Timing (Ivy Bridge nico/kotori, N100): PENDING (timing hosts drained today).
5. Measured fallback rates and largest errors: read `~/work/LIBM-F64-EXP-FAMILY-QUICK-2/{unit,bnd}.log` on keke.

## Not to redo

The bound split (python check in the run's scratch and the Rust test agree); the 2^-5 threshold (balances the two
regions); Dekker only on expm1's SSE2 path (exp/exp2/exp10 keep D-10.64's drop). Any quick.rs edit needs
`cargo xtask kernel-index` and the regenerated KERNELS.md committed.
