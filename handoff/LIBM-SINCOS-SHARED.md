# LIBM-SINCOS-SHARED: sincos/sincosf/sincosl share one reduction

State: branch `task/libm-sincos-shared`, code commits 3d9f4f9ba + 7c371d328 (head: the handoff commit on top), built
on kanon `~/w/kn1` (Skylake-SP). Decision record D-10.89 (reserved; `records/decisions/D-10/D-10.89.toml`).

## What changed

* `sincosf`: the export now takes an in-place fast entry, `mret::sincos_fast::<HW_ANY, f32>` with
  `hot::sincosf_nearest` -> `fast32::sincos_nearest_widen` (one `trig_core_with` call, both results rounded by the
  sinf/cosf test; either refusal goes to the cold fallback `fast32::sincos`). Before, every call went through the
  full `mret::sincos` envelope (MXCSR save/switch/restore), which is where the 3x over sinf came from: the reduction
  was already shared in `fast32::sincos`. Row `sincos f` split out of `sincos flq` in `s10_math.tsv`;
  `s10_math_gen.rs` regenerated with `cargo xtask math-gen` (2 lines: sincosf, sincosf32).
* `double`: the in-place tier already shared one reduction (`sincos_tiered`, unchanged). The envelope path
  `trig::sincos` now uses `fast::sincos` (one reduce/poly, two assembles) and one double-double `sin_cos` for the
  results the fast tier leaves (also the `float` accurate fallback).
* x87 `long double`: `wide_int::kernel` split into `reduced` (steps 1-2) + `place` (step 3, lazy polys);
  `sin_cos_pair` reduces once and evaluates both series once; `wide::sincos` uses it and one 384-bit `sin_cos_of`
  for refusals. `_Float128` unchanged (two `quad::sin_cos`).
* Bound (D-10.89): each result is computed by exactly the operations of its single function on the same values, so
  bits and flags equal sin*/cos*: correctly rounded in every mode (<= 0.5 ulp). The 1-ulp differences from glibc
  seen by BENCH-ROWS-SINCOS-TOWCTRANS are within it.

## Profile / counts (instr per call, cbench_libm rows, Skylake-SP, job libm-sincos-shared-count0 = origin/next)

| row | Liella before | glibc | sinf/sin/sinl alone |
|---|---:|---:|---:|
| sincosf | 251.4 | 57.6 | 85.2 (cosf 85.1) |
| sincos | 188.6 | 151.3 | 127.8 (cos 119.7) |
| sincosl | 1146.0 | 1179.1 | 620.0 (cosl 620.7) |

The after-counts are job `libm-sincos-shared-counts` (queued behind the gate; NOT READ by me). Expected: sincosf
near sinf + the second rounding (~100), sincosl well under sinl + cosl. sincos (double) is unchanged on the bench
(already shared); its 1.25x by instructions is the tier itself, not the sharing: a follow-up if Ivy Bridge timing
stays above 1.3x.

## Tests and plants

* `s10_3_c_r4` (math clauses): sincos vs sin/cos bits and flags, every mode, f64 and f32, fast, double-double,
  tiny and special operands.
* New `math_float_sincos_fast_entry_matches_the_envelope_in_every_mode` (boundary): the sincosf fast entry vs the
  DAZ-forced envelope (bits, flags, errno) and the directed modes vs `Real::sincos`.
* Plants `handoff/LIBM-SINCOS-SHARED-plants.txt`: f64 cos quadrant dropped (CAUGHT with msg, seen), long double cos
  quadrant dropped, sincosf cos taken from sin, and the long double second reduction (equivalent: same bits by
  construction; the counts tell).
* Accuracy: `sinf_fast_path_agrees_exhaustively` (every float operand x 4 modes, sincosf vs sinf/cosf at the math
  level) is job `libm-sincos-shared-accuracy`. NOT RUN: the 10^7 jobs for sincos/sincosl (by construction the same
  operations as sin/cos; s10_3_c_r4 and elem_ld_products check equality on fixed operands).

## Jobs (kanon) at hand-back

libm-sincos-shared-gate (affected --base origin/next), libm-sincos-shared-plants, libm-sincos-shared-counts (after
the gate), libm-sincos-shared-accuracy (after the counts): all running at hand-back. clippy rc 0 and static rc 0 run
in the foreground on the final code.

## Next step

1. Read the four jobs (`~/bin/jobwait` on kanon); counts: sincosf and sincosl must be below single + half the other.
2. Booked timing on Ivy Bridge (nico or kotori, the shorter queue) and N100: `cargo xtask bench --stage`, rows
   sinf,cosf,sincosf,sin,cos,sincos,sinl,cosl,sincosl; target <= 1.3x glibc.
3. The 10^7 random jobs for sincos and sincosl if the coordinator still wants them beyond the by-construction proof.

Plan rows to read: D-10.89, D-10.52, D-10.67, D-10.79.
