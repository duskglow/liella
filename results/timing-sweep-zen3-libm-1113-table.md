# Timing sweep Zen 3 libm (1113): every libm row on Zen 3, beside the 11-07 Zen 3 libm sweep

Build: origin/next bc146cce (bc146ccec5f057f5801902059728eed988c5528b). Staged on a build host with `cargo xtask bench
--stage <tar> --only cbench_libm` (tarball sha256 3fe7b8ab17f8202772a6af5e8b4d3ff338aeb0ce63b7d3b83b4f007992b86587,
checked on arrival), unpacked whole on the timing host and run unchanged with the sweep tool (docs/benchmarks.md
section 6), selected as the 11-07 libm sweep was: `./xtask-bin sweep run --out <dir> --only cbench_libm`. It runs
`bench --no-build --staged --counters --only cbench_libm`: Liella and glibc on the same host, rounds interleaved.
Exit 0, `sweep: COMPLETE: cpu AMD Ryzen 5 5500; all 176 selected ratio_host rows measured (28 faster, 7 parity,
141 slower); bench rc 0; 0 rows flagged by the limit check; 3 counted rows outside the selection`.

CPU class: Zen 3 (AMD Ryzen 5 5500 class: 6 cores / 12 threads, AVX2, no AVX-512), host glibc 2.39, the same machine
as results/timing-sweep-zen3-1107-table.md. The comparison below is therefore the same CPU class and host but a
different commit (11-07: next 66b8d3ba): it measures what changed in Liella's libm (and in the bench rows) between
the two commits, with glibc unchanged as the reference in both.

Measurement conditions (details in the sweep's handoff note, named in the analyzer's header):
- governor performance on all 12 CPUs, scaling max frequency 3.6 GHz (the base clock; the cap keeps the CPU from
  boosting); no swap active (11-07: a swap device present, 0 bytes used); periodic system timers not masked, none
  due during the run. The run was booked and held every slot of the host's timing queue; the host's timing-intruder
  log has no entry for this run.
- No `ratio_host` limits exist for this CPU model in bench/host_limits.tsv: rows are measured and reported, not gated.
- Run-to-run spread median 0.007 (11-07: 0.006). Every row settled at 3 adaptive rounds.

Columns: `instr` / `cycles` = Liella/glibc instructions / cycles per call (counters of this run); `time` = median
Liella/glibc time ratio over the interleaved rounds; `spread` = (max - min) / median of the round ratios; verdict
faster (< 0.95), parity, slower (> 1.05), as the sweep tool judges them; `time 11-07` / `verdict 11-07` from
results/timing-sweep-zen3-1107-table.md; `change` = time / time 11-07. The 11-07 table counted the 3 baseline rows
outside the ratio_host selection among its 169 judged rows; the summary gives both views, each sweep counted the same
way. Analyzer: handoff/timing-sweep-zen3-libm-1113-analyze.py.

Reading the change: the median libm time ratio fell from 6.19 to 1.98 and the geomean from 9.76 to 3.25; 21 rows
moved from slower to faster or parity. The slowest rows (complex, long double, erf/gamma families) are where they
were on 11-07 (change 0.95-1.05), except clogl (0.34). Rows whose time ratio grew by more than 5 % are in the
all-rows table (`change` > 1.05). For cexpl (1.128), cpowl (1.092), j0 (1.058), tgammal (1.140) and y1 (1.134) the
instruction ratio is unchanged within 0.5 % against 11-07 and the growth is larger than either run's spread: a time-only
change on this class between the two runs (one run each, 3 rounds), to recheck before reading it as a regression.
y1f (1.053, now slower; 11-07 parity) also moved in instructions (0.731 -> 0.822).

## Zen 3, libm rows: summary

| sweep | rows judged | faster | parity | slower | time median | time geomean | spread median | spread p90 | spread max | instr median (counted rows) |
|---|---|---|---|---|---|---|---|---|---|---|
| 11-13, next bc146cce | 179 | 28 | 10 | 141 | 1.981 | 3.252 | 0.007 | 0.036 | 0.409 | 1.668 (179) |
| 11-07, next 66b8d3ba | 169 | 5 | 8 | 156 | 6.189 | 9.756 | 0.006 | - | - | - |

Without the 3 baseline rows outside the ratio_host selection (add_baseline, mxcsr_pair, mxcsr_wrapper_seq): 11-13 176 rows, 28 / 7 / 141 (the sweep tool's own count); 11-07 166 rows, 5 / 5 / 156.
Rows flagged by the bench's per-host limit check: 0 (no limits exist for this CPU model).
Judged rows whose verdict lies inside their own spread (spread > 0.10 and |time - 1| < spread): 0

## Change against the 11-07 Zen 3 libm sweep (same CPU class, different commit)

Rows in both sweeps: 169; only in 11-13: 10 (cbench_libm/clog_axis, cbench_libm/clog_near1, cbench_libm/clogf_axis, cbench_libm/clogf_near1, cbench_libm/csqrt_axis, cbench_libm/csqrtf_axis, cbench_libm/sincos, cbench_libm/sincosf, cbench_libm/sincosf128, cbench_libm/sincosl); only in 11-07: 0.
Change = 11-13 time ratio / 11-07 time ratio (< 1: Liella moved closer to or further ahead of glibc). Median 0.888, geomean 0.357, p10 0.011, p90 1.008; rows with change < 0.95: 90, 0.95-1.05: 72, > 1.05: 7.

| 11-07 verdict -> 11-13 verdict | rows |
|---|---|
| faster -> faster | 4 |
| faster -> parity | 1 |
| parity -> faster | 2 |
| parity -> parity | 4 |
| parity -> slower | 2 |
| slower -> faster | 19 |
| slower -> parity | 4 |
| slower -> slower | 133 |

### Zen 3, libm rows: slowest 20 rows (11-13)

| bench/row | time | instr | cycles | spread | rounds | time 11-07 | change |
|---|---|---|---|---|---|---|---|
| cbench_libm/cpow | 580.676 | 518.028 | 578.249 | 0.016 | 3 | 578.476 | 1.004 |
| cbench_libm/cpowf | 428.030 | 776.835 | 428.545 | 0.002 | 3 | 437.637 | 0.978 |
| cbench_libm/csin | 344.893 | 325.669 | 345.276 | 0.002 | 3 | 344.763 | 1.000 |
| cbench_libm/csinf | 241.139 | 399.766 | 241.305 | 0.004 | 3 | 241.355 | 0.999 |
| cbench_libm/erfl | 211.939 | 922.378 | 211.791 | 0.295 | 3 | 212.440 | 0.998 |
| cbench_libm/erfcl | 210.054 | 911.389 | 209.799 | 0.286 | 3 | 209.996 | 1.000 |
| cbench_libm/asinhl | 203.721 | 1175.981 | 206.123 | 0.409 | 3 | 204.722 | 0.995 |
| cbench_libm/acoshl | 170.649 | 959.791 | 171.596 | 0.009 | 3 | 171.363 | 0.996 |
| cbench_libm/lgammal | 162.897 | 971.530 | 163.779 | 0.036 | 3 | 171.672 | 0.949 |
| cbench_libm/tanhl | 138.593 | 426.322 | 138.802 | 0.366 | 3 | 138.009 | 1.004 |
| cbench_libm/atanhl | 123.046 | 747.244 | 124.309 | 0.008 | 3 | 123.901 | 0.993 |
| cbench_libm/sinhl | 112.634 | 336.134 | 112.959 | 0.001 | 3 | 112.068 | 1.005 |
| cbench_libm/tgammal | 109.961 | 607.550 | 109.153 | 0.109 | 3 | 96.442 | 1.140 |
| cbench_libm/y0l | 99.694 | 190.803 | 99.440 | 0.018 | 3 | 101.616 | 0.981 |
| cbench_libm/y1l | 99.621 | 190.739 | 99.255 | 0.017 | 3 | 101.552 | 0.981 |
| cbench_libm/coshl | 76.412 | 377.832 | 76.029 | 0.004 | 3 | 75.914 | 1.007 |
| cbench_libm/cpowl | 74.837 | 225.879 | 75.566 | 0.022 | 3 | 68.522 | 1.092 |
| cbench_libm/clogl | 58.914 | 241.293 | 61.584 | 0.062 | 3 | 171.524 | 0.343 |
| cbench_libm/ynl | 50.405 | 93.975 | 50.326 | 0.013 | 3 | 50.966 | 0.989 |
| cbench_libm/csqrtl | 49.743 | 215.782 | 54.227 | 0.113 | 3 | 53.872 | 0.923 |

### Zen 3, libm rows: all rows

| bench/row | instr | cycles | time | spread | rounds | verdict | time 11-07 | verdict 11-07 | change |
|---|---|---|---|---|---|---|---|---|---|
| cbench_libm/acos | 2.208 | 6.177 | 6.470 | 0.005 | 3 | slower | 7.283 | slower | 0.888 |
| cbench_libm/acosf | 1.146 | 0.987 | 0.947 | 0.036 | 3 | faster | 5.491 | slower | 0.172 |
| cbench_libm/acosf128 | 0.138 | 0.177 | 0.176 | 0.010 | 3 | faster | 16.952 | slower | 0.010 |
| cbench_libm/acosh | 1.467 | 1.869 | 1.868 | 0.012 | 3 | slower | 28.920 | slower | 0.065 |
| cbench_libm/acoshf | 1.278 | 1.788 | 1.789 | 0.002 | 3 | slower | 2.093 | slower | 0.855 |
| cbench_libm/acoshf128 | 7.834 | 7.546 | 7.630 | 0.013 | 3 | slower | 7.568 | slower | 1.008 |
| cbench_libm/acoshl | 959.791 | 171.596 | 170.649 | 0.009 | 3 | slower | 171.363 | slower | 0.996 |
| cbench_libm/acosl | 15.401 | 2.066 | 2.051 | 0.011 | 3 | slower | 274.910 | slower | 0.007 |
| cbench_libm/add_baseline | 1.000 | 1.001 | 1.000 | 0.000 | 3 | parity | 1.000 | parity | 1.000 |
| cbench_libm/asin | 2.382 | 7.057 | 7.127 | 0.002 | 3 | slower | 8.380 | slower | 0.850 |
| cbench_libm/asinf | 1.294 | 1.126 | 1.088 | 0.044 | 3 | slower | 6.133 | slower | 0.177 |
| cbench_libm/asinf128 | 0.141 | 0.180 | 0.178 | 0.009 | 3 | faster | 17.228 | slower | 0.010 |
| cbench_libm/asinh | 1.345 | 1.569 | 1.582 | 0.013 | 3 | slower | 25.754 | slower | 0.061 |
| cbench_libm/asinhf | 1.123 | 1.395 | 1.394 | 0.003 | 3 | slower | 2.377 | slower | 0.586 |
| cbench_libm/asinhf128 | 7.477 | 7.090 | 7.145 | 0.009 | 3 | slower | 7.097 | slower | 1.007 |
| cbench_libm/asinhl | 1175.981 | 206.123 | 203.721 | 0.409 | 3 | slower | 204.722 | slower | 0.995 |
| cbench_libm/asinl | 6.377 | 3.220 | 3.207 | 0.013 | 3 | slower | 437.520 | slower | 0.007 |
| cbench_libm/atan | 1.741 | 2.108 | 2.079 | 0.014 | 3 | slower | 2.366 | slower | 0.879 |
| cbench_libm/atan2 | 1.020 | 1.394 | 1.386 | 0.009 | 3 | slower | 1.531 | slower | 0.905 |
| cbench_libm/atan2f | 0.544 | 0.703 | 0.687 | 0.022 | 3 | faster | 1.656 | slower | 0.415 |
| cbench_libm/atan2f128 | 0.190 | 0.233 | 0.231 | 0.010 | 3 | faster | 23.084 | slower | 0.010 |
| cbench_libm/atan2l | 16.499 | 2.163 | 2.206 | 0.020 | 3 | slower | 290.139 | slower | 0.008 |
| cbench_libm/atanf | 0.981 | 0.953 | 0.954 | 0.002 | 3 | parity | 3.295 | slower | 0.290 |
| cbench_libm/atanf128 | 0.172 | 0.223 | 0.220 | 0.016 | 3 | faster | 24.534 | slower | 0.009 |
| cbench_libm/atanh | 1.244 | 1.408 | 1.401 | 0.014 | 3 | slower | 22.351 | slower | 0.063 |
| cbench_libm/atanhf | 0.824 | 0.861 | 0.855 | 0.008 | 3 | faster | 1.025 | parity | 0.834 |
| cbench_libm/atanhf128 | 6.908 | 7.637 | 7.638 | 0.009 | 3 | slower | 7.644 | slower | 0.999 |
| cbench_libm/atanhl | 747.244 | 124.309 | 123.046 | 0.008 | 3 | slower | 123.901 | slower | 0.993 |
| cbench_libm/atanl | 26.170 | 1.924 | 1.963 | 0.021 | 3 | slower | 293.399 | slower | 0.007 |
| cbench_libm/cbrt | 0.926 | 1.844 | 1.845 | 0.002 | 3 | slower | 2.030 | slower | 0.909 |
| cbench_libm/cbrtf | 1.179 | 1.713 | 1.711 | 0.004 | 3 | slower | 1.719 | slower | 0.995 |
| cbench_libm/cbrtf128 | 6.659 | 5.208 | 5.224 | 0.005 | 3 | slower | 5.216 | slower | 1.002 |
| cbench_libm/cbrtl | 230.790 | 44.917 | 44.623 | 0.004 | 3 | slower | 44.642 | slower | 1.000 |
| cbench_libm/cexp | 1.565 | 2.446 | 2.429 | 0.018 | 3 | slower | 714.404 | slower | 0.003 |
| cbench_libm/cexpf | 1.082 | 1.193 | 1.195 | 0.001 | 3 | slower | 376.413 | slower | 0.003 |
| cbench_libm/cexpl | 100.579 | 40.706 | 39.681 | 0.035 | 3 | slower | 35.169 | slower | 1.128 |
| cbench_libm/clog | 1.269 | 1.717 | 1.725 | 0.011 | 3 | slower | 215.938 | slower | 0.008 |
| cbench_libm/clog_axis | 1.338 | 2.075 | 2.070 | 0.004 | 3 | slower | - | - | - |
| cbench_libm/clog_near1 | 0.691 | 0.835 | 0.830 | 0.008 | 3 | faster | - | - | - |
| cbench_libm/clogf | 1.003 | 1.056 | 1.054 | 0.001 | 3 | slower | 191.509 | slower | 0.006 |
| cbench_libm/clogf_axis | 1.053 | 1.090 | 1.088 | 0.000 | 3 | slower | - | - | - |
| cbench_libm/clogf_near1 | 0.839 | 0.974 | 0.974 | 0.000 | 3 | parity | - | - | - |
| cbench_libm/clogl | 241.293 | 61.584 | 58.914 | 0.062 | 3 | slower | 171.524 | slower | 0.343 |
| cbench_libm/cos | 1.142 | 1.399 | 1.400 | 0.001 | 3 | slower | 2.061 | slower | 0.679 |
| cbench_libm/cosf | 2.212 | 2.240 | 2.243 | 0.002 | 3 | slower | 3.089 | slower | 0.726 |
| cbench_libm/cosf128 | 0.178 | 0.178 | 0.178 | 0.013 | 3 | faster | 11.985 | slower | 0.015 |
| cbench_libm/cosh | 1.668 | 1.915 | 1.892 | 0.013 | 3 | slower | 4.789 | slower | 0.395 |
| cbench_libm/coshf | 0.902 | 1.374 | 1.376 | 0.001 | 3 | slower | 1.811 | slower | 0.760 |
| cbench_libm/coshf128 | 5.858 | 5.596 | 5.613 | 0.008 | 3 | slower | 5.565 | slower | 1.009 |
| cbench_libm/coshl | 377.832 | 76.029 | 76.412 | 0.004 | 3 | slower | 75.914 | slower | 1.007 |
| cbench_libm/cosl | 0.577 | 0.550 | 0.549 | 0.007 | 3 | faster | 56.844 | slower | 0.010 |
| cbench_libm/cpow | 518.028 | 578.249 | 580.676 | 0.016 | 3 | slower | 578.476 | slower | 1.004 |
| cbench_libm/cpowf | 776.835 | 428.545 | 428.030 | 0.002 | 3 | slower | 437.637 | slower | 0.978 |
| cbench_libm/cpowl | 225.879 | 75.566 | 74.837 | 0.022 | 3 | slower | 68.522 | slower | 1.092 |
| cbench_libm/csin | 325.669 | 345.276 | 344.893 | 0.002 | 3 | slower | 344.763 | slower | 1.000 |
| cbench_libm/csinf | 399.766 | 241.305 | 241.139 | 0.004 | 3 | slower | 241.355 | slower | 0.999 |
| cbench_libm/csqrt | 2.103 | 3.547 | 3.546 | 0.002 | 3 | slower | 195.019 | slower | 0.018 |
| cbench_libm/csqrt_axis | 1.380 | 1.948 | 1.950 | 0.002 | 3 | slower | - | - | - |
| cbench_libm/csqrtf | 0.835 | 1.072 | 1.068 | 0.001 | 3 | slower | 90.208 | slower | 0.012 |
| cbench_libm/csqrtf_axis | 1.311 | 1.436 | 1.435 | 0.002 | 3 | slower | - | - | - |
| cbench_libm/csqrtl | 215.782 | 54.227 | 49.743 | 0.113 | 3 | slower | 53.872 | slower | 0.923 |
| cbench_libm/erf | 0.739 | 0.828 | 0.834 | 0.005 | 3 | faster | 2.074 | slower | 0.402 |
| cbench_libm/erfc | 1.067 | 1.755 | 1.723 | 0.029 | 3 | slower | 2.485 | slower | 0.693 |
| cbench_libm/erfcf | 1.302 | 1.801 | 1.796 | 0.017 | 3 | slower | 1.813 | slower | 0.991 |
| cbench_libm/erfcf128 | 9.544 | 9.453 | 9.463 | 0.002 | 3 | slower | 9.447 | slower | 1.002 |
| cbench_libm/erfcl | 911.389 | 209.799 | 210.054 | 0.286 | 3 | slower | 209.996 | slower | 1.000 |
| cbench_libm/erff | 1.264 | 1.701 | 1.707 | 0.024 | 3 | slower | 1.729 | slower | 0.987 |
| cbench_libm/erff128 | 9.459 | 9.396 | 9.324 | 0.008 | 3 | slower | 9.363 | slower | 0.996 |
| cbench_libm/erfl | 922.378 | 211.791 | 211.939 | 0.295 | 3 | slower | 212.440 | slower | 0.998 |
| cbench_libm/exp | 1.269 | 1.772 | 1.772 | 0.002 | 3 | slower | 3.238 | slower | 0.547 |
| cbench_libm/exp10 | 1.817 | 2.624 | 2.625 | 0.008 | 3 | slower | 5.181 | slower | 0.507 |
| cbench_libm/exp10f | 1.372 | 1.162 | 1.166 | 0.005 | 3 | slower | 3.577 | slower | 0.326 |
| cbench_libm/exp10f128 | 2.594 | 2.556 | 2.567 | 0.016 | 3 | slower | 2.541 | slower | 1.010 |
| cbench_libm/exp10l | 9.191 | 3.753 | 3.889 | 0.039 | 3 | slower | 8.101 | slower | 0.480 |
| cbench_libm/exp2 | 1.687 | 2.661 | 2.656 | 0.014 | 3 | slower | 4.177 | slower | 0.636 |
| cbench_libm/exp2f | 1.486 | 1.099 | 1.093 | 0.009 | 3 | slower | 3.517 | slower | 0.311 |
| cbench_libm/exp2f128 | 4.679 | 4.720 | 4.676 | 0.005 | 3 | slower | 4.653 | slower | 1.005 |
| cbench_libm/exp2l | 11.960 | 4.810 | 4.806 | 0.007 | 3 | slower | 10.083 | slower | 0.477 |
| cbench_libm/exp_overflow_errno | 2.488 | 2.662 | 2.663 | 0.001 | 3 | slower | 2.739 | slower | 0.972 |
| cbench_libm/exp_qnan | 2.339 | 1.785 | 1.785 | 0.005 | 3 | slower | 1.855 | slower | 0.962 |
| cbench_libm/exp_small | 1.269 | 1.767 | 1.774 | 0.003 | 3 | slower | 3.063 | slower | 0.579 |
| cbench_libm/exp_tiny | 4.855 | 2.731 | 2.735 | 0.003 | 3 | slower | 2.841 | slower | 0.963 |
| cbench_libm/exp_upward | 5.460 | 9.336 | 9.138 | 0.027 | 3 | slower | 9.017 | slower | 1.013 |
| cbench_libm/expf | 1.410 | 1.127 | 1.116 | 0.011 | 3 | slower | 3.591 | slower | 0.311 |
| cbench_libm/expf128 | 0.115 | 0.119 | 0.120 | 0.018 | 3 | faster | 5.063 | slower | 0.024 |
| cbench_libm/expf_qnan | 1.347 | 1.063 | 1.062 | 0.006 | 3 | slower | 3.608 | slower | 0.294 |
| cbench_libm/expf_upward | 1.410 | 1.120 | 1.115 | 0.003 | 3 | slower | 9.089 | slower | 0.123 |
| cbench_libm/expl | 7.253 | 3.266 | 3.303 | 0.022 | 3 | slower | 7.544 | slower | 0.438 |
| cbench_libm/expm1 | 1.449 | 1.643 | 1.646 | 0.002 | 3 | slower | 2.434 | slower | 0.676 |
| cbench_libm/expm1f | 1.764 | 1.629 | 1.630 | 0.009 | 3 | slower | 1.626 | slower | 1.002 |
| cbench_libm/expm1f128 | 3.674 | 4.245 | 4.263 | 0.015 | 3 | slower | 4.206 | slower | 1.014 |
| cbench_libm/expm1l | 10.830 | 3.942 | 3.942 | 0.007 | 3 | slower | 8.505 | slower | 0.463 |
| cbench_libm/hypot | 1.617 | 1.971 | 1.981 | 0.007 | 3 | slower | 2.226 | slower | 0.890 |
| cbench_libm/hypotf | 1.372 | 2.502 | 2.496 | 0.000 | 3 | slower | 2.507 | slower | 0.996 |
| cbench_libm/hypotf128 | 1.424 | 0.828 | 0.827 | 0.004 | 3 | faster | 0.831 | faster | 0.995 |
| cbench_libm/hypotl | 42.228 | 10.985 | 10.995 | 0.006 | 3 | slower | 10.998 | slower | 1.000 |
| cbench_libm/j0 | 1.188 | 1.791 | 1.796 | 0.001 | 3 | slower | 1.697 | slower | 1.058 |
| cbench_libm/j0f | 0.745 | 1.015 | 1.014 | 0.003 | 3 | parity | 0.978 | parity | 1.037 |
| cbench_libm/j0f128 | 3.547 | 3.824 | 3.858 | 0.007 | 3 | slower | 3.849 | slower | 1.002 |
| cbench_libm/j0l | 49.941 | 28.303 | 28.482 | 0.003 | 3 | slower | 29.052 | slower | 0.980 |
| cbench_libm/j1 | 1.216 | 1.577 | 1.548 | 0.012 | 3 | slower | 1.566 | slower | 0.989 |
| cbench_libm/j1f | 0.752 | 0.991 | 0.989 | 0.005 | 3 | parity | 0.946 | faster | 1.045 |
| cbench_libm/j1f128 | 3.518 | 3.791 | 3.827 | 0.002 | 3 | slower | 3.825 | slower | 1.001 |
| cbench_libm/j1l | 49.869 | 28.568 | 28.588 | 0.002 | 3 | slower | 29.082 | slower | 0.983 |
| cbench_libm/jn | 3.022 | 4.594 | 4.565 | 0.033 | 3 | slower | 146.488 | slower | 0.031 |
| cbench_libm/jnf | 4.150 | 6.186 | 6.161 | 0.006 | 3 | slower | 201.248 | slower | 0.031 |
| cbench_libm/jnf128 | 1.630 | 1.779 | 1.781 | 0.008 | 3 | slower | 1.760 | slower | 1.012 |
| cbench_libm/jnl | 24.259 | 14.089 | 14.067 | 0.005 | 3 | slower | 14.109 | slower | 0.997 |
| cbench_libm/lgamma | 1.780 | 2.394 | 2.306 | 0.047 | 3 | slower | 2.196 | slower | 1.050 |
| cbench_libm/lgammaf | 1.138 | 1.785 | 1.772 | 0.020 | 3 | slower | 9.149 | slower | 0.194 |
| cbench_libm/lgammaf128 | 7.895 | 7.490 | 7.500 | 0.004 | 3 | slower | 7.492 | slower | 1.001 |
| cbench_libm/lgammal | 971.530 | 163.779 | 162.897 | 0.036 | 3 | slower | 171.672 | slower | 0.949 |
| cbench_libm/log | 1.413 | 2.000 | 2.002 | 0.003 | 3 | slower | 5.366 | slower | 0.373 |
| cbench_libm/log10 | 1.039 | 1.476 | 1.470 | 0.041 | 3 | slower | 3.719 | slower | 0.395 |
| cbench_libm/log10f | 0.756 | 0.694 | 0.694 | 0.002 | 3 | faster | 2.110 | slower | 0.329 |
| cbench_libm/log10f128 | 9.407 | 10.791 | 10.720 | 0.008 | 3 | slower | 10.786 | slower | 0.994 |
| cbench_libm/log10l | 13.191 | 3.005 | 3.102 | 0.042 | 3 | slower | 8.162 | slower | 0.380 |
| cbench_libm/log1p | 1.308 | 1.411 | 1.406 | 0.003 | 3 | slower | 4.007 | slower | 0.351 |
| cbench_libm/log1pf | 1.998 | 2.412 | 2.409 | 0.003 | 3 | slower | 2.431 | slower | 0.991 |
| cbench_libm/log1pf128 | 7.861 | 9.211 | 9.136 | 0.007 | 3 | slower | 9.184 | slower | 0.995 |
| cbench_libm/log1pl | 18.034 | 3.260 | 3.358 | 0.038 | 3 | slower | 7.976 | slower | 0.421 |
| cbench_libm/log2 | 1.699 | 2.883 | 2.886 | 0.004 | 3 | slower | 6.510 | slower | 0.443 |
| cbench_libm/log2f | 1.404 | 1.112 | 1.115 | 0.000 | 3 | slower | 3.572 | slower | 0.312 |
| cbench_libm/log2f128 | 10.477 | 11.914 | 11.965 | 0.011 | 3 | slower | 11.885 | slower | 1.007 |
| cbench_libm/log2l | 13.180 | 2.915 | 3.042 | 0.045 | 3 | slower | 8.150 | slower | 0.373 |
| cbench_libm/log_upward | 5.513 | 11.173 | 11.168 | 0.001 | 3 | slower | 11.094 | slower | 1.007 |
| cbench_libm/logf | 1.348 | 1.044 | 1.045 | 0.003 | 3 | parity | 3.623 | slower | 0.288 |
| cbench_libm/logf128 | 0.115 | 0.125 | 0.123 | 0.018 | 3 | faster | 10.633 | slower | 0.012 |
| cbench_libm/logl | 11.417 | 2.465 | 2.460 | 0.005 | 3 | slower | 7.834 | slower | 0.314 |
| cbench_libm/mxcsr_pair | 0.999 | 1.000 | 1.000 | 0.001 | 3 | parity | 1.000 | parity | 1.000 |
| cbench_libm/mxcsr_wrapper_seq | 0.996 | 1.000 | 1.000 | 0.000 | 3 | parity | 1.000 | parity | 1.000 |
| cbench_libm/pow | 1.206 | 1.507 | 1.506 | 0.006 | 3 | slower | 4.051 | slower | 0.372 |
| cbench_libm/pow_upward | 6.171 | 10.029 | 10.033 | 0.009 | 3 | slower | 10.139 | slower | 0.990 |
| cbench_libm/powf | 3.278 | 5.781 | 5.808 | 0.002 | 3 | slower | 5.753 | slower | 1.010 |
| cbench_libm/powf128 | 5.207 | 5.725 | 5.749 | 0.006 | 3 | slower | 5.752 | slower | 0.999 |
| cbench_libm/powl | 151.053 | 29.950 | 29.879 | 0.057 | 3 | slower | 29.969 | slower | 0.997 |
| cbench_libm/sin | 1.275 | 1.534 | 1.533 | 0.000 | 3 | slower | 2.196 | slower | 0.698 |
| cbench_libm/sin_huge | 0.644 | 0.681 | 0.682 | 0.003 | 3 | faster | 5.816 | slower | 0.117 |
| cbench_libm/sin_upward | 4.494 | 2.489 | 2.484 | 0.004 | 3 | slower | 2.545 | slower | 0.976 |
| cbench_libm/sincos | 1.244 | 1.323 | 1.322 | 0.002 | 3 | slower | - | - | - |
| cbench_libm/sincosf | 2.295 | 2.319 | 2.324 | 0.003 | 3 | slower | - | - | - |
| cbench_libm/sincosf128 | 0.201 | 0.183 | 0.182 | 0.009 | 3 | faster | - | - | - |
| cbench_libm/sincosl | 0.590 | 0.338 | 0.348 | 0.034 | 3 | faster | - | - | - |
| cbench_libm/sinf | 2.220 | 2.563 | 2.574 | 0.012 | 3 | slower | 3.315 | slower | 0.776 |
| cbench_libm/sinf128 | 0.178 | 0.181 | 0.179 | 0.014 | 3 | faster | 11.949 | slower | 0.015 |
| cbench_libm/sinf_upward | 5.364 | 8.508 | 8.533 | 0.016 | 3 | slower | 8.663 | slower | 0.985 |
| cbench_libm/sinh | 0.995 | 0.916 | 0.913 | 0.006 | 3 | faster | 2.244 | slower | 0.407 |
| cbench_libm/sinhf | 0.631 | 0.627 | 0.620 | 0.038 | 3 | faster | 0.733 | faster | 0.846 |
| cbench_libm/sinhf128 | 4.214 | 4.521 | 4.506 | 0.009 | 3 | slower | 4.512 | slower | 0.999 |
| cbench_libm/sinhl | 336.134 | 112.959 | 112.634 | 0.001 | 3 | slower | 112.068 | slower | 1.005 |
| cbench_libm/sinl | 0.577 | 0.553 | 0.551 | 0.002 | 3 | faster | 57.015 | slower | 0.010 |
| cbench_libm/sqrt | 0.765 | 1.000 | 1.000 | 0.000 | 3 | parity | 2.421 | slower | 0.413 |
| cbench_libm/sqrtf | 0.766 | 1.000 | 1.000 | 0.000 | 3 | parity | 2.338 | slower | 0.428 |
| cbench_libm/sqrtf128 | 3.898 | 1.870 | 1.877 | 0.009 | 3 | slower | 1.867 | slower | 1.005 |
| cbench_libm/sqrtl | 0.801 | 0.874 | 0.867 | 0.002 | 3 | faster | 3.690 | slower | 0.235 |
| cbench_libm/tan | 1.430 | 1.738 | 1.727 | 0.016 | 3 | slower | 2.404 | slower | 0.718 |
| cbench_libm/tanf | 0.952 | 0.825 | 0.827 | 0.004 | 3 | faster | 1.046 | parity | 0.791 |
| cbench_libm/tanf128 | 0.207 | 0.213 | 0.212 | 0.009 | 3 | faster | 11.110 | slower | 0.019 |
| cbench_libm/tanh | 1.233 | 1.393 | 1.415 | 0.010 | 3 | slower | 3.119 | slower | 0.454 |
| cbench_libm/tanhf | 0.723 | 0.682 | 0.678 | 0.005 | 3 | faster | 0.814 | faster | 0.833 |
| cbench_libm/tanhf128 | 4.936 | 5.115 | 5.101 | 0.002 | 3 | slower | 5.105 | slower | 0.999 |
| cbench_libm/tanhl | 426.322 | 138.802 | 138.593 | 0.366 | 3 | slower | 138.009 | slower | 1.004 |
| cbench_libm/tanl | 0.682 | 0.672 | 0.671 | 0.005 | 3 | faster | 62.171 | slower | 0.011 |
| cbench_libm/tgamma | 1.232 | 1.710 | 1.701 | 0.004 | 3 | slower | 1.701 | slower | 1.000 |
| cbench_libm/tgammaf | 0.477 | 0.518 | 0.518 | 0.002 | 3 | faster | 0.518 | faster | 1.000 |
| cbench_libm/tgammaf128 | 14.799 | 13.880 | 13.911 | 0.013 | 3 | slower | 13.893 | slower | 1.001 |
| cbench_libm/tgammal | 607.550 | 109.153 | 109.961 | 0.109 | 3 | slower | 96.442 | slower | 1.140 |
| cbench_libm/y0 | 1.138 | 1.491 | 1.462 | 0.014 | 3 | slower | 1.530 | slower | 0.956 |
| cbench_libm/y0f | 0.762 | 1.056 | 1.055 | 0.007 | 3 | slower | 1.042 | parity | 1.012 |
| cbench_libm/y0f128 | 13.055 | 12.397 | 12.437 | 0.003 | 3 | slower | 12.405 | slower | 1.003 |
| cbench_libm/y0l | 190.803 | 99.440 | 99.694 | 0.018 | 3 | slower | 101.616 | slower | 0.981 |
| cbench_libm/y1 | 1.140 | 1.763 | 1.779 | 0.004 | 3 | slower | 1.569 | slower | 1.134 |
| cbench_libm/y1f | 0.822 | 1.097 | 1.098 | 0.005 | 3 | slower | 1.043 | parity | 1.053 |
| cbench_libm/y1f128 | 12.969 | 12.326 | 12.352 | 0.002 | 3 | slower | 12.341 | slower | 1.001 |
| cbench_libm/y1l | 190.739 | 99.255 | 99.621 | 0.017 | 3 | slower | 101.552 | slower | 0.981 |
| cbench_libm/yn | 2.503 | 3.515 | 3.502 | 0.011 | 3 | slower | 527.059 | slower | 0.007 |
| cbench_libm/ynf | 3.404 | 4.668 | 4.639 | 0.008 | 3 | slower | 725.192 | slower | 0.006 |
| cbench_libm/ynf128 | 6.441 | 6.194 | 6.210 | 0.004 | 3 | slower | 6.189 | slower | 1.003 |
| cbench_libm/ynl | 93.975 | 50.326 | 50.405 | 0.013 | 3 | slower | 50.966 | slower | 0.989 |
