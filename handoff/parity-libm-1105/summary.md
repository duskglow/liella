# PARITY-LIBM-1105 summary: libm rows above 1.05x instructions:u, by group

Counter run: `cargo xtask bench --compare --counters --runs 3 cbench_libm` on origin/next 0005889c, job label `parity-libm-1105-counts` (host mei, Intel Xeon Platinum 8151, Skylake-SP; the brief named Zen 1 / Haswell-EP but this run was admitted to the mei role; instruction counts are architectural, time ratios are indicative only). Raw: bench-cbench_libm.out. Per-call profiles: `parity-libm-1105-percall4` (validation/percall/parity_libm1105.c, one representative per group; earlier `-percall`, `-percall2`, `-percall3` were superseded: base not cached, cargo not on PATH, a stale liella-cc dropping -lm, fixed in validation/wl_percall_run.sh).
Every row of cbench_libm not listed is at or below 1.05x. Instr ratio = Liella/glibc instructions:u per op; time ratio = Liella/glibc ns (dynamic glibc).

## Percall representatives (instructions:u per call, Liella / glibc)

| Group | Representative | Liella | glibc | ratio |
|---|---|---:|---:|---:|
| EXPM1-LOG1P | expm1 | 1428.9 | 89.3 | 16.0 |
| TRIG | sin | 360.5 | 104.6 | 3.45 |
| INVTRIG | atan2 | 1693.4 | 163.0 | 10.4 |
| HYPERBOLIC | tanh | 1591.6 | 134.7 | 11.8 |
| SPECIAL | erf | 223222.1 | 158.9 | 1404.5 |
| LONGDOUBLE | expl | 40964.0 | 87.0 | 470.9 |

## Rows

| Row | Instr ratio | Time ratio | Story or reason not briefed |
|---|---:|---:|---|
| exp10f | 1.61 | 3.23 | PERF-LIBM-EXPM1-LOG1P |
| exp10 | 7.48 | 16.40 | PERF-LIBM-EXPM1-LOG1P |
| expm1f | 1.76 | 1.38 | PERF-LIBM-EXPM1-LOG1P |
| expm1 | 15.88 | 26.05 | PERF-LIBM-EXPM1-LOG1P |
| log1pf | 2.00 | 2.62 | PERF-LIBM-EXPM1-LOG1P |
| log1p | 16.01 | 33.13 | PERF-LIBM-EXPM1-LOG1P |
| cbrtf | 1.18 | 1.42 | PERF-LIBM-EXPM1-LOG1P |
| cbrt | 7.21 | 13.02 | PERF-LIBM-EXPM1-LOG1P |
| sin_huge | 4.89 | 6.29 | PERF-LIBM-TRIG |
| sin_upward | 8.75 | 11.87 | PERF-LIBM-TRIG |
| sinf_upward | 5.88 | 22.54 | PERF-LIBM-TRIG |
| sinf | 3.99 | 5.53 | PERF-LIBM-TRIG |
| sin | 3.39 | 6.38 | PERF-LIBM-TRIG |
| cosf | 3.97 | 5.57 | PERF-LIBM-TRIG |
| cos | 3.21 | 6.00 | PERF-LIBM-TRIG |
| tanf | 1.63 | 1.60 | PERF-LIBM-TRIG |
| tan | 4.61 | 7.85 | PERF-LIBM-TRIG |
| asinf | 28.96 | 83.97 | PERF-LIBM-INVTRIG |
| asin | 20.79 | 63.95 | PERF-LIBM-INVTRIG |
| acosf | 25.42 | 62.31 | PERF-LIBM-INVTRIG |
| acos | 19.57 | 62.80 | PERF-LIBM-INVTRIG |
| atanf | 20.70 | 45.40 | PERF-LIBM-INVTRIG |
| atan | 17.67 | 41.80 | PERF-LIBM-INVTRIG |
| hypotf | 1.37 | 1.82 | PERF-LIBM-INVTRIG |
| hypot | 59.75 | 49.14 | PERF-LIBM-INVTRIG |
| atan2f | 10.96 | 24.28 | PERF-LIBM-INVTRIG |
| atan2 | 10.00 | 21.82 | PERF-LIBM-INVTRIG |
| sinhf | 10.46 | 15.49 | PERF-LIBM-HYPERBOLIC |
| sinh | 10.70 | 17.80 | PERF-LIBM-HYPERBOLIC |
| coshf | 19.09 | 47.66 | PERF-LIBM-HYPERBOLIC |
| cosh | 18.07 | 45.03 | PERF-LIBM-HYPERBOLIC |
| tanhf | 11.59 | 18.95 | PERF-LIBM-HYPERBOLIC |
| tanh | 11.77 | 21.92 | PERF-LIBM-HYPERBOLIC |
| asinhf | 16.68 | 31.51 | PERF-LIBM-HYPERBOLIC |
| asinh | 15.21 | 27.29 | PERF-LIBM-HYPERBOLIC |
| acoshf | 19.81 | 42.34 | PERF-LIBM-HYPERBOLIC |
| acosh | 17.35 | 32.53 | PERF-LIBM-HYPERBOLIC |
| atanhf | 12.93 | 24.48 | PERF-LIBM-HYPERBOLIC |
| atanh | 13.43 | 26.50 | PERF-LIBM-HYPERBOLIC |
| erff | 1567.34 | 2270.58 | PERF-LIBM-SPECIAL |
| erf | 1379.00 | 2323.17 | PERF-LIBM-SPECIAL |
| erfcf | 1534.37 | 2240.88 | PERF-LIBM-SPECIAL |
| erfc | 1349.11 | 2276.35 | PERF-LIBM-SPECIAL |
| tgammaf | 1314.31 | 1426.43 | PERF-LIBM-SPECIAL |
| tgamma | 1024.56 | 1082.64 | PERF-LIBM-SPECIAL |
| lgammaf | 2119.67 | 2375.05 | PERF-LIBM-SPECIAL |
| lgamma | 1580.75 | 1693.70 | PERF-LIBM-SPECIAL |
| j0f | 514.21 | 932.84 | PERF-LIBM-SPECIAL |
| j0 | 343.40 | 707.81 | PERF-LIBM-SPECIAL |
| j1f | 506.23 | 921.06 | PERF-LIBM-SPECIAL |
| j1 | 339.30 | 706.38 | PERF-LIBM-SPECIAL |
| y0f | 2021.78 | 2739.53 | PERF-LIBM-SPECIAL |
| y0 | 1366.94 | 2063.16 | PERF-LIBM-SPECIAL |
| y1f | 2009.66 | 2701.04 | PERF-LIBM-SPECIAL |
| y1 | 1368.81 | 2073.87 | PERF-LIBM-SPECIAL |
| jnf | 205.69 | 355.19 | PERF-LIBM-SPECIAL |
| jn | 146.32 | 282.48 | PERF-LIBM-SPECIAL |
| ynf | 859.38 | 1095.55 | PERF-LIBM-SPECIAL |
| yn | 611.04 | 892.49 | PERF-LIBM-SPECIAL |
| expl | 487.42 | 134.64 | PERF-LIBM-LONGDOUBLE |
| exp2l | 644.33 | 208.69 | PERF-LIBM-LONGDOUBLE |
| exp10l | 1654.81 | 488.82 | PERF-LIBM-LONGDOUBLE |
| expm1l | 582.87 | 140.17 | PERF-LIBM-LONGDOUBLE |
| logl | 1837.34 | 691.29 | PERF-LIBM-LONGDOUBLE |
| log2l | 2149.17 | 726.10 | PERF-LIBM-LONGDOUBLE |
| log10l | 4323.99 | 1497.90 | PERF-LIBM-LONGDOUBLE |
| log1pl | 1842.55 | 637.46 | PERF-LIBM-LONGDOUBLE |
| cbrtl | 291.44 | 85.13 | PERF-LIBM-LONGDOUBLE |
| sinl | 89.65 | 123.08 | PERF-LIBM-LONGDOUBLE |
| cosl | 89.64 | 123.15 | PERF-LIBM-LONGDOUBLE |
| tanl | 101.04 | 129.68 | PERF-LIBM-LONGDOUBLE |
| asinl | 2126.42 | 959.53 | PERF-LIBM-LONGDOUBLE |
| acosl | 4993.64 | 417.05 | PERF-LIBM-LONGDOUBLE |
| atanl | 9398.92 | 474.43 | PERF-LIBM-LONGDOUBLE |
| sinhl | 409.43 | 131.62 | PERF-LIBM-LONGDOUBLE |
| coshl | 461.30 | 128.27 | PERF-LIBM-LONGDOUBLE |
| tanhl | 521.34 | 154.33 | PERF-LIBM-LONGDOUBLE |
| asinhl | 1462.05 | 478.45 | PERF-LIBM-LONGDOUBLE |
| acoshl | 1192.45 | 412.75 | PERF-LIBM-LONGDOUBLE |
| atanhl | 929.84 | 379.08 | PERF-LIBM-LONGDOUBLE |
| erfl | 1121.89 | 340.76 | PERF-LIBM-LONGDOUBLE |
| erfcl | 1109.58 | 340.75 | PERF-LIBM-LONGDOUBLE |
| tgammal | 831.51 | 315.64 | PERF-LIBM-LONGDOUBLE |
| lgammal | 1340.43 | 581.44 | PERF-LIBM-LONGDOUBLE |
| j0l | 58.87 | 86.64 | PERF-LIBM-LONGDOUBLE |
| j1l | 58.73 | 86.95 | PERF-LIBM-LONGDOUBLE |
| y0l | 241.14 | 266.75 | PERF-LIBM-LONGDOUBLE |
| y1l | 241.18 | 266.90 | PERF-LIBM-LONGDOUBLE |
| powl | 185.50 | 83.42 | PERF-LIBM-LONGDOUBLE |
| hypotl | 42.37 | 19.52 | PERF-LIBM-LONGDOUBLE |
| atan2l | 4692.50 | 472.56 | PERF-LIBM-LONGDOUBLE |
| jnl | 28.54 | 41.78 | PERF-LIBM-LONGDOUBLE |
| ynl | 118.74 | 132.83 | PERF-LIBM-LONGDOUBLE |
| cexpf | 864.76 | 760.55 | not briefed: complex rows (cexp, clog, csin, csqrt, cpow, f32/f64/long double) (see Not briefed) |
| cexp | 581.61 | 1123.41 | not briefed: complex rows (cexp, clog, csin, csqrt, cpow, f32/f64/long double) (see Not briefed) |
| cexpl | 123.74 | 117.69 | not briefed: complex rows (cexp, clog, csin, csqrt, cpow, f32/f64/long double) (see Not briefed) |
| clogf | 339.62 | 309.59 | not briefed: complex rows (cexp, clog, csin, csqrt, cpow, f32/f64/long double) (see Not briefed) |
| clog | 221.20 | 335.88 | not briefed: complex rows (cexp, clog, csin, csqrt, cpow, f32/f64/long double) (see Not briefed) |
| clogl | 985.65 | 347.88 | not briefed: complex rows (cexp, clog, csin, csqrt, cpow, f32/f64/long double) (see Not briefed) |
| csinf | 490.70 | 381.00 | not briefed: complex rows (cexp, clog, csin, csqrt, cpow, f32/f64/long double) (see Not briefed) |
| csin | 399.77 | 557.29 | not briefed: complex rows (cexp, clog, csin, csqrt, cpow, f32/f64/long double) (see Not briefed) |
| csqrtf | 410.48 | 133.79 | not briefed: complex rows (cexp, clog, csin, csqrt, cpow, f32/f64/long double) (see Not briefed) |
| csqrt | 350.98 | 236.04 | not briefed: complex rows (cexp, clog, csin, csqrt, cpow, f32/f64/long double) (see Not briefed) |
| csqrtl | 271.08 | 106.15 | not briefed: complex rows (cexp, clog, csin, csqrt, cpow, f32/f64/long double) (see Not briefed) |
| cpowf | 967.20 | 676.16 | not briefed: complex rows (cexp, clog, csin, csqrt, cpow, f32/f64/long double) (see Not briefed) |
| cpow | 644.95 | 772.59 | not briefed: complex rows (cexp, clog, csin, csqrt, cpow, f32/f64/long double) (see Not briefed) |
| cpowl | 282.32 | 181.40 | not briefed: complex rows (cexp, clog, csin, csqrt, cpow, f32/f64/long double) (see Not briefed) |
| exp_qnan | 1.06 | 0.81 | not briefed: f64/f32 exp/exp2/log/log2/log10/pow and the envelope rows (qnan/tiny/small/overflow/upward) (see Not briefed) |
| expf_qnan | 4.95 | 4.57 | not briefed: f64/f32 exp/exp2/log/log2/log10/pow and the envelope rows (qnan/tiny/small/overflow/upward) (see Not briefed) |
| exp_tiny | 11.22 | 9.05 | not briefed: f64/f32 exp/exp2/log/log2/log10/pow and the envelope rows (qnan/tiny/small/overflow/upward) (see Not briefed) |
| exp_small | 2.70 | 4.01 | not briefed: f64/f32 exp/exp2/log/log2/log10/pow and the envelope rows (qnan/tiny/small/overflow/upward) (see Not briefed) |
| exp_overflow_errno | 2.77 | 2.79 | not briefed: f64/f32 exp/exp2/log/log2/log10/pow and the envelope rows (qnan/tiny/small/overflow/upward) (see Not briefed) |
| exp_upward | 6.07 | 19.08 | not briefed: f64/f32 exp/exp2/log/log2/log10/pow and the envelope rows (qnan/tiny/small/overflow/upward) (see Not briefed) |
| log_upward | 6.16 | 22.13 | not briefed: f64/f32 exp/exp2/log/log2/log10/pow and the envelope rows (qnan/tiny/small/overflow/upward) (see Not briefed) |
| expf_upward | 6.21 | 28.13 | not briefed: f64/f32 exp/exp2/log/log2/log10/pow and the envelope rows (qnan/tiny/small/overflow/upward) (see Not briefed) |
| pow_upward | 6.47 | 11.87 | not briefed: f64/f32 exp/exp2/log/log2/log10/pow and the envelope rows (qnan/tiny/small/overflow/upward) (see Not briefed) |
| expf | 1.80 | 2.63 | not briefed: f64/f32 exp/exp2/log/log2/log10/pow and the envelope rows (qnan/tiny/small/overflow/upward) (see Not briefed) |
| exp | 2.70 | 4.00 | not briefed: f64/f32 exp/exp2/log/log2/log10/pow and the envelope rows (qnan/tiny/small/overflow/upward) (see Not briefed) |
| exp2f | 1.89 | 2.69 | not briefed: f64/f32 exp/exp2/log/log2/log10/pow and the envelope rows (qnan/tiny/small/overflow/upward) (see Not briefed) |
| exp2 | 3.65 | 5.33 | not briefed: f64/f32 exp/exp2/log/log2/log10/pow and the envelope rows (qnan/tiny/small/overflow/upward) (see Not briefed) |
| logf | 1.79 | 3.29 | not briefed: f64/f32 exp/exp2/log/log2/log10/pow and the envelope rows (qnan/tiny/small/overflow/upward) (see Not briefed) |
| log | 3.45 | 6.84 | not briefed: f64/f32 exp/exp2/log/log2/log10/pow and the envelope rows (qnan/tiny/small/overflow/upward) (see Not briefed) |
| log2f | 1.86 | 3.47 | not briefed: f64/f32 exp/exp2/log/log2/log10/pow and the envelope rows (qnan/tiny/small/overflow/upward) (see Not briefed) |
| log2 | 3.62 | 7.95 | not briefed: f64/f32 exp/exp2/log/log2/log10/pow and the envelope rows (qnan/tiny/small/overflow/upward) (see Not briefed) |
| log10 | 2.36 | 4.44 | not briefed: f64/f32 exp/exp2/log/log2/log10/pow and the envelope rows (qnan/tiny/small/overflow/upward) (see Not briefed) |
| sqrtf | 1.82 | 1.91 | not briefed: f64/f32 exp/exp2/log/log2/log10/pow and the envelope rows (qnan/tiny/small/overflow/upward) (see Not briefed) |
| sqrt | 1.94 | 1.87 | not briefed: f64/f32 exp/exp2/log/log2/log10/pow and the envelope rows (qnan/tiny/small/overflow/upward) (see Not briefed) |
| sqrtl | 2.77 | 3.37 | not briefed: f64/f32 exp/exp2/log/log2/log10/pow and the envelope rows (qnan/tiny/small/overflow/upward) (see Not briefed) |
| powf | 3.28 | 6.45 | not briefed: f64/f32 exp/exp2/log/log2/log10/pow and the envelope rows (qnan/tiny/small/overflow/upward) (see Not briefed) |
| pow | 3.41 | 5.26 | not briefed: f64/f32 exp/exp2/log/log2/log10/pow and the envelope rows (qnan/tiny/small/overflow/upward) (see Not briefed) |
| expf128 | 6.05 | 7.36 | not briefed: _Float128 real functions (4-34x) (see Not briefed) |
| exp2f128 | 5.64 | 6.87 | not briefed: _Float128 real functions (4-34x) (see Not briefed) |
| exp10f128 | 10.30 | 13.35 | not briefed: _Float128 real functions (4-34x) (see Not briefed) |
| expm1f128 | 4.44 | 6.53 | not briefed: _Float128 real functions (4-34x) (see Not briefed) |
| logf128 | 11.18 | 18.91 | not briefed: _Float128 real functions (4-34x) (see Not briefed) |
| log2f128 | 12.93 | 20.09 | not briefed: _Float128 real functions (4-34x) (see Not briefed) |
| log10f128 | 23.41 | 37.40 | not briefed: _Float128 real functions (4-34x) (see Not briefed) |
| log1pf128 | 9.72 | 15.59 | not briefed: _Float128 real functions (4-34x) (see Not briefed) |
| cbrtf128 | 8.38 | 5.89 | not briefed: _Float128 real functions (4-34x) (see Not briefed) |
| sqrtf128 | 3.90 | 1.46 | not briefed: _Float128 real functions (4-34x) (see Not briefed) |
| sinf128 | 13.92 | 24.17 | not briefed: _Float128 real functions (4-34x) (see Not briefed) |
| cosf128 | 13.92 | 24.47 | not briefed: _Float128 real functions (4-34x) (see Not briefed) |
| tanf128 | 13.20 | 20.34 | not briefed: _Float128 real functions (4-34x) (see Not briefed) |
| asinf128 | 23.77 | 20.47 | not briefed: _Float128 real functions (4-34x) (see Not briefed) |
| acosf128 | 23.31 | 20.30 | not briefed: _Float128 real functions (4-34x) (see Not briefed) |
| atanf128 | 34.41 | 30.61 | not briefed: _Float128 real functions (4-34x) (see Not briefed) |
| sinhf128 | 5.12 | 6.84 | not briefed: _Float128 real functions (4-34x) (see Not briefed) |
| coshf128 | 7.12 | 7.93 | not briefed: _Float128 real functions (4-34x) (see Not briefed) |
| tanhf128 | 6.03 | 7.62 | not briefed: _Float128 real functions (4-34x) (see Not briefed) |
| asinhf128 | 9.27 | 10.69 | not briefed: _Float128 real functions (4-34x) (see Not briefed) |
| acoshf128 | 9.70 | 11.89 | not briefed: _Float128 real functions (4-34x) (see Not briefed) |
| atanhf128 | 8.55 | 12.57 | not briefed: _Float128 real functions (4-34x) (see Not briefed) |
| erff128 | 11.52 | 16.08 | not briefed: _Float128 real functions (4-34x) (see Not briefed) |
| erfcf128 | 11.63 | 16.15 | not briefed: _Float128 real functions (4-34x) (see Not briefed) |
| tgammaf128 | 20.08 | 23.65 | not briefed: _Float128 real functions (4-34x) (see Not briefed) |
| lgammaf128 | 10.80 | 12.87 | not briefed: _Float128 real functions (4-34x) (see Not briefed) |
| j0f128 | 4.17 | 7.75 | not briefed: _Float128 real functions (4-34x) (see Not briefed) |
| j1f128 | 4.14 | 7.68 | not briefed: _Float128 real functions (4-34x) (see Not briefed) |
| y0f128 | 16.47 | 22.44 | not briefed: _Float128 real functions (4-34x) (see Not briefed) |
| y1f128 | 16.37 | 22.30 | not briefed: _Float128 real functions (4-34x) (see Not briefed) |
| powf128 | 6.37 | 9.54 | not briefed: _Float128 real functions (4-34x) (see Not briefed) |
| hypotf128 | 1.42 | 0.76 | not briefed: _Float128 real functions (4-34x) (see Not briefed) |
| atan2f128 | 33.01 | 27.42 | not briefed: _Float128 real functions (4-34x) (see Not briefed) |
| jnf128 | 1.91 | 3.46 | not briefed: _Float128 real functions (4-34x) (see Not briefed) |
| ynf128 | 8.13 | 11.05 | not briefed: _Float128 real functions (4-34x) (see Not briefed) |

## Not briefed (named drops)

- N-elem: f64/f32 exp/exp2/log/log2/log10/pow and the envelope rows (qnan/tiny/small/overflow/upward): docs/saturation/16 counts the first-principles floor (exp 1.35x without FMA, parity only with FMA or fewer double-double steps); open stories math-m6-f64-gap-* and math-f64-fma-path own them. sqrt/sqrtf/sqrtl (1.8-2.8x, 17 vs 31 instructions): a small wrapper excess, not briefed (cap of 6 groups; cheap follow-up: fold into PERF-LIBM-TRIG).
- N-f128: _Float128 real functions (4-34x): no open story, not briefed only because of the 6-group cap; next round. They share the `wide` kernel with PERF-LIBM-LONGDOUBLE, so that story may move them.
- N-cplx: complex rows (cexp, clog, csin, csqrt, cpow, f32/f64/long double): the complex Ziv chain (ZIV-C, CPOW-EXACT, CPLX-DIRECTED, MATH-M5-CLOSE) owns them; they also call the real functions of the briefed groups and will improve with them.

The counts above are Liella being slower in instructions on every listed row (the ratio is Liella/glibc). The very large ratios (E, F, complex, f128) show a multiprecision tier running on every call; the 1.4-1.7k class (A, C, D) shows the double-double accurate path with no fast tier.
