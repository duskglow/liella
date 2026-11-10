# Timing sweep 1107b: the rows changed by the 11-07 landings, Ivy Bridge and N100 (Alder Lake-N)

TIMING-SWEEP-1107B. Same method as results/timing-sweep-libm-1107-table.md (the morning baseline).

- Products: `next` at 9643c81b006a1e9369a713137bfc0909e09dcf24, staged with
  `cargo xtask bench --stage <tar> --only cbench_libm,cbench_locale,cbench_memref` (manifest tree ad6c7c87face97a6,
  profile release; tarball sha256 b63062abe37dd54d...) and `--only cbench_hot` (sha256 78b680728306e37f...). Each
  tarball was unpacked whole on both booked timing hosts.
- Job per host and group: `./xtask-bin sweep run --out <dir> --only <group>` (interleaved Liella/glibc rounds, 5 per
  row on both classes). Every run ended `sweep: COMPLETE`. Ivy Bridge cbench_memref: bench rc 1 because the limit check
  flagged `__memcmpeq_eq_128` and `__memcmpeq_tail_1k` (findings, not a run failure). Ivy Bridge cbench_libm was
  run twice (see "Ivy Bridge exp-family spread").
- Per row: Liella ns and glibc ns are the medians over the rounds; ratio is the median of the per-round
  Liella/glibc ratios; spread is (max - min) / median of those ratios (the `sweep.tsv` definitions; the script was
  checked against the morning `sweep.tsv`, e.g. `exp` 2.901 / 0.845 reproduced exactly). Verdict: faster < 0.95,
  slower > 1.05, parity in between. "morning" columns: that row of the morning sweep (11-07, next 66b8d3ba) on the
  same class; change = ratio now / morning ratio. Only libm has a morning baseline.
- Skipped (no bench row on next; not guessed): `sincos`, `towctrans`. `strncmp_*` rows (cbench_hot): see the last
  section.

## Summary

- `sinl` and `cosl` are now faster than glibc: ratio 112.7 -> 0.58 (Ivy Bridge) and 88.2 -> 0.70 (N100);
  Liella about 119 ns (Ivy Bridge) and 154 ns (N100) per call, down from 19-23 us.
- `log` improved: 5.18 -> 1.93 (Ivy Bridge), 5.88 -> 1.93 (N100; Liella 49.4 -> 17.8 ns, glibc unchanged).
- `exp` improved: 3.35 -> 2.17 on N100 (Liella 26.8 -> 17.4 ns), and 2.90 -> 1.69 on Ivy Bridge (high spread, below).
- **`expf` and `exp2f` are slower than in the morning on both classes.** N100: 2.60 -> 3.10 and 2.59 -> 3.13
  (Liella 13.30 -> 15.91 ns, glibc 5.12 -> 5.13 ns, spread <= 0.013). Ivy Bridge: 2.11 -> 2.52 and 2.31 -> 2.73
  (spread 0.36-0.40, same direction). The products differ from the morning's in more than one landing; which change
  moved these two rows was not isolated here.
- `exp2`, `exp10`, `expm1`, `log2`, `log10`, `log1p`, `pow`, `sin`, `cos`, `tan`, `cexp`, `cexpf`: on N100 (the
  low-spread class) each is within 1.5% of the morning ratio. `cexp`/`cexpf` stay 500-850x glibc (pair median 663 Ivy Bridge, 671 N100).
- Wide classification (cbench_locale, no morning baseline): median 1.02 (Ivy Bridge), 1.08 (N100).
  `iswctype_en_us` 1.99 / 1.82 and `iswalpha_l_en_us` 1.29 / 1.46 are the slow rows; `towupper_l_en_us` 1.13 / 1.10.
- strstr: the miss rows are faster (0.30-0.65), `strstr_hit_4k` 0.95 / 0.76, `strstr_hit_64` slower 1.25 / 1.18.
- Compare rows (cbench_memref): median bcmp 0.85 / 0.59, `__memcmpeq` 0.94 / 0.62, memcmp 0.99 / 0.80 (Ivy Bridge /
  N100). On Ivy Bridge the 128-byte and `head_1k` bcmp/`__memcmpeq` rows are slower (1.10-1.23) and
  `memcmp_diff8_{64,96,127}` are slower (1.25-1.42); the same rows on N100 are faster or at parity, except
  `memcmp_256` (1.06 on both).

## Ivy Bridge exp-family spread

On Ivy Bridge the exp/log rows that run first in the bench (exp, exp2, exp10, expm1, log, log2, expf, exp2f and the
`add_baseline` calibration row) had round spreads of 0.37-0.68 in both runs, and glibc's own time for them was
higher than in the morning (exp 13.55 -> 18.8 ns, `add_baseline` 1.21 -> 1.64 / 1.89 ns). The glibc times of all
169 libm rows have median new/morning 1.00, and the morning `exp` spread was already 0.845. These Ivy Bridge
verdicts are therefore labelled low confidence. N100 (spread <= 0.12 on the same rows) decides the libm changes
above. Run 2 (same products) per-row ratios: exp 1.69, exp2 6.09, exp10 5.71, expm1 2.03, log 2.41, log2 5.54,
expf 2.51, exp2f 2.75. The tables below are run 1.

## Per-row tables

### Ivy Bridge: cbench_libm

| row | family | Liella ns | glibc ns | ratio | spread | rounds | verdict | morning ratio | morning spread | change |
|---|---|---|---|---|---|---|---|---|---|---|
| exp | exp/log f64 | 31.76 | 18.76 | 1.690 | 0.390 | 5 | slower | 2.901 | 0.845 | 0.582 |
| exp2 | exp/log f64 | 48.17 | 8.92 | 5.459 | 0.245 | 5 | slower | 5.485 | 0.292 | 0.995 |
| exp10 | exp/log f64 | 71.83 | 12.57 | 5.723 | 0.342 | 5 | slower | 5.887 | 0.011 | 0.972 |
| expm1 | exp/log f64 | 57.45 | 31.06 | 2.028 | 0.225 | 5 | slower | 2.038 | 0.009 | 0.995 |
| log | exp/log f64 | 25.97 | 13.40 | 1.931 | 0.231 | 5 | slower | 5.175 | 0.009 | 0.373 |
| log2 | exp/log f64 | 64.35 | 16.16 | 4.508 | 0.139 | 5 | slower | 4.568 | 0.002 | 0.987 |
| log10 | exp/log f64 | 69.02 | 19.12 | 3.609 | 0.138 | 5 | slower | 3.620 | 0.008 | 0.997 |
| log1p | exp/log f64 | 68.59 | 18.73 | 3.659 | 0.128 | 5 | slower | 3.676 | 0.009 | 0.995 |
| pow | exp/log f64 | 136.97 | 40.56 | 3.378 | 0.140 | 5 | slower | 3.351 | 0.362 | 1.008 |
| sin | trig f64 | 49.27 | 22.11 | 2.230 | 0.021 | 5 | slower | 2.219 | 0.016 | 1.005 |
| cos | trig f64 | 48.47 | 22.71 | 2.136 | 0.013 | 5 | slower | 2.134 | 0.012 | 1.001 |
| tan | trig f64 | 79.63 | 28.16 | 2.830 | 0.018 | 5 | slower | 2.831 | 0.008 | 1.000 |
| sinl | trig long double | 119.09 | 206.18 | 0.579 | 0.008 | 5 | faster | 112.724 | 0.004 | 0.005 |
| cosl | trig long double | 118.75 | 206.72 | 0.574 | 0.011 | 5 | faster | 112.043 | 0.009 | 0.005 |
| expf | exp f32 | 24.13 | 9.59 | 2.519 | 0.402 | 5 | slower | 2.114 | 0.840 | 1.192 |
| exp2f | exp f32 | 21.72 | 8.77 | 2.728 | 0.356 | 5 | slower | 2.307 | 0.284 | 1.182 |
| cexp | complex exp | 34081.41 | 48.63 | 703.580 | 0.401 | 5 | slower | 704.954 | 0.011 | 0.998 |
| cexpf | complex exp | 34051.39 | 54.75 | 622.284 | 0.389 | 5 | slower | 621.450 | 0.010 | 1.001 |

### Ivy Bridge: cbench_locale

| row | family | Liella ns | glibc ns | ratio | spread | rounds | verdict |
|---|---|---|---|---|---|---|---|
| iswalpha_en_us | wide classification | 8.32 | 9.25 | 0.899 | 0.023 | 5 | faster |
| iswalpha_l_en_us | wide classification | 9.16 | 7.11 | 1.287 | 0.069 | 5 | slower |
| iswalpha_utf8 | wide classification | 8.44 | 9.25 | 0.913 | 0.005 | 5 | faster |
| iswctype_en_us | wide classification | 9.20 | 4.63 | 1.987 | 0.109 | 5 | slower |
| towupper_l_en_us | wide classification | 7.24 | 6.40 | 1.130 | 0.003 | 5 | slower |
| towupper_utf8 | wide classification | 7.24 | 8.58 | 0.844 | 0.002 | 5 | faster |

### Ivy Bridge: cbench_memref

| row | family | Liella ns | glibc ns | ratio | spread | rounds | verdict |
|---|---|---|---|---|---|---|---|
| strstr_hit_4k | strstr | 456.86 | 480.33 | 0.951 | 0.406 | 5 | parity |
| strstr_hit_64 | strstr | 15.08 | 12.04 | 1.252 | 0.189 | 5 | slower |
| strstr_miss_4k | strstr | 516.06 | 1302.19 | 0.395 | 0.943 | 5 | faster |
| strstr_miss_64 | strstr | 13.60 | 21.09 | 0.645 | 0.227 | 5 | faster |
| bcmp_eq_128 | bcmp | 8.84 | 7.63 | 1.159 | 0.050 | 5 | slower |
| bcmp_eq_16 | bcmp | 3.62 | 4.42 | 0.819 | 0.002 | 5 | faster |
| bcmp_eq_1k | bcmp | 37.31 | 36.90 | 1.011 | 0.011 | 5 | parity |
| bcmp_eq_32 | bcmp | 4.43 | 4.82 | 0.919 | 0.002 | 5 | faster |
| bcmp_eq_64 | bcmp | 5.63 | 6.43 | 0.876 | 0.000 | 5 | faster |
| bcmp_eq_8 | bcmp | 3.62 | 4.02 | 0.900 | 0.000 | 5 | faster |
| bcmp_head_128 | bcmp | 6.03 | 5.22 | 1.155 | 0.002 | 5 | slower |
| bcmp_head_16 | bcmp | 3.62 | 5.22 | 0.693 | 0.000 | 5 | faster |
| bcmp_head_1k | bcmp | 6.03 | 5.22 | 1.155 | 0.002 | 5 | slower |
| bcmp_head_32 | bcmp | 3.62 | 5.22 | 0.693 | 0.004 | 5 | faster |
| bcmp_head_64 | bcmp | 3.62 | 5.22 | 0.693 | 0.002 | 5 | faster |
| bcmp_head_8 | bcmp | 3.62 | 4.82 | 0.751 | 0.000 | 5 | faster |
| bcmp_tail_128 | bcmp | 8.03 | 8.06 | 0.996 | 0.048 | 5 | parity |
| bcmp_tail_16 | bcmp | 3.22 | 4.82 | 0.668 | 0.000 | 5 | faster |
| bcmp_tail_1k | bcmp | 36.94 | 37.74 | 0.979 | 0.020 | 5 | parity |
| bcmp_tail_32 | bcmp | 4.03 | 5.62 | 0.717 | 0.002 | 5 | faster |
| bcmp_tail_64 | bcmp | 5.23 | 6.83 | 0.766 | 0.000 | 5 | faster |
| bcmp_tail_8 | bcmp | 3.62 | 4.42 | 0.819 | 0.002 | 5 | faster |
| __memcmpeq_eq_128 | __memcmpeq | 8.43 | 7.63 | 1.105 | 0.000 | 5 | slower |
| __memcmpeq_eq_16 | __memcmpeq | 3.62 | 4.02 | 0.900 | 0.000 | 5 | faster |
| __memcmpeq_eq_1k | __memcmpeq | 37.31 | 36.53 | 1.022 | 0.245 | 5 | parity |
| __memcmpeq_eq_32 | __memcmpeq | 4.43 | 4.42 | 1.002 | 0.002 | 5 | parity |
| __memcmpeq_eq_64 | __memcmpeq | 5.63 | 6.03 | 0.934 | 0.000 | 5 | faster |
| __memcmpeq_eq_8 | __memcmpeq | 3.62 | 3.62 | 1.000 | 0.003 | 5 | parity |
| __memcmpeq_head_128 | __memcmpeq | 6.43 | 5.22 | 1.232 | 0.000 | 5 | slower |
| __memcmpeq_head_16 | __memcmpeq | 4.02 | 4.42 | 0.910 | 0.002 | 5 | faster |
| __memcmpeq_head_1k | __memcmpeq | 6.43 | 5.22 | 1.232 | 0.000 | 5 | slower |
| __memcmpeq_head_32 | __memcmpeq | 4.02 | 5.22 | 0.770 | 0.000 | 5 | faster |
| __memcmpeq_head_64 | __memcmpeq | 4.02 | 5.22 | 0.770 | 0.000 | 5 | faster |
| __memcmpeq_head_8 | __memcmpeq | 4.02 | 4.02 | 1.000 | 0.000 | 5 | parity |
| __memcmpeq_tail_128 | __memcmpeq | 8.03 | 8.43 | 0.953 | 0.000 | 5 | parity |
| __memcmpeq_tail_16 | __memcmpeq | 3.22 | 4.42 | 0.729 | 0.002 | 5 | faster |
| __memcmpeq_tail_1k | __memcmpeq | 36.94 | 36.90 | 1.001 | 0.000 | 5 | parity |
| __memcmpeq_tail_32 | __memcmpeq | 4.03 | 4.82 | 0.836 | 0.002 | 5 | faster |
| __memcmpeq_tail_64 | __memcmpeq | 5.23 | 6.43 | 0.813 | 0.000 | 5 | faster |
| __memcmpeq_tail_8 | __memcmpeq | 3.62 | 4.02 | 0.900 | 0.000 | 5 | faster |
| memcmp_128 | memcmp | 7.63 | 7.63 | 1.000 | 0.000 | 5 | parity |
| memcmp_144 | memcmp | 9.24 | 9.23 | 1.001 | 0.043 | 5 | parity |
| memcmp_16 | memcmp | 3.23 | 4.42 | 0.731 | 0.121 | 5 | faster |
| memcmp_16_mix | memcmp | 4.83 | 5.23 | 0.924 | 0.083 | 5 | faster |
| memcmp_256 | memcmp | 13.25 | 12.44 | 1.065 | 0.031 | 5 | slower |
| memcmp_4k | memcmp | 142.84 | 144.84 | 0.986 | 0.009 | 5 | parity |
| memcmp_64 | memcmp | 5.23 | 6.43 | 0.813 | 0.000 | 5 | faster |
| memcmp_64_mix | memcmp | 6.96 | 7.25 | 0.961 | 0.011 | 5 | parity |
| memcmp_64k | memcmp | 3956.67 | 4119.69 | 0.960 | 0.163 | 5 | parity |
| memcmp_diff8_127 | memcmp | 6.83 | 4.82 | 1.417 | 0.000 | 5 | slower |
| memcmp_diff8_64 | memcmp | 6.03 | 4.82 | 1.251 | 0.000 | 5 | slower |
| memcmp_diff8_96 | memcmp | 6.83 | 4.82 | 1.417 | 0.000 | 5 | slower |

### Ivy Bridge: cbench_hot: NOT MEASURED (no rounds.tsv)


### N100: cbench_libm

| row | family | Liella ns | glibc ns | ratio | spread | rounds | verdict | morning ratio | morning spread | change |
|---|---|---|---|---|---|---|---|---|---|---|
| exp | exp/log f64 | 17.35 | 8.00 | 2.169 | 0.040 | 5 | slower | 3.348 | 0.004 | 0.648 |
| exp2 | exp/log f64 | 23.63 | 6.73 | 3.514 | 0.021 | 5 | slower | 3.510 | 0.001 | 1.001 |
| exp10 | exp/log f64 | 37.11 | 7.08 | 5.242 | 0.004 | 5 | slower | 5.213 | 0.003 | 1.005 |
| expm1 | exp/log f64 | 40.56 | 19.30 | 2.101 | 0.031 | 5 | slower | 2.086 | 0.025 | 1.007 |
| log | exp/log f64 | 17.84 | 9.24 | 1.931 | 0.119 | 5 | slower | 5.879 | 0.003 | 0.328 |
| log2 | exp/log f64 | 50.42 | 9.05 | 5.570 | 0.004 | 5 | slower | 5.564 | 0.001 | 1.001 |
| log10 | exp/log f64 | 53.05 | 15.05 | 3.526 | 0.004 | 5 | slower | 3.517 | 0.002 | 1.003 |
| log1p | exp/log f64 | 60.72 | 15.79 | 3.845 | 0.034 | 5 | slower | 3.849 | 0.010 | 0.999 |
| pow | exp/log f64 | 99.58 | 27.85 | 3.576 | 0.006 | 5 | slower | 3.585 | 0.003 | 0.998 |
| sin | trig f64 | 35.23 | 16.59 | 2.122 | 0.017 | 5 | slower | 2.154 | 0.017 | 0.985 |
| cos | trig f64 | 32.48 | 17.45 | 1.861 | 0.010 | 5 | slower | 1.853 | 0.014 | 1.004 |
| tan | trig f64 | 48.66 | 18.43 | 2.640 | 0.010 | 5 | slower | 2.613 | 0.020 | 1.010 |
| sinl | trig long double | 153.50 | 220.92 | 0.695 | 0.008 | 5 | faster | 88.243 | 0.041 | 0.008 |
| cosl | trig long double | 154.07 | 220.62 | 0.700 | 0.010 | 5 | faster | 88.550 | 0.031 | 0.008 |
| expf | exp f32 | 15.91 | 5.13 | 3.103 | 0.005 | 5 | slower | 2.598 | 0.005 | 1.195 |
| exp2f | exp f32 | 15.92 | 5.09 | 3.130 | 0.013 | 5 | slower | 2.593 | 0.015 | 1.207 |
| cexp | complex exp | 28006.20 | 33.16 | 844.323 | 0.056 | 5 | slower | 856.801 | 0.038 | 0.985 |
| cexpf | complex exp | 28763.27 | 57.87 | 497.548 | 0.038 | 5 | slower | 498.386 | 0.029 | 0.998 |

### N100: cbench_locale

| row | family | Liella ns | glibc ns | ratio | spread | rounds | verdict |
|---|---|---|---|---|---|---|---|
| iswalpha_en_us | wide classification | 5.88 | 5.93 | 0.992 | 0.002 | 3 | parity |
| iswalpha_l_en_us | wide classification | 7.35 | 5.02 | 1.464 | 0.000 | 3 | slower |
| iswalpha_utf8 | wide classification | 6.28 | 5.92 | 1.061 | 0.000 | 3 | slower |
| iswctype_en_us | wide classification | 7.61 | 4.18 | 1.821 | 0.000 | 3 | slower |
| towupper_l_en_us | wide classification | 4.60 | 4.18 | 1.100 | 0.000 | 3 | slower |
| towupper_utf8 | wide classification | 5.44 | 5.35 | 1.017 | 0.017 | 3 | parity |

### N100: cbench_memref

| row | family | Liella ns | glibc ns | ratio | spread | rounds | verdict |
|---|---|---|---|---|---|---|---|
| strstr_hit_4k | strstr | 167.46 | 221.19 | 0.756 | 0.021 | 5 | faster |
| strstr_hit_64 | strstr | 12.41 | 10.45 | 1.181 | 0.080 | 5 | slower |
| strstr_miss_4k | strstr | 244.72 | 815.65 | 0.300 | 0.009 | 5 | faster |
| strstr_miss_64 | strstr | 10.32 | 16.55 | 0.620 | 0.089 | 5 | faster |
| bcmp_eq_128 | bcmp | 4.61 | 5.43 | 0.849 | 0.009 | 5 | faster |
| bcmp_eq_16 | bcmp | 2.51 | 5.43 | 0.462 | 0.015 | 5 | faster |
| bcmp_eq_1k | bcmp | 28.41 | 27.99 | 1.015 | 0.000 | 5 | parity |
| bcmp_eq_32 | bcmp | 2.93 | 4.59 | 0.638 | 0.003 | 5 | faster |
| bcmp_eq_64 | bcmp | 2.94 | 4.59 | 0.641 | 0.013 | 5 | faster |
| bcmp_eq_8 | bcmp | 2.51 | 4.59 | 0.547 | 0.016 | 5 | faster |
| bcmp_head_128 | bcmp | 2.93 | 4.59 | 0.638 | 0.011 | 5 | faster |
| bcmp_head_16 | bcmp | 2.10 | 5.01 | 0.419 | 0.024 | 5 | faster |
| bcmp_head_1k | bcmp | 2.93 | 4.59 | 0.638 | 0.015 | 5 | faster |
| bcmp_head_32 | bcmp | 2.10 | 4.59 | 0.458 | 0.020 | 5 | faster |
| bcmp_head_64 | bcmp | 2.52 | 4.59 | 0.547 | 0.017 | 5 | faster |
| bcmp_head_8 | bcmp | 2.10 | 5.01 | 0.419 | 0.028 | 5 | faster |
| bcmp_tail_128 | bcmp | 4.61 | 5.43 | 0.849 | 0.011 | 5 | faster |
| bcmp_tail_16 | bcmp | 2.10 | 4.59 | 0.458 | 0.006 | 5 | faster |
| bcmp_tail_1k | bcmp | 5.44 | 28.41 | 0.191 | 0.000 | 5 | faster |
| bcmp_tail_32 | bcmp | 2.73 | 4.18 | 0.653 | 0.002 | 5 | faster |
| bcmp_tail_64 | bcmp | 2.94 | 4.60 | 0.639 | 0.009 | 5 | faster |
| bcmp_tail_8 | bcmp | 2.10 | 4.59 | 0.458 | 0.018 | 5 | faster |
| __memcmpeq_eq_128 | __memcmpeq | 4.60 | 5.01 | 0.918 | 0.012 | 5 | faster |
| __memcmpeq_eq_16 | __memcmpeq | 2.51 | 4.60 | 0.546 | 0.013 | 5 | faster |
| __memcmpeq_eq_1k | __memcmpeq | 28.41 | 27.99 | 1.015 | 0.000 | 5 | parity |
| __memcmpeq_eq_32 | __memcmpeq | 2.93 | 4.18 | 0.701 | 0.020 | 5 | faster |
| __memcmpeq_eq_64 | __memcmpeq | 2.94 | 4.18 | 0.703 | 0.014 | 5 | faster |
| __memcmpeq_eq_8 | __memcmpeq | 2.52 | 4.18 | 0.603 | 0.010 | 5 | faster |
| __memcmpeq_head_128 | __memcmpeq | 2.94 | 4.59 | 0.638 | 0.014 | 5 | faster |
| __memcmpeq_head_16 | __memcmpeq | 2.51 | 4.59 | 0.547 | 0.017 | 5 | faster |
| __memcmpeq_head_1k | __memcmpeq | 2.93 | 4.59 | 0.638 | 0.011 | 5 | faster |
| __memcmpeq_head_32 | __memcmpeq | 2.51 | 4.59 | 0.547 | 0.003 | 5 | faster |
| __memcmpeq_head_64 | __memcmpeq | 2.93 | 4.59 | 0.638 | 0.020 | 5 | faster |
| __memcmpeq_head_8 | __memcmpeq | 2.51 | 4.59 | 0.547 | 0.012 | 5 | faster |
| __memcmpeq_tail_128 | __memcmpeq | 4.61 | 5.43 | 0.849 | 0.011 | 5 | faster |
| __memcmpeq_tail_16 | __memcmpeq | 2.10 | 4.59 | 0.458 | 0.033 | 5 | faster |
| __memcmpeq_tail_1k | __memcmpeq | 5.44 | 27.99 | 0.194 | 0.000 | 5 | faster |
| __memcmpeq_tail_32 | __memcmpeq | 2.73 | 4.59 | 0.595 | 0.013 | 5 | faster |
| __memcmpeq_tail_64 | __memcmpeq | 2.94 | 4.59 | 0.641 | 0.006 | 5 | faster |
| __memcmpeq_tail_8 | __memcmpeq | 2.10 | 4.59 | 0.458 | 0.014 | 5 | faster |
| memcmp_128 | memcmp | 4.19 | 5.43 | 0.772 | 0.000 | 5 | faster |
| memcmp_144 | memcmp | 5.02 | 7.10 | 0.707 | 0.004 | 5 | faster |
| memcmp_16 | memcmp | 2.38 | 4.59 | 0.519 | 0.000 | 5 | faster |
| memcmp_16_mix | memcmp | 3.77 | 6.80 | 0.554 | 0.025 | 5 | faster |
| memcmp_256 | memcmp | 7.53 | 7.10 | 1.061 | 0.000 | 5 | slower |
| memcmp_4k | memcmp | 109.23 | 107.34 | 1.018 | 0.002 | 5 | parity |
| memcmp_64 | memcmp | 3.35 | 4.59 | 0.730 | 0.000 | 5 | faster |
| memcmp_64_mix | memcmp | 5.44 | 5.45 | 0.998 | 0.000 | 5 | parity |
| memcmp_64k | memcmp | 1985.06 | 2384.59 | 0.832 | 0.006 | 5 | faster |
| memcmp_diff8_127 | memcmp | 3.35 | 4.18 | 0.801 | 0.018 | 5 | faster |
| memcmp_diff8_64 | memcmp | 3.35 | 4.18 | 0.801 | 0.000 | 5 | faster |
| memcmp_diff8_96 | memcmp | 3.35 | 4.18 | 0.801 | 0.027 | 5 | faster |

### N100: cbench_hot: NOT MEASURED (no rounds.tsv)


## Per-family medians (median of the per-row time ratios)

| family | Ivy Bridge rows | Ivy Bridge median | Ivy Bridge morning median | N100 rows | N100 median | N100 morning median |
|---|---:|---:|---:|---:|---:|---:|
| exp/log f64 | 9 | 3.609 | 3.676 | 9 | 3.526 | 3.585 |
| trig f64 | 3 | 2.230 | 2.219 | 3 | 2.122 | 2.154 |
| trig long double | 2 | 0.576 | 112.383 | 2 | 0.698 | 88.397 |
| exp f32 | 2 | 2.623 | 2.210 | 2 | 3.116 | 2.595 |
| complex exp | 2 | 662.932 | 663.202 | 2 | 670.935 | 677.594 |
| wide classification | 6 | 1.022 | - | 6 | 1.081 | - |
| strstr | 4 | 0.798 | - | 4 | 0.688 | - |
| bcmp | 18 | 0.847 | - | 18 | 0.593 | - |
| __memcmpeq | 18 | 0.943 | - | 18 | 0.621 | - |
| memcmp | 12 | 0.993 | - | 12 | 0.801 | - |

## strncmp rows (cbench_hot)

Pending: the cbench_hot jobs were still running at hand-back (17:53 UTC). A `NOT MEASURED` line above for cbench_hot means that; the next run adds them.
