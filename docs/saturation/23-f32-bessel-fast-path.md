## 23. float j0f/j1f/y0f/y1f fast path: counts (LIBM-F32-BESSEL-4, 2026-11-05, counted)

Code: the double-kernel fast path in `crates/math/src/fast32/bessel.rs` (power series and Remez fits below 24,
Hankel's form above, an absolute bound turned into a per-call rounding margin; operands that the margin cannot decide,
near the zeros, go to the accurate path; D-10.49). Broadwell host (Xeon E5-2699 v4), `cargo xtask bench --compare
--counters --only cbench_libm`, instructions per call, operands from the cbench_libm ranges. "before" is origin/next
(the accurate path for every operand), "after" the story's head.

| Row | before | after | glibc | instr ratio (after / glibc) |
|---|---:|---:|---:|---:|
| j0f | 121,749.6 | 196.6 | 278.4 | 0.71 |
| j1f | 121,833.7 | 200.7 | 283.0 | 0.71 |
| y0f | 472,877.0 | 216.0 | 294.9 | 0.73 |
| y1f | 472,875.7 | 217.1 | 296.9 | 0.73 |

* No other cbench_libm row moved more than 0.5% in instructions except `mxcsr_wrapper_seq` (10.2 to 10.3, +0.1 per
  call; this story does not touch that path). The erff/erfcf rows also change between the two trees because the
  story stacks on LIBM-F32-ERF-5 (174,288 to 180.1 and 174,788 to 191.5).
* The refusal rate in the swept range [2^-20, 2^19) is 3.4e-6 to 6.8e-6 of the operands (D-10.49), so the accurate
  path's cost does not show in these averages.
* Not timed: by instructions the rows are below glibc's. The verdict comes from the periodic timing sweep.
