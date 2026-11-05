# LIBM-F32-DOUBLE-KERNELS(-2): handoff (inverse trig family: asinf, acosf, atanf, atan2f)

State: branch `task/libm-f32-double-kernels-2` (stacks on `task/libm-f32-double-kernels`), code head `c4e9bf9d`;
this note and the report are committed on top (handoff/ only). Role shiki:~/w/sh1 (Haswell-EP, 7-CPU slots,
150-minute shared-slot cap). Scope (ruling #2784): ONLY the inverse trig kernels; the other families are
separate stories (LIBM-F32-HYPERBOLIC, -ERF, -GAMMA, -BESSEL, -SINF-SHRINK) stacked on this one.

## Done
* -1: `fast32` kernels for asinf, acosf, atanf, atan2f (`crates/math/src/fast32.rs`): one `double` atan
  reduction (`atan_red`, c = i/32 from a 33-entry table, degree-7 odd minimax), proved bounds (atan 7.0u,
  asin/acos 10.4u, atan2 10.7u), margins 16/24/32 ulp, `fast32::round`, fallback to `crate::trig`.
  Fit `crates/math/fits/fast32_atan.fit` -> `cargo xtask remez` -> `coeffs/fast32_atan.rs`.
* -2 (`c4e9bf9d`): the sweep fix. -1's `libm-f32-double-kernels-sweep` ended rc 124 (the 150-min slot cap):
  one test ran asinf+acosf+atanf (3·2^34 calls) beside the atan2f sample, and the harness gave each thread
  one fixed slice of encodings (threads with NaN / refused slices idled). Now: one ignored test per function
  (`asinf_/acosf_/atanf_fast_path_agrees_exhaustively`, plus `atan2_fast_path_agrees_on_a_wide_sample`), each
  its own job; threads take 2^16-operand chunks from one atomic counter; the harness asserts every chunk was
  checked exactly once; `LIELLA_SWEEP_STRIDE=k` checks every k-th encoding (sizing; default 1 = exhaustive).

## Sweep sizes (shiki, 7 CPUs per slot; for sizing later sweeps)
| job | operands | wall; fast path taken (calls) | CPU (user) |
|---|---|---|---|
| sizing, stride 64 (`-size`) | 2^26 x 4 modes each | asinf 6.3 s, acosf 9.5 s, atanf 12.9 s | 43 / 65 / 89 s |
| `-sweep-asinf` | 2^32 x 4 | 378 s test (482 s job incl. build); taken 1,522,263,440 | 2,613 s | |
| `-sweep-acosf` | 2^32 x 4 | 602 s (602 s job); taken 8,522,825,384 | 4,163 s | |
| `-sweep-atanf` | 2^32 x 4 | 778 s (778 s job); taken 10,169,870,168 | 5,381 s | |
| `-sweep-atan2f` | 2^24 y x 64 x x 4 | 247 s (247 s job); taken 4,022,192,792 of 2^32 | 1,660 s | |
On keke (Zen 1, loaded) -1's combined job used 55,577 CPU s in 9,000 s without finishing; all four here took
13,800 CPU s. keke's atan2f sample alone took ~41 min sharing 7 CPUs with the other test, 4 min here: per CPU second
that host ran this workload about 4-5x slower (an observation for sizing, cause not investigated).

## Counts (instructions per call, Liella / glibc; cbench_libm)
| row | before (keke, base, -1 `before2`) | after (keke, -1 `after`) | after (shiki, `-2-after`) |
|---|---|---|---|
| asinf | 1550.0 / 53.5 | 195.6 / 53.5 | 195.7 / 53.5 |
| acosf | 1555.7 / 61.2 | 193.0 / 61.2 | 193.0 / 61.2 |
| atanf | 1380.1 / 66.7 | 160.8 / 66.7 | 160.8 / 66.7 |
| atan2f | 1619.6 / 147.7 | 202.5 / 147.8 | 202.5 / 147.7 |
| sinf (control) | 167.3 / 42.0 | 167.2 / 42.0 | 167.2 / 42.0 |
No row slower. The 1.05x target is not met (3-4x glibc); the next lever is the reduction/division, not sqrt.

## Decisions in force
* Ruling #2783 (sqrt): keep the software `sqrt64`. asinf - atanf = ~35 instructions, which holds sqrt64 and
  the 1 - a² setup: under 20% of asinf, not dominant. If a later story wants it: `core::arch` `_mm_sqrt_sd`
  under `cfg(target_arch = "x86_64")` (safe on the pinned 1.99.1 per the ruling), software path elsewhere.
* Ruling #2784: one story per family on this pattern.

## PATTERN (for LIBM-F32-HYPERBOLIC, -ERF, -GAMMA, -BESSEL, -SINF-SHRINK)
* Module: everything in `crates/math/src/fast32.rs`. A kernel is `fn <f>_core(x: f32) -> Option<f64>`
  returning a `double` y with a PROVED bound (doc comment: each rounding, the polynomial remainder, the
  sum), `None` outside its domain. Add a `Fast::<F>` variant, an `eval` arm `(core(x)?, <F>_DELTA, "<name>")`
  (DELTA = margin in `double` ulp, >= 1.5·bound + 1, checked by a test), then `finish` does
  `round(y, delta, mode)` (refuses near a `float` rounding boundary), stats and flags. The public f32 entry
  calls `eval` and falls back to the accurate module (`crate::trig`, `crate::hyper`, ...). Add the variant
  to `ALL`/`NAMES` and `accurate()` in the tests.
* Coefficients: a `.fit` in `crates/math/fits/` -> `cargo xtask remez` -> `crates/math/src/coeffs/`; tables
  from a high-precision generator script in `fits/`, re-checked against the accurate path in a test. Note:
  generated files escalate the direct-tier gate to FULL (`affected` treats them as generated sources).
* Tests: tables/margins test; `<f>_core_is_within_its_bound` against the double-double reference; the row in
  `fast_path_agrees_with_the_accurate_path`; and `#[ignore] <f>_fast_path_agrees_exhaustively` =
  `exhaustive(&[Fast::<F>])`, ONE job per function (`sh ~/work/LIBM-F32-DOUBLE-KERNELS-2/sweep.sh <role>
  <test> [stride]` prints wall/CPU). Size it first with stride 64 (full ~ 64x).
* Plants: bound/margin, one coefficient, reduction edge, one table entry (`cargo xtask plants`); the file used
  here is `~/work/LIBM-F32-DOUBLE-KERNELS-2/plants.txt` on shiki (also a sweep-chunk plant on the harness).

## Jobs (shiki) on c4e9bf9d
gate `libm-f32-double-kernels-2-gate` (escalated to FULL by the tool), plants `-2-plants` (5/5 CAUGHT),
counts `-2-after`, sweeps `-2-sweep-{asinf,acosf,atanf,atan2f}`: all rc 0. Gate: 32 suites ok, 10824 passed,
0 failed, 1181 ignored; conformance 117 passed; peak line `gate: peak memory 16G (page cache included; reclaimed
under the limit) oom_kill=0 rc=0`. Sizing job `-2-size` (stride 64) ran on the pre-assert harness (evidence only).

## Next step
Land. Then the family stories above on the PATTERN.
