# Timing sweep 1110: libm, cbench_hot, cbench_locale and cbench_memref on Ivy Bridge and N100 (Alder Lake-N)

TIMING-SWEEP-1110. The first timing of the tree after nine libm landings that had no booked timing of their own
(LIBM-CPLX-EXP-TIER-2, LIBM-ENVELOPE-ASM, LIBM-F64-TRIG-QUICK-3, LIBM-F64-EXP-FAMILY-QUICK-2, LIBM-F64-POW-QUICK-2,
LIBM-F64-LOG-FAMILY-WIRE-3, LIBM-EXPF-RATCHET, LIBM-F64-INVHYP-QUICK-2, the expm1 part). Same method as
results/timing-sweep-1107b-table.md.

- Products: `next` at b14f639ccc985f08ef8b9dc1abffb9f30988ef9b, staged once with
  `cargo xtask bench --stage <tar> --only cbench_libm,cbench_hot,cbench_locale,cbench_memref` (manifest tree
  6f81339ed45b8669, profile release, 52 products; tarball sha256 b514340f03e0493e...). The tarball was unpacked whole
  on both booked timing hosts.
- Job per host and group: `./xtask-bin sweep run --out <dir> --only <group>` (interleaved Liella/glibc rounds, 5 per
  row). All eight runs ended `sweep: COMPLETE` with every selected `ratio_host` row measured (166 libm, 37 hot,
  36 locale, 96 memref per class). Ivy Bridge cbench_memref: bench rc 1 because the limit check flagged
  `__memcmpeq_eq_128` (1.105) and `__memcmpeq_tail_1k` (1.001), the same two rows as in 1107b (findings, not a run
  failure). Default dispatch on both hosts (the Ivy Bridge part has no FMA; the N100 runs its FMA paths).
- Per row: Liella ns and glibc ns are the medians over the rounds; ratio is the median of the per-round Liella/glibc
  ratios; spread is (max - min) / median of those ratios (the `sweep.tsv` definitions). Verdict: faster < 0.95,
  slower > 1.05, parity in between. instr is the instruction ratio per call from the same run's counters.
- Before column: the row's ratio on the same class in results/timing-sweep-1107b-table.md if that sweep timed it,
  else results/timing-sweep-libm-1107-table.md (`## Table`), else results/timing-sweep-1106e-table.md (`## Table`);
  `before src` names the table. change = ratio now / before. The cbench_hot rows (strncmp included) were not
  collected by 1107b, so their before figures are the 11-06e sweep; most memref and locale rows also come from 1106e.
- Families: a libm row by family and precision (the grouping of `sweep run`'s libm-families.md, xtask/src/sweep.rs);
  any other row by bench and function name.
- Not measured: `towctrans` (no bench row on `next`, as in 1107b; the wide-character rows timed are iswalpha*,
  iswctype_en_us and towupper*).

## Summary

- **exp on the N100 is no longer above 2x glibc: 1.728 (Liella 14.00 ns, glibc 8.12 ns, spread 0.095), down from
  2.169 in 1107b.** Ivy Bridge exp: 1.489 (27.97 / 18.79 ns, spread 0.108), from 1.690.
- libm overall median ratio: Ivy Bridge 6.78 -> 5.70, N100 5.32 -> 4.64 (166 rows each).
- f64 exp/log/pow family median: 3.61 -> 1.49 (Ivy Bridge), 3.53 -> 1.73 (N100). pow 3.38 -> 1.33 / 3.58 -> 1.45;
  expm1 2.03 -> 1.26 / 2.10 -> 1.00; log 1.93 -> 1.71 / 1.93 -> 1.83.
- f32 exp/log/pow median 2.52 -> 2.09 / 3.06 -> 2.17. expf and exp2f, which had regressed in 1107b, are now
  1.38 / 1.52 (Ivy Bridge, from 2.52 / 2.73) and 1.66 / 1.67 (N100, from 3.10 / 3.13).
- f64 trig median 2.59 -> 2.23 / 2.64 -> 1.86 (sin 2.23 -> 1.66 / 2.12 -> 1.49, cos 2.14 -> 1.53 / 1.86 -> 1.31,
  tan 2.83 -> 2.23 / 2.64 -> 1.86).
- f64 hyperbolic median 9.01 -> 1.60 / 11.97 -> 1.94; f32 hyperbolic 1.10 -> 0.95 / 1.49 -> 0.85.
- Complex: cexp 704 -> 5.34 (Ivy Bridge), 844 -> 5.70 (N100); cexpf 622 -> 1.11 / 498 -> 1.00. The complex f32 and f64
  medians fell 322 -> 241 and 366 -> 213 (Ivy Bridge), 304 -> 215 and 406 -> 255 (N100); cpow and csin still 300-560x (csin 365 / 404, cpow 498 / 561).
- Long double and f128 families are unchanged (trig long double median 395 / 264; bessel long double 95 / 71).
- Reopen rows (instructions <= 1.05, time > 1.05): coshf is now parity on Ivy Bridge (1.017) and faster on the N100
  (0.865, from 1.819). The other eight are unchanged within spread: log10f 1.08 / 1.64, tanf 1.32 / 1.23, y0 1.24 / 1.63,
  y1 1.25 / 1.62, j0f 1.11 / 0.97, j1f 1.10 / 0.99, y0f 1.17 / 1.06, y1f 1.20 / 1.06 (Ivy Bridge / N100).
- strncmp (cbench_hot): strncmp_diff8 parity on both (1.000); strncmp_eq16 1.051 (spread 0.219) / 1.100;
  strncmp_names64 1.080 / 0.846. Before (1106e): eq16 1.000 / 1.000, names64 0.968 / 0.846.
- Group medians: cbench_hot 0.962 / 0.701, cbench_locale 0.865 / 0.809, cbench_memref 0.929 / 0.755 (Ivy Bridge / N100).
  iswctype_en_us 1.98 / 1.82 is still the slowest wide-character row.
- **Slower than before:** on the N100, strcasecmp_c_256, strcasecmp_en_256 and strcasecmp_utf8_256 went from 0.80
  (1106e) to 1.55 (glibc 42.1 ns, Liella 65.3 ns, spread <= 0.003; instructions 1.12); the new rows
  strcasecmp_c_256_pageend / _xpage / _odd are 1.94 / 1.66 / 1.37 there. On Ivy Bridge the same 256-byte rows are
  faster (0.94) but strcasecmp_c_256_pageend is 1.82. nl_langinfo_l_en_us 1.22 -> 1.50 (Ivy Bridge, spread 0).
  Ivy Bridge exp_qnan 0.85 (11-07 morning) -> 1.48 with spread 0.455 (N100 unchanged at 3.13): low confidence. The
  landing behind the strcasecmp change was not identified in this story (several landings since 1106e; the
  `_pageend`, `_xpage` and `_odd` rows are new since then).
- Ivy Bridge round spreads are high on many libm rows (acos 0.57, log10f 0.56, pow 0.56; median spread over the libm
  rows 0.19), as in 1107b; verdicts on rows with spread above about 0.3 there are low confidence. N100: median libm
  spread 0.015.

## Per-row tables

### Ivy Bridge: cbench_libm (166 rows)

| row | family | Liella ns | glibc ns | ratio | spread | instr | verdict | before | before src | change |
|---|---|---:|---:|---:|---:|---:|---|---:|---|---:|
| acos | trig f64 | 125.79 | 14.65 | 8.630 | 0.567 | 3.375 | slower | 8.678 | 1107 | 0.994 |
| acosf | trig f32 | 79.91 | 12.09 | 6.610 | 0.577 | 3.157 | slower | 6.619 | 1107 | 0.999 |
| acosf128 | trig f128 | 32608.20 | 1755.09 | 18.557 | 0.385 | 18.453 | slower | 18.496 | 1107 | 1.003 |
| acosh | hyperbolic f64 | 38.39 | 29.61 | 1.296 | 0.004 | 1.869 | slower | 14.689 | 1107 | 0.088 |
| acoshf | hyperbolic f32 | 45.16 | 19.02 | 2.374 | 0.005 | 1.120 | slower | 2.354 | 1107 | 1.008 |
| acoshf128 | hyperbolic f128 | 23033.60 | 2224.93 | 10.354 | 0.002 | 7.837 | slower | 10.366 | 1107 | 0.999 |
| acoshl | hyperbolic long double | 23011.24 | 64.98 | 354.128 | 0.005 | 960.213 | slower | 354.335 | 1107 | 0.999 |
| acosl | trig long double | 32586.00 | 82.39 | 395.005 | 0.563 | 3933.251 | slower | 395.671 | 1107 | 0.998 |
| asin | trig f64 | 136.19 | 14.08 | 9.673 | 0.007 | 3.873 | slower | 9.792 | 1107 | 0.988 |
| asinf | trig f32 | 79.07 | 9.28 | 8.520 | 0.021 | 3.659 | slower | 8.599 | 1107 | 0.991 |
| asinf128 | trig f128 | 31855.92 | 1711.84 | 18.611 | 0.008 | 18.837 | slower | 18.594 | 1107 | 1.001 |
| asinh | hyperbolic f64 | 39.56 | 33.21 | 1.192 | 0.005 | 1.714 | slower | 13.837 | 1107 | 0.086 |
| asinhf | hyperbolic f32 | 53.21 | 23.28 | 2.285 | 0.007 | 1.071 | slower | 2.273 | 1107 | 1.005 |
| asinhf128 | hyperbolic f128 | 23402.83 | 2465.39 | 9.494 | 0.007 | 7.482 | slower | 9.520 | 1107 | 0.997 |
| asinhl | hyperbolic long double | 23368.63 | 57.34 | 407.273 | 0.008 | 1177.413 | slower | 410.470 | 1107 | 0.992 |
| asinl | trig long double | 31822.58 | 38.83 | 817.210 | 0.014 | 1682.588 | slower | 817.355 | 1107 | 1.000 |
| atan | trig f64 | 61.57 | 25.06 | 2.457 | 0.389 | 1.942 | slower | 2.591 | 1107 | 0.948 |
| atan2 | trig f64 | 75.04 | 43.41 | 1.729 | 0.254 | 1.211 | slower | 1.797 | 1107 | 0.962 |
| atan2f | trig f32 | 50.97 | 29.84 | 1.721 | 0.242 | 1.371 | slower | 1.688 | 1107 | 1.020 |
| atan2f128 | trig f128 | 30579.72 | 1254.53 | 24.361 | 0.367 | 26.109 | slower | 24.350 | 1107 | 1.000 |
| atan2l | trig long double | 30551.15 | 68.17 | 447.898 | 0.135 | 3698.195 | slower | 449.396 | 1107 | 0.997 |
| atanf | trig f32 | 43.64 | 14.76 | 2.957 | 0.407 | 2.414 | slower | 2.971 | 1107 | 0.995 |
| atanf128 | trig f128 | 29545.63 | 1077.56 | 27.295 | 0.251 | 27.204 | slower | 27.137 | 1107 | 1.006 |
| atanh | hyperbolic f64 | 39.44 | 29.34 | 1.346 | 0.015 | 1.463 | slower | 15.454 | 1107 | 0.087 |
| atanhf | hyperbolic f32 | 25.62 | 29.09 | 0.884 | 0.015 | 0.824 | faster | 0.984 | 1107 | 0.898 |
| atanhf128 | hyperbolic f128 | 21650.45 | 1993.33 | 10.861 | 0.002 | 6.912 | slower | 10.845 | 1107 | 1.001 |
| atanhl | hyperbolic long double | 21603.92 | 69.32 | 311.704 | 0.004 | 749.164 | slower | 312.135 | 1107 | 0.999 |
| atanl | trig long double | 29371.41 | 64.13 | 457.334 | 0.258 | 7359.662 | slower | 457.419 | 1107 | 1.000 |
| cbrt | roots/hypot f64 | 75.61 | 34.43 | 2.195 | 0.571 | 1.244 | slower | 2.333 | 1107 | 0.941 |
| cbrtf | roots/hypot f32 | 37.31 | 29.93 | 1.244 | 0.408 | 1.178 | slower | 1.243 | 1107 | 1.001 |
| cbrtf128 | roots/hypot f128 | 5993.89 | 1154.12 | 5.188 | 0.389 | 6.660 | slower | 5.167 | 1107 | 1.004 |
| cbrtl | roots/hypot long double | 5931.04 | 82.69 | 71.726 | 0.385 | 230.868 | slower | 71.538 | 1107 | 1.003 |
| cexp | complex f64 | 258.25 | 48.35 | 5.341 | 0.362 | 3.771 | slower | 703.580 | 1107b | 0.008 |
| cexpf | complex f32 | 61.09 | 54.92 | 1.112 | 0.359 | 0.956 | slower | 622.284 | 1107b | 0.002 |
| cexpl | complex long double | 34111.40 | 320.88 | 106.189 | 0.723 | 100.978 | slower | 106.120 | 1107 | 1.001 |
| clog | complex f64 | 21610.89 | 101.62 | 213.039 | 0.763 | 158.714 | slower | 214.837 | 1107 | 0.992 |
| clogf | complex f32 | 21200.51 | 88.27 | 240.536 | 0.746 | 265.044 | slower | 241.181 | 1107 | 0.997 |
| clogl | complex long double | 51972.27 | 169.10 | 307.455 | 0.607 | 780.772 | slower | 307.599 | 1107 | 1.000 |
| cos | trig f64 | 34.68 | 22.70 | 1.527 | 0.134 | 1.308 | slower | 2.136 | 1107b | 0.715 |
| cosf | trig f32 | 22.56 | 9.87 | 2.286 | 0.162 | 1.672 | slower | 2.280 | 1107 | 1.003 |
| cosf128 | trig f128 | 23229.32 | 1153.66 | 20.136 | 0.138 | 11.368 | slower | 20.102 | 1107 | 1.002 |
| cosh | hyperbolic f64 | 75.03 | 18.60 | 4.049 | 0.138 | 3.661 | slower | 4.190 | 1107 | 0.966 |
| coshf | hyperbolic f32 | 15.79 | 15.53 | 1.017 | 0.142 | 0.860 | parity | 1.210 | 1107 | 0.840 |
| coshf128 | hyperbolic f128 | 11357.20 | 1584.53 | 7.168 | 0.008 | 5.860 | slower | 7.182 | 1107 | 0.998 |
| coshl | hyperbolic long double | 11312.63 | 95.82 | 118.046 | 0.138 | 377.922 | slower | 117.928 | 1107 | 1.001 |
| cosl | trig long double | 118.98 | 207.01 | 0.574 | 0.145 | 0.541 | faster | 0.574 | 1107b | 1.000 |
| cpow | complex f64 | 85974.83 | 172.79 | 497.606 | 0.283 | 457.553 | slower | 498.311 | 1107 | 0.999 |
| cpowf | complex f32 | 85909.11 | 156.04 | 551.613 | 0.281 | 740.874 | slower | 555.264 | 1107 | 0.993 |
| cpowl | complex long double | 85973.68 | 542.27 | 158.544 | 0.290 | 226.642 | slower | 165.151 | 1107 | 0.960 |
| csin | complex f64 | 36748.02 | 100.74 | 364.546 | 0.367 | 285.733 | slower | 366.469 | 1107 | 0.995 |
| csinf | complex f32 | 36971.68 | 114.55 | 322.765 | 0.502 | 380.668 | slower | 322.069 | 1107 | 1.002 |
| csqrt | complex f64 | 6703.01 | 41.30 | 162.273 | 0.383 | 281.534 | slower | 162.508 | 1107 | 0.999 |
| csqrtf | complex f32 | 6690.81 | 57.68 | 115.770 | 0.391 | 329.190 | slower | 115.977 | 1107 | 0.998 |
| csqrtl | complex long double | 6815.91 | 82.99 | 82.109 | 0.367 | 216.071 | slower | 82.307 | 1107 | 0.998 |
| erf | erf f64 | 69.97 | 35.25 | 1.985 | 0.008 | 1.364 | slower | 2.063 | 1107 | 0.962 |
| erfc | erf f64 | 92.92 | 35.87 | 2.589 | 0.002 | 1.806 | slower | 2.667 | 1107 | 0.971 |
| erfcf | erf f32 | 59.38 | 33.02 | 1.797 | 0.014 | 1.327 | slower | 1.802 | 1107 | 0.997 |
| erfcf128 | erf f128 | 51550.73 | 3706.89 | 13.927 | 0.015 | 9.546 | slower | 13.922 | 1107 | 1.000 |
| erfcl | erf long double | 51528.91 | 173.11 | 297.666 | 0.010 | 909.876 | slower | 298.669 | 1107 | 0.997 |
| erff | erf f32 | 53.76 | 32.43 | 1.657 | 0.015 | 1.277 | slower | 1.658 | 1107 | 0.999 |
| erff128 | erf f128 | 51475.60 | 3738.57 | 13.787 | 0.009 | 9.463 | slower | 13.829 | 1107 | 0.997 |
| erfl | erf long double | 51435.93 | 172.92 | 297.422 | 0.009 | 920.259 | slower | 298.633 | 1107 | 0.996 |
| exp | exp/log/pow f64 | 27.97 | 18.79 | 1.489 | 0.108 | 1.314 | slower | 1.690 | 1107b | 0.881 |
| exp10 | exp/log/pow f64 | 33.31 | 12.52 | 2.654 | 0.101 | 2.154 | slower | 5.723 | 1107b | 0.464 |
| exp10f | exp/log/pow f32 | 22.48 | 8.69 | 2.362 | 0.119 | 1.490 | slower | 2.600 | 1107 | 0.908 |
| exp10f128 | exp/log/pow f128 | 12311.33 | 3502.79 | 3.518 | 0.213 | 2.596 | slower | 3.521 | 1107 | 0.999 |
| exp10l | exp/log/pow long double | 444.79 | 89.19 | 4.982 | 0.099 | 14.412 | slower | 4.968 | 1107 | 1.003 |
| exp2 | exp/log/pow f64 | 26.87 | 9.77 | 3.051 | 0.193 | 1.967 | slower | 5.459 | 1107b | 0.559 |
| exp2f | exp/log/pow f32 | 13.36 | 8.82 | 1.515 | 0.194 | 1.310 | slower | 2.728 | 1107b | 0.555 |
| exp2f128 | exp/log/pow f128 | 12259.28 | 1951.26 | 5.732 | 0.120 | 4.683 | slower | 6.336 | 1107 | 0.905 |
| exp2l | exp/log/pow long double | 443.81 | 72.02 | 6.842 | 0.104 | 18.785 | slower | 6.834 | 1107 | 1.001 |
| exp_overflow_errno | special f64 | 50.32 | 27.12 | 1.857 | 0.021 | 2.220 | slower | 2.101 | 1107 | 0.884 |
| exp_qnan | special f64 | 25.20 | 17.01 | 1.481 | 0.455 | 2.299 | slower | 0.854 | 1107 | 1.734 |
| exp_small | special f64 | 30.79 | 21.12 | 1.459 | 0.023 | 1.300 | slower | 2.911 | 1107 | 0.501 |
| exp_tiny | special f64 | 46.55 | 14.49 | 3.213 | 0.560 | 4.812 | slower | 3.689 | 1107 | 0.871 |
| exp_upward | special f64 | 237.13 | 21.12 | 11.222 | 0.004 | 4.766 | slower | 11.221 | 1107 | 1.000 |
| expf | exp/log/pow f32 | 13.36 | 9.68 | 1.379 | 0.114 | 1.280 | slower | 2.519 | 1107b | 0.547 |
| expf128 | exp/log/pow f128 | 13489.55 | 1997.82 | 6.762 | 0.036 | 5.023 | slower | 6.784 | 1107 | 0.997 |
| expf_qnan | special f32 | 8.85 | 8.83 | 1.002 | 0.141 | 1.167 | parity | 3.565 | 1107 | 0.281 |
| expf_upward | special f32 | 15.05 | 9.80 | 1.364 | 0.242 | 1.280 | slower | 18.337 | 1107 | 0.074 |
| expl | exp/log/pow long double | 450.73 | 99.83 | 4.513 | 0.012 | 12.481 | slower | 4.521 | 1107 | 0.998 |
| expm1 | exp/log/pow f64 | 39.17 | 31.17 | 1.257 | 0.330 | 1.436 | slower | 2.028 | 1107b | 0.620 |
| expm1f | exp/log/pow f32 | 46.53 | 29.10 | 1.604 | 0.324 | 1.764 | slower | 1.604 | 1107 | 1.000 |
| expm1f128 | exp/log/pow f128 | 11117.43 | 2098.22 | 5.355 | 0.500 | 3.676 | slower | 5.928 | 1107 | 0.903 |
| expm1l | exp/log/pow long double | 411.30 | 88.75 | 4.624 | 0.495 | 16.948 | slower | 5.089 | 1107 | 0.909 |
| hypot | roots/hypot f64 | 39.29 | 14.71 | 2.676 | 0.269 | 2.292 | slower | 2.871 | 1107 | 0.932 |
| hypotf | roots/hypot f32 | 10.20 | 5.93 | 1.720 | 0.251 | 1.372 | slower | 1.724 | 1107 | 0.998 |
| hypotf128 | roots/hypot f128 | 1181.30 | 1324.82 | 0.892 | 0.250 | 1.425 | faster | 0.888 | 1107 | 1.005 |
| hypotl | roots/hypot long double | 717.96 | 38.40 | 18.701 | 0.274 | 42.320 | slower | 18.770 | 1107 | 0.996 |
| j0 | bessel f64 | 129.97 | 101.86 | 1.276 | 0.009 | 1.074 | slower | 1.274 | 1107 | 1.002 |
| j0f | bessel f32 | 76.33 | 69.07 | 1.106 | 0.007 | 0.658 | slower | 1.100 | 1107 | 1.005 |
| j0f128 | bessel f128 | 44218.98 | 6549.25 | 6.756 | 0.003 | 3.552 | slower | 6.772 | 1107 | 0.998 |
| j0l | bessel long double | 44193.23 | 580.93 | 76.113 | 0.005 | 50.115 | slower | 76.097 | 1107 | 1.000 |
| j1 | bessel f64 | 132.55 | 100.35 | 1.320 | 0.008 | 1.094 | slower | 1.326 | 1107 | 0.995 |
| j1f | bessel f32 | 76.36 | 69.44 | 1.097 | 0.007 | 0.662 | slower | 1.101 | 1107 | 0.996 |
| j1f128 | bessel f128 | 44232.04 | 6605.82 | 6.697 | 0.003 | 3.522 | slower | 6.713 | 1107 | 0.998 |
| j1l | bessel long double | 44192.53 | 581.59 | 75.992 | 0.004 | 49.989 | slower | 76.095 | 1107 | 0.999 |
| jn | bessel f64 | 44152.25 | 254.35 | 173.568 | 0.367 | 112.872 | slower | 173.914 | 1107 | 0.998 |
| jnf | bessel f32 | 44104.35 | 179.53 | 245.886 | 0.359 | 165.069 | slower | 246.848 | 1107 | 0.996 |
| jnf128 | bessel f128 | 44209.22 | 14745.23 | 2.998 | 0.362 | 1.632 | slower | 3.005 | 1107 | 0.998 |
| jnl | bessel long double | 44137.26 | 1229.11 | 35.924 | 0.368 | 24.304 | slower | 35.936 | 1107 | 1.000 |
| lgamma | gamma f64 | 179.44 | 99.87 | 1.798 | 0.010 | 1.563 | slower | 1.788 | 1107 | 1.006 |
| lgammaf | gamma f32 | 570.75 | 62.18 | 9.183 | 0.015 | 7.765 | slower | 9.178 | 1107 | 1.001 |
| lgammaf128 | gamma f128 | 100572.01 | 10032.18 | 10.020 | 0.005 | 7.898 | slower | 10.053 | 1107 | 0.997 |
| lgammal | gamma long double | 100523.38 | 223.39 | 450.262 | 0.001 | 979.392 | slower | 450.873 | 1107 | 0.999 |
| log | exp/log/pow f64 | 23.11 | 13.48 | 1.714 | 0.275 | 1.488 | slower | 1.931 | 1107b | 0.888 |
| log10 | exp/log/pow f64 | 27.84 | 21.66 | 1.285 | 0.843 | 1.174 | slower | 3.609 | 1107b | 0.356 |
| log10f | exp/log/pow f32 | 18.13 | 16.90 | 1.075 | 0.563 | 0.786 | slower | 1.213 | 1107 | 0.886 |
| log10f128 | exp/log/pow f128 | 22028.50 | 1447.96 | 15.278 | 0.686 | 9.415 | slower | 15.261 | 1107 | 1.001 |
| log10l | exp/log/pow long double | 388.10 | 38.52 | 10.075 | 0.560 | 26.359 | slower | 11.581 | 1107 | 0.870 |
| log1p | exp/log/pow f64 | 24.47 | 18.67 | 1.307 | 0.570 | 1.372 | slower | 3.659 | 1107b | 0.357 |
| log1pf | exp/log/pow f32 | 41.82 | 19.26 | 2.171 | 0.723 | 1.998 | slower | 2.182 | 1107 | 0.995 |
| log1pf128 | exp/log/pow f128 | 20496.43 | 1528.25 | 13.367 | 0.392 | 7.865 | slower | 13.334 | 1107 | 1.002 |
| log1pl | exp/log/pow long double | 418.76 | 35.82 | 11.691 | 0.746 | 31.401 | slower | 11.671 | 1107 | 1.002 |
| log2 | exp/log/pow f64 | 27.66 | 16.24 | 1.710 | 0.279 | 1.687 | slower | 4.508 | 1107b | 0.379 |
| log2f | exp/log/pow f32 | 18.11 | 8.72 | 2.085 | 0.265 | 1.461 | slower | 2.354 | 1107 | 0.886 |
| log2f128 | exp/log/pow f128 | 22084.81 | 1507.40 | 14.638 | 0.138 | 10.482 | slower | 16.610 | 1107 | 0.881 |
| log2l | exp/log/pow long double | 386.99 | 38.72 | 10.000 | 0.266 | 26.337 | slower | 11.359 | 1107 | 0.880 |
| log_upward | special f64 | 255.77 | 18.33 | 13.954 | 0.148 | 4.474 | slower | 14.115 | 1107 | 0.989 |
| logf | exp/log/pow f32 | 21.48 | 9.32 | 2.298 | 0.376 | 1.442 | slower | 2.595 | 1107 | 0.886 |
| logf128 | exp/log/pow f128 | 23908.60 | 1508.33 | 15.824 | 0.234 | 9.073 | slower | 15.845 | 1107 | 0.999 |
| logl | exp/log/pow long double | 424.99 | 38.69 | 10.984 | 0.233 | 23.989 | slower | 10.973 | 1107 | 1.001 |
| pow | exp/log/pow f64 | 54.08 | 40.54 | 1.333 | 0.562 | 1.257 | slower | 3.378 | 1107b | 0.395 |
| pow_upward | special f64 | 402.81 | 56.50 | 7.119 | 0.231 | 4.563 | slower | 7.130 | 1107 | 0.998 |
| powf | exp/log/pow f32 | 75.89 | 16.96 | 4.485 | 0.562 | 2.739 | slower | 4.466 | 1107 | 1.004 |
| powf128 | exp/log/pow f128 | 29802.71 | 3543.20 | 8.419 | 0.255 | 5.210 | slower | 8.422 | 1107 | 1.000 |
| powl | exp/log/pow long double | 29753.00 | 381.28 | 77.988 | 0.393 | 151.423 | slower | 77.803 | 1107 | 1.002 |
| sin | trig f64 | 36.59 | 22.16 | 1.655 | 0.231 | 1.383 | slower | 2.230 | 1107b | 0.742 |
| sin_huge | special f64 | 94.29 | 139.33 | 0.680 | 0.012 | 0.759 | faster | 4.575 | 1107 | 0.149 |
| sin_upward | special f64 | 268.08 | 42.17 | 5.668 | 0.232 | 3.731 | slower | 5.657 | 1107 | 1.002 |
| sinf | trig f32 | 17.40 | 9.75 | 1.784 | 0.259 | 1.677 | slower | 2.047 | 1107 | 0.872 |
| sinf128 | trig f128 | 23275.23 | 1168.65 | 19.935 | 0.249 | 11.370 | slower | 19.937 | 1107 | 1.000 |
| sinf_upward | special f32 | 174.91 | 13.54 | 12.913 | 0.236 | 4.338 | slower | 14.503 | 1107 | 0.890 |
| sinh | hyperbolic f64 | 78.13 | 42.01 | 1.859 | 0.252 | 2.280 | slower | 1.912 | 1107 | 0.972 |
| sinhf | hyperbolic f32 | 24.17 | 40.56 | 0.597 | 0.255 | 0.630 | faster | 0.658 | 1107 | 0.907 |
| sinhf128 | hyperbolic f128 | 11412.32 | 1868.36 | 6.103 | 0.135 | 4.215 | slower | 6.080 | 1107 | 1.004 |
| sinhl | hyperbolic long double | 11338.61 | 92.36 | 122.765 | 0.144 | 335.492 | slower | 122.878 | 1107 | 0.999 |
| sinl | trig long double | 119.55 | 206.24 | 0.578 | 0.261 | 0.541 | faster | 0.579 | 1107b | 0.998 |
| sqrt | roots/hypot f64 | 8.49 | 5.63 | 1.508 | 0.253 | 1.939 | slower | 1.504 | 1107 | 1.003 |
| sqrtf | roots/hypot f32 | 8.47 | 4.05 | 2.091 | 0.390 | 1.823 | slower | 2.091 | 1107 | 1.000 |
| sqrtf128 | roots/hypot f128 | 1079.16 | 627.64 | 1.719 | 0.254 | 3.895 | slower | 1.728 | 1107 | 0.995 |
| sqrtl | roots/hypot long double | 36.18 | 10.05 | 3.600 | 0.253 | 2.767 | slower | 3.581 | 1107 | 1.005 |
| tan | trig f64 | 62.84 | 28.17 | 2.231 | 0.134 | 1.895 | slower | 2.830 | 1107b | 0.788 |
| tanf | trig f32 | 35.33 | 26.73 | 1.322 | 0.142 | 0.878 | slower | 1.313 | 1107 | 1.007 |
| tanf128 | trig f128 | 24310.95 | 1445.36 | 16.816 | 0.002 | 10.760 | slower | 16.845 | 1107 | 0.998 |
| tanh | hyperbolic f64 | 103.37 | 37.04 | 2.791 | 0.003 | 3.137 | slower | 2.788 | 1107 | 1.001 |
| tanhf | hyperbolic f32 | 23.60 | 35.26 | 0.669 | 0.003 | 0.722 | faster | 0.753 | 1107 | 0.888 |
| tanhf128 | hyperbolic f128 | 12452.29 | 1846.09 | 6.744 | 0.008 | 4.938 | slower | 6.745 | 1107 | 1.000 |
| tanhl | hyperbolic long double | 12420.39 | 86.36 | 143.821 | 0.006 | 425.177 | slower | 143.602 | 1107 | 1.002 |
| tanl | trig long double | 24270.24 | 207.19 | 117.115 | 0.008 | 82.262 | slower | 117.058 | 1107 | 1.000 |
| tgamma | gamma f64 | 238.27 | 159.96 | 1.491 | 0.005 | 1.143 | slower | 1.489 | 1107 | 1.001 |
| tgammaf | gamma f32 | 68.97 | 119.46 | 0.579 | 0.013 | 0.451 | faster | 0.577 | 1107 | 1.003 |
| tgammaf128 | gamma f128 | 110116.37 | 5938.80 | 18.561 | 0.004 | 14.801 | slower | 18.620 | 1107 | 0.997 |
| tgammal | gamma long double | 110131.16 | 439.36 | 250.663 | 0.005 | 611.678 | slower | 250.644 | 1107 | 1.000 |
| y0 | bessel f64 | 130.66 | 105.62 | 1.237 | 0.007 | 1.024 | slower | 1.232 | 1107 | 1.004 |
| y0f | bessel f32 | 85.32 | 73.20 | 1.167 | 0.017 | 0.683 | slower | 1.173 | 1107 | 0.995 |
| y0f128 | bessel f128 | 133931.59 | 6955.25 | 19.267 | 0.137 | 13.065 | slower | 19.374 | 1107 | 0.994 |
| y0l | bessel long double | 133770.66 | 579.03 | 231.093 | 0.007 | 191.158 | slower | 230.823 | 1107 | 1.001 |
| y1 | bessel f64 | 129.75 | 104.12 | 1.247 | 0.138 | 1.024 | slower | 1.241 | 1107 | 1.005 |
| y1f | bessel f32 | 85.74 | 71.48 | 1.204 | 0.140 | 0.681 | slower | 1.190 | 1107 | 1.012 |
| y1f128 | bessel f128 | 134171.66 | 7009.80 | 19.164 | 0.210 | 12.975 | slower | 19.202 | 1107 | 0.998 |
| y1l | bessel long double | 134067.54 | 580.15 | 230.988 | 0.008 | 191.092 | slower | 231.125 | 1107 | 0.999 |
| yn | bessel f64 | 135071.35 | 242.57 | 556.835 | 0.366 | 438.241 | slower | 554.980 | 1107 | 1.003 |
| ynf | bessel f32 | 134870.51 | 177.25 | 760.435 | 0.533 | 641.474 | slower | 763.086 | 1107 | 0.997 |
| ynf128 | bessel f128 | 135132.95 | 14226.38 | 9.503 | 0.286 | 6.445 | slower | 9.548 | 1107 | 0.995 |
| ynl | bessel long double | 135033.56 | 1177.20 | 114.707 | 0.363 | 94.163 | slower | 114.701 | 1107 | 1.000 |

### Ivy Bridge: cbench_hot (37 rows)

| row | family | Liella ns | glibc ns | ratio | spread | instr | verdict | before | before src | change |
|---|---|---:|---:|---:|---:|---:|---|---:|---|---:|
| ctype_b_loc | cbench_hot ctype | 2.41 | 2.81 | 0.858 | 0.004 | 1.199 | faster | 0.858 | 1106e | 1.000 |
| ctype_get_mb_cur_max | cbench_hot ctype | 2.81 | 2.81 | 1.000 | 0.000 | 0.909 | parity | 1.000 | 1106e | 1.000 |
| errno_location | cbench_hot errno | 2.82 | 2.81 | 1.004 | 0.152 | 0.900 | parity | 1.000 | 1106e | 1.004 |
| fwrite_unlocked_16 | cbench_hot fwrite | 10.69 | 15.07 | 0.708 | 0.011 | 0.729 | faster | 0.706 | 1106e | 1.003 |
| fwrite_unlocked_64 | cbench_hot fwrite | 15.18 | 21.15 | 0.718 | 0.048 | 0.704 | faster | 0.724 | 1106e | 0.992 |
| getc | cbench_hot getc | 4.56 | 4.57 | 0.998 | 0.004 | 0.817 | parity | 1.000 | 1106e | 0.998 |
| getc_unlocked | cbench_hot getc | 2.62 | 2.96 | 0.885 | 0.008 | 1.000 | faster | 0.881 | 1106e | 1.005 |
| getenv_hit_last | cbench_hot getenv | 166.21 | 286.56 | 0.581 | 0.011 | 0.465 | faster | 0.572 | 1106e | 1.016 |
| getenv_hit_mid | cbench_hot getenv | 97.76 | 104.25 | 0.938 | 0.012 | 0.755 | faster | 0.933 | 1106e | 1.005 |
| getenv_miss | cbench_hot getenv | 100.73 | 184.60 | 0.546 | 0.005 | 0.418 | faster | 0.513 | 1106e | 1.064 |
| memcpy_chk_64 | cbench_hot memcpy | 4.82 | 6.03 | 0.799 | 0.000 | 0.692 | faster | 0.799 | 1106e | 1.000 |
| mempcpy_16 | cbench_hot mempcpy | 4.02 | 4.02 | 1.000 | 0.077 | 0.955 | parity | 1.000 | 1106e | 1.000 |
| mempcpy_64 | cbench_hot mempcpy | 4.82 | 6.03 | 0.799 | 0.002 | 0.719 | faster | 0.799 | 1106e | 1.000 |
| memrchr_4k | cbench_hot memrchr | 117.56 | 129.42 | 0.908 | 0.001 | 0.860 | faster | 0.887 | 1106e | 1.024 |
| memrchr_64 | cbench_hot memrchr | 6.83 | 6.43 | 1.062 | 0.001 | 0.906 | slower | 1.000 | 1106e | 1.062 |
| memrchr_64_hit | cbench_hot memrchr | 4.82 | 3.46 | 1.393 | 0.000 | 1.107 | slower | 1.277 | 1106e | 1.091 |
| mutex_lock_unlock_normal | cbench_hot mutex | 8.04 | 8.45 | 0.951 | 0.001 | 0.847 | parity | 0.953 | 1106e | 0.998 |
| mutex_lock_unlock_normal_mt | cbench_hot mutex | 23.77 | 29.00 | 0.820 | 0.002 | 0.824 | faster | 0.820 | 1106e | 1.000 |
| mutex_lock_unlock_recursive | cbench_hot mutex | 10.05 | 12.89 | 0.780 | 0.001 | 0.878 | faster | 0.780 | 1106e | 1.000 |
| mutex_lock_unlock_recursive_mt | cbench_hot mutex | 26.58 | 27.63 | 0.962 | 0.003 | 0.850 | parity | 0.961 | 1106e | 1.001 |
| putc_unlocked | cbench_hot putc | 2.50 | 2.91 | 0.862 | 0.128 | 0.897 | faster | 0.862 | 1106e | 1.000 |
| strchr_16_hit | cbench_hot strchr | 4.42 | 3.96 | 1.116 | 0.000 | 1.000 | slower | 1.116 | 1106e | 1.000 |
| strchr_16_miss | cbench_hot strchr | 6.03 | 6.57 | 0.918 | 0.000 | 0.714 | faster | 0.918 | 1106e | 1.000 |
| strchr_64_hit | cbench_hot strchr | 7.24 | 6.73 | 1.076 | 0.019 | 1.000 | slower | 1.135 | 1106e | 0.948 |
| strchr_64_miss | cbench_hot strchr | 7.64 | 6.57 | 1.163 | 0.009 | 1.000 | slower | 1.289 | 1106e | 0.902 |
| strchrnul_16_hit | cbench_hot strchrnul | 4.02 | 3.62 | 1.110 | 0.251 | 1.000 | slower | 1.110 | 1106e | 1.000 |
| strchrnul_16_miss | cbench_hot strchrnul | 5.23 | 6.43 | 0.813 | 0.000 | 0.698 | faster | 0.813 | 1106e | 1.000 |
| strchrnul_64_hit | cbench_hot strchrnul | 6.84 | 6.03 | 1.134 | 0.003 | 1.000 | slower | 1.269 | 1106e | 0.894 |
| strchrnul_64_miss | cbench_hot strchrnul | 6.83 | 6.43 | 1.062 | 0.001 | 1.000 | slower | 1.194 | 1106e | 0.889 |
| strcpy_chk_16 | cbench_hot strcpy | 6.03 | 10.06 | 0.599 | 0.002 | 0.488 | faster | 0.599 | 1106e | 1.000 |
| strcpy_chk_64 | cbench_hot strcpy | 7.64 | 12.08 | 0.633 | 0.002 | 0.587 | faster | 0.599 | 1106e | 1.057 |
| strncmp_diff8 | cbench_hot strncmp | 4.82 | 4.82 | 1.000 | 0.000 | 0.923 | parity | 1.417 | 1106e | 0.706 |
| strncmp_eq16 | cbench_hot strncmp | 8.06 | 7.65 | 1.051 | 0.219 | 0.743 | slower | 1.000 | 1106e | 1.051 |
| strncmp_names64 | cbench_hot strncmp | 10.86 | 10.07 | 1.080 | 0.002 | 0.844 | slower | 0.968 | 1106e | 1.116 |
| sys_close_ebadf | cbench_hot sys | 500.48 | 489.60 | 1.018 | 0.052 | 1.063 | parity | 1.002 | 1106e | 1.016 |
| sys_fstat | cbench_hot sys | 800.31 | 812.51 | 0.990 | 0.036 | 1.008 | parity | 1.005 | 1106e | 0.985 |
| sys_read_1 | cbench_hot sys | 686.44 | 703.15 | 0.988 | 0.107 | 1.012 | parity | 1.009 | 1106e | 0.979 |

### Ivy Bridge: cbench_locale (36 rows)

| row | family | Liella ns | glibc ns | ratio | spread | instr | verdict | before | before src | change |
|---|---|---:|---:|---:|---:|---:|---|---:|---|---:|
| fnmatch_casefold_en_32 | cbench_locale fnmatch | 794.07 | 1266.35 | 0.628 | 0.014 | 0.781 | faster | 0.804 | 1106e | 0.781 |
| fnmatch_en_us | cbench_locale fnmatch | 57.38 | 194.06 | 0.296 | 0.008 | 0.277 | faster | 0.312 | 1106e | 0.949 |
| iconv_utf8_8859_15_4k | cbench_locale iconv | 6178.07 | 22318.69 | 0.277 | 0.001 | 0.321 | faster | 0.277 | 1106e | 1.000 |
| iconv_utf8_ascii_translit_4k | cbench_locale iconv | 32775.08 | 66449.16 | 0.493 | 0.005 | 0.410 | faster | 0.492 | 1106e | 1.002 |
| iconv_utf8_utf16_4k | cbench_locale iconv | 7212.78 | 12707.59 | 0.568 | 0.002 | 0.485 | faster | 0.567 | 1106e | 1.002 |
| isalpha_c | cbench_locale isalpha | 4.17 | 4.47 | 0.960 | 0.141 | 1.436 | parity | 1.000 | 1106e | 0.960 |
| iswalpha_en_us | cbench_locale iswalpha | 8.30 | 9.26 | 0.897 | 0.040 | 1.163 | faster | 0.899 | 1107b | 0.998 |
| iswalpha_l_en_us | cbench_locale iswalpha | 9.15 | 7.13 | 1.288 | 0.134 | 1.404 | slower | 1.287 | 1107b | 1.001 |
| iswalpha_utf8 | cbench_locale iswalpha | 4.82 | 9.26 | 0.521 | 0.002 | 0.633 | faster | 0.913 | 1107b | 0.571 |
| iswctype_en_us | cbench_locale iswctype | 9.16 | 4.63 | 1.978 | 0.224 | 1.651 | slower | 1.987 | 1107b | 0.995 |
| localeconv_en_us | cbench_locale localeconv | 10.89 | 20.49 | 0.531 | 0.045 | 0.491 | faster | 0.532 | 1106e | 0.998 |
| mbrtowc_en_us_3 | cbench_locale mbrtowc | 27.79 | 31.41 | 0.885 | 0.002 | 0.889 | faster | 0.884 | 1106e | 1.001 |
| mbrtowc_utf8_1 | cbench_locale mbrtowc | 7.24 | 26.18 | 0.277 | 0.146 | 0.202 | faster | 0.277 | 1106e | 1.000 |
| mbrtowc_utf8_2 | cbench_locale mbrtowc | 8.44 | 28.19 | 0.299 | 0.084 | 0.241 | faster | 0.300 | 1106e | 0.997 |
| mbrtowc_utf8_3 | cbench_locale mbrtowc | 28.19 | 31.41 | 0.897 | 0.007 | 0.889 | faster | 0.910 | 1106e | 0.986 |
| newlocale_free | cbench_locale newlocale | 2570.87 | 7751.07 | 0.331 | 0.029 | 0.343 | faster | 0.337 | 1106e | 0.982 |
| nl_langinfo_en_us | cbench_locale nl | 4.42 | 4.02 | 1.100 | 0.000 | 0.839 | slower | 1.100 | 1106e | 1.000 |
| nl_langinfo_l_en_us | cbench_locale nl | 4.82 | 3.22 | 1.497 | 0.000 | 1.250 | slower | 1.221 | 1106e | 1.226 |
| snprintf_f_en_us | cbench_locale snprintf | 175.89 | 609.22 | 0.289 | 0.002 | 0.433 | faster | 0.301 | 1106e | 0.960 |
| strcasecmp_c_16 | cbench_locale strcasecmp | 12.48 | 12.08 | 1.033 | 0.001 | 1.010 | parity | 1.022 | 1106e | 1.011 |
| strcasecmp_c_256 | cbench_locale strcasecmp | 44.29 | 47.27 | 0.937 | 0.001 | 1.104 | faster | 0.971 | 1106e | 0.965 |
| strcasecmp_c_256_odd | cbench_locale strcasecmp | 42.49 | 50.86 | 0.835 | 0.039 | 0.928 | faster | - | - | - |
| strcasecmp_c_256_pageend | cbench_locale strcasecmp | 80.21 | 44.13 | 1.817 | 0.121 | 1.934 | slower | - | - | - |
| strcasecmp_c_256_xpage | cbench_locale strcasecmp | 51.25 | 47.27 | 1.084 | 0.000 | 1.306 | slower | - | - | - |
| strcasecmp_en_256 | cbench_locale strcasecmp | 44.22 | 47.02 | 0.940 | 0.000 | 1.104 | faster | 0.976 | 1106e | 0.963 |
| strcasecmp_utf8_256 | cbench_locale strcasecmp | 44.29 | 47.02 | 0.942 | 0.001 | 1.104 | faster | 0.975 | 1106e | 0.966 |
| strcoll_utf8_16 | cbench_locale strcoll | 8.46 | 13.29 | 0.637 | 0.067 | 0.621 | faster | 0.729 | 1106e | 0.874 |
| strftime_names_en_us | cbench_locale strftime | 48.76 | 51.86 | 0.940 | 0.010 | 1.011 | faster | 0.954 | 1106e | 0.985 |
| strncasecmp_c_64 | cbench_locale strncasecmp | 15.36 | 15.30 | 1.003 | 0.002 | 0.932 | parity | 0.950 | 1106e | 1.056 |
| strtod_c | cbench_locale strtod | 40.42 | 176.44 | 0.229 | 0.008 | 0.319 | faster | 0.222 | 1106e | 1.032 |
| strtod_en_us | cbench_locale strtod | 40.40 | 176.41 | 0.229 | 0.013 | 0.319 | faster | 0.223 | 1106e | 1.027 |
| strtod_utf8 | cbench_locale strtod | 40.40 | 176.49 | 0.229 | 0.009 | 0.319 | faster | 0.221 | 1106e | 1.036 |
| toupper_c | cbench_locale toupper | 3.26 | 3.48 | 0.931 | 0.122 | 1.050 | faster | 1.000 | 1106e | 0.931 |
| towupper_l_en_us | cbench_locale towupper | 7.24 | 6.40 | 1.131 | 0.002 | 1.000 | slower | 1.130 | 1107b | 1.001 |
| towupper_utf8 | cbench_locale towupper | 7.24 | 8.58 | 0.844 | 0.003 | 1.324 | faster | 0.844 | 1107b | 1.000 |
| wcwidth_utf8 | cbench_locale wcwidth | 6.03 | 7.64 | 0.789 | 0.009 | 1.162 | faster | 0.791 | 1106e | 0.997 |

### Ivy Bridge: cbench_memref (96 rows)

| row | family | Liella ns | glibc ns | ratio | spread | instr | verdict | before | before src | change |
|---|---|---:|---:|---:|---:|---:|---|---:|---|---:|
| __memcmpeq_eq_128 | cbench_memref __memcmpeq | 8.43 | 7.63 | 1.105 | 0.001 | 0.907 | slower | 1.105 | 1107b | 1.000 |
| __memcmpeq_eq_16 | cbench_memref __memcmpeq | 3.62 | 4.02 | 0.900 | 0.000 | 1.001 | faster | 0.900 | 1107b | 1.000 |
| __memcmpeq_eq_1k | cbench_memref __memcmpeq | 37.31 | 36.50 | 1.022 | 0.003 | 0.953 | parity | 1.022 | 1107b | 1.000 |
| __memcmpeq_eq_32 | cbench_memref __memcmpeq | 4.43 | 4.42 | 1.002 | 0.000 | 1.072 | parity | 1.002 | 1107b | 1.000 |
| __memcmpeq_eq_64 | cbench_memref __memcmpeq | 5.63 | 6.03 | 0.934 | 0.000 | 0.951 | faster | 0.934 | 1107b | 1.000 |
| __memcmpeq_eq_8 | cbench_memref __memcmpeq | 3.62 | 3.62 | 1.000 | 0.000 | 1.005 | parity | 1.000 | 1107b | 1.000 |
| __memcmpeq_head_128 | cbench_memref __memcmpeq | 6.43 | 5.22 | 1.232 | 0.000 | 1.095 | slower | 1.232 | 1107b | 1.000 |
| __memcmpeq_head_16 | cbench_memref __memcmpeq | 4.02 | 4.42 | 0.910 | 0.002 | 0.783 | faster | 0.910 | 1107b | 1.000 |
| __memcmpeq_head_1k | cbench_memref __memcmpeq | 6.43 | 5.22 | 1.232 | 0.000 | 1.095 | slower | 1.232 | 1107b | 1.000 |
| __memcmpeq_head_32 | cbench_memref __memcmpeq | 4.02 | 5.22 | 0.770 | 0.000 | 0.860 | faster | 0.770 | 1107b | 1.000 |
| __memcmpeq_head_64 | cbench_memref __memcmpeq | 4.02 | 5.22 | 0.770 | 0.000 | 0.861 | faster | 0.770 | 1107b | 1.000 |
| __memcmpeq_head_8 | cbench_memref __memcmpeq | 4.02 | 4.02 | 1.000 | 0.000 | 0.786 | parity | 1.000 | 1107b | 1.000 |
| __memcmpeq_tail_128 | cbench_memref __memcmpeq | 8.03 | 8.04 | 0.999 | 0.048 | 0.907 | parity | 0.953 | 1107b | 1.048 |
| __memcmpeq_tail_16 | cbench_memref __memcmpeq | 3.22 | 4.42 | 0.729 | 0.002 | 1.005 | faster | 0.729 | 1107b | 1.000 |
| __memcmpeq_tail_1k | cbench_memref __memcmpeq | 36.95 | 36.90 | 1.001 | 0.000 | 0.954 | parity | 1.001 | 1107b | 1.000 |
| __memcmpeq_tail_32 | cbench_memref __memcmpeq | 4.03 | 4.82 | 0.836 | 0.000 | 1.072 | faster | 0.836 | 1107b | 1.000 |
| __memcmpeq_tail_64 | cbench_memref __memcmpeq | 5.23 | 6.43 | 0.813 | 0.000 | 0.953 | faster | 0.813 | 1107b | 1.000 |
| __memcmpeq_tail_8 | cbench_memref __memcmpeq | 3.62 | 4.02 | 0.900 | 0.002 | 0.787 | faster | 0.900 | 1107b | 1.000 |
| bcmp_eq_128 | cbench_memref bcmp | 8.44 | 7.63 | 1.106 | 0.096 | 0.880 | slower | 1.159 | 1107b | 0.954 |
| bcmp_eq_16 | cbench_memref bcmp | 3.62 | 4.42 | 0.819 | 0.002 | 0.919 | faster | 0.819 | 1107b | 1.000 |
| bcmp_eq_1k | cbench_memref bcmp | 37.32 | 36.90 | 1.019 | 0.406 | 0.951 | parity | 1.011 | 1107b | 1.008 |
| bcmp_eq_32 | cbench_memref bcmp | 4.43 | 4.82 | 0.919 | 0.004 | 1.032 | faster | 0.919 | 1107b | 1.000 |
| bcmp_eq_64 | cbench_memref bcmp | 5.63 | 6.43 | 0.876 | 0.000 | 0.928 | faster | 0.876 | 1107b | 1.000 |
| bcmp_eq_8 | cbench_memref bcmp | 3.62 | 4.02 | 0.900 | 0.000 | 0.796 | faster | 0.900 | 1107b | 1.000 |
| bcmp_head_128 | cbench_memref bcmp | 6.03 | 5.22 | 1.155 | 0.000 | 0.925 | slower | 1.155 | 1107b | 1.000 |
| bcmp_head_16 | cbench_memref bcmp | 3.62 | 5.22 | 0.693 | 0.000 | 0.724 | faster | 0.693 | 1107b | 1.000 |
| bcmp_head_1k | cbench_memref bcmp | 6.03 | 5.22 | 1.155 | 0.000 | 0.922 | slower | 1.155 | 1107b | 1.000 |
| bcmp_head_32 | cbench_memref bcmp | 3.62 | 5.22 | 0.693 | 0.000 | 0.727 | faster | 0.693 | 1107b | 1.000 |
| bcmp_head_64 | cbench_memref bcmp | 3.62 | 5.22 | 0.693 | 0.000 | 0.724 | faster | 0.693 | 1107b | 1.000 |
| bcmp_head_8 | cbench_memref bcmp | 3.62 | 4.82 | 0.751 | 0.000 | 0.550 | faster | 0.751 | 1107b | 1.000 |
| bcmp_tail_128 | cbench_memref bcmp | 8.03 | 8.04 | 0.999 | 0.048 | 0.785 | parity | 0.996 | 1107b | 1.003 |
| bcmp_tail_16 | cbench_memref bcmp | 3.22 | 4.82 | 0.668 | 0.000 | 0.796 | faster | 0.668 | 1107b | 1.000 |
| bcmp_tail_1k | cbench_memref bcmp | 36.95 | 37.73 | 0.979 | 0.002 | 0.927 | parity | 0.979 | 1107b | 1.000 |
| bcmp_tail_32 | cbench_memref bcmp | 4.03 | 5.62 | 0.717 | 0.000 | 0.884 | faster | 0.717 | 1107b | 1.000 |
| bcmp_tail_64 | cbench_memref bcmp | 5.23 | 6.83 | 0.766 | 0.000 | 0.779 | faster | 0.766 | 1107b | 1.000 |
| bcmp_tail_8 | cbench_memref bcmp | 3.62 | 4.42 | 0.819 | 0.000 | 0.553 | faster | 0.819 | 1107b | 1.000 |
| memchr_16_mix | cbench_memref memchr | 6.43 | 7.23 | 0.897 | 0.070 | 0.937 | faster | 0.949 | 1106e | 0.945 |
| memchr_4k | cbench_memref memchr | 116.00 | 136.90 | 0.847 | 0.022 | 0.838 | faster | 0.786 | 1106e | 1.078 |
| memchr_64 | cbench_memref memchr | 8.03 | 6.83 | 1.176 | 0.000 | 0.817 | slower | 1.117 | 1106e | 1.053 |
| memchr_64_mix | cbench_memref memchr | 8.03 | 9.24 | 0.869 | 0.047 | 0.775 | faster | 0.894 | 1106e | 0.972 |
| memchr_64k | cbench_memref memchr | 2308.69 | 2303.99 | 1.001 | 0.147 | 0.852 | parity | 1.010 | 1106e | 0.991 |
| memcmp_128 | cbench_memref memcmp | 7.63 | 7.63 | 1.000 | 0.000 | 0.880 | parity | 1.000 | 1107b | 1.000 |
| memcmp_144 | cbench_memref memcmp | 9.24 | 9.23 | 1.001 | 0.043 | 0.876 | parity | 1.001 | 1107b | 1.000 |
| memcmp_16 | cbench_memref memcmp | 3.23 | 4.42 | 0.731 | 0.000 | 1.076 | faster | 0.731 | 1107b | 1.000 |
| memcmp_16_mix | cbench_memref memcmp | 4.83 | 5.23 | 0.924 | 0.000 | 1.050 | faster | 0.924 | 1107b | 1.000 |
| memcmp_256 | cbench_memref memcmp | 13.25 | 12.44 | 1.065 | 0.031 | 0.981 | slower | 1.065 | 1107b | 1.000 |
| memcmp_4k | cbench_memref memcmp | 143.25 | 144.85 | 0.989 | 0.003 | 0.952 | parity | 0.986 | 1107b | 1.003 |
| memcmp_64 | cbench_memref memcmp | 5.23 | 6.43 | 0.813 | 0.000 | 1.027 | faster | 0.813 | 1107b | 1.000 |
| memcmp_64_mix | cbench_memref memcmp | 6.94 | 7.25 | 0.957 | 0.009 | 1.018 | parity | 0.961 | 1107b | 0.996 |
| memcmp_64k | cbench_memref memcmp | 3967.86 | 4125.48 | 0.959 | 0.039 | 0.945 | parity | 0.960 | 1107b | 0.999 |
| memcmp_diff8_127 | cbench_memref memcmp | 6.83 | 4.82 | 1.417 | 0.000 | 1.282 | slower | 1.417 | 1107b | 1.000 |
| memcmp_diff8_64 | cbench_memref memcmp | 6.03 | 4.82 | 1.251 | 0.000 | 1.318 | slower | 1.251 | 1107b | 1.000 |
| memcmp_diff8_96 | cbench_memref memcmp | 6.83 | 4.82 | 1.417 | 0.000 | 1.277 | slower | 1.417 | 1107b | 1.000 |
| memcpy_128 | cbench_memref memcpy | 5.63 | 5.24 | 1.074 | 0.000 | 0.972 | slower | 0.998 | 1106e | 1.076 |
| memcpy_144 | cbench_memref memcpy | 7.64 | 7.24 | 1.055 | 0.000 | 0.869 | slower | 1.010 | 1106e | 1.045 |
| memcpy_16 | cbench_memref memcpy | 3.22 | 3.63 | 0.887 | 0.000 | 0.949 | faster | 0.777 | 1106e | 1.142 |
| memcpy_16_mix | cbench_memref memcpy | 15.27 | 15.25 | 1.001 | 0.000 | 1.001 | parity | 1.001 | 1106e | 1.000 |
| memcpy_256 | cbench_memref memcpy | 7.64 | 10.05 | 0.760 | 0.041 | 0.621 | faster | 0.751 | 1106e | 1.012 |
| memcpy_4k | cbench_memref memcpy | 110.24 | 127.45 | 0.866 | 0.843 | 26.436 | faster | 0.863 | 1106e | 1.003 |
| memcpy_4k_off | cbench_memref memcpy | 172.61 | 194.94 | 0.885 | 0.000 | 0.992 | faster | 0.885 | 1106e | 1.000 |
| memcpy_64 | cbench_memref memcpy | 4.02 | 5.64 | 0.713 | 0.000 | 0.691 | faster | 0.642 | 1106e | 1.111 |
| memcpy_64_mix | cbench_memref memcpy | 15.30 | 15.28 | 1.001 | 0.001 | 0.997 | parity | 1.001 | 1106e | 1.000 |
| memcpy_64k | cbench_memref memcpy | 3348.58 | 3507.52 | 0.952 | 0.184 | 2.094 | parity | 0.969 | 1106e | 0.982 |
| memmove_128 | cbench_memref memmove | 9.64 | 9.63 | 1.001 | 0.001 | 0.972 | parity | 1.001 | 1106e | 1.000 |
| memmove_144 | cbench_memref memmove | 14.05 | 12.04 | 1.167 | 0.000 | 0.814 | slower | 1.167 | 1106e | 1.000 |
| memmove_16 | cbench_memref memmove | 7.23 | 8.03 | 0.900 | 0.000 | 0.949 | faster | 0.900 | 1106e | 1.000 |
| memmove_256 | cbench_memref memmove | 13.65 | 13.64 | 1.001 | 0.000 | 0.686 | parity | 1.001 | 1106e | 1.000 |
| memmove_4k | cbench_memref memmove | 125.51 | 124.70 | 1.006 | 0.000 | 0.892 | parity | 1.007 | 1106e | 0.999 |
| memmove_64 | cbench_memref memmove | 7.64 | 7.63 | 1.001 | 0.001 | 0.694 | parity | 1.000 | 1106e | 1.001 |
| memmove_overlap64_4k | cbench_memref memmove | 109.31 | 115.48 | 0.947 | 0.002 | 26.150 | faster | 0.948 | 1106e | 0.999 |
| memmove_overlap8_4k | cbench_memref memmove | 125.91 | 161.19 | 0.781 | 0.000 | 25.741 | faster | 0.781 | 1106e | 1.000 |
| memset_128 | cbench_memref memset | 4.43 | 6.03 | 0.735 | 0.000 | 0.745 | faster | 0.735 | 1106e | 1.000 |
| memset_16 | cbench_memref memset | 2.82 | 3.62 | 0.779 | 0.004 | 0.954 | faster | 0.779 | 1106e | 1.000 |
| memset_16_mix | cbench_memref memset | 14.85 | 16.04 | 0.926 | 0.001 | 1.001 | faster | 1.000 | 1106e | 0.926 |
| memset_4k | cbench_memref memset | 108.67 | 108.47 | 1.002 | 0.000 | 1.001 | parity | 1.002 | 1106e | 1.000 |
| memset_4k_off | cbench_memref memset | 172.80 | 171.59 | 1.007 | 0.000 | 1.004 | parity | 1.007 | 1106e | 1.000 |
| memset_64 | cbench_memref memset | 3.62 | 5.22 | 0.693 | 0.002 | 0.716 | faster | 0.693 | 1106e | 1.000 |
| memset_64_mix | cbench_memref memset | 14.85 | 16.04 | 0.926 | 0.000 | 1.006 | faster | 1.000 | 1106e | 0.926 |
| memset_64k | cbench_memref memset | 2479.97 | 2481.85 | 1.000 | 0.007 | 1.086 | parity | 0.997 | 1106e | 1.003 |
| rawmemchr_16_mix | cbench_memref rawmemchr | 5.23 | 8.03 | 0.651 | 0.044 | 0.915 | faster | 0.700 | 1106e | 0.930 |
| rawmemchr_4k | cbench_memref rawmemchr | 133.87 | 123.03 | 1.088 | 0.000 | 0.908 | slower | 1.078 | 1106e | 1.009 |
| rawmemchr_64 | cbench_memref rawmemchr | 6.83 | 6.83 | 1.000 | 0.000 | 0.921 | parity | 0.941 | 1106e | 1.063 |
| rawmemchr_64_mix | cbench_memref rawmemchr | 6.83 | 8.90 | 0.767 | 0.068 | 0.920 | faster | 0.816 | 1106e | 0.940 |
| rawmemchr_64k | cbench_memref rawmemchr | 2289.52 | 2263.47 | 1.012 | 0.013 | 0.901 | parity | 1.011 | 1106e | 1.001 |
| strcmp_16_mix | cbench_memref strcmp | 7.97 | 19.07 | 0.417 | 0.016 | 0.742 | faster | 0.391 | 1106e | 1.066 |
| strcmp_64_mix | cbench_memref strcmp | 14.28 | 22.56 | 0.633 | 0.022 | 0.967 | faster | 0.561 | 1106e | 1.128 |
| strlen_16_mix | cbench_memref strlen | 5.63 | 6.45 | 0.873 | 0.008 | 0.696 | faster | 0.888 | 1106e | 0.983 |
| strlen_4k | cbench_memref strlen | 82.32 | 80.41 | 1.024 | 0.000 | 0.924 | parity | 1.008 | 1106e | 1.016 |
| strlen_64 | cbench_memref strlen | 7.23 | 8.69 | 0.832 | 0.003 | 0.663 | faster | 0.789 | 1106e | 1.054 |
| strlen_64_mix | cbench_memref strlen | 7.63 | 12.81 | 0.596 | 0.002 | 0.695 | faster | 0.564 | 1106e | 1.057 |
| strlen_64k | cbench_memref strlen | 2063.25 | 2058.96 | 1.003 | 0.205 | 0.892 | parity | 1.002 | 1106e | 1.001 |
| strnlen_16_mix | cbench_memref strnlen | 7.23 | 7.76 | 0.932 | 0.010 | 0.849 | faster | 0.999 | 1106e | 0.933 |
| strnlen_4k | cbench_memref strnlen | 91.88 | 115.29 | 0.797 | 0.001 | 0.817 | faster | 0.662 | 1106e | 1.204 |
| strnlen_64 | cbench_memref strnlen | 8.03 | 9.24 | 0.869 | 0.001 | 0.866 | faster | 0.869 | 1106e | 1.000 |
| strnlen_64_mix | cbench_memref strnlen | 8.49 | 11.75 | 0.723 | 0.003 | 0.876 | faster | 0.740 | 1106e | 0.977 |
| strnlen_64k | cbench_memref strnlen | 2066.83 | 2025.95 | 1.019 | 0.015 | 0.755 | parity | 1.022 | 1106e | 0.997 |

### N100: cbench_libm (166 rows)

| row | family | Liella ns | glibc ns | ratio | spread | instr | verdict | before | before src | change |
|---|---|---:|---:|---:|---:|---:|---|---:|---|---:|
| acos | trig f64 | 82.08 | 12.19 | 6.734 | 0.091 | 2.231 | slower | 7.039 | 1107 | 0.957 |
| acosf | trig f32 | 48.43 | 10.19 | 4.762 | 0.026 | 3.153 | slower | 4.741 | 1107 | 1.004 |
| acosf128 | trig f128 | 27245.54 | 2104.49 | 12.949 | 0.012 | 18.445 | slower | 12.996 | 1107 | 0.996 |
| acosh | hyperbolic f64 | 31.75 | 16.24 | 1.954 | 0.007 | 1.467 | slower | 26.192 | 1107 | 0.075 |
| acoshf | hyperbolic f32 | 43.86 | 11.30 | 3.881 | 0.004 | 1.225 | slower | 3.802 | 1107 | 1.021 |
| acoshf128 | hyperbolic f128 | 19162.47 | 2572.01 | 7.421 | 0.011 | 7.832 | slower | 7.449 | 1107 | 0.996 |
| acoshl | hyperbolic long double | 19120.29 | 97.03 | 197.055 | 0.009 | 958.783 | slower | 197.191 | 1107 | 0.999 |
| acosl | trig long double | 27230.00 | 102.75 | 264.000 | 0.037 | 3932.709 | slower | 265.302 | 1107 | 0.995 |
| asin | trig f64 | 85.58 | 10.67 | 8.038 | 0.053 | 2.441 | slower | 8.393 | 1107 | 0.958 |
| asinf | trig f32 | 47.88 | 8.31 | 5.751 | 0.033 | 3.654 | slower | 5.828 | 1107 | 0.987 |
| asinf128 | trig f128 | 26582.96 | 2052.83 | 12.949 | 0.022 | 18.829 | slower | 12.970 | 1107 | 0.998 |
| asinh | hyperbolic f64 | 32.24 | 19.67 | 1.639 | 0.010 | 1.345 | slower | 22.533 | 1107 | 0.073 |
| asinhf | hyperbolic f32 | 49.11 | 16.01 | 3.067 | 0.021 | 1.137 | slower | 3.066 | 1107 | 1.000 |
| asinhf128 | hyperbolic f128 | 19473.89 | 2812.05 | 6.931 | 0.012 | 7.477 | slower | 6.936 | 1107 | 0.999 |
| asinhl | hyperbolic long double | 19436.26 | 92.65 | 209.785 | 0.013 | 1173.469 | slower | 209.899 | 1107 | 0.999 |
| asinl | trig long double | 26775.28 | 56.13 | 477.789 | 0.011 | 1681.266 | slower | 475.766 | 1107 | 1.004 |
| atan | trig f64 | 38.71 | 13.21 | 2.930 | 0.026 | 1.803 | slower | 3.203 | 1107 | 0.915 |
| atan2 | trig f64 | 43.87 | 28.86 | 1.520 | 0.024 | 1.029 | slower | 1.648 | 1107 | 0.922 |
| atan2f | trig f32 | 30.98 | 23.23 | 1.334 | 0.052 | 1.370 | slower | 1.348 | 1107 | 0.990 |
| atan2f128 | trig f128 | 25438.37 | 1420.41 | 17.864 | 0.011 | 26.101 | slower | 17.883 | 1107 | 0.999 |
| atan2l | trig long double | 25597.17 | 87.34 | 293.075 | 0.019 | 3694.018 | slower | 291.352 | 1107 | 1.006 |
| atanf | trig f32 | 32.79 | 12.34 | 2.657 | 0.014 | 2.413 | slower | 2.660 | 1107 | 0.999 |
| atanf128 | trig f128 | 24690.54 | 1294.22 | 19.071 | 0.012 | 27.192 | slower | 19.117 | 1107 | 0.998 |
| atanh | hyperbolic f64 | 31.25 | 22.77 | 1.371 | 0.036 | 1.245 | slower | 19.173 | 1107 | 0.072 |
| atanhf | hyperbolic f32 | 19.38 | 22.92 | 0.843 | 0.032 | 0.825 | faster | 1.163 | 1107 | 0.725 |
| atanhf128 | hyperbolic f128 | 18035.91 | 2392.32 | 7.539 | 0.009 | 6.908 | slower | 7.544 | 1107 | 0.999 |
| atanhl | hyperbolic long double | 17962.45 | 95.58 | 187.813 | 0.015 | 748.650 | slower | 188.027 | 1107 | 0.999 |
| atanl | trig long double | 24707.80 | 85.77 | 288.070 | 0.003 | 7358.869 | slower | 288.780 | 1107 | 0.998 |
| cbrt | roots/hypot f64 | 56.15 | 24.52 | 2.290 | 0.013 | 0.927 | slower | 2.537 | 1107 | 0.903 |
| cbrtf | roots/hypot f32 | 30.25 | 21.96 | 1.380 | 0.008 | 1.178 | slower | 1.373 | 1107 | 1.005 |
| cbrtf128 | roots/hypot f128 | 4989.51 | 1250.12 | 3.987 | 0.024 | 6.659 | slower | 3.965 | 1107 | 1.006 |
| cbrtl | roots/hypot long double | 4932.83 | 110.82 | 44.488 | 0.039 | 230.777 | slower | 44.387 | 1107 | 1.002 |
| cexp | complex f64 | 189.04 | 33.20 | 5.700 | 0.010 | 3.357 | slower | 844.323 | 1107b | 0.007 |
| cexpf | complex f32 | 57.72 | 57.79 | 0.998 | 0.002 | 1.051 | parity | 497.548 | 1107b | 0.002 |
| cexpl | complex long double | 28080.21 | 460.00 | 61.044 | 0.038 | 100.876 | slower | 61.042 | 1107 | 1.000 |
| clog | complex f64 | 18017.79 | 70.57 | 255.365 | 0.026 | 178.035 | slower | 255.969 | 1107 | 0.998 |
| clogf | complex f32 | 17614.24 | 81.50 | 214.797 | 0.021 | 269.736 | slower | 213.697 | 1107 | 1.005 |
| clogl | complex long double | 43184.21 | 195.04 | 221.229 | 0.019 | 781.917 | slower | 222.574 | 1107 | 0.994 |
| cos | trig f64 | 23.01 | 17.54 | 1.312 | 0.019 | 1.142 | slower | 1.861 | 1107b | 0.705 |
| cosf | trig f32 | 19.75 | 6.36 | 3.105 | 0.038 | 2.023 | slower | 3.122 | 1107 | 0.995 |
| cosf128 | trig f128 | 18892.15 | 1385.68 | 13.634 | 0.091 | 11.360 | slower | 13.640 | 1107 | 1.000 |
| cosh | hyperbolic f64 | 56.45 | 12.16 | 4.643 | 0.002 | 3.036 | slower | 4.770 | 1107 | 0.973 |
| coshf | hyperbolic f32 | 9.37 | 10.83 | 0.865 | 0.019 | 0.902 | faster | 1.819 | 1107 | 0.476 |
| coshf128 | hyperbolic f128 | 9543.32 | 1767.39 | 5.399 | 0.019 | 5.856 | slower | 5.404 | 1107 | 0.999 |
| coshl | hyperbolic long double | 9629.26 | 201.58 | 47.486 | 0.018 | 375.631 | slower | 47.871 | 1107 | 0.992 |
| cosl | trig long double | 154.19 | 220.38 | 0.700 | 0.014 | 0.542 | faster | 0.700 | 1107b | 1.000 |
| cpow | complex f64 | 71387.66 | 127.76 | 560.983 | 0.012 | 518.059 | slower | 561.984 | 1107 | 0.998 |
| cpowf | complex f32 | 71339.65 | 144.68 | 494.174 | 0.027 | 776.588 | slower | 494.705 | 1107 | 0.999 |
| cpowl | complex long double | 71512.92 | 692.32 | 103.280 | 0.026 | 226.494 | slower | 103.156 | 1107 | 1.001 |
| csin | complex f64 | 30876.25 | 75.94 | 404.371 | 0.031 | 325.714 | slower | 406.067 | 1107 | 0.996 |
| csinf | complex f32 | 30050.49 | 99.94 | 303.367 | 0.016 | 399.944 | slower | 304.444 | 1107 | 0.996 |
| csqrt | complex f64 | 5601.17 | 28.95 | 193.477 | 0.052 | 281.696 | slower | 196.265 | 1107 | 0.986 |
| csqrtf | complex f32 | 5651.44 | 57.64 | 97.321 | 0.022 | 329.191 | slower | 98.245 | 1107 | 0.991 |
| csqrtl | complex long double | 5726.63 | 60.16 | 94.506 | 0.016 | 216.258 | slower | 96.769 | 1107 | 0.977 |
| erf | erf f64 | 48.32 | 26.47 | 1.825 | 0.018 | 1.010 | slower | 2.040 | 1107 | 0.895 |
| erfc | erf f64 | 59.70 | 26.50 | 2.253 | 0.007 | 1.247 | slower | 2.388 | 1107 | 0.943 |
| erfcf | erf f32 | 49.25 | 24.32 | 2.028 | 0.031 | 1.382 | slower | 2.010 | 1107 | 1.009 |
| erfcf128 | erf f128 | 42409.76 | 4267.41 | 9.929 | 0.004 | 9.543 | slower | 9.940 | 1107 | 0.999 |
| erfcl | erf long double | 42371.33 | 339.96 | 124.636 | 0.007 | 904.250 | slower | 124.607 | 1107 | 1.000 |
| erff | erf f32 | 44.22 | 24.43 | 1.803 | 0.013 | 1.331 | slower | 1.794 | 1107 | 1.005 |
| erff128 | erf f128 | 42437.69 | 4315.31 | 9.853 | 0.011 | 9.460 | slower | 9.823 | 1107 | 1.003 |
| erfl | erf long double | 42377.58 | 340.31 | 124.515 | 0.014 | 915.427 | slower | 124.250 | 1107 | 1.002 |
| exp | exp/log/pow f64 | 14.00 | 8.12 | 1.728 | 0.095 | 1.314 | slower | 2.169 | 1107b | 0.797 |
| exp10 | exp/log/pow f64 | 20.75 | 7.07 | 2.935 | 0.014 | 1.817 | slower | 5.242 | 1107b | 0.560 |
| exp10f | exp/log/pow f32 | 16.76 | 5.48 | 3.057 | 0.015 | 1.490 | slower | 3.055 | 1107 | 1.001 |
| exp10f128 | exp/log/pow f128 | 8447.34 | 3201.19 | 2.639 | 0.023 | 2.594 | slower | 2.625 | 1107 | 1.005 |
| exp10l | exp/log/pow long double | 357.67 | 176.78 | 2.023 | 0.003 | 14.277 | slower | 2.019 | 1107 | 1.002 |
| exp2 | exp/log/pow f64 | 13.35 | 6.73 | 1.984 | 0.001 | 1.686 | slower | 3.514 | 1107b | 0.565 |
| exp2f | exp/log/pow f32 | 8.50 | 5.08 | 1.673 | 0.005 | 1.486 | slower | 3.130 | 1107b | 0.535 |
| exp2f128 | exp/log/pow f128 | 8382.21 | 1724.02 | 4.865 | 0.014 | 4.680 | slower | 4.826 | 1107 | 1.008 |
| exp2l | exp/log/pow long double | 356.70 | 130.08 | 2.742 | 0.013 | 18.598 | slower | 2.740 | 1107 | 1.001 |
| exp_overflow_errno | special f64 | 38.53 | 12.58 | 3.063 | 0.034 | 2.489 | slower | 3.130 | 1107 | 0.979 |
| exp_qnan | special f64 | 26.52 | 8.41 | 3.130 | 0.018 | 2.340 | slower | 3.139 | 1107 | 0.997 |
| exp_small | special f64 | 14.00 | 8.40 | 1.665 | 0.001 | 1.314 | slower | 3.189 | 1107 | 0.522 |
| exp_tiny | special f64 | 26.42 | 7.58 | 3.484 | 0.028 | 4.857 | slower | 3.537 | 1107 | 0.985 |
| exp_upward | special f64 | 133.58 | 8.40 | 15.902 | 0.116 | 5.468 | slower | 15.880 | 1107 | 1.001 |
| expf | exp/log/pow f32 | 8.50 | 5.12 | 1.662 | 0.010 | 1.410 | slower | 3.103 | 1107b | 0.536 |
| expf128 | exp/log/pow f128 | 8220.15 | 1619.32 | 5.083 | 0.018 | 5.018 | slower | 5.079 | 1107 | 1.001 |
| expf_qnan | special f32 | 5.02 | 5.05 | 0.994 | 0.016 | 1.217 | parity | 4.261 | 1107 | 0.233 |
| expf_upward | special f32 | 8.50 | 5.48 | 1.551 | 0.004 | 1.410 | slower | 17.883 | 1107 | 0.087 |
| expl | exp/log/pow long double | 336.24 | 176.88 | 1.901 | 0.010 | 12.372 | slower | 1.903 | 1107 | 0.999 |
| expm1 | exp/log/pow f64 | 19.76 | 19.72 | 1.002 | 0.015 | 1.275 | parity | 2.101 | 1107b | 0.477 |
| expm1f | exp/log/pow f32 | 22.44 | 21.51 | 1.043 | 0.018 | 1.763 | parity | 1.039 | 1107 | 1.004 |
| expm1f128 | exp/log/pow f128 | 8331.01 | 1961.96 | 4.248 | 0.028 | 3.673 | slower | 4.248 | 1107 | 1.000 |
| expm1l | exp/log/pow long double | 367.49 | 216.62 | 1.696 | 0.003 | 16.726 | slower | 1.689 | 1107 | 1.004 |
| hypot | roots/hypot f64 | 19.02 | 9.48 | 2.008 | 0.085 | 1.679 | slower | 2.160 | 1107 | 0.930 |
| hypotf | roots/hypot f32 | 7.15 | 5.06 | 1.415 | 0.006 | 1.371 | slower | 1.418 | 1107 | 0.998 |
| hypotf128 | roots/hypot f128 | 2288.67 | 1464.10 | 1.560 | 0.013 | 1.427 | slower | 1.564 | 1107 | 0.997 |
| hypotl | roots/hypot long double | 1469.24 | 56.49 | 26.043 | 0.016 | 42.344 | slower | 26.143 | 1107 | 0.996 |
| j0 | bessel f64 | 124.29 | 73.35 | 1.694 | 0.008 | 1.201 | slower | 1.681 | 1107 | 1.008 |
| j0f | bessel f32 | 45.93 | 47.34 | 0.970 | 0.037 | 0.706 | parity | 0.978 | 1107 | 0.992 |
| j0f128 | bessel f128 | 34796.51 | 7514.36 | 4.627 | 0.009 | 3.548 | slower | 4.627 | 1107 | 1.000 |
| j0l | bessel long double | 34790.95 | 594.09 | 58.583 | 0.004 | 50.030 | slower | 58.473 | 1107 | 1.002 |
| j1 | bessel f64 | 122.99 | 74.53 | 1.652 | 0.025 | 1.222 | slower | 1.659 | 1107 | 0.996 |
| j1f | bessel f32 | 46.91 | 47.42 | 0.989 | 0.026 | 0.709 | parity | 0.992 | 1107 | 0.997 |
| j1f128 | bessel f128 | 34729.13 | 7580.47 | 4.582 | 0.006 | 3.520 | slower | 4.572 | 1107 | 1.002 |
| j1l | bessel long double | 34768.68 | 592.48 | 58.690 | 0.009 | 49.957 | slower | 58.491 | 1107 | 1.003 |
| jn | bessel f64 | 34765.78 | 176.94 | 196.317 | 0.008 | 124.645 | slower | 198.239 | 1107 | 0.990 |
| jnf | bessel f32 | 34720.04 | 118.92 | 291.778 | 0.018 | 175.101 | slower | 289.772 | 1107 | 1.007 |
| jnf128 | bessel f128 | 34714.07 | 16429.48 | 2.112 | 0.005 | 1.631 | slower | 2.111 | 1107 | 1.000 |
| jnl | bessel long double | 34770.43 | 1220.60 | 28.497 | 0.006 | 24.292 | slower | 28.375 | 1107 | 1.004 |
| lgamma | gamma f64 | 174.24 | 73.72 | 2.361 | 0.019 | 1.730 | slower | 2.357 | 1107 | 1.002 |
| lgammaf | gamma f32 | 451.39 | 50.45 | 8.989 | 0.019 | 8.078 | slower | 9.084 | 1107 | 0.990 |
| lgammaf128 | gamma f128 | 84380.94 | 12230.85 | 6.899 | 0.013 | 7.895 | slower | 6.915 | 1107 | 0.998 |
| lgammal | gamma long double | 84350.37 | 312.19 | 270.038 | 0.017 | 978.071 | slower | 271.032 | 1107 | 0.996 |
| log | exp/log/pow f64 | 15.39 | 8.41 | 1.830 | 0.092 | 1.413 | slower | 1.931 | 1107b | 0.948 |
| log10 | exp/log/pow f64 | 19.22 | 15.06 | 1.276 | 0.007 | 1.039 | slower | 3.526 | 1107b | 0.362 |
| log10f | exp/log/pow f32 | 18.95 | 11.55 | 1.642 | 0.001 | 0.850 | slower | 1.634 | 1107 | 1.005 |
| log10f128 | exp/log/pow f128 | 18348.09 | 1737.09 | 10.569 | 0.011 | 9.408 | slower | 10.544 | 1107 | 1.002 |
| log10l | exp/log/pow long double | 395.07 | 63.36 | 6.236 | 0.003 | 26.249 | slower | 6.221 | 1107 | 1.002 |
| log1p | exp/log/pow f64 | 21.03 | 15.85 | 1.327 | 0.062 | 1.308 | slower | 3.845 | 1107b | 0.345 |
| log1pf | exp/log/pow f32 | 35.76 | 16.42 | 2.171 | 0.008 | 1.997 | slower | 2.175 | 1107 | 0.998 |
| log1pf128 | exp/log/pow f128 | 16975.64 | 1850.40 | 9.182 | 0.008 | 7.861 | slower | 9.179 | 1107 | 1.000 |
| log1pl | exp/log/pow long double | 412.61 | 65.98 | 6.253 | 0.005 | 31.256 | slower | 6.235 | 1107 | 1.003 |
| log2 | exp/log/pow f64 | 18.67 | 9.04 | 2.066 | 0.003 | 1.699 | slower | 5.570 | 1107b | 0.371 |
| log2f | exp/log/pow f32 | 18.95 | 6.31 | 3.003 | 0.047 | 1.739 | slower | 2.990 | 1107 | 1.004 |
| log2f128 | exp/log/pow f128 | 18348.97 | 1547.38 | 11.859 | 0.007 | 10.477 | slower | 11.860 | 1107 | 1.000 |
| log2l | exp/log/pow long double | 395.37 | 63.35 | 6.242 | 0.005 | 26.217 | slower | 6.224 | 1107 | 1.003 |
| log_upward | special f64 | 141.68 | 9.00 | 15.789 | 0.088 | 5.520 | slower | 16.151 | 1107 | 0.978 |
| logf | exp/log/pow f32 | 19.18 | 5.49 | 3.492 | 0.070 | 1.676 | slower | 3.501 | 1107 | 0.997 |
| logf128 | exp/log/pow f128 | 17521.08 | 1638.27 | 10.662 | 0.015 | 9.066 | slower | 10.690 | 1107 | 0.997 |
| logl | exp/log/pow long double | 381.52 | 63.77 | 5.983 | 0.004 | 23.878 | slower | 5.979 | 1107 | 1.001 |
| pow | exp/log/pow f64 | 40.31 | 27.85 | 1.445 | 0.007 | 1.206 | slower | 3.576 | 1107b | 0.404 |
| pow_upward | special f64 | 248.87 | 27.85 | 8.931 | 0.012 | 6.171 | slower | 8.899 | 1107 | 1.004 |
| powf | exp/log/pow f32 | 60.92 | 12.54 | 4.858 | 0.001 | 3.277 | slower | 4.858 | 1107 | 1.000 |
| powf128 | exp/log/pow f128 | 25003.95 | 4295.31 | 5.828 | 0.007 | 5.207 | slower | 5.834 | 1107 | 0.999 |
| powl | exp/log/pow long double | 24905.94 | 513.75 | 48.534 | 0.010 | 151.221 | slower | 48.542 | 1107 | 1.000 |
| sin | trig f64 | 24.66 | 16.57 | 1.488 | 0.033 | 1.274 | slower | 2.122 | 1107b | 0.701 |
| sin_huge | special f64 | 61.45 | 74.27 | 0.828 | 0.014 | 0.644 | faster | 5.977 | 1107 | 0.139 |
| sin_upward | special f64 | 155.38 | 29.71 | 5.206 | 0.023 | 4.473 | slower | 5.224 | 1107 | 0.997 |
| sinf | trig f32 | 14.02 | 6.05 | 2.317 | 0.088 | 2.029 | slower | 3.282 | 1107 | 0.706 |
| sinf128 | trig f128 | 18990.51 | 1393.92 | 13.618 | 0.039 | 11.361 | slower | 13.999 | 1107 | 0.973 |
| sinf_upward | special f32 | 108.70 | 6.00 | 18.080 | 0.053 | 5.231 | slower | 18.497 | 1107 | 0.977 |
| sinh | hyperbolic f64 | 57.77 | 30.05 | 1.922 | 0.020 | 1.868 | slower | 2.066 | 1107 | 0.930 |
| sinhf | hyperbolic f32 | 14.11 | 30.17 | 0.468 | 0.008 | 0.630 | faster | 0.850 | 1107 | 0.551 |
| sinhf128 | hyperbolic f128 | 9589.23 | 2167.90 | 4.431 | 0.021 | 4.213 | slower | 4.434 | 1107 | 0.999 |
| sinhl | hyperbolic long double | 9482.24 | 238.21 | 39.897 | 0.042 | 333.011 | slower | 39.917 | 1107 | 0.999 |
| sinl | trig long double | 154.25 | 220.61 | 0.700 | 0.016 | 0.541 | faster | 0.695 | 1107b | 1.007 |
| sqrt | roots/hypot f64 | 6.73 | 5.02 | 1.341 | 0.105 | 1.938 | slower | 1.225 | 1107 | 1.095 |
| sqrtf | roots/hypot f32 | 5.70 | 5.02 | 1.135 | 0.002 | 1.821 | slower | 1.135 | 1107 | 1.000 |
| sqrtf128 | roots/hypot f128 | 2973.45 | 659.58 | 4.479 | 0.073 | 3.902 | slower | 4.503 | 1107 | 0.995 |
| sqrtl | roots/hypot long double | 45.41 | 20.50 | 2.215 | 0.062 | 2.763 | slower | 2.216 | 1107 | 1.000 |
| tan | trig f64 | 34.47 | 18.54 | 1.861 | 0.009 | 1.430 | slower | 2.640 | 1107b | 0.705 |
| tanf | trig f32 | 29.48 | 24.05 | 1.226 | 0.004 | 0.877 | slower | 1.242 | 1107 | 0.987 |
| tanf128 | trig f128 | 19788.42 | 1670.80 | 11.861 | 0.041 | 10.754 | slower | 11.849 | 1107 | 1.001 |
| tanh | hyperbolic f64 | 76.70 | 26.55 | 2.889 | 0.025 | 2.208 | slower | 2.985 | 1107 | 0.968 |
| tanhf | hyperbolic f32 | 14.27 | 26.41 | 0.540 | 0.025 | 0.723 | faster | 0.953 | 1107 | 0.567 |
| tanhf128 | hyperbolic f128 | 10495.43 | 2137.54 | 4.913 | 0.013 | 4.936 | slower | 4.962 | 1107 | 0.990 |
| tanhl | hyperbolic long double | 10391.59 | 232.63 | 44.628 | 0.046 | 421.845 | slower | 44.886 | 1107 | 0.994 |
| tanl | trig long double | 19762.92 | 219.25 | 90.300 | 0.040 | 82.235 | slower | 90.153 | 1107 | 1.002 |
| tgamma | gamma f64 | 227.79 | 131.08 | 1.736 | 0.012 | 1.232 | slower | 1.738 | 1107 | 0.999 |
| tgammaf | gamma f32 | 52.66 | 88.79 | 0.593 | 0.010 | 0.477 | faster | 0.596 | 1107 | 0.995 |
| tgammaf128 | gamma f128 | 92697.79 | 7126.02 | 13.000 | 0.024 | 14.797 | slower | 13.043 | 1107 | 0.997 |
| tgammal | gamma long double | 92399.80 | 749.45 | 123.218 | 0.027 | 610.685 | slower | 123.293 | 1107 | 0.999 |
| y0 | bessel f64 | 126.06 | 77.34 | 1.626 | 0.019 | 1.146 | slower | 1.640 | 1107 | 0.991 |
| y0f | bessel f32 | 52.85 | 49.78 | 1.062 | 0.020 | 0.732 | slower | 1.060 | 1107 | 1.002 |
| y0f128 | bessel f128 | 108323.72 | 7928.70 | 13.668 | 0.010 | 13.058 | slower | 13.624 | 1107 | 1.003 |
| y0l | bessel long double | 108558.89 | 649.21 | 167.240 | 0.010 | 191.094 | slower | 166.767 | 1107 | 1.003 |
| y1 | bessel f64 | 125.76 | 77.41 | 1.621 | 0.007 | 1.146 | slower | 1.640 | 1107 | 0.988 |
| y1f | bessel f32 | 52.88 | 49.87 | 1.060 | 0.026 | 0.731 | slower | 1.051 | 1107 | 1.009 |
| y1f128 | bessel f128 | 108252.65 | 8000.84 | 13.483 | 0.019 | 12.970 | slower | 13.520 | 1107 | 0.997 |
| y1l | bessel long double | 108616.82 | 648.89 | 167.152 | 0.009 | 191.013 | slower | 167.639 | 1107 | 0.997 |
| yn | bessel f64 | 109120.94 | 175.34 | 622.544 | 0.016 | 484.585 | slower | 628.331 | 1107 | 0.991 |
| ynf | bessel f32 | 109009.44 | 120.21 | 906.900 | 0.010 | 681.538 | slower | 912.498 | 1107 | 0.994 |
| ynf128 | bessel f128 | 109533.88 | 16062.46 | 6.820 | 0.010 | 6.441 | slower | 6.816 | 1107 | 1.001 |
| ynl | bessel long double | 109100.13 | 1304.55 | 83.550 | 0.009 | 94.128 | slower | 83.581 | 1107 | 1.000 |

### N100: cbench_hot (37 rows)

| row | family | Liella ns | glibc ns | ratio | spread | instr | verdict | before | before src | change |
|---|---|---:|---:|---:|---:|---:|---|---:|---|---:|
| ctype_b_loc | cbench_hot ctype | 1.26 | 3.35 | 0.376 | 0.000 | 1.198 | faster | 0.376 | 1106e | 1.000 |
| ctype_get_mb_cur_max | cbench_hot ctype | 1.67 | 3.35 | 0.499 | 0.000 | 0.908 | faster | 0.499 | 1106e | 1.000 |
| errno_location | cbench_hot errno | 1.67 | 3.35 | 0.499 | 0.000 | 0.899 | faster | 0.499 | 1106e | 1.000 |
| fwrite_unlocked_16 | cbench_hot fwrite | 7.70 | 12.81 | 0.601 | 0.001 | 0.729 | faster | 0.601 | 1106e | 1.000 |
| fwrite_unlocked_64 | cbench_hot fwrite | 9.99 | 15.69 | 0.637 | 0.001 | 0.757 | faster | 0.637 | 1106e | 1.000 |
| getc | cbench_hot getc | 3.01 | 4.68 | 0.643 | 0.000 | 0.815 | faster | 0.643 | 1106e | 1.000 |
| getc_unlocked | cbench_hot getc | 1.45 | 3.43 | 0.423 | 0.000 | 0.999 | faster | 0.423 | 1106e | 1.000 |
| getenv_hit_last | cbench_hot getenv | 108.81 | 166.80 | 0.652 | 0.005 | 0.806 | faster | 0.678 | 1106e | 0.962 |
| getenv_hit_mid | cbench_hot getenv | 64.23 | 84.13 | 0.763 | 0.000 | 0.846 | faster | 0.773 | 1106e | 0.987 |
| getenv_miss | cbench_hot getenv | 70.83 | 154.52 | 0.458 | 0.006 | 0.461 | faster | 0.374 | 1106e | 1.225 |
| memcpy_chk_64 | cbench_hot memcpy | 2.93 | 4.60 | 0.637 | 0.000 | 0.931 | faster | 0.637 | 1106e | 1.000 |
| mempcpy_16 | cbench_hot mempcpy | 2.51 | 4.60 | 0.546 | 0.000 | 0.954 | faster | 0.546 | 1106e | 1.000 |
| mempcpy_64 | cbench_hot mempcpy | 2.93 | 4.18 | 0.701 | 0.000 | 1.000 | faster | 0.701 | 1106e | 1.000 |
| memrchr_4k | cbench_hot memrchr | 79.81 | 76.91 | 1.038 | 0.070 | 0.901 | parity | 1.037 | 1106e | 1.001 |
| memrchr_64 | cbench_hot memrchr | 3.35 | 5.02 | 0.667 | 0.000 | 0.846 | faster | 0.667 | 1106e | 1.000 |
| memrchr_64_hit | cbench_hot memrchr | 2.93 | 3.77 | 0.777 | 0.000 | 1.000 | faster | 0.777 | 1106e | 1.000 |
| mutex_lock_unlock_normal | cbench_hot mutex | 5.86 | 8.79 | 0.667 | 0.000 | 0.847 | faster | 0.667 | 1106e | 1.000 |
| mutex_lock_unlock_normal_mt | cbench_hot mutex | 22.75 | 23.09 | 0.985 | 0.001 | 0.825 | parity | 0.985 | 1106e | 1.000 |
| mutex_lock_unlock_recursive | cbench_hot mutex | 7.22 | 11.72 | 0.616 | 0.001 | 0.878 | faster | 0.616 | 1106e | 1.000 |
| mutex_lock_unlock_recursive_mt | cbench_hot mutex | 20.90 | 29.68 | 0.704 | 0.031 | 0.849 | faster | 0.704 | 1106e | 1.000 |
| putc_unlocked | cbench_hot putc | 1.58 | 3.39 | 0.466 | 0.000 | 0.897 | faster | 0.466 | 1106e | 1.000 |
| strchr_16_hit | cbench_hot strchr | 2.93 | 3.77 | 0.777 | 0.000 | 0.965 | faster | 0.777 | 1106e | 1.000 |
| strchr_16_miss | cbench_hot strchr | 3.35 | 4.60 | 0.728 | 0.000 | 0.965 | faster | 0.728 | 1106e | 1.000 |
| strchr_64_hit | cbench_hot strchr | 4.18 | 4.60 | 0.909 | 0.000 | 0.921 | faster | 0.909 | 1106e | 1.000 |
| strchr_64_miss | cbench_hot strchr | 4.18 | 4.60 | 0.909 | 0.000 | 0.921 | faster | 0.909 | 1106e | 1.000 |
| strchrnul_16_hit | cbench_hot strchrnul | 2.93 | 4.18 | 0.701 | 0.000 | 0.926 | faster | 0.701 | 1106e | 1.000 |
| strchrnul_16_miss | cbench_hot strchrnul | 2.93 | 4.18 | 0.701 | 0.000 | 0.926 | faster | 0.701 | 1106e | 1.000 |
| strchrnul_64_hit | cbench_hot strchrnul | 3.77 | 4.60 | 0.820 | 0.000 | 0.889 | faster | 0.820 | 1106e | 1.000 |
| strchrnul_64_miss | cbench_hot strchrnul | 3.77 | 5.02 | 0.751 | 0.000 | 0.889 | faster | 0.751 | 1106e | 1.000 |
| strcpy_chk_16 | cbench_hot strcpy | 3.14 | 7.11 | 0.442 | 0.106 | 0.590 | faster | 0.442 | 1106e | 1.000 |
| strcpy_chk_64 | cbench_hot strcpy | 4.18 | 8.37 | 0.499 | 0.122 | 0.586 | faster | 0.512 | 1106e | 0.975 |
| strncmp_diff8 | cbench_hot strncmp | 4.18 | 4.18 | 1.000 | 0.000 | 1.029 | parity | 1.000 | 1106e | 1.000 |
| strncmp_eq16 | cbench_hot strncmp | 4.60 | 4.18 | 1.100 | 0.000 | 1.059 | slower | 1.000 | 1106e | 1.100 |
| strncmp_names64 | cbench_hot strncmp | 4.60 | 5.44 | 0.846 | 0.000 | 0.977 | faster | 0.846 | 1106e | 1.000 |
| sys_close_ebadf | cbench_hot sys | 243.29 | 242.56 | 1.003 | 0.002 | 1.066 | parity | 1.003 | 1106e | 1.000 |
| sys_fstat | cbench_hot sys | 476.03 | 480.32 | 0.991 | 0.016 | 1.008 | parity | 1.001 | 1106e | 0.990 |
| sys_read_1 | cbench_hot sys | 374.92 | 373.67 | 1.003 | 0.000 | 1.012 | parity | 1.003 | 1106e | 1.000 |

### N100: cbench_locale (36 rows)

| row | family | Liella ns | glibc ns | ratio | spread | instr | verdict | before | before src | change |
|---|---|---:|---:|---:|---:|---:|---|---:|---|---:|
| fnmatch_casefold_en_32 | cbench_locale fnmatch | 647.29 | 830.53 | 0.779 | 0.004 | 0.795 | faster | 0.984 | 1106e | 0.792 |
| fnmatch_en_us | cbench_locale fnmatch | 38.85 | 133.16 | 0.292 | 0.006 | 0.293 | faster | 0.323 | 1106e | 0.904 |
| iconv_utf8_8859_15_4k | cbench_locale iconv | 6630.68 | 18991.09 | 0.349 | 0.002 | 0.321 | faster | 0.349 | 1106e | 1.000 |
| iconv_utf8_ascii_translit_4k | cbench_locale iconv | 25637.50 | 45375.53 | 0.565 | 0.004 | 0.410 | faster | 0.563 | 1106e | 1.004 |
| iconv_utf8_utf16_4k | cbench_locale iconv | 7912.31 | 10687.20 | 0.741 | 0.007 | 0.485 | faster | 0.743 | 1106e | 0.997 |
| isalpha_c | cbench_locale isalpha | 2.09 | 3.77 | 0.554 | 0.000 | 1.436 | faster | 0.554 | 1106e | 1.000 |
| iswalpha_en_us | cbench_locale iswalpha | 5.88 | 5.92 | 0.993 | 0.003 | 1.163 | parity | 0.992 | 1107b | 1.001 |
| iswalpha_l_en_us | cbench_locale iswalpha | 7.35 | 5.02 | 1.464 | 0.000 | 1.404 | slower | 1.464 | 1107b | 1.000 |
| iswalpha_utf8 | cbench_locale iswalpha | 3.77 | 5.91 | 0.638 | 0.003 | 0.633 | faster | 1.061 | 1107b | 0.601 |
| iswctype_en_us | cbench_locale iswctype | 7.62 | 4.18 | 1.823 | 0.000 | 1.651 | slower | 1.821 | 1107b | 1.001 |
| localeconv_en_us | cbench_locale localeconv | 7.11 | 13.67 | 0.520 | 0.000 | 0.491 | faster | 0.520 | 1106e | 1.000 |
| mbrtowc_en_us_3 | cbench_locale mbrtowc | 20.10 | 23.96 | 0.839 | 0.001 | 0.889 | faster | 0.838 | 1106e | 1.001 |
| mbrtowc_utf8_1 | cbench_locale mbrtowc | 4.60 | 20.14 | 0.228 | 0.004 | 0.202 | faster | 0.228 | 1106e | 1.000 |
| mbrtowc_utf8_2 | cbench_locale mbrtowc | 5.86 | 21.88 | 0.268 | 0.003 | 0.241 | faster | 0.268 | 1106e | 1.000 |
| mbrtowc_utf8_3 | cbench_locale mbrtowc | 20.54 | 23.96 | 0.857 | 0.002 | 0.889 | faster | 0.856 | 1106e | 1.001 |
| newlocale_free | cbench_locale newlocale | 1793.20 | 5523.92 | 0.323 | 0.022 | 0.291 | faster | 0.321 | 1106e | 1.006 |
| nl_langinfo_en_us | cbench_locale nl | 2.93 | 4.18 | 0.701 | 0.000 | 0.839 | faster | 0.701 | 1106e | 1.000 |
| nl_langinfo_l_en_us | cbench_locale nl | 3.35 | 3.77 | 0.889 | 0.000 | 1.250 | faster | 0.801 | 1106e | 1.110 |
| snprintf_f_en_us | cbench_locale snprintf | 124.29 | 308.69 | 0.403 | 0.015 | 0.433 | faster | 0.414 | 1106e | 0.973 |
| strcasecmp_c_16 | cbench_locale strcasecmp | 6.28 | 5.86 | 1.072 | 0.000 | 1.060 | slower | 1.066 | 1106e | 1.006 |
| strcasecmp_c_256 | cbench_locale strcasecmp | 65.27 | 42.15 | 1.549 | 0.001 | 1.122 | slower | 0.804 | 1106e | 1.927 |
| strcasecmp_c_256_odd | cbench_locale strcasecmp | 57.74 | 42.25 | 1.367 | 0.001 | 1.087 | slower | - | - | - |
| strcasecmp_c_256_pageend | cbench_locale strcasecmp | 60.01 | 31.01 | 1.935 | 0.004 | 1.371 | slower | - | - | - |
| strcasecmp_c_256_xpage | cbench_locale strcasecmp | 69.69 | 42.10 | 1.655 | 0.009 | 1.263 | slower | - | - | - |
| strcasecmp_en_256 | cbench_locale strcasecmp | 65.28 | 42.15 | 1.550 | 0.003 | 1.122 | slower | 0.803 | 1106e | 1.930 |
| strcasecmp_utf8_256 | cbench_locale strcasecmp | 65.28 | 42.11 | 1.550 | 0.000 | 1.122 | slower | 0.803 | 1106e | 1.930 |
| strcoll_utf8_16 | cbench_locale strcoll | 5.02 | 6.69 | 0.750 | 0.001 | 0.896 | faster | 0.866 | 1106e | 0.866 |
| strftime_names_en_us | cbench_locale strftime | 39.33 | 42.45 | 0.927 | 0.037 | 1.011 | faster | 0.936 | 1106e | 0.990 |
| strncasecmp_c_64 | cbench_locale strncasecmp | 9.62 | 8.37 | 1.149 | 0.000 | 1.147 | slower | 1.050 | 1106e | 1.094 |
| strtod_c | cbench_locale strtod | 28.03 | 114.17 | 0.246 | 0.005 | 0.317 | faster | 0.245 | 1106e | 1.004 |
| strtod_en_us | cbench_locale strtod | 28.03 | 114.48 | 0.245 | 0.001 | 0.317 | faster | 0.246 | 1106e | 0.996 |
| strtod_utf8 | cbench_locale strtod | 28.03 | 114.42 | 0.245 | 0.006 | 0.317 | faster | 0.245 | 1106e | 1.000 |
| toupper_c | cbench_locale toupper | 2.09 | 3.77 | 0.554 | 0.000 | 1.049 | faster | 0.554 | 1106e | 1.000 |
| towupper_l_en_us | cbench_locale towupper | 4.60 | 4.18 | 1.100 | 0.000 | 1.000 | slower | 1.100 | 1107b | 1.000 |
| towupper_utf8 | cbench_locale towupper | 5.44 | 5.34 | 1.019 | 0.007 | 1.325 | parity | 1.017 | 1107b | 1.002 |
| wcwidth_utf8 | cbench_locale wcwidth | 4.39 | 5.02 | 0.875 | 0.000 | 1.163 | faster | 1.000 | 1106e | 0.875 |

### N100: cbench_memref (96 rows)

| row | family | Liella ns | glibc ns | ratio | spread | instr | verdict | before | before src | change |
|---|---|---:|---:|---:|---:|---:|---|---:|---|---:|
| __memcmpeq_eq_128 | cbench_memref __memcmpeq | 4.61 | 5.01 | 0.918 | 0.012 | 1.058 | faster | 0.918 | 1107b | 1.000 |
| __memcmpeq_eq_16 | cbench_memref __memcmpeq | 2.51 | 4.60 | 0.546 | 0.011 | 0.767 | faster | 0.546 | 1107b | 1.000 |
| __memcmpeq_eq_1k | cbench_memref __memcmpeq | 28.41 | 27.99 | 1.015 | 0.000 | 0.924 | parity | 1.015 | 1107b | 1.000 |
| __memcmpeq_eq_32 | cbench_memref __memcmpeq | 2.93 | 4.18 | 0.701 | 0.007 | 1.153 | faster | 0.701 | 1107b | 1.000 |
| __memcmpeq_eq_64 | cbench_memref __memcmpeq | 2.94 | 4.18 | 0.703 | 0.007 | 0.960 | faster | 0.703 | 1107b | 1.000 |
| __memcmpeq_eq_8 | cbench_memref __memcmpeq | 2.51 | 4.18 | 0.600 | 0.007 | 0.854 | faster | 0.603 | 1107b | 0.995 |
| __memcmpeq_head_128 | cbench_memref __memcmpeq | 2.93 | 4.59 | 0.638 | 0.013 | 1.199 | faster | 0.638 | 1107b | 1.000 |
| __memcmpeq_head_16 | cbench_memref __memcmpeq | 2.51 | 4.59 | 0.547 | 0.011 | 0.670 | faster | 0.547 | 1107b | 1.000 |
| __memcmpeq_head_1k | cbench_memref __memcmpeq | 2.93 | 4.63 | 0.633 | 0.015 | 1.199 | faster | 0.638 | 1107b | 0.992 |
| __memcmpeq_head_32 | cbench_memref __memcmpeq | 2.51 | 4.59 | 0.547 | 0.009 | 0.903 | faster | 0.547 | 1107b | 1.000 |
| __memcmpeq_head_64 | cbench_memref __memcmpeq | 2.93 | 4.59 | 0.638 | 0.011 | 0.804 | faster | 0.638 | 1107b | 1.000 |
| __memcmpeq_head_8 | cbench_memref __memcmpeq | 2.51 | 4.59 | 0.547 | 0.009 | 0.670 | faster | 0.547 | 1107b | 1.000 |
| __memcmpeq_tail_128 | cbench_memref __memcmpeq | 4.61 | 5.43 | 0.847 | 0.006 | 1.054 | faster | 0.849 | 1107b | 0.998 |
| __memcmpeq_tail_16 | cbench_memref __memcmpeq | 2.10 | 4.59 | 0.458 | 0.011 | 0.854 | faster | 0.458 | 1107b | 1.000 |
| __memcmpeq_tail_1k | cbench_memref __memcmpeq | 5.44 | 27.99 | 0.194 | 0.000 | 0.252 | faster | 0.194 | 1107b | 1.000 |
| __memcmpeq_tail_32 | cbench_memref __memcmpeq | 2.73 | 4.59 | 0.595 | 0.011 | 1.496 | faster | 0.595 | 1107b | 1.000 |
| __memcmpeq_tail_64 | cbench_memref __memcmpeq | 2.94 | 4.59 | 0.641 | 0.009 | 0.963 | faster | 0.641 | 1107b | 1.000 |
| __memcmpeq_tail_8 | cbench_memref __memcmpeq | 2.10 | 4.59 | 0.458 | 0.013 | 0.670 | faster | 0.458 | 1107b | 1.000 |
| bcmp_eq_128 | cbench_memref bcmp | 4.61 | 5.43 | 0.849 | 0.009 | 0.977 | faster | 0.849 | 1107b | 1.000 |
| bcmp_eq_16 | cbench_memref bcmp | 2.51 | 5.43 | 0.462 | 0.009 | 0.659 | faster | 0.462 | 1107b | 1.000 |
| bcmp_eq_1k | cbench_memref bcmp | 28.41 | 27.99 | 1.015 | 0.000 | 0.919 | parity | 1.015 | 1107b | 1.000 |
| bcmp_eq_32 | cbench_memref bcmp | 2.93 | 4.60 | 0.637 | 0.011 | 1.111 | faster | 0.638 | 1107b | 0.998 |
| bcmp_eq_64 | cbench_memref bcmp | 2.94 | 4.59 | 0.641 | 0.015 | 0.925 | faster | 0.641 | 1107b | 1.000 |
| bcmp_eq_8 | cbench_memref bcmp | 2.51 | 4.59 | 0.547 | 0.011 | 0.796 | faster | 0.547 | 1107b | 1.000 |
| bcmp_head_128 | cbench_memref bcmp | 2.93 | 4.59 | 0.638 | 0.013 | 0.998 | faster | 0.638 | 1107b | 1.000 |
| bcmp_head_16 | cbench_memref bcmp | 2.10 | 5.02 | 0.418 | 0.010 | 0.566 | faster | 0.419 | 1107b | 0.998 |
| bcmp_head_1k | cbench_memref bcmp | 2.93 | 4.59 | 0.638 | 0.009 | 1.001 | faster | 0.638 | 1107b | 1.000 |
| bcmp_head_32 | cbench_memref bcmp | 2.10 | 4.59 | 0.458 | 0.013 | 0.751 | faster | 0.458 | 1107b | 1.000 |
| bcmp_head_64 | cbench_memref bcmp | 2.51 | 4.59 | 0.547 | 0.011 | 0.671 | faster | 0.547 | 1107b | 1.000 |
| bcmp_head_8 | cbench_memref bcmp | 2.10 | 5.02 | 0.418 | 0.010 | 0.567 | faster | 0.419 | 1107b | 0.998 |
| bcmp_tail_128 | cbench_memref bcmp | 4.61 | 5.43 | 0.847 | 0.011 | 0.865 | faster | 0.849 | 1107b | 0.998 |
| bcmp_tail_16 | cbench_memref bcmp | 2.10 | 4.59 | 0.458 | 0.011 | 0.720 | faster | 0.458 | 1107b | 1.000 |
| bcmp_tail_1k | cbench_memref bcmp | 5.44 | 28.41 | 0.191 | 0.000 | 0.244 | faster | 0.191 | 1107b | 1.000 |
| bcmp_tail_32 | cbench_memref bcmp | 2.73 | 4.18 | 0.653 | 0.009 | 1.252 | faster | 0.653 | 1107b | 1.000 |
| bcmp_tail_64 | cbench_memref bcmp | 2.94 | 4.60 | 0.639 | 0.009 | 0.782 | faster | 0.639 | 1107b | 1.000 |
| bcmp_tail_8 | cbench_memref bcmp | 2.10 | 4.59 | 0.458 | 0.006 | 0.566 | faster | 0.458 | 1107b | 1.000 |
| memchr_16_mix | cbench_memref memchr | 3.35 | 5.43 | 0.617 | 0.000 | 0.942 | faster | 0.617 | 1106e | 1.000 |
| memchr_4k | cbench_memref memchr | 74.98 | 76.19 | 0.984 | 0.001 | 0.783 | parity | 0.988 | 1106e | 0.996 |
| memchr_64 | cbench_memref memchr | 3.76 | 5.43 | 0.692 | 0.008 | 0.726 | faster | 0.541 | 1106e | 1.279 |
| memchr_64_mix | cbench_memref memchr | 3.77 | 8.15 | 0.463 | 0.017 | 0.694 | faster | 0.464 | 1106e | 0.998 |
| memchr_64k | cbench_memref memchr | 1367.45 | 1279.00 | 1.073 | 0.031 | 0.771 | slower | 1.066 | 1106e | 1.007 |
| memcmp_128 | cbench_memref memcmp | 4.19 | 5.43 | 0.772 | 0.000 | 0.924 | faster | 0.772 | 1107b | 1.000 |
| memcmp_144 | cbench_memref memcmp | 5.02 | 7.10 | 0.707 | 0.014 | 0.739 | faster | 0.707 | 1107b | 1.000 |
| memcmp_16 | cbench_memref memcmp | 2.38 | 4.59 | 0.519 | 0.000 | 0.932 | faster | 0.519 | 1107b | 1.000 |
| memcmp_16_mix | cbench_memref memcmp | 3.77 | 6.76 | 0.558 | 0.030 | 0.940 | faster | 0.554 | 1107b | 1.007 |
| memcmp_256 | cbench_memref memcmp | 7.53 | 7.10 | 1.061 | 0.000 | 0.932 | slower | 1.061 | 1107b | 1.000 |
| memcmp_4k | cbench_memref memcmp | 109.23 | 107.34 | 1.018 | 0.001 | 0.877 | parity | 1.018 | 1107b | 1.000 |
| memcmp_64 | cbench_memref memcmp | 3.35 | 4.59 | 0.730 | 0.000 | 1.001 | faster | 0.730 | 1107b | 1.000 |
| memcmp_64_mix | cbench_memref memcmp | 5.44 | 5.45 | 0.998 | 0.000 | 1.001 | parity | 0.998 | 1107b | 1.000 |
| memcmp_64k | cbench_memref memcmp | 1982.95 | 2388.70 | 0.830 | 0.008 | 0.943 | faster | 0.832 | 1107b | 0.998 |
| memcmp_diff8_127 | cbench_memref memcmp | 3.35 | 4.18 | 0.801 | 0.002 | 1.203 | faster | 0.801 | 1107b | 1.000 |
| memcmp_diff8_64 | cbench_memref memcmp | 3.35 | 4.18 | 0.801 | 0.012 | 1.249 | faster | 0.801 | 1107b | 1.000 |
| memcmp_diff8_96 | cbench_memref memcmp | 3.35 | 4.18 | 0.801 | 0.007 | 1.207 | faster | 0.801 | 1107b | 1.000 |
| memcpy_128 | cbench_memref memcpy | 3.77 | 5.01 | 0.752 | 0.000 | 0.968 | faster | 0.752 | 1106e | 1.000 |
| memcpy_144 | cbench_memref memcpy | 5.03 | 5.44 | 0.925 | 0.002 | 0.921 | faster | 0.925 | 1106e | 1.000 |
| memcpy_16 | cbench_memref memcpy | 2.11 | 4.18 | 0.505 | 0.070 | 0.949 | faster | 0.404 | 1106e | 1.250 |
| memcpy_16_mix | cbench_memref memcpy | 17.54 | 17.31 | 1.014 | 0.021 | 1.001 | parity | 1.010 | 1106e | 1.004 |
| memcpy_256 | cbench_memref memcpy | 5.03 | 5.44 | 0.925 | 0.002 | 0.922 | faster | 0.925 | 1106e | 1.000 |
| memcpy_4k | cbench_memref memcpy | 73.92 | 127.40 | 0.580 | 0.176 | 10.288 | faster | 0.551 | 1106e | 1.053 |
| memcpy_4k_off | cbench_memref memcpy | 147.02 | 140.35 | 1.048 | 0.000 | 1.004 | parity | 1.048 | 1106e | 1.000 |
| memcpy_64 | cbench_memref memcpy | 2.51 | 3.76 | 0.668 | 0.012 | 1.001 | faster | 0.559 | 1106e | 1.195 |
| memcpy_64_mix | cbench_memref memcpy | 17.80 | 17.36 | 1.023 | 0.019 | 1.001 | parity | 1.030 | 1106e | 0.993 |
| memcpy_64k | cbench_memref memcpy | 3013.16 | 3051.56 | 0.988 | 0.004 | 95.491 | parity | 0.986 | 1106e | 1.002 |
| memmove_128 | cbench_memref memmove | 7.74 | 8.36 | 0.926 | 0.001 | 0.967 | faster | 0.901 | 1106e | 1.028 |
| memmove_144 | cbench_memref memmove | 11.49 | 13.37 | 0.859 | 0.001 | 0.922 | faster | 0.859 | 1106e | 1.000 |
| memmove_16 | cbench_memref memmove | 6.28 | 6.69 | 0.939 | 0.002 | 0.950 | faster | 0.925 | 1106e | 1.015 |
| memmove_256 | cbench_memref memmove | 11.08 | 11.70 | 0.947 | 0.000 | 0.922 | faster | 0.947 | 1106e | 1.000 |
| memmove_4k | cbench_memref memmove | 84.79 | 86.04 | 0.985 | 0.033 | 1.036 | parity | 0.971 | 1106e | 1.014 |
| memmove_64 | cbench_memref memmove | 6.27 | 6.69 | 0.937 | 0.002 | 1.001 | faster | 0.937 | 1106e | 1.000 |
| memmove_overlap64_4k | cbench_memref memmove | 67.68 | 126.04 | 0.537 | 0.006 | 10.170 | faster | 0.520 | 1106e | 1.033 |
| memmove_overlap8_4k | cbench_memref memmove | 86.17 | 87.92 | 0.986 | 0.017 | 0.998 | parity | 0.992 | 1106e | 0.994 |
| memset_128 | cbench_memref memset | 3.35 | 4.18 | 0.801 | 0.063 | 0.962 | faster | 0.801 | 1106e | 1.000 |
| memset_16 | cbench_memref memset | 1.89 | 4.18 | 0.452 | 0.002 | 1.053 | faster | 0.452 | 1106e | 1.000 |
| memset_16_mix | cbench_memref memset | 17.04 | 17.13 | 0.995 | 0.001 | 1.001 | parity | 0.995 | 1106e | 1.000 |
| memset_4k | cbench_memref memset | 126.57 | 126.56 | 1.000 | 0.000 | 1.035 | parity | 1.000 | 1106e | 1.000 |
| memset_4k_off | cbench_memref memset | 130.01 | 130.00 | 1.000 | 0.000 | 1.001 | parity | 1.000 | 1106e | 1.000 |
| memset_64 | cbench_memref memset | 2.10 | 3.76 | 0.559 | 0.010 | 1.001 | faster | 0.559 | 1106e | 1.000 |
| memset_64_mix | cbench_memref memset | 17.04 | 17.04 | 1.000 | 0.000 | 1.001 | parity | 1.000 | 1106e | 1.000 |
| memset_64k | cbench_memref memset | 2089.62 | 2078.82 | 1.000 | 0.051 | 1.036 | parity | 1.007 | 1106e | 0.993 |
| rawmemchr_16_mix | cbench_memref rawmemchr | 2.94 | 4.59 | 0.641 | 0.002 | 0.935 | faster | 0.639 | 1106e | 1.003 |
| rawmemchr_4k | cbench_memref rawmemchr | 81.24 | 78.53 | 1.035 | 0.157 | 0.864 | parity | 1.035 | 1106e | 1.000 |
| rawmemchr_64 | cbench_memref rawmemchr | 3.76 | 4.60 | 0.819 | 0.008 | 0.941 | faster | 0.728 | 1106e | 1.125 |
| rawmemchr_64_mix | cbench_memref rawmemchr | 4.19 | 5.53 | 0.758 | 0.000 | 0.952 | faster | 0.758 | 1106e | 1.000 |
| rawmemchr_64k | cbench_memref rawmemchr | 1414.39 | 1341.63 | 1.055 | 0.029 | 0.822 | slower | 1.064 | 1106e | 0.992 |
| strcmp_16_mix | cbench_memref strcmp | 6.39 | 5.99 | 1.063 | 0.042 | 1.135 | slower | 0.947 | 1106e | 1.122 |
| strcmp_64_mix | cbench_memref strcmp | 8.66 | 8.88 | 0.974 | 0.300 | 1.169 | parity | 0.992 | 1106e | 0.982 |
| strlen_16_mix | cbench_memref strlen | 2.94 | 4.18 | 0.701 | 0.005 | 0.930 | faster | 0.701 | 1106e | 1.000 |
| strlen_4k | cbench_memref strlen | 56.39 | 55.97 | 1.008 | 0.264 | 0.909 | parity | 1.008 | 1106e | 1.000 |
| strlen_64 | cbench_memref strlen | 3.35 | 4.59 | 0.730 | 0.011 | 0.941 | faster | 0.728 | 1106e | 1.003 |
| strlen_64_mix | cbench_memref strlen | 4.19 | 5.11 | 0.820 | 0.000 | 0.951 | faster | 0.820 | 1106e | 1.000 |
| strlen_64k | cbench_memref strlen | 1194.98 | 1186.84 | 1.011 | 0.056 | 0.901 | parity | 0.985 | 1106e | 1.026 |
| strnlen_16_mix | cbench_memref strnlen | 3.77 | 5.01 | 0.752 | 0.000 | 1.061 | faster | 0.752 | 1106e | 1.000 |
| strnlen_4k | cbench_memref strnlen | 56.44 | 55.13 | 1.024 | 0.001 | 0.946 | parity | 1.024 | 1106e | 1.000 |
| strnlen_64 | cbench_memref strnlen | 5.02 | 5.85 | 0.858 | 0.020 | 0.854 | faster | 0.856 | 1106e | 1.002 |
| strnlen_64_mix | cbench_memref strnlen | 5.64 | 6.80 | 0.831 | 0.002 | 0.873 | faster | 0.829 | 1106e | 1.002 |
| strnlen_64k | cbench_memref strnlen | 1202.75 | 1165.92 | 1.029 | 0.023 | 0.919 | parity | 1.028 | 1106e | 1.001 |

## Per-family medians (median of the per-row time ratios)

Before = median of the before column over the rows that have one (n = that count); after = median over all rows measured now, and over the same rows as before.

| family | Ivy Bridge rows | Ivy Bridge after | Ivy Bridge before (n) | Ivy Bridge after, same rows | N100 rows | N100 after | N100 before (n) | N100 after, same rows |
|---|---:|---:|---|---:|---:|---:|---|---:|
| bessel f128 | 6 | 8.130 | 8.160 (6) | 8.130 | 6 | 5.723 | 5.721 (6) | 5.723 |
| bessel f32 | 6 | 1.185 | 1.181 (6) | 1.185 | 6 | 1.061 | 1.055 (6) | 1.061 |
| bessel f64 | 6 | 1.298 | 1.300 (6) | 1.298 | 6 | 1.673 | 1.670 (6) | 1.673 |
| bessel long double | 6 | 95.410 | 95.399 (6) | 95.410 | 6 | 71.120 | 71.036 (6) | 71.120 |
| complex f32 | 5 | 240.536 | 322.069 (5) | 240.536 | 5 | 214.797 | 304.444 (5) | 214.797 |
| complex f64 | 5 | 213.039 | 366.469 (5) | 213.039 | 5 | 255.365 | 406.067 (5) | 255.365 |
| complex long double | 4 | 132.367 | 135.636 (4) | 132.367 | 4 | 98.893 | 99.963 (4) | 98.893 |
| erf f128 | 2 | 13.857 | 13.876 (2) | 13.857 | 2 | 9.891 | 9.881 (2) | 9.891 |
| erf f32 | 2 | 1.727 | 1.730 (2) | 1.727 | 2 | 1.915 | 1.902 (2) | 1.915 |
| erf f64 | 2 | 2.287 | 2.365 (2) | 2.287 | 2 | 2.039 | 2.214 (2) | 2.039 |
| erf long double | 2 | 297.544 | 298.651 (2) | 297.544 | 2 | 124.576 | 124.428 (2) | 124.576 |
| exp/log/pow f128 | 9 | 8.419 | 8.422 (9) | 8.419 | 9 | 5.828 | 5.834 (9) | 5.828 |
| exp/log/pow f32 | 9 | 2.085 | 2.519 (9) | 2.085 | 9 | 2.171 | 3.055 (9) | 2.171 |
| exp/log/pow f64 | 9 | 1.489 | 3.609 (9) | 1.489 | 9 | 1.728 | 3.526 (9) | 1.728 |
| exp/log/pow long double | 9 | 10.000 | 10.973 (9) | 10.000 | 9 | 5.983 | 5.979 (9) | 5.983 |
| gamma f128 | 2 | 14.290 | 14.337 (2) | 14.290 | 2 | 9.950 | 9.979 (2) | 9.950 |
| gamma f32 | 2 | 4.881 | 4.878 (2) | 4.881 | 2 | 4.791 | 4.840 (2) | 4.791 |
| gamma f64 | 2 | 1.645 | 1.639 (2) | 1.645 | 2 | 2.049 | 2.048 (2) | 2.049 |
| gamma long double | 2 | 350.462 | 350.759 (2) | 350.462 | 2 | 196.628 | 197.162 (2) | 196.628 |
| hyperbolic f128 | 6 | 8.331 | 8.351 (6) | 8.331 | 6 | 6.165 | 6.170 (6) | 6.165 |
| hyperbolic f32 | 6 | 0.950 | 1.097 (6) | 0.950 | 6 | 0.854 | 1.491 (6) | 0.854 |
| hyperbolic f64 | 6 | 1.603 | 9.014 (6) | 1.603 | 6 | 1.938 | 11.971 (6) | 1.938 |
| hyperbolic long double | 6 | 227.762 | 227.868 (6) | 227.762 | 6 | 117.649 | 117.949 (6) | 117.649 |
| roots/hypot f128 | 3 | 1.719 | 1.728 (3) | 1.719 | 3 | 3.987 | 3.965 (3) | 3.987 |
| roots/hypot f32 | 3 | 1.720 | 1.724 (3) | 1.720 | 3 | 1.380 | 1.373 (3) | 1.380 |
| roots/hypot f64 | 3 | 2.195 | 2.333 (3) | 2.195 | 3 | 2.008 | 2.160 (3) | 2.008 |
| roots/hypot long double | 3 | 18.701 | 18.770 (3) | 18.701 | 3 | 26.043 | 26.143 (3) | 26.043 |
| special f32 | 3 | 1.364 | 14.503 (3) | 1.364 | 3 | 1.551 | 17.883 (3) | 1.551 |
| special f64 | 9 | 3.213 | 4.575 (9) | 3.213 | 9 | 3.484 | 5.224 (9) | 3.484 |
| trig f128 | 7 | 19.935 | 19.937 (7) | 19.935 | 7 | 13.618 | 13.640 (7) | 13.618 |
| trig f32 | 7 | 2.286 | 2.280 (7) | 2.286 | 7 | 2.657 | 3.122 (7) | 2.657 |
| trig f64 | 7 | 2.231 | 2.591 (7) | 2.231 | 7 | 1.861 | 2.640 (7) | 1.861 |
| trig long double | 7 | 395.005 | 395.671 (7) | 395.005 | 7 | 264.000 | 265.302 (7) | 264.000 |
| cbench_hot ctype | 2 | 0.929 | 0.929 (2) | 0.929 | 2 | 0.438 | 0.438 (2) | 0.438 |
| cbench_hot errno | 1 | 1.004 | 1.000 (1) | 1.004 | 1 | 0.499 | 0.499 (1) | 0.499 |
| cbench_hot fwrite | 2 | 0.713 | 0.715 (2) | 0.713 | 2 | 0.619 | 0.619 (2) | 0.619 |
| cbench_hot getc | 2 | 0.942 | 0.941 (2) | 0.942 | 2 | 0.533 | 0.533 (2) | 0.533 |
| cbench_hot getenv | 3 | 0.581 | 0.572 (3) | 0.581 | 3 | 0.652 | 0.678 (3) | 0.652 |
| cbench_hot memcpy | 1 | 0.799 | 0.799 (1) | 0.799 | 1 | 0.637 | 0.637 (1) | 0.637 |
| cbench_hot mempcpy | 2 | 0.899 | 0.899 (2) | 0.899 | 2 | 0.623 | 0.623 (2) | 0.623 |
| cbench_hot memrchr | 3 | 1.062 | 1.000 (3) | 1.062 | 3 | 0.777 | 0.777 (3) | 0.777 |
| cbench_hot mutex | 4 | 0.885 | 0.886 (4) | 0.885 | 4 | 0.685 | 0.685 (4) | 0.685 |
| cbench_hot putc | 1 | 0.862 | 0.862 (1) | 0.862 | 1 | 0.466 | 0.466 (1) | 0.466 |
| cbench_hot strchr | 4 | 1.096 | 1.126 (4) | 1.096 | 4 | 0.843 | 0.843 (4) | 0.843 |
| cbench_hot strchrnul | 4 | 1.086 | 1.152 (4) | 1.086 | 4 | 0.726 | 0.726 (4) | 0.726 |
| cbench_hot strcpy | 2 | 0.616 | 0.599 (2) | 0.616 | 2 | 0.471 | 0.477 (2) | 0.471 |
| cbench_hot strncmp | 3 | 1.051 | 1.000 (3) | 1.051 | 3 | 1.000 | 1.000 (3) | 1.000 |
| cbench_hot sys | 3 | 0.990 | 1.005 (3) | 0.990 | 3 | 1.003 | 1.003 (3) | 1.003 |
| cbench_locale fnmatch | 2 | 0.462 | 0.558 (2) | 0.462 | 2 | 0.535 | 0.653 (2) | 0.535 |
| cbench_locale iconv | 3 | 0.493 | 0.492 (3) | 0.493 | 3 | 0.565 | 0.563 (3) | 0.565 |
| cbench_locale isalpha | 1 | 0.960 | 1.000 (1) | 0.960 | 1 | 0.554 | 0.554 (1) | 0.554 |
| cbench_locale iswalpha | 3 | 0.897 | 0.913 (3) | 0.897 | 3 | 0.993 | 1.061 (3) | 0.993 |
| cbench_locale iswctype | 1 | 1.978 | 1.987 (1) | 1.978 | 1 | 1.823 | 1.821 (1) | 1.823 |
| cbench_locale localeconv | 1 | 0.531 | 0.532 (1) | 0.531 | 1 | 0.520 | 0.520 (1) | 0.520 |
| cbench_locale mbrtowc | 4 | 0.592 | 0.592 (4) | 0.592 | 4 | 0.553 | 0.553 (4) | 0.553 |
| cbench_locale newlocale | 1 | 0.331 | 0.337 (1) | 0.331 | 1 | 0.323 | 0.321 (1) | 0.323 |
| cbench_locale nl | 2 | 1.299 | 1.161 (2) | 1.299 | 2 | 0.795 | 0.751 (2) | 0.795 |
| cbench_locale snprintf | 1 | 0.289 | 0.301 (1) | 0.289 | 1 | 0.403 | 0.414 (1) | 0.403 |
| cbench_locale strcasecmp | 7 | 0.942 | 0.976 (4) | 0.941 | 7 | 1.550 | 0.804 (4) | 1.550 |
| cbench_locale strcoll | 1 | 0.637 | 0.729 (1) | 0.637 | 1 | 0.750 | 0.866 (1) | 0.750 |
| cbench_locale strftime | 1 | 0.940 | 0.954 (1) | 0.940 | 1 | 0.927 | 0.936 (1) | 0.927 |
| cbench_locale strncasecmp | 1 | 1.003 | 0.950 (1) | 1.003 | 1 | 1.149 | 1.050 (1) | 1.149 |
| cbench_locale strtod | 3 | 0.229 | 0.222 (3) | 0.229 | 3 | 0.245 | 0.245 (3) | 0.245 |
| cbench_locale toupper | 1 | 0.931 | 1.000 (1) | 0.931 | 1 | 0.554 | 0.554 (1) | 0.554 |
| cbench_locale towupper | 2 | 0.988 | 0.987 (2) | 0.988 | 2 | 1.059 | 1.058 (2) | 1.059 |
| cbench_locale wcwidth | 1 | 0.789 | 0.791 (1) | 0.789 | 1 | 0.875 | 1.000 (1) | 0.875 |
| cbench_memref __memcmpeq | 18 | 0.967 | 0.944 (18) | 0.967 | 18 | 0.617 | 0.621 (18) | 0.617 |
| cbench_memref bcmp | 18 | 0.847 | 0.847 (18) | 0.847 | 18 | 0.592 | 0.593 (18) | 0.592 |
| cbench_memref memchr | 5 | 0.897 | 0.949 (5) | 0.897 | 5 | 0.692 | 0.617 (5) | 0.692 |
| cbench_memref memcmp | 12 | 0.994 | 0.993 (12) | 0.994 | 12 | 0.801 | 0.801 (12) | 0.801 |
| cbench_memref memcpy | 10 | 0.919 | 0.927 (10) | 0.919 | 10 | 0.925 | 0.925 (10) | 0.925 |
| cbench_memref memmove | 8 | 1.001 | 1.000 (8) | 1.001 | 8 | 0.938 | 0.931 (8) | 0.938 |
| cbench_memref memset | 8 | 0.926 | 0.998 (8) | 0.926 | 8 | 0.998 | 0.998 (8) | 0.998 |
| cbench_memref rawmemchr | 5 | 1.000 | 0.941 (5) | 1.000 | 5 | 0.819 | 0.758 (5) | 0.819 |
| cbench_memref strcmp | 2 | 0.525 | 0.476 (2) | 0.525 | 2 | 1.018 | 0.970 (2) | 1.018 |
| cbench_memref strlen | 5 | 0.873 | 0.888 (5) | 0.873 | 5 | 0.820 | 0.820 (5) | 0.820 |
| cbench_memref strnlen | 5 | 0.869 | 0.869 (5) | 0.869 | 5 | 0.858 | 0.856 (5) | 0.858 |

## Ivy Bridge: ten largest ratios

| row | ratio | Liella ns | glibc ns | spread | before |
|---|---:|---:|---:|---:|---:|
| cbench_libm/asinl | 817.210 | 31822.58 | 38.83 | 0.014 | 817.355 |
| cbench_libm/ynf | 760.435 | 134870.51 | 177.25 | 0.533 | 763.086 |
| cbench_libm/yn | 556.835 | 135071.35 | 242.57 | 0.366 | 554.980 |
| cbench_libm/cpowf | 551.613 | 85909.11 | 156.04 | 0.281 | 555.264 |
| cbench_libm/cpow | 497.606 | 85974.83 | 172.79 | 0.283 | 498.311 |
| cbench_libm/atanl | 457.334 | 29371.41 | 64.13 | 0.258 | 457.419 |
| cbench_libm/lgammal | 450.262 | 100523.38 | 223.39 | 0.001 | 450.873 |
| cbench_libm/atan2l | 447.898 | 30551.15 | 68.17 | 0.135 | 449.396 |
| cbench_libm/asinhl | 407.273 | 23368.63 | 57.34 | 0.008 | 410.470 |
| cbench_libm/acosl | 395.005 | 32586.00 | 82.39 | 0.563 | 395.671 |

## Ivy Bridge: rows slower than their before figure (now/before > 1.05): 20

| row | before | src | now | now/before | spread now |
|---|---:|---|---:|---:|---:|
| cbench_libm/exp_qnan | 0.854 | 1107 | 1.481 | 1.734 | 0.455 |
| cbench_locale/nl_langinfo_l_en_us | 1.221 | 1106e | 1.497 | 1.226 | 0.000 |
| cbench_memref/strnlen_4k | 0.662 | 1106e | 0.797 | 1.204 | 0.001 |
| cbench_memref/memcpy_16 | 0.777 | 1106e | 0.887 | 1.142 | 0.000 |
| cbench_memref/strcmp_64_mix | 0.561 | 1106e | 0.633 | 1.128 | 0.022 |
| cbench_hot/strncmp_names64 | 0.968 | 1106e | 1.080 | 1.116 | 0.002 |
| cbench_memref/memcpy_64 | 0.642 | 1106e | 0.713 | 1.111 | 0.000 |
| cbench_hot/memrchr_64_hit | 1.277 | 1106e | 1.393 | 1.091 | 0.000 |
| cbench_memref/memchr_4k | 0.786 | 1106e | 0.847 | 1.078 | 0.022 |
| cbench_memref/memcpy_128 | 0.998 | 1106e | 1.074 | 1.076 | 0.000 |
| cbench_memref/strcmp_16_mix | 0.391 | 1106e | 0.417 | 1.066 | 0.016 |
| cbench_hot/getenv_miss | 0.513 | 1106e | 0.546 | 1.064 | 0.005 |
| cbench_memref/rawmemchr_64 | 0.941 | 1106e | 1.000 | 1.063 | 0.000 |
| cbench_hot/memrchr_64 | 1.000 | 1106e | 1.062 | 1.062 | 0.001 |
| cbench_hot/strcpy_chk_64 | 0.599 | 1106e | 0.633 | 1.057 | 0.002 |
| cbench_memref/strlen_64_mix | 0.564 | 1106e | 0.596 | 1.057 | 0.002 |
| cbench_locale/strncasecmp_c_64 | 0.950 | 1106e | 1.003 | 1.056 | 0.002 |
| cbench_memref/strlen_64 | 0.789 | 1106e | 0.832 | 1.054 | 0.003 |
| cbench_memref/memchr_64 | 1.117 | 1106e | 1.176 | 1.053 | 0.000 |
| cbench_hot/strncmp_eq16 | 1.000 | 1106e | 1.051 | 1.051 | 0.219 |

## Ivy Bridge: the nine reopen rows

| row | instr | ratio | spread | verdict | before |
|---|---:|---:|---:|---|---:|
| coshf | 0.860 | 1.017 | 0.142 | parity | 1.210 |
| log10f | 0.786 | 1.075 | 0.563 | slower | 1.213 |
| tanf | 0.878 | 1.322 | 0.142 | slower | 1.313 |
| y0 | 1.024 | 1.237 | 0.007 | slower | 1.232 |
| y1 | 1.024 | 1.247 | 0.138 | slower | 1.241 |
| j0f | 0.658 | 1.106 | 0.007 | slower | 1.100 |
| j1f | 0.662 | 1.097 | 0.007 | slower | 1.101 |
| y0f | 0.683 | 1.167 | 0.017 | slower | 1.173 |
| y1f | 0.681 | 1.204 | 0.140 | slower | 1.190 |

Ivy Bridge: 335 rows, median ratio 1.134; faster 102 / parity 45 / slower 188

## N100: ten largest ratios

| row | ratio | Liella ns | glibc ns | spread | before |
|---|---:|---:|---:|---:|---:|
| cbench_libm/ynf | 906.900 | 109009.44 | 120.21 | 0.010 | 912.498 |
| cbench_libm/yn | 622.544 | 109120.94 | 175.34 | 0.016 | 628.331 |
| cbench_libm/cpow | 560.983 | 71387.66 | 127.76 | 0.012 | 561.984 |
| cbench_libm/cpowf | 494.174 | 71339.65 | 144.68 | 0.027 | 494.705 |
| cbench_libm/asinl | 477.789 | 26775.28 | 56.13 | 0.011 | 475.766 |
| cbench_libm/csin | 404.371 | 30876.25 | 75.94 | 0.031 | 406.067 |
| cbench_libm/csinf | 303.367 | 30050.49 | 99.94 | 0.016 | 304.444 |
| cbench_libm/atan2l | 293.075 | 25597.17 | 87.34 | 0.019 | 291.352 |
| cbench_libm/jnf | 291.778 | 34720.04 | 118.92 | 0.018 | 289.772 |
| cbench_libm/atanl | 288.070 | 24707.80 | 85.77 | 0.003 | 288.780 |

## N100: rows slower than their before figure (now/before > 1.05): 14

| row | before | src | now | now/before | spread now |
|---|---:|---|---:|---:|---:|
| cbench_locale/strcasecmp_en_256 | 0.803 | 1106e | 1.550 | 1.930 | 0.003 |
| cbench_locale/strcasecmp_utf8_256 | 0.803 | 1106e | 1.550 | 1.930 | 0.000 |
| cbench_locale/strcasecmp_c_256 | 0.804 | 1106e | 1.549 | 1.927 | 0.001 |
| cbench_memref/memchr_64 | 0.541 | 1106e | 0.692 | 1.279 | 0.008 |
| cbench_memref/memcpy_16 | 0.404 | 1106e | 0.505 | 1.250 | 0.070 |
| cbench_hot/getenv_miss | 0.374 | 1106e | 0.458 | 1.225 | 0.006 |
| cbench_memref/memcpy_64 | 0.559 | 1106e | 0.668 | 1.195 | 0.012 |
| cbench_memref/rawmemchr_64 | 0.728 | 1106e | 0.819 | 1.125 | 0.008 |
| cbench_memref/strcmp_16_mix | 0.947 | 1106e | 1.063 | 1.122 | 0.042 |
| cbench_locale/nl_langinfo_l_en_us | 0.801 | 1106e | 0.889 | 1.110 | 0.000 |
| cbench_hot/strncmp_eq16 | 1.000 | 1106e | 1.100 | 1.100 | 0.000 |
| cbench_libm/sqrt | 1.225 | 1107 | 1.341 | 1.095 | 0.105 |
| cbench_locale/strncasecmp_c_64 | 1.050 | 1106e | 1.149 | 1.094 | 0.000 |
| cbench_memref/memcpy_4k | 0.551 | 1106e | 0.580 | 1.053 | 0.176 |

## N100: the nine reopen rows

| row | instr | ratio | spread | verdict | before |
|---|---:|---:|---:|---|---:|
| coshf | 0.902 | 0.865 | 0.019 | faster | 1.819 |
| log10f | 0.850 | 1.642 | 0.001 | slower | 1.634 |
| tanf | 0.877 | 1.226 | 0.004 | slower | 1.242 |
| y0 | 1.146 | 1.626 | 0.019 | slower | 1.640 |
| y1 | 1.146 | 1.621 | 0.007 | slower | 1.640 |
| j0f | 0.706 | 0.970 | 0.037 | parity | 0.978 |
| j1f | 0.709 | 0.989 | 0.026 | parity | 0.992 |
| y0f | 0.732 | 1.062 | 0.020 | slower | 1.060 |
| y1f | 0.731 | 1.060 | 0.026 | slower | 1.051 |

N100: 335 rows, median ratio 1.055; faster 131 / parity 36 / slower 168

