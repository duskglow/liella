# LIBM-WIDE-DD-TIER handoff (INCOMPLETE: context checkpoint)

Branch task/libm-wide-dd-tier (cut from origin/next 2c1e25b9). Role kanon:~/w/kn1. Scratch (coordinator host):
~/w/.scratch/LIBM-WIDE-DD-TIER/ (gen.py = the constant generator, Python decimal at 80 digits).

## State
* Done: `crates/math/src/wide_dd.rs`, a double-double first tier hooked in front of the 384-bit route in
  `wide.rs` for expl, exp2l, exp10l, expm1l, logl, log2l, log10l, log1pl. Proved bounds in the module doc
  (2^-100; expm1 where it subtracts 1: 2^-92). Generic `decide::<F>` rounding test (any PREC; refuses a
  bound that cannot decide the format, so _Float128 falls through until a triple-double kernel exists).
  Results stay correctly rounded: no accuracy/differences row changes.
* Verified: unit tests on kanon (job libm-wide-dd-tier-unit2, rc 0, release): constants pinned against mp,
  rounding test at midpoints/representable values in all 4 modes, sweep of 8 kernels x 300 operands x 4 modes
  against the 384-bit route (0 mismatches). Measured worst error: exp/exp2/exp10 2^-105, logs 2^-104,
  expm1 (subtract branch) 2^-95 (hence b = 92).
* Gate libm-wide-dd-tier-gate (head a7d6a6fb): RED on clippy (test-module order, panic!, >= y+1), fixed in 0e9e83b2,
  gate NOT re-run (role busy with the plants job, sync refused). Plants libm-wide-dd-tier-plants ran on a7d6a6fb
  (wide_dd.rs differs from the final head only in test layout/lint).
* Started: libm-wide-dd-tier-gate (lint: xtask clippy + static, then
  `cargo xtask affected --base origin/next --run`), libm-wide-dd-tier-plants (handoff/LIBM-WIDE-DD-TIER-plants.txt,
  5 plants: bound, table entry, reduction threshold, directed grid check, nearest midpoint check).
* NOT DONE: counts before/after (cbench_libm *l rows), booked timing run, the long accuracy sweep
  (`cargo test --release -p liella-math --lib wide_dd -- --ignored`), the saturation row, a per-phase profile.
  The other families (sin/cos/tan with Cody-Waite + `mp::reduce_wide`; atan/asin/acos/atan2 with an atan(j/64)
  table; hyperbolics from the exp kernel; cbrt Newton; powl via a triple-double log/exp) are not started.

## Next step
1. Read the two jobs (`ssh claude@10.0.100.81 '~/bin/jobwait libm-wide-dd-tier-gate libm-wide-dd-tier-plants'`);
   fix any red.
2. Counts: cbench_libm before (origin/next) and after on the same host; book timing (`ops timing-book`) for the
   x87 rows (#2761: judged in time). Record the residual in docs/saturation/20-x87-long-double-libm.md
   (branch task/perf-libm-longdouble has that file; it is not on next).
3. Run the long sweep as the accuracy job, then the next families in the same module (same `decide`).

## Do not redo
The 192-bit middle step of task/perf-libm-longdouble was not merged (no merge allowed by the brief); this tier
falls back to the 384-bit route directly. Splitting `Mp::div_u64` measured slower (that branch's note).
