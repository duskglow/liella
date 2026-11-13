# LIBM-TAIL-PROBE: per-input libm cost, Liella vs glibc (handoff + report)

State: branch task/libm-tail-probe (head: see `git log -1`), measurement only, no libm change. Decisions in force:
none new. Data: handoff/LIBM-TAIL-PROBE/ (summary.tsv, bands-top300.tsv, hot-x32.tsv, binades-f32.tsv). Tool:
validation/libm_tail/ (probe.c, run.sh, suite.sh, analyze.py). Raw output on keke: ~/work/libm-tail-probe/full.

## Method (what the numbers are)

* Cost = user-mode retired instructions of one call (perf userpage: offset + rdpmc under the seqlock), minus an
  identity function through the same code (calibration min = median within 1 instruction: deterministic). One
  source, two binaries: the installed glibc 2.39 libm (gcc, dynamic, -z now; black box) and Liella's libm.a
  (scripts/liella-cc, the link cbench uses; Liella's ld.so cannot search libraries by name, so no dynamic run).
* A counter read costs ~35 us on the Zen 1 build VM (rdpmc traps: 81 s task-clock, 0.5 s user cycles for 2.1M
  reads). Per-input timing of 2^32 inputs is therefore infeasible; instead:
  - **x32, exhaustive:** every f32 input, one reading per 1024 consecutive inputs (sums exact; resolution 1024
    ulps): 24 unary f32 functions.
  - **u32, per input:** one pseudo-random input from every run of 1024 (4.2M inputs, uniform over bit patterns):
    all 31 unary f32 functions. Medians/percentiles come from here. Where both exist, x32 and u32 means agree to
    0.1% (summary.tsv).
  - **Not exhaustive: lgammaf tgammaf j0f j1f y0f y1f erfcf.** Liella costs 1e5-5e5 instructions on 20-80% of
    their inputs: the exhaustive Liella run projected to ~30 h per function per 4-core slot (lgammaf covered 2%
    in 40 min), so it was stopped; the slow regions are whole binades and u32 resolves them.
  - f32 binary (atan2f powf hypotf fmodf remainderf): 64 random pairs per (x binade, y binade) batched, 4 per pair
    per input. f64: 256 random inputs per 1/64 binade (batched) and 4 per input; f64 binary per 8x8 binades;
    hard points (n64): +-1..256 ulps and +-2^j ulps around zeros/poles/thresholds (suite.sh lists them).
* Percentiles above 255 are histogram lower bounds (16 buckets per octave, <= 6.25% low). "frac" = share of the
  input BIT PATTERNS (all of f32, or the f64 sample), not of values a program is likely to pass.

## Result 1: glibc has no tail; Liella has large ones

glibc's per-input max is <= 1025 instructions in every function and mode except f64 fmod/remainder (below).
Liella share of inputs over 2000 instructions (summary.tsv, frac_over2000), mean Liella / glibc:

| function | over 2000 | mean L / G | median L / G | p99.99 L / G | max L |
|---|---|---|---|---|---|
| lgammaf | 46% | 175008 / 119 | 254 / 99 | 458752 / 544 | 548030 |
| tgammaf | 48% | 191175 / 165 | 464 / 137 | 507904 / 608 | 558820 |
| j0f / j1f | 58% / 84% | 78791 / 83780 vs 162 / 171 | 17408 / 18432 | ~330k | 357562 |
| y0f / y1f | 42% / 42% | 69468 / 70809 vs 145 / 141 | 148 | ~330k | 513888 |
| erfcf | 22% | 4087 / 29 | 416 / 26 | 475136 / 168 | 4789608 |
| erff | 0.35% | 153 / - | 113 | 13312 | 35280 |
| sincosf | 1.0% | 813 / 55 | 320 / 54 | 2304 / 110 | 2413 |
| lgamma (f64) | 73% | 207596 / 117 | 98304 / 116 | 491520 / 736 | 541258 |
| tgamma (f64) | 49% | 203940 / 142 | 704 / 128 | 524288 / 992 | 571282 |
| j0 j1 y0 y1 (f64) | 25-50% | 1581-1816 / ~60-170 | 141-512 | 2688-147456 | 178236 |
| hypot (f64) | 98% | 3727 / 26 | 3712 / 26 | 3840 / 60 | 3878 |
| atan2 (f64) | 22% | 1367 / 99 | 384 / 96 | 4352 / 156 | 4633 |
| pow (f64) | 2.8% | 948 / 77 | 1280 / 78 | 2048 / 162 | 2106 |
| atan2f / powf | 6.4% / 0.5% | 232 / 964 vs ~55-70 | 70 / 1152 | 2560 / 2432 | 2709 / 2587 |
| erf (f64) near 0 | subnormals | 149229 (n64 at 0) | | | 321281 |

## Result 2: bands, ranked by fraction x extra cost (score = extra instructions per input over the whole space)

| # | band (Liella slow) | frac of inputs | mean L / G | score | Liella entry (s10_math_gen.rs) |
|---|---|---|---|---|---|
| 1 | tgammaf, 0 < \|x\| < 2^-20 (9.5e-7) | 41.8% | 3.9-4.7e5 / 108-202 | 1.79e5 | mret::elem -> math::special::tgamma |
| 2 | lgammaf, 0 < \|x\| < 2^-20 | 41.8% | 3.6-4.3e5 / 87-99 | 1.65e5 | mret::lgamma -> math::special::lgamma |
| 3 | tgamma/lgamma (f64), 0 < \|x\| < 2^-60 | 47% | 3.6-4.6e5 / 100-178 | 2.0e5 / 1.85e5 | same entries, f64 |
| 4 | j0f/j1f, \|x\| >= 2^19; y0f/y1f x >= 2^19 | 42.6% / 21.3% | 1.78-1.80e5 / 275-298 | 7.6e4 / 3.8e4 | mret::elem -> math::special::{j0,j1,y0,y1} |
| 5 | y0f/y1f, 0 < x < 2^-20 | 20.9% | 1.5e5 / 51-83 | 3.2e4 | same |
| 6 | lgamma (f64), x >= 16 | 24.9% | 7.9e4 / 122 | 2.0e4 | math::special::lgamma |
| 7 | tgammaf x <= -34, lgammaf x <= -42; tgammaf x >= 35 | 3.5%; 2.9% | 2.4e5; 1.1e5 / 145-270 | 8.6e3; 3.2e3 | special::tgamma/lgamma |
| 8 | hypot (f64), nearly all inputs | 98% | 3712-3840 / 26 | 3.7e3 | mret::hypot |
| 9 | j1f, 0 < \|x\| < 2^-20 | 41.8% | 16720 / 55 | 3.5e3 | special::j1 |
| 10 | erfcf, 0 < \|x\| < ~2.4e-14 / 4.9e-14 | 32% | 11.1e3 / 21 | 3.6e3 | mret::elem -> fast32::erfc |
| 11 | atan2 (f64), x subnormal or 0 with y in most binades; others | 22% (over 2000) | 4352 / 113 | ~940 est. (0.22 x 4250; spread over many 8x8-binade blocks) | elem_fast(hot::atan2_nearest) -> Real::atan2 |
| 12 | erf (f64) subnormal x; erff \|x\| <= 1.04e-38 | 0.35% (f32, exhaustive) | 13.1e3 (f32), 1.5-3e5 (f64) / ~20 | 46 (f32) | fast32::erf / special |
| 13 | sincosf 1.49e-8 <= \|x\| <= 9.48e-8 | 1.0% (exhaustive) | 2377 / ~55 | 24 | sincos_fast(hot::sincosf_nearest) -> Real::sincos |

Above 10x glibc but under 2000 (all exhaustive x32): sinf/cosf/tanf \|x\| >= 2^19: 42.6%, 1327-1474 vs 60-124;
expf/exp2f/exp10f/expm1f: 220-236 for \|x\| < 2^-26 (~45% of inputs), 905 for 8.7e-19 <= \|x\| <= 1.1e-13 (6.6%),
1344-1388 at the overflow/underflow edges (0.6%), vs 25-28 (entry elem32_onepass: inputs the fast32 one-pass
kernel refuses take elem32_onepass_cold -> Real::exp); asinf \|x\| < 1.57e-7 (40.8%) 358 vs 26; log1pf tiny
(6.6%) 611 vs 24. The typical range is near glibc: expf 46 vs 28 for 2^-26 <= \|x\| < 64.

Paths are named by the exported entry. That 1e5-5e5 instructions means a multi-precision fallback inside
math::special is an inference from the size, NOT VERIFIED (next step: one wl_trace.sh per band).

## glibc tail (informational)

f64 fmod / remainder with a large exponent gap (x ~1e297-1e308, y ~1e-301): 4% of the 8x8-binade sample over
2000 (p99 2304-2432, max 3082); Liella's mean there is 615 / 727, not slow. No other glibc row reaches 2000.

## RULING NEEDED / next step

* Rethink stories (RUT magnitude trigger, 10x+): lgammaf/tgammaf (and f64) for tiny \|x\| and large \|x\|; the
  Bessel family for large \|x\| and tiny x; erfcf/erf for tiny/subnormal x; hypot (f64, every input ~3.7k).
  Recommend one RETHINK story per family, in that order (scores above). Brief files not written in this run.
* Cheap fast-path candidates (special-case dispatch): exp-family tiny \|x\| and overflow edges, sin/cos/tan
  \|x\| >= 2^19 reduction, asinf/log1pf tiny x, sincosf [1.5e-8, 9.5e-8].
* Re-run after fixes: `bash validation/libm_tail/suite.sh <group> <tree> <outdir>` (groups x32a x32b x32c s32
  f64 analyze; resumable), under jobrun on a build host, ~25 min per group. Not to redo: the dynamic Liella run
  (its ld.so has no library search) and exhaustive runs of the 7 slow functions before they are fixed.
