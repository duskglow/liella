# LIBM-F32-HYPERBOLIC: handoff (float hyperbolic family on the fast32 pattern)

State: branch `task/libm-f32-hyperbolic-2` (on `task/libm-f32-hyperbolic`, which stacks on
`task/libm-f32-double-kernels-2`), code head `0b446d69`; handoff commits on top. Role keke:~/w/n3
(-1 ran on shiki:~/w/sh1). Report: handoff/reports/LIBM-F32-HYPERBOLIC-2.json.

## Done (-1, 872c0c88)
* `crates/math/src/fast32.rs`: `sinh_core`, `cosh_core`, `tanh_core`, `asinh_core`, `acosh_core`, `atanh_core`,
  `Fast::{Sinh..Atanh}`, `fast_fn!` entries falling back to `crate::hyper`; `real.rs` routes `f32` to fast32.
  sinh = (E + E/(E+1))/2, tanh = E/(E+2) (E from `expm1_core`), cosh = (E + 1/E)/2 (`exp_core`); asinh, acosh,
  atanh from the shared `log1p64(w)` plus `sqrt64`. Proofs in the doc comments. Small-|x| cut 2^-26 (HYP_MIN).
* No new polynomial or table, so no Remez fit (D-10.3 vacuous): log1p64 reuses log1p's D3..D7, LOG_INV/LOG_C.

## Done (-2, 0b446d69): the surviving coefficient plant
* -1's plant "drop D8 in log1p64" survived because the term does not matter at the bound. log1p64 now uses the
  series one degree shorter (D3..D7). Recomputed (doc comment): the remainder r^8/8 adds ≤ 2.01u relative in
  case A (z < 1 + 2^-7, r = w < 2^-7), 0.02u in case B, 0.04u in case C. Case bounds A 5.3u, B 4.4u, C 8.2u:
  the overall LOG1P64_BOUND (8.2u) and every caller's bound and margin are unchanged.
* `hyperbolic_cores_are_within_their_bounds` checks log1p64 per case (A 5.3u, B 4.4u, C 8.2u) and samples
  [2^-8, 2^-7) densely (16k points) where case A's remainder peaks. The coefficient plant is now "drop D7"
  (about 290u in case A).
* Sweep oracle (ruling #2889): `crate::hyper` is double-double only with a final `elem::finish` (no mp tier), so
  the accurate path already costs ~1.5e3 instructions per call, the ruling's target; mp-fallback rate 0 by
  construction. No separate oracle was built for this family; the ERF-2 helper had not landed.

## Jobs (keke) on 0b446d69, all rc 0
* `libm-f32-hyperbolic-2-lint` (clippy --workspace --all-targets --all-features -D warnings, xtask static).
* `-gate` (direct tier, --base origin/next): 5 of 234 targets, 368 passed, 0 failed.
* `-plants` (`cargo xtask plants`, ~/work/LIBM-F32-HYPERBOLIC-2/plants.txt, /dev/shm/.../plants-1): bound,
  coefficient (drop D7), small-x switch, tanh saturation edge, sinh term: 5 CAUGHT, 0 WARN.
* `-sweep-size` (LIELLA_SWEEP_STRIDE=64, all six) then `-sweep-<f>` one job per function, every `float` in all
  four modes against `crate::hyper` (7-CPU slots, RAM copy of the role): wall / CPU s: sinhf 581/4067,
  coshf 605/4229, tanhf 2698/18879, asinhf 3008/21046, acoshf 1208/8451, atanhf 475/3324. No mismatch.
* `-after` (`cargo xtask bench --compare --counters cbench_libm`). Before: -1's `libm-f32-hyperbolic-before`
  (shiki, base 617417a6).

| row | before instr Liella / glibc | after instr Liella / glibc (ratio) | after time ratio (dynamic) |
|---|---|---|---|
| sinhf | 1520.1 / 145.4 | 174.9 / 145.4 (1.20) | 1.01 |
| coshf | 1505.0 / 78.8 | 151.3 / 78.9 (1.92) | 2.11 |
| tanhf | 1520.3 / 131.2 | 179.8 / 131.3 (1.37) | 1.07 |
| asinhf | 1482.4 / 88.9 | 212.3 / 88.9 (2.39) | 3.29 |
| acoshf | 1444.4 / 72.9 | 199.5 / 72.9 (2.74) | 4.25 |
| atanhf | 1494.3 / 115.7 | 182.5 / 115.7 (1.58) | 1.42 |

## Open / next
* The 1.05x target is not met on any row (instructions). coshf, asinhf and acoshf stay above 2x glibc in time
  (#2760's threshold). Next step, in order: (1) profile one asinhf/acoshf call (sqrt64 share) and, if sqrt
  dominates, ruling #2783's `_mm_sqrt_sd` (safe core::arch, no unsafe), with the sqrt64 bound test kept;
  (2) coshf: the 1/E division and exp_core's share; (3) re-count; only then any #2760 ulp-bound question, with
  that data.
* Not to redo: the D8 question (settled above), the exhaustive sweeps (green on 0b446d69; rerun only if a kernel
  changes), the #2889 oracle for this family (not needed: no mp tier).
* Plan rows: ruling #2760 (A), #2783, #2784, #2889; handoff/libm-f32-double-kernels.md PATTERN.
