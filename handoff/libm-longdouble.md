# PERF-LIBM-LONGDOUBLE: handoff (NOT READY: checkpoint at budget)

Branch task/perf-libm-longdouble. Role keke:~/w/r5 (synced to the commit before the last one).

## State
* Done: a 192-bit first step for the x87 long double functions (`crates/math/src/mpn.rs`, the `fast` module of
  `wide.rs`; `wide_impl.rs` is the shared source, included twice: `slow` = 384 bit, `fast` = 192 bit with the
  rounding test, window 2^(ex(v) - 96)). `Mp::pack` funnel-shifts words. Unit tests in `wide.rs`
  (`tier_tests`: fast equals slow wherever it decides, all four modes; the rounding test on midpoint cases).
  The unit tests (wide:: mpn::) passed on keke before the pack change; not re-run after it as a separate job.
* Counts (cbench_libm, same host, back to back, before = origin/next; jobs counts-before, counts3 rc 0): table in
  docs/saturation/20-x87-long-double-libm.md. exp/log/trig/pow rows 3.5-4.3x fewer instructions; erf/gamma/
  Bessel ~15-20 % fewer (pack); no row with more instructions or more time (min of three, 3 % tolerance).
  No row reaches 1.05x (expl 113x instructions, 58x time); floor and next steps in the saturation file.
* NOT DONE / NOT VERIFIED: the gate went RED on clippy (`pub use slow::*` unreachable_pub, fixed in the last commit
  as `pub(crate) use`, not re-run). Plants job (perf-libm-longdouble-plants) started on the earlier head and may
  have copied a re-synced tree for later plants; its result is not a verified one for the final head. No separate
  accuracy job; the affected gate would run elem_ld_products, special_ld_products, mp_kernels.

## Exact next step
1. Sync, then `cargo xtask clippy`, `cargo xtask static`, then the gate (`cargo xtask affected --base origin/next --run`).
2. Rerun plants/plants.sh (copy in ~/work/perf-libm-longdouble/plants.sh; scripts are in the scratch dir
   $SCRATCH/work/): rounding-test-skipped, guard-shrunk, upper-end-not-widened, dispatch-condition, dispatch-never,
   undecided-ignored, cbrt-steps, kernel-constant.
3. Possible: a table-driven exp/log in `mpn` (removes the 12 squarings); a two-limb step for the relative-bound functions.

## Do not redo
Splitting `Mp::div_u64` into 64-bit divisions: fewer instructions, slower (5-14 % on erf/Bessel); reverted.
The proof for the narrow width: bounds of crate::mpw docs at B = 192; largest relative 2^(77-B) (logs near 1).
