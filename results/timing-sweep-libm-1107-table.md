# Timing sweep libm 1107: every libm row on Ivy Bridge and N100 (Alder Lake-N)

TIMING-SWEEP-LIBM-1107. First libm timing table since the 11-05 snapshot (docs/benchmarks.md, appendix
`cbench_libm`, commit a6fc34c2).

- Products: staged with `cargo xtask bench --stage <tar> --only cbench_libm` from `next` at
  66b8d3ba1541eca19261431257fe570d4e9fc337 (manifest tree 6c7f3ee8bcf2fa04, profile release); the same tarball
  (sha256 e2126541...) was unpacked whole on both timing hosts.
- Job per host, booked: `./xtask-bin sweep run --out ~/work/TIMING-SWEEP-LIBM-1107/out --only cbench_libm`. Both
  ended rc 0 with `sweep: COMPLETE`, 166 of 166 selected `ratio_host` rows measured, 0 rows flagged (libm has no
  per-host limits on either class: "not gated"). Ivy Bridge (Xeon E3-1265L v2): 5 rounds per row, bench 63 s.
  N100: 3 rounds per row.
- Verdicts on the median Liella/glibc time ratio: faster < 0.95, slower > 1.05, parity in between.
- Counts below use the 166 `ratio_host` rows of `bench/budgets.tsv`. The libm bench also counts `add_baseline`,
  `mxcsr_pair` and `mxcsr_wrapper_seq` (calibration rows). They are in `sweep.tsv` and in the `sweep compare` output
  ("judged 169", "rows not in 1105: 3"), but not in these counts. The `sweep: COMPLETE` line of this run counted them
  as well. That is fixed on this branch: the line now counts the selected rows only.
- Baseline: the 11-05 Ivy Bridge figures are the appendix's instruction ratio and its "now time vs glibc
  (E3-1265L v2)" column (the same CPU model as this run), converted to a `## Table` for
  `sweep compare --old-table`. There is no earlier libm timing on N100, so N100 shows absolute ratios only.

## Summary

| class | rows | faster | parity | slower | median time ratio | 11-05 faster / parity / slower |
|---|---:|---:|---:|---:|---:|---|
| Ivy Bridge | 166 | 5 | 1 | 160 | 6.98 | 2 / 0 / 164 |
| N100 | 166 | 2 | 4 | 160 | 5.83 | no baseline |

The `sweep compare` counts for Ivy Bridge (5 / 4 / 160 over 169) include the 3 calibration rows, and so does its
spread p90 (0.824 against 0.366 over the 166 libm rows).

Ivy Bridge against 11-05: no regressions (list a). Four rows improved from slower to parity or faster: atanhf
17.91 -> 0.98, sinhf 11.83 -> 0.66, tanhf 13.61 -> 0.75, tgammaf 1269.6 -> 0.58. The median change in the time ratio
is 0.81 and the median change in the instruction ratio is 0.80. One row is more than 10% slower than 11-05 and still
slower: `exp` 1.86 -> 2.90 (see list (b) and the table).

The long double rows dominate the slow tail on both classes (median 114.7 on Ivy Bridge, 83.6 on N100), followed
by complex (median 274 / 239). f32 is closest to glibc (median 2.27 / 2.66).

# `sweep compare` output (Ivy Bridge against the 11-05 baseline)

## Sweep comparison: ivb against 1105

Verdicts on the median Liella/glibc time ratio: faster < 0.95, slower > 1.05, parity between; rows named wordexp are timed, not judged.

## Counts

| class | faster | parity | slower | judged | 1105 faster | 1105 parity | 1105 slower | 1105 judged (rows measured now) |
|---|---|---|---|---|---|---|---|---|
| ivb | 5 | 4 | 160 | 169 | 2 | 0 | 164 | 166 |

## Run-to-run spread and change

spread = (max - min) / median of the round ratios; change = time now / time in 1105.

| class | spread n | spread median | spread p90 | spread max | change n | change median | change p10 | change p90 |
|---|---|---|---|---|---|---|---|---|
| ivb | 169 | 0.009 | 0.824 | 0.944 | 166 | 0.808 | 0.007 | 1.002 |

## (a) Regressions: slower now, parity or faster in 1105: 0

## (b) Reopen: instr <= 1.05, time > 1.05: 9

- cbench_libm/coshf | ivb | instr 0.921 (1105 18.200) | time 1.210 (1105 30.920) | spread 0.009
- cbench_libm/j0f | ivb | instr 0.658 (1105 478.640) | time 1.100 (1105 777.410) | spread 0.019
- cbench_libm/j1f | ivb | instr 0.662 (1105 471.760) | time 1.101 (1105 769.960) | spread 0.046
- cbench_libm/log10f | ivb | instr 0.839 (1105 0.840) | time 1.213 (1105 1.220) | spread 0.064
- cbench_libm/tanf | ivb | instr 0.925 (1105 1.630) | time 1.313 (1105 1.590) | spread 0.010
- cbench_libm/y0 | ivb | instr 1.023 (1105 1221.590) | time 1.232 (1105 1555.010) | spread 0.362
- cbench_libm/y0f | ivb | instr 0.683 (1105 1883.970) | time 1.173 (1105 2272.310) | spread 0.023
- cbench_libm/y1 | ivb | instr 1.024 (1105 1223.150) | time 1.241 (1105 1580.730) | spread 0.363
- cbench_libm/y1f | ivb | instr 0.681 (1105 1871.480) | time 1.190 (1105 2298.680) | spread 0.360

## (c) Improved: slower in 1105, parity or faster now: 4

- cbench_libm/atanhf | ivb | instr 0.868 (1105 12.920) | time 0.984 (1105 17.910) | spread 0.012
- cbench_libm/sinhf | ivb | instr 0.665 (1105 10.450) | time 0.658 (1105 11.830) | spread 0.008
- cbench_libm/tanhf | ivb | instr 0.761 (1105 11.590) | time 0.753 (1105 13.610) | spread 0.005
- cbench_libm/tgammaf | ivb | instr 0.451 (1105 1242.050) | time 0.577 (1105 1269.630) | spread 0.019

## Rows not in 1105: 3

- cbench_libm/add_baseline | ivb | instr 0.999 (1105 1.000) | time 1.000 (1105 -) | spread 0.922
- cbench_libm/mxcsr_pair | ivb | instr 1.001 (1105 1.000) | time 1.000 (1105 -) | spread 0.926
- cbench_libm/mxcsr_wrapper_seq | ivb | instr 1.000 (1105 1.000) | time 1.000 (1105 -) | spread 0.923

## Rows in 1105 not measured now: 0

## Deferred rows (timed, not judged): 0


# Per-host analysis (166 ratio_host rows)

Baseline rows read from the 11-05 appendix: 169

## Ivy Bridge: 166 rows: faster 5 / parity 1 / slower 160
time ratio median 6.982; spread median 0.009 p90 0.366 max 0.944; flagged 0
rows without counters: 0 

### Slowest 20 (time ratio Liella/glibc)

| row | time | instr | cycles | spread |
|---|---:|---:|---:|---:|
| asinl | 817.355 | 1683.064 | 805.461 | 0.013 |
| ynf | 763.086 | 641.378 | 758.806 | 0.006 |
| cexp | 704.954 | 411.266 | 706.489 | 0.011 |
| cexpf | 621.450 | 642.213 | 618.063 | 0.010 |
| cpowf | 555.264 | 740.644 | 549.488 | 0.005 |
| yn | 554.980 | 438.185 | 552.272 | 0.035 |
| cpow | 498.311 | 457.493 | 495.567 | 0.007 |
| atanl | 457.419 | 7377.101 | 458.820 | 0.007 |
| lgammal | 450.873 | 979.649 | 450.177 | 0.003 |
| atan2l | 449.396 | 3697.263 | 447.584 | 0.033 |
| asinhl | 410.470 | 1177.956 | 409.556 | 0.002 |
| acosl | 395.671 | 3934.333 | 395.145 | 0.012 |
| csin | 366.469 | 285.789 | 364.318 | 0.022 |
| acoshl | 354.335 | 960.807 | 354.111 | 0.003 |
| csinf | 322.069 | 380.868 | 321.908 | 0.015 |
| atanhl | 312.135 | 749.566 | 311.029 | 0.003 |
| clogl | 307.599 | 780.743 | 306.877 | 0.007 |
| erfcl | 298.669 | 910.287 | 296.866 | 0.006 |
| erfl | 298.633 | 921.429 | 301.796 | 0.046 |
| tgammal | 250.644 | 612.485 | 249.167 | 0.002 |

### Faster or parity rows

tgammaf 0.577, sinhf 0.658, tanhf 0.753, exp_qnan 0.854, hypotf128 0.888, atanhf 0.984

### Median time ratio per family and precision

| family | rows | median | f32 | f64 | long double | f128 |
|---|---:|---:|---:|---:|---:|---:|
| exp/log/pow | 36 | 5.03 | 2.31 (9) | 3.68 (9) | 10.97 (9) | 8.42 (9) |
| trig | 28 | 13.32 | 2.28 (7) | 2.59 (7) | 395.67 (7) | 19.94 (7) |
| hyperbolic | 24 | 8.35 | 1.10 (6) | 9.01 (6) | 227.87 (6) | 8.35 (6) |
| gamma | 8 | 9.62 | 4.88 (2) | 1.64 (2) | 350.76 (2) | 14.34 (2) |
| bessel | 24 | 14.38 | 1.18 (6) | 1.30 (6) | 95.40 (6) | 8.16 (6) |
| erf | 8 | 8.25 | 1.73 (2) | 2.37 (2) | 298.65 (2) | 13.88 (2) |
| roots/hypot | 12 | 2.21 | 1.72 (3) | 2.33 (3) | 18.77 (3) | 1.73 (3) |
| complex | 14 | 274.39 | 322.07 (5) | 366.47 (5) | 135.64 (4) | - |
| special | 12 | 5.12 | 14.50 (3) | 4.58 (9) | - | - |
| all | 166 | 6.98 | 2.27 (43) | 3.62 (49) | 114.70 (39) | 10.37 (35) |

## N100: 166 rows: faster 2 / parity 4 / slower 160
time ratio median 5.831; spread median 0.009 p90 0.026 max 0.128; flagged 0
rows without counters: 0 

### Slowest 20 (time ratio Liella/glibc)

| row | time | instr | cycles | spread |
|---|---:|---:|---:|---:|
| ynf | 912.498 | 681.554 | 905.854 | 0.006 |
| cexp | 856.801 | 475.210 | 861.667 | 0.038 |
| yn | 628.331 | 484.585 | 621.108 | 0.007 |
| cpow | 561.984 | 518.047 | 561.393 | 0.010 |
| cexpf | 498.386 | 706.270 | 486.340 | 0.029 |
| cpowf | 494.705 | 776.654 | 494.804 | 0.008 |
| asinl | 475.766 | 1681.288 | 474.340 | 0.019 |
| csin | 406.067 | 325.762 | 396.684 | 0.022 |
| csinf | 304.444 | 399.890 | 308.165 | 0.028 |
| atan2l | 291.352 | 3694.189 | 293.232 | 0.009 |
| jnf | 289.772 | 175.089 | 287.777 | 0.018 |
| atanl | 288.780 | 7345.646 | 286.441 | 0.006 |
| lgammal | 271.032 | 978.001 | 269.150 | 0.006 |
| acosl | 265.302 | 3932.505 | 262.970 | 0.026 |
| clog | 255.969 | 178.024 | 255.939 | 0.012 |
| clogl | 222.574 | 781.930 | 220.481 | 0.020 |
| clogf | 213.697 | 269.810 | 213.612 | 0.024 |
| asinhl | 209.899 | 1173.116 | 208.142 | 0.006 |
| jn | 198.239 | 124.624 | 195.717 | 0.018 |
| acoshl | 197.191 | 959.310 | 196.149 | 0.007 |

### Faster or parity rows

tgammaf 0.596, sinhf 0.850, tanhf 0.953, j0f 0.978, j1f 0.992, expm1f 1.039

### Median time ratio per family and precision

| family | rows | median | f32 | f64 | long double | f128 |
|---|---:|---:|---:|---:|---:|---:|
| exp/log/pow | 36 | 3.72 | 2.60 (9) | 3.58 (9) | 5.98 (9) | 5.83 (9) |
| trig | 28 | 10.12 | 3.12 (7) | 2.61 (7) | 265.30 (7) | 13.64 (7) |
| hyperbolic | 24 | 6.17 | 1.49 (6) | 11.97 (6) | 117.95 (6) | 6.17 (6) |
| gamma | 8 | 8.00 | 4.84 (2) | 2.05 (2) | 197.16 (2) | 9.98 (2) |
| bessel | 24 | 10.17 | 1.06 (6) | 1.67 (6) | 71.04 (6) | 5.72 (6) |
| erf | 8 | 6.11 | 1.90 (2) | 2.21 (2) | 124.43 (2) | 9.88 (2) |
| roots/hypot | 12 | 2.19 | 1.37 (3) | 2.16 (3) | 26.14 (3) | 3.96 (3) |
| complex | 14 | 239.27 | 304.44 (5) | 406.07 (5) | 99.96 (4) | - |
| special | 12 | 5.60 | 17.88 (3) | 5.22 (9) | - | - |
| all | 166 | 5.83 | 2.66 (43) | 3.52 (49) | 83.58 (39) | 7.45 (35) |

## Ivy Bridge now vs 11-05 (E3-1265L v2 column): 166 rows in both
11-05 verdicts over these rows: faster 2 / parity 0 / slower 164
change now/11-05 time ratio: median 0.808 p10 0.007 p90 1.001
change now/11-05 instr ratio: median 0.795 p10 0.006 p90 0.999 (166 rows)
(a) regressions (slower now, parity/faster 11-05): 0 
(c) improved (slower 11-05, parity/faster now): 4 atanhf 17.910->0.984, sinhf 11.830->0.658, tanhf 13.610->0.753, tgammaf 1269.630->0.577
rows >10% slower than 11-05: 1 exp 1.860->2.901
rows new since 11-05: []; rows gone: ['add_baseline', 'mxcsr_pair', 'mxcsr_wrapper_seq']

## Table

| bench/row | instr IVB | time IVB | spread IVB | verdict IVB | 1105 time IVB | now/1105 IVB | instr N100 | time N100 | spread N100 | verdict N100 |
|---|---:|---:|---:|---|---:|---:|---:|---:|---:|---|
| cbench_libm/acos | 3.422 | 8.678 | 0.018 | slower | 40.560 | 0.214 | 2.281 | 7.039 | 0.015 | slower |
| cbench_libm/acosf | 3.158 | 6.619 | 0.005 | slower | 50.680 | 0.131 | 3.153 | 4.741 | 0.026 | slower |
| cbench_libm/acosf128 | 18.450 | 18.496 | 0.003 | slower | 23.260 | 0.795 | 18.442 | 12.996 | 0.011 | slower |
| cbench_libm/acosh | 10.216 | 14.689 | 0.006 | slower | 16.790 | 0.875 | 12.072 | 26.192 | 0.011 | slower |
| cbench_libm/acoshf | 1.182 | 2.354 | 0.005 | slower | 26.320 | 0.089 | 1.294 | 3.802 | 0.007 | slower |
| cbench_libm/acoshf128 | 7.838 | 10.366 | 0.004 | slower | 12.800 | 0.810 | 7.832 | 7.449 | 0.008 | slower |
| cbench_libm/acoshl | 960.807 | 354.335 | 0.003 | slower | 440.730 | 0.804 | 959.310 | 197.191 | 0.007 | slower |
| cbench_libm/acosl | 3934.333 | 395.671 | 0.012 | slower | 497.860 | 0.795 | 3932.505 | 265.302 | 0.026 | slower |
| cbench_libm/asin | 3.922 | 9.792 | 0.011 | slower | 39.900 | 0.245 | 2.494 | 8.393 | 0.012 | slower |
| cbench_libm/asinf | 3.659 | 8.599 | 0.012 | slower | 62.270 | 0.138 | 3.654 | 5.828 | 0.037 | slower |
| cbench_libm/asinf128 | 18.839 | 18.594 | 0.009 | slower | 23.250 | 0.800 | 18.833 | 12.970 | 0.003 | slower |
| cbench_libm/asinh | 9.254 | 13.837 | 0.021 | slower | 15.860 | 0.872 | 10.728 | 22.533 | 0.012 | slower |
| cbench_libm/asinhf | 1.124 | 2.273 | 0.009 | slower | 22.330 | 0.102 | 1.193 | 3.066 | 0.006 | slower |
| cbench_libm/asinhf128 | 7.481 | 9.520 | 0.004 | slower | 11.740 | 0.811 | 7.477 | 6.936 | 0.005 | slower |
| cbench_libm/asinhl | 1177.956 | 410.470 | 0.002 | slower | 506.970 | 0.810 | 1173.116 | 209.899 | 0.006 | slower |
| cbench_libm/asinl | 1683.064 | 817.355 | 0.013 | slower | 1030.820 | 0.793 | 1681.288 | 475.766 | 0.019 | slower |
| cbench_libm/atan | 1.979 | 2.591 | 0.006 | slower | 19.420 | 0.133 | 1.854 | 3.203 | 0.006 | slower |
| cbench_libm/atan2 | 1.256 | 1.797 | 0.026 | slower | 12.260 | 0.147 | 1.085 | 1.648 | 0.011 | slower |
| cbench_libm/atan2f | 1.371 | 1.688 | 0.019 | slower | 17.990 | 0.094 | 1.371 | 1.348 | 0.048 | slower |
| cbench_libm/atan2f128 | 26.111 | 24.350 | 0.027 | slower | 30.380 | 0.802 | 26.101 | 17.883 | 0.006 | slower |
| cbench_libm/atan2l | 3697.263 | 449.396 | 0.033 | slower | 563.670 | 0.797 | 3694.189 | 291.352 | 0.009 | slower |
| cbench_libm/atanf | 2.413 | 2.971 | 0.003 | slower | 33.360 | 0.089 | 2.413 | 2.660 | 0.021 | slower |
| cbench_libm/atanf128 | 27.204 | 27.137 | 0.005 | slower | 34.260 | 0.792 | 27.196 | 19.117 | 0.010 | slower |
| cbench_libm/atanh | 8.172 | 15.454 | 0.022 | slower | 17.630 | 0.877 | 9.479 | 19.173 | 0.010 | slower |
| cbench_libm/atanhf | 0.868 | 0.984 | 0.012 | parity | 17.910 | 0.055 | 0.868 | 1.163 | 0.015 | slower |
| cbench_libm/atanhf128 | 6.912 | 10.845 | 0.005 | slower | 13.400 | 0.809 | 6.909 | 7.544 | 0.001 | slower |
| cbench_libm/atanhl | 749.566 | 312.135 | 0.003 | slower | 385.470 | 0.810 | 748.607 | 188.027 | 0.007 | slower |
| cbench_libm/atanl | 7377.101 | 457.419 | 0.007 | slower | 574.500 | 0.796 | 7345.646 | 288.780 | 0.006 | slower |
| cbench_libm/cbrt | 1.272 | 2.333 | 0.016 | slower | 10.180 | 0.229 | 0.954 | 2.537 | 0.002 | slower |
| cbench_libm/cbrtf | 1.178 | 1.243 | 0.005 | slower | 1.240 | 1.002 | 1.178 | 1.373 | 0.017 | slower |
| cbench_libm/cbrtf128 | 6.660 | 5.167 | 0.009 | slower | 6.590 | 0.784 | 6.658 | 3.965 | 0.012 | slower |
| cbench_libm/cbrtl | 231.019 | 71.538 | 0.008 | slower | 90.990 | 0.786 | 230.785 | 44.387 | 0.018 | slower |
| cbench_libm/cexp | 411.266 | 704.954 | 0.011 | slower | 866.910 | 0.813 | 475.210 | 856.801 | 0.038 | slower |
| cbench_libm/cexpf | 642.213 | 621.450 | 0.010 | slower | 766.200 | 0.811 | 706.270 | 498.386 | 0.029 | slower |
| cbench_libm/cexpl | 101.012 | 106.120 | 0.568 | slower | 130.780 | 0.811 | 100.896 | 61.042 | 0.026 | slower |
| cbench_libm/clog | 158.723 | 214.837 | 0.007 | slower | 264.610 | 0.812 | 178.024 | 255.969 | 0.012 | slower |
| cbench_libm/clogf | 264.957 | 241.181 | 0.010 | slower | 301.730 | 0.799 | 269.810 | 213.697 | 0.024 | slower |
| cbench_libm/clogl | 780.743 | 307.599 | 0.007 | slower | 383.010 | 0.803 | 781.930 | 222.574 | 0.020 | slower |
| cbench_libm/cos | 1.897 | 2.134 | 0.012 | slower | 3.920 | 0.544 | 1.492 | 1.853 | 0.014 | slower |
| cbench_libm/cosf | 1.769 | 2.280 | 0.010 | slower | 3.670 | 0.621 | 2.142 | 3.122 | 0.047 | slower |
| cbench_libm/cosf128 | 11.367 | 20.102 | 0.003 | slower | 25.440 | 0.790 | 11.359 | 13.640 | 0.038 | slower |
| cbench_libm/cosh | 3.704 | 4.190 | 0.009 | slower | 25.570 | 0.164 | 3.084 | 4.770 | 0.003 | slower |
| cbench_libm/coshf | 0.921 | 1.210 | 0.009 | slower | 30.920 | 0.039 | 0.966 | 1.819 | 0.002 | slower |
| cbench_libm/coshf128 | 5.860 | 7.182 | 0.007 | slower | 8.530 | 0.842 | 5.856 | 5.404 | 0.022 | slower |
| cbench_libm/coshl | 378.146 | 117.928 | 0.004 | slower | 139.830 | 0.843 | 375.809 | 47.871 | 0.012 | slower |
| cbench_libm/cosl | 73.158 | 112.043 | 0.009 | slower | 142.120 | 0.788 | 73.122 | 88.550 | 0.031 | slower |
| cbench_libm/cpow | 457.493 | 498.311 | 0.007 | slower | 617.720 | 0.807 | 518.047 | 561.984 | 0.010 | slower |
| cbench_libm/cpowf | 740.644 | 555.264 | 0.005 | slower | 685.130 | 0.810 | 776.654 | 494.705 | 0.008 | slower |
| cbench_libm/cpowl | 226.544 | 165.151 | 0.007 | slower | 204.310 | 0.808 | 226.520 | 103.156 | 0.014 | slower |
| cbench_libm/csin | 285.789 | 366.469 | 0.022 | slower | 452.810 | 0.809 | 325.762 | 406.067 | 0.022 | slower |
| cbench_libm/csinf | 380.868 | 322.069 | 0.015 | slower | 400.410 | 0.804 | 399.890 | 304.444 | 0.028 | slower |
| cbench_libm/csqrt | 281.492 | 162.508 | 0.016 | slower | 204.490 | 0.795 | 281.652 | 196.265 | 0.010 | slower |
| cbench_libm/csqrtf | 329.293 | 115.977 | 0.032 | slower | 146.760 | 0.790 | 329.190 | 98.245 | 0.026 | slower |
| cbench_libm/csqrtl | 216.053 | 82.307 | 0.013 | slower | 104.250 | 0.790 | 216.309 | 96.769 | 0.023 | slower |
| cbench_libm/erf | 1.388 | 2.063 | 0.008 | slower | 1807.950 | 0.001 | 1.036 | 2.040 | 0.004 | slower |
| cbench_libm/erfc | 1.829 | 2.667 | 0.007 | slower | 1772.200 | 0.002 | 1.273 | 2.388 | 0.013 | slower |
| cbench_libm/erfcf | 1.327 | 1.802 | 0.002 | slower | 1935.400 | 0.001 | 1.382 | 2.010 | 0.013 | slower |
| cbench_libm/erfcf128 | 9.550 | 13.922 | 0.007 | slower | 17.170 | 0.811 | 9.543 | 9.940 | 0.005 | slower |
| cbench_libm/erfcl | 910.287 | 298.669 | 0.006 | slower | 366.840 | 0.814 | 903.886 | 124.607 | 0.004 | slower |
| cbench_libm/erff | 1.277 | 1.658 | 0.002 | slower | 1950.650 | 0.001 | 1.331 | 1.794 | 0.003 | slower |
| cbench_libm/erff128 | 9.464 | 13.829 | 0.011 | slower | 17.000 | 0.813 | 9.459 | 9.823 | 0.005 | slower |
| cbench_libm/erfl | 921.429 | 298.633 | 0.046 | slower | 366.810 | 0.814 | 915.420 | 124.250 | 0.003 | slower |
| cbench_libm/exp | 2.353 | 2.901 | 0.845 | slower | 1.860 | 1.560 | 1.881 | 3.348 | 0.004 | slower |
| cbench_libm/exp10 | 3.904 | 5.887 | 0.011 | slower | 12.680 | 0.464 | 2.521 | 5.213 | 0.003 | slower |
| cbench_libm/exp10f | 1.606 | 2.600 | 0.006 | slower | 2.590 | 1.004 | 1.606 | 3.055 | 0.002 | slower |
| cbench_libm/exp10f128 | 2.595 | 3.521 | 0.022 | slower | 14.280 | 0.247 | 2.594 | 2.625 | 0.013 | slower |
| cbench_libm/exp10l | 14.411 | 4.968 | 0.002 | slower | 558.400 | 0.009 | 14.293 | 2.019 | 0.004 | slower |
| cbench_libm/exp2 | 3.825 | 5.485 | 0.292 | slower | 5.380 | 1.020 | 2.451 | 3.510 | 0.001 | slower |
| cbench_libm/exp2f | 1.667 | 2.307 | 0.284 | slower | 2.300 | 1.003 | 1.892 | 2.593 | 0.015 | slower |
| cbench_libm/exp2f128 | 4.682 | 6.336 | 0.296 | slower | 7.320 | 0.866 | 4.679 | 4.826 | 0.017 | slower |
| cbench_libm/exp2l | 18.788 | 6.834 | 0.281 | slower | 217.440 | 0.031 | 18.601 | 2.740 | 0.004 | slower |
| cbench_libm/exp_overflow_errno | 2.439 | 2.101 | 0.929 | slower | 2.190 | 0.959 | 2.523 | 3.130 | 0.003 | slower |
| cbench_libm/exp_qnan | 1.059 | 0.854 | 0.928 | faster | 0.850 | 1.005 | 2.340 | 3.139 | 0.018 | slower |
| cbench_libm/exp_small | 2.352 | 2.911 | 0.925 | slower | 2.920 | 0.997 | 1.881 | 3.189 | 0.001 | slower |
| cbench_libm/exp_tiny | 5.249 | 3.689 | 0.923 | slower | 7.870 | 0.469 | 4.857 | 3.537 | 0.002 | slower |
| cbench_libm/exp_upward | 4.776 | 11.221 | 0.928 | slower | 12.390 | 0.906 | 5.483 | 15.880 | 0.004 | slower |
| cbench_libm/expf | 1.629 | 2.114 | 0.840 | slower | 2.100 | 1.007 | 1.795 | 2.598 | 0.005 | slower |
| cbench_libm/expf128 | 5.023 | 6.784 | 0.288 | slower | 7.870 | 0.862 | 5.018 | 5.079 | 0.023 | slower |
| cbench_libm/expf_qnan | 4.619 | 3.565 | 0.924 | slower | 3.420 | 1.042 | 4.821 | 4.261 | 0.020 | slower |
| cbench_libm/expf_upward | 4.758 | 18.337 | 0.824 | slower | 17.240 | 1.064 | 5.231 | 17.883 | 0.002 | slower |
| cbench_libm/expl | 12.484 | 4.521 | 0.841 | slower | 155.910 | 0.029 | 12.378 | 1.903 | 0.006 | slower |
| cbench_libm/expm1 | 2.085 | 2.038 | 0.009 | slower | 15.510 | 0.131 | 1.809 | 2.086 | 0.025 | slower |
| cbench_libm/expm1f | 1.764 | 1.604 | 0.008 | slower | 1.600 | 1.002 | 1.763 | 1.039 | 0.009 | parity |
| cbench_libm/expm1f128 | 3.676 | 5.928 | 0.043 | slower | 6.850 | 0.865 | 3.673 | 4.248 | 0.010 | slower |
| cbench_libm/expm1l | 16.976 | 5.089 | 0.004 | slower | 159.590 | 0.032 | 16.750 | 1.689 | 0.005 | slower |
| cbench_libm/hypot | 2.402 | 2.871 | 0.037 | slower | 40.780 | 0.070 | 1.789 | 2.160 | 0.063 | slower |
| cbench_libm/hypotf | 1.372 | 1.724 | 0.363 | slower | 1.730 | 0.997 | 1.371 | 1.418 | 0.000 | slower |
| cbench_libm/hypotf128 | 1.425 | 0.888 | 0.004 | faster | 0.890 | 0.998 | 1.426 | 1.564 | 0.007 | slower |
| cbench_libm/hypotl | 42.333 | 18.770 | 0.012 | slower | 18.910 | 0.993 | 42.348 | 26.143 | 0.003 | slower |
| cbench_libm/j0 | 1.073 | 1.274 | 0.008 | slower | 528.590 | 0.002 | 1.202 | 1.681 | 0.008 | slower |
| cbench_libm/j0f | 0.658 | 1.100 | 0.019 | slower | 777.410 | 0.001 | 0.706 | 0.978 | 0.040 | parity |
| cbench_libm/j0f128 | 3.551 | 6.772 | 0.003 | slower | 8.230 | 0.823 | 3.548 | 4.627 | 0.011 | slower |
| cbench_libm/j0l | 50.060 | 76.097 | 0.005 | slower | 92.440 | 0.823 | 50.035 | 58.473 | 0.015 | slower |
| cbench_libm/j1 | 1.093 | 1.326 | 0.006 | slower | 535.520 | 0.002 | 1.222 | 1.659 | 0.011 | slower |
| cbench_libm/j1f | 0.662 | 1.101 | 0.046 | slower | 769.960 | 0.001 | 0.709 | 0.992 | 0.017 | parity |
| cbench_libm/j1f128 | 3.522 | 6.713 | 0.003 | slower | 8.170 | 0.822 | 3.519 | 4.572 | 0.010 | slower |
| cbench_libm/j1l | 49.981 | 76.095 | 0.004 | slower | 92.390 | 0.824 | 49.954 | 58.491 | 0.009 | slower |
| cbench_libm/jn | 112.881 | 173.914 | 0.026 | slower | 211.850 | 0.821 | 124.624 | 198.239 | 0.018 | slower |
| cbench_libm/jnf | 165.021 | 246.848 | 0.008 | slower | 300.090 | 0.823 | 175.089 | 289.772 | 0.018 | slower |
| cbench_libm/jnf128 | 1.632 | 3.005 | 0.008 | slower | 3.660 | 0.821 | 1.631 | 2.111 | 0.005 | slower |
| cbench_libm/jnl | 24.306 | 35.936 | 0.010 | slower | 43.650 | 0.823 | 24.298 | 28.375 | 0.008 | slower |
| cbench_libm/lgamma | 1.564 | 1.788 | 0.005 | slower | 1407.060 | 0.001 | 1.730 | 2.357 | 0.013 | slower |
| cbench_libm/lgammaf | 7.769 | 9.178 | 0.017 | slower | 2260.860 | 0.004 | 8.078 | 9.084 | 0.018 | slower |
| cbench_libm/lgammaf128 | 7.898 | 10.053 | 0.004 | slower | 14.050 | 0.716 | 7.894 | 6.915 | 0.006 | slower |
| cbench_libm/lgammal | 979.649 | 450.873 | 0.003 | slower | 630.110 | 0.716 | 978.001 | 271.032 | 0.006 | slower |
| cbench_libm/log | 2.785 | 5.175 | 0.009 | slower | 5.170 | 1.001 | 2.891 | 5.879 | 0.003 | slower |
| cbench_libm/log10 | 2.136 | 3.620 | 0.008 | slower | 3.520 | 1.028 | 1.920 | 3.517 | 0.002 | slower |
| cbench_libm/log10f | 0.839 | 1.213 | 0.064 | slower | 1.220 | 0.994 | 0.908 | 1.634 | 0.001 | slower |
| cbench_libm/log10f128 | 9.412 | 15.261 | 0.004 | slower | 38.870 | 0.393 | 9.409 | 10.544 | 0.005 | slower |
| cbench_libm/log10l | 26.358 | 11.581 | 0.010 | slower | 1667.330 | 0.007 | 26.248 | 6.221 | 0.004 | slower |
| cbench_libm/log1p | 2.425 | 3.676 | 0.009 | slower | 22.340 | 0.165 | 2.522 | 3.849 | 0.010 | slower |
| cbench_libm/log1pf | 1.998 | 2.182 | 0.011 | slower | 2.170 | 1.006 | 1.998 | 2.175 | 0.006 | slower |
| cbench_libm/log1pf128 | 7.867 | 13.334 | 0.004 | slower | 16.450 | 0.811 | 7.860 | 9.179 | 0.002 | slower |
| cbench_libm/log1pl | 31.404 | 11.671 | 0.005 | slower | 701.700 | 0.017 | 31.263 | 6.235 | 0.004 | slower |
| cbench_libm/log2 | 2.871 | 4.568 | 0.002 | slower | 4.480 | 1.020 | 2.971 | 5.564 | 0.001 | slower |
| cbench_libm/log2f | 1.561 | 2.354 | 0.032 | slower | 2.360 | 0.997 | 1.858 | 2.990 | 0.000 | slower |
| cbench_libm/log2f128 | 10.483 | 16.610 | 0.003 | slower | 20.480 | 0.811 | 10.477 | 11.860 | 0.008 | slower |
| cbench_libm/log2l | 26.327 | 11.359 | 0.008 | slower | 805.450 | 0.014 | 26.228 | 6.224 | 0.003 | slower |
| cbench_libm/log_upward | 4.482 | 14.115 | 0.914 | slower | 15.010 | 0.940 | 5.535 | 16.151 | 0.000 | slower |
| cbench_libm/logf | 1.542 | 2.595 | 0.068 | slower | 2.570 | 1.010 | 1.792 | 3.501 | 0.001 | slower |
| cbench_libm/logf128 | 9.071 | 15.845 | 0.002 | slower | 19.560 | 0.810 | 9.064 | 10.690 | 0.008 | slower |
| cbench_libm/logl | 23.996 | 10.973 | 0.008 | slower | 762.090 | 0.014 | 23.898 | 5.979 | 0.003 | slower |
| cbench_libm/pow | 2.575 | 3.351 | 0.362 | slower | 3.290 | 1.019 | 2.233 | 3.585 | 0.003 | slower |
| cbench_libm/pow_upward | 4.575 | 7.130 | 0.848 | slower | 7.450 | 0.957 | 6.194 | 8.899 | 0.007 | slower |
| cbench_libm/powf | 2.738 | 4.466 | 0.364 | slower | 4.460 | 1.001 | 3.278 | 4.858 | 0.002 | slower |
| cbench_libm/powf128 | 5.211 | 8.422 | 0.362 | slower | 10.150 | 0.830 | 5.207 | 5.834 | 0.011 | slower |
| cbench_libm/powl | 151.446 | 77.803 | 0.368 | slower | 93.800 | 0.829 | 151.260 | 48.542 | 0.003 | slower |
| cbench_libm/sin | 1.982 | 2.219 | 0.016 | slower | 4.040 | 0.549 | 1.580 | 2.154 | 0.017 | slower |
| cbench_libm/sin_huge | 3.555 | 4.575 | 0.928 | slower | 5.400 | 0.847 | 3.677 | 5.977 | 0.003 | slower |
| cbench_libm/sin_upward | 3.733 | 5.657 | 0.944 | slower | 8.390 | 0.674 | 4.482 | 5.224 | 0.014 | slower |
| cbench_libm/sinf | 1.777 | 2.047 | 0.007 | slower | 3.740 | 0.547 | 2.150 | 3.282 | 0.017 | slower |
| cbench_libm/sinf128 | 11.368 | 19.937 | 0.002 | slower | 25.220 | 0.791 | 11.362 | 13.999 | 0.030 | slower |
| cbench_libm/sinf_upward | 4.374 | 14.503 | 0.504 | slower | 13.480 | 1.076 | 5.278 | 18.497 | 0.013 | slower |
| cbench_libm/sinh | 2.305 | 1.912 | 0.007 | slower | 11.460 | 0.167 | 1.897 | 2.066 | 0.002 | slower |
| cbench_libm/sinhf | 0.665 | 0.658 | 0.008 | faster | 11.830 | 0.056 | 0.665 | 0.850 | 0.015 | faster |
| cbench_libm/sinhf128 | 4.216 | 6.080 | 0.003 | slower | 7.200 | 0.844 | 4.213 | 4.434 | 0.007 | slower |
| cbench_libm/sinhl | 335.596 | 122.878 | 0.006 | slower | 145.150 | 0.847 | 332.880 | 39.917 | 0.012 | slower |
| cbench_libm/sinl | 73.173 | 112.724 | 0.004 | slower | 142.270 | 0.792 | 73.140 | 88.243 | 0.041 | slower |
| cbench_libm/sqrt | 1.939 | 1.504 | 0.000 | slower | 1.500 | 1.003 | 1.939 | 1.225 | 0.128 | slower |
| cbench_libm/sqrtf | 1.823 | 2.091 | 0.000 | slower | 2.090 | 1.000 | 1.821 | 1.135 | 0.018 | slower |
| cbench_libm/sqrtf128 | 3.895 | 1.728 | 0.006 | slower | 1.730 | 0.999 | 3.902 | 4.503 | 0.051 | slower |
| cbench_libm/sqrtl | 2.767 | 3.581 | 0.001 | slower | 3.580 | 1.000 | 2.762 | 2.216 | 0.062 | slower |
| cbench_libm/tan | 2.843 | 2.831 | 0.008 | slower | 4.960 | 0.571 | 1.634 | 2.613 | 0.020 | slower |
| cbench_libm/tanf | 0.925 | 1.313 | 0.010 | slower | 1.590 | 0.826 | 0.924 | 1.242 | 0.008 | slower |
| cbench_libm/tanf128 | 10.761 | 16.845 | 0.005 | slower | 21.340 | 0.789 | 10.755 | 11.849 | 0.035 | slower |
| cbench_libm/tanh | 3.164 | 2.788 | 0.020 | slower | 13.040 | 0.214 | 2.238 | 2.985 | 0.024 | slower |
| cbench_libm/tanhf | 0.761 | 0.753 | 0.005 | faster | 13.610 | 0.055 | 0.761 | 0.953 | 0.003 | parity |
| cbench_libm/tanhf128 | 4.938 | 6.745 | 0.007 | slower | 8.030 | 0.840 | 4.936 | 4.962 | 0.013 | slower |
| cbench_libm/tanhl | 425.428 | 143.602 | 0.008 | slower | 171.050 | 0.840 | 422.119 | 44.886 | 0.008 | slower |
| cbench_libm/tanl | 82.264 | 117.058 | 0.010 | slower | 148.460 | 0.788 | 82.232 | 90.153 | 0.027 | slower |
| cbench_libm/tgamma | 1.144 | 1.489 | 0.012 | slower | 947.670 | 0.002 | 1.232 | 1.738 | 0.002 | slower |
| cbench_libm/tgammaf | 0.451 | 0.577 | 0.019 | faster | 1269.630 | 0.000 | 0.477 | 0.596 | 0.016 | faster |
| cbench_libm/tgammaf128 | 14.804 | 18.620 | 0.001 | slower | 25.590 | 0.728 | 14.797 | 13.043 | 0.014 | slower |
| cbench_libm/tgammal | 612.485 | 250.644 | 0.002 | slower | 344.780 | 0.727 | 610.753 | 123.293 | 0.008 | slower |
| cbench_libm/y0 | 1.023 | 1.232 | 0.362 | slower | 1555.010 | 0.001 | 1.145 | 1.640 | 0.006 | slower |
| cbench_libm/y0f | 0.683 | 1.173 | 0.023 | slower | 2272.310 | 0.001 | 0.732 | 1.060 | 0.021 | slower |
| cbench_libm/y0f128 | 13.065 | 19.374 | 0.362 | slower | 23.810 | 0.814 | 13.057 | 13.624 | 0.004 | slower |
| cbench_libm/y0l | 191.141 | 230.823 | 0.366 | slower | 283.580 | 0.814 | 191.097 | 166.767 | 0.006 | slower |
| cbench_libm/y1 | 1.024 | 1.241 | 0.363 | slower | 1580.730 | 0.001 | 1.147 | 1.640 | 0.014 | slower |
| cbench_libm/y1f | 0.681 | 1.190 | 0.360 | slower | 2298.680 | 0.001 | 0.731 | 1.051 | 0.014 | slower |
| cbench_libm/y1f128 | 12.978 | 19.202 | 0.364 | slower | 23.590 | 0.814 | 12.969 | 13.520 | 0.007 | slower |
| cbench_libm/y1l | 191.050 | 231.125 | 0.363 | slower | 283.310 | 0.816 | 190.985 | 167.639 | 0.006 | slower |
| cbench_libm/yn | 438.185 | 554.980 | 0.035 | slower | 680.080 | 0.816 | 484.585 | 628.331 | 0.007 | slower |
| cbench_libm/ynf | 641.378 | 763.086 | 0.006 | slower | 934.710 | 0.816 | 681.554 | 912.498 | 0.006 | slower |
| cbench_libm/ynf128 | 6.445 | 9.548 | 0.002 | slower | 11.690 | 0.817 | 6.442 | 6.816 | 0.009 | slower |
| cbench_libm/ynl | 94.158 | 114.701 | 0.003 | slower | 140.720 | 0.815 | 94.130 | 83.581 | 0.007 | slower |
