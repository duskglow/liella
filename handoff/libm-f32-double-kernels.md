# LIBM-F32-DOUBLE-KERNELS: handoff (partial delivery, first family)

State: branch `task/libm-f32-double-kernels`, code head `bc5b4bf6` (this note is committed on top; it
touches only handoff/). Role keke:~/w/r5.

## Done (this run)
* `fast32` kernels for **asinf, acosf, atanf, atan2f** (`crates/math/src/fast32.rs`): one `double` atan
  reduction (`atan_red`: c = i/32 from a 33-entry table, u = (s - c)/(1 + s c) exact numerator by
  Sterbenz, degree-7 odd minimax polynomial), the proofs in the doc comments (atan 7.0u, asin/acos
  10.4u, atan2 10.7u; u = 2^-53), margins 16/24/32 ulp, then `fast32::round`; fallback to `crate::trig`.
* Coefficients: `crates/math/fits/fast32_atan.fit` → `cargo xtask remez` → `coeffs/fast32_atan.rs`
  (levelled error 2^-57.95). Table: `crates/math/fits/fast32_atan_table_gen.py` (100-digit decimal).
* `sqrt64`: software √ (rsqrt Newton ×3 + Heron, ≤ 2u, tested): the SSE2 `_mm_sqrt_pd` intrinsic is a
  `#[target_feature]` call that rustc 1.99 treats as unsafe in this crate (sse2 not counted as enabled
  in the safe-call check) and the crate has `forbid(unsafe_code)`. See open question 1.
* Tests (fast32::tests): `inverse_trig_tables_and_margins`, `inverse_trig_cores_are_within_their_bounds`,
  `atan2_fast_path_agrees_with_the_accurate_path` (incl. Annex F specials), the new rows in
  `fast_path_agrees_with_the_accurate_path`; ignored: `inverse_trig_fast_path_agrees_exhaustively`
  (all 2^32 operands × 4 modes) and `atan2_fast_path_agrees_on_a_wide_sample` (2^24 × 64 pairs × 4).

## Jobs (keke) on bc5b4bf6
* `libm-f32-double-kernels-before2`: counts on the base (rc 0). Before (Liella instr / glibc instr):
  asinf 1550.0/53.5, acosf 1555.7/61.2, atanf 1380.1/66.7, atan2f 1619.6/147.7, sinf 167.3/42.0.
* `libm-f32-double-kernels-gate` (direct tier, `--base origin/next`), `-plants`
  (`~/work/LIBM-F32-DOUBLE-KERNELS/plants.txt`: bound, coefficient, reduction-edge, table-entry),
  `-after` (counts), `-sweep` (the two ignored tests). gate → after → sweep run in sequence from
  `~/work/LIBM-F32-DOUBLE-KERNELS/seq.sh`; logs `~/work/libm-f32-{gate,plants,after,sweep}.log`.
  Results at hand-back time are in the hand-back; the dispatcher reads jobs.tsv.

## Not done (remaining scope of the brief)
sinhf/coshf/tanhf/asinhf/acoshf/atanhf (exp_core/log_core/log1p_core based), erff/erfcf,
lgammaf/tgammaf, j0f/j1f/y0f/y1f (no Bessel zero-table plant possible yet), the sinf/cosf/tanf shrink
(167 vs 42 instr). Each is its own story-sized step; split the brief per family.

## Open questions
1. RULING: hardware sqrt in `liella-math`. Options: (a) keep the software sqrt64 (~20 flops);
   (b) a safe `sqrt` helper exported from `liella-rt`/boundary with an `UNSAFE(arch)` site. Recommend (b)
   if the counts show sqrt64 dominates asinf/acosf, else (a).
2. The 1.05x target is not expected for asinf/acosf with a division plus a software sqrt; see the after
   counts.

## Next step
Read the `-after` and `-sweep` logs; then the hyperbolic family on the same pattern (one kernel per
family, Remez fit, bound test, exhaustive ignored test, plants on bound/coefficient/edge).
