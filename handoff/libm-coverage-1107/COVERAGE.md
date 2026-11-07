# LIBM-COVERAGE-1107: what covers each slower libm row

Source: results/timing-sweep-libm-1107-table.md (origin/next 66b8d3ba, time ratio Liella/glibc, Ivy Bridge 5 rounds,
N100 3 rounds). Architecture: handoff/libm-rethink/DESIGN.md and D-10.51 (two-phase `double`), D-10.52 (integer
fixed point for x87 and `_Float128`), D-10.53 (complex from the real quick phases). Open libm stories at 11-07:
LIBM-F64-EXP-QUICK, LIBM-F64-EXP-QUICK-WIRE, LIBM-CPLX-EXP-TIER, LIBM-LD-TRIG-INT (each with a -2 resume). No libm
story landed between 66b8d3ba and origin/next acc8cfcd (LIBM-RETHINK-2 is planning only), so nothing needs "re-time" for
a landing; the "re-time" rows are the sweep's reopen list (instructions <= 1.05, time > 1.05).

Coverage: "covered by <story>" = an open brief takes the row; "NONE -> <group>" = the group a new brief takes (ranks
1-6 have briefs in this directory, 7-16 are ranked with their method but not briefed yet); "re-time" = a booked
re-time first (7 rounds), then the group named in the re-time note.

## Profile of the uncovered groups (job keke:libm-coverage-1107-percall, Zen 1, FMA builds, instructions per call)

Spec: cov.c (scratch, copied to keke:~/work/LIBM-COVERAGE-1107/cov.c), 64 operands in the cbench ranges.

| case | glibc | Liella | ratio | tier that runs (code, counts and the trace below) |
|---|---:|---:|---:|---|
| log / log2 / log10 / log1p | 66 / 66 / 106 / 85 | 190 / 195 / 204 / 243 | 2.87 / 2.95 / 1.93 / 2.86 | fast64 accurate phase (~2^-70) on every call; time 4-5x on IVB (serialising step, DESIGN 5.4) |
| sin / cos / tan | 105 / 107 / 112 | 152 / 156 / 181 | 1.45 / 1.45 / 1.62 | fast64 trig tier (dd) on every call |
| pow | 143 | 319 | 2.23 | fast64 pow tier (dd log x y + dd exp) on every call |
| exp2 / exp10 / expm1 | 54 / 59 / 89 | 133 / 150 / 194 | 2.46 / 2.55 / 2.18 | fast64 dd tier on every call |
| expf / exp2f / logf / log2f / sinf / cosf / powf | 42 / 40 / 46 / 45 / 45 / 45 / 78 | 73 / 73 / 80 / 81 / 93 / 93 / 243 | 1.74-3.12 | fast32 double kernel + rounding check |
| asin / acos | 78 / 83 | 204 / 181 | 2.62 / 2.18 | invtrig fast tier |
| acosh / asinh / atanh | 86 / 102 / 115 | 984 / 1039 / 1054 | 11.4 / 10.2 / 9.1 | `hyper.rs` double-double `log_dd`/`log1p_dd` + dd sqrt/div on every call: no quick tier |
| cosh | 87 | 279 | 3.20 | fast64 hyperbolic tier |
| asinf / acosf | 59 / 65 | 173 / 173 | 2.96 / 2.68 | fast32 |
| jn / yn / jnf | 806 / 827 / 572 | 122 617 / 471 062 / 123 165 | 152 / 569 / 215 | generic `special::jn`/`yn`: 384-bit route on every call (f32 costs what f64 costs) |
| log_upward / expf_upward | 66 / 42 | 371 / 202 | 5.62 / 4.81 | `elem_switched` envelope (MXCSR switch, software flags) |
| expl / logl | 87 / 52 | 1054 / 1161 | 12.1 / 22.3 | wide_dd accurate tier (x87 judged in cycles) |
| tanl / atanl / asinl | 1023 / 26 / 110 | 87 217 / 160 923 / 176 669 | 85 / 6187 / 1600 | 384-bit route on every call |
| asinhl / tanhl / sinhl / powl / cbrtl / erfl | 78 / 120 / 131 / 668 / 161 / 134 | 84 806 / 48 684 / 41 811 / 102 628 / 36 719 / 290 592 | 228-2171 | 384-bit route on every call |
| logf128 / sinf128 / atanf128 | 7780 / 7162 / 5909 | 70 648 / 80 450 / 161 470 | 9.1 / 11.2 / 27.3 | 384-bit route |
| clog / clogf / csqrt / csin | 333 / 260 / 144 / 383 | 72 379 / 71 402 / 40 971 / 128 039 | 218-335 | 384-bit route (float = double cost) |
| sqrt | 10 | 10 | 1.00 | the probe inlines `sqrtsd` (gcc -O2): the cbench row's 1.9x is the library entry, measure through a pointer |
| lgammaf | 264 | 274 | 1.04 | fast tier (time 9.2x on IVB is not instructions: re-time + latency check) |

MISMATCH lines in the log (atanl, csin, clogf, yn, sinhl, jn, jnf, log_upward) are glibc's results differing from
Liella's correctly rounded ones in the last place (the accuracy difference recorded in docs/differences-from-glibc.md).
SSE2 builds (what Ivy Bridge runs; job keke:libm-coverage-1107-percall-sse2, `GLIBC_TUNABLES=glibc.cpu.hwcaps=-FMA`
for both libraries), glibc / Liella / ratio: log 81 / 225.6 / 2.79, log2 86 / 248.6 / 2.89, log10 121.2 / 260 / 2.14,
log1p 104 / 281 / 2.70, pow 193 / 494.4 / 2.56, sin 126.9 / 232 / 1.83, cos 128.7 / 234.6 / 1.82, tan 128.8 / 358.5 /
2.78, exp2 54 / 203 / 3.76, exp10 59 / 226.9 / 3.85, expm1 106.8 / 253 / 2.37, asin 81.7 / 330.5 / 4.05, acos 85.7 /
283.9 / 3.31, cosh 97.4 / 404 / 4.15, acosh 101.4 / 983.5 / 9.70, asinh 117.6 / 1039.3 / 8.84, atanh 133.5 / 1054.4 /
7.90 (f32 unchanged: no FMA builds). The SSE2 builds add 40-180 instructions per f64 call: the Ivy Bridge gap is
larger than the Zen 1 counts suggest, so every f64 brief counts both builds.

Trace (job keke:libm-coverage-1107-trace, one call, Liella only, Zen 1): acosh 972 instructions, `exp_log::log_dd` 640
+ `dd::sqrt` 89 + `elem::finish` 83 (the dd tier on every call, confirmed); jn 155 525 and jnf 156 431, `mp::pack`
alone 51 709 / 52 095 (the 384-bit route, f32 = f64 cost, confirmed); tanl 82 341, asinhl 93 708, clog 79 826, all
`mp::*` (confirmed); logl 1144 in `wide_dd::eval` + the generic `float::round::<F80>` 165; log_upward 356 of which
`elem_switched` 52 + `fpu::raise` 45 (envelope, confirmed); expf_upward 189 (`elem_switched` 46 + `fpu::raise` 45);
powf 232 (`fast32::exp_log::pow_core` 98 + `pow` 80 + `mathret::elem` 49); asinf 162 (`fast32::asin` 112 + entry 45);
lgammaf 255 (`special::lgamma::<f32>` 196). The f64 hot kernels (log, tan, exp2, asin, pow) are inlined into the
exported symbol, so their split shows only "<fn>f64 18 + liella_machine_mxcsr_get 3" and unnamed addresses: a brief
needs `--list` or the kernel as a separate symbol to split them. The job ends rc 1 only because its `sqrt` case makes
no library call (gcc -O2 inlines `sqrtsd`; the same probe artifact as LIBM-RETHINK's `sqrtl`); the 19 other traces
completed.

## Re-time note
coshf, log10f, tanf -> LIBM-F32-ONEPASS if confirmed; j0f, j1f, y0f, y1f -> LIBM-F32-REST; y0, y1 ->
LIBM-F64-SPECIAL-QUICK. All are at or below glibc in instructions, so a confirmed gap is latency or a slow instruction
(division, MXCSR access), not a missing tier: the brief must measure latency form (each argument depends on the
previous result) besides throughput.

## Ranking
Rank = gap x how common the function is (bench/ has no libm-heavy workload: only `ceil` outside cbench_libm, so
commonness is judged from general program use: f64 exp/log/pow/trig first, then f32, inverse trig/hyperbolic; long
double, Bessel of order n, `_Float128` and complex last). Long double ranks 7-8 despite 100-800x gaps because it is rare
in real programs and LIBM-LD-TRIG-INT builds the integer kernels it needs first.

## Table (every row slower on at least one class)

| row | instr IVB | time IVB | time N100 | coverage | group rank | method |
|---|---:|---:|---:|---|---:|---|
| cexp | 411.266 | 704.954 | 856.801 | covered by LIBM-CPLX-EXP-TIER | - | D-10.53 compose real quick phases (e) |
| cexpf | 642.213 | 621.450 | 498.386 | covered by LIBM-CPLX-EXP-TIER | - | D-10.53 compose real quick phases (e) |
| exp_tiny | 5.249 | 3.689 | 3.537 | covered by LIBM-F64-EXP-QUICK | - | D-10.51 quick phase (b)+(f), tier 2 = exp_near; regression: LIBM-F64-EXP-IVB-REGRESS |
| exp_small | 2.352 | 2.911 | 3.189 | covered by LIBM-F64-EXP-QUICK | - | D-10.51 quick phase (b)+(f), tier 2 = exp_near; regression: LIBM-F64-EXP-IVB-REGRESS |
| exp | 2.353 | 2.901 | 3.348 | covered by LIBM-F64-EXP-QUICK | - | D-10.51 quick phase (b)+(f), tier 2 = exp_near; regression: LIBM-F64-EXP-IVB-REGRESS |
| exp_overflow_errno | 2.439 | 2.101 | 3.130 | covered by LIBM-F64-EXP-QUICK | - | D-10.51 quick phase (b)+(f), tier 2 = exp_near; regression: LIBM-F64-EXP-IVB-REGRESS |
| exp_qnan | 1.059 | 0.854 | 3.139 | covered by LIBM-F64-EXP-QUICK | - | D-10.51 quick phase (b)+(f), tier 2 = exp_near; regression: LIBM-F64-EXP-IVB-REGRESS |
| sinl | 73.173 | 112.724 | 88.243 | covered by LIBM-LD-TRIG-INT | - | D-10.52 integer fixed point (d-style limbs for F80) |
| cosl | 73.158 | 112.043 | 88.550 | covered by LIBM-LD-TRIG-INT | - | D-10.52 integer fixed point (d-style limbs for F80) |
| tanf | 0.925 | 1.313 | 1.242 | re-time | - | see re-time note |
| y1 | 1.024 | 1.241 | 1.640 | re-time | - | see re-time note |
| y0 | 1.023 | 1.232 | 1.640 | re-time | - | see re-time note |
| log10f | 0.839 | 1.213 | 1.634 | re-time | - | see re-time note |
| coshf | 0.921 | 1.210 | 1.819 | re-time | - | see re-time note |
| y1f | 0.681 | 1.190 | 1.051 | re-time | - | see re-time note |
| y0f | 0.683 | 1.173 | 1.060 | re-time | - | see re-time note |
| j1f | 0.662 | 1.101 | 0.992 | re-time | - | see re-time note |
| j0f | 0.658 | 1.100 | 0.978 | re-time | - | see re-time note |
| log | 2.785 | 5.175 | 5.879 | NONE -> LIBM-F64-LOG-QUICK | 1 | (b) Tang-table quick log ~2^-62 + near test; (f) Estrin; (g) VEX/FMA variants |
| log2 | 2.871 | 4.568 | 5.564 | NONE -> LIBM-F64-LOG-QUICK | 1 | (b) Tang-table quick log ~2^-62 + near test; (f) Estrin; (g) VEX/FMA variants |
| log1p | 2.425 | 3.676 | 3.849 | NONE -> LIBM-F64-LOG-QUICK | 1 | (b) Tang-table quick log ~2^-62 + near test; (f) Estrin; (g) VEX/FMA variants |
| log10 | 2.136 | 3.620 | 3.517 | NONE -> LIBM-F64-LOG-QUICK | 1 | (b) Tang-table quick log ~2^-62 + near test; (f) Estrin; (g) VEX/FMA variants |
| sin_huge | 3.555 | 4.575 | 5.977 | NONE -> LIBM-F64-TRIG-QUICK | 2 | (b) quick sin/cos/tan in double, shared sincos table; (f) |
| tan | 2.843 | 2.831 | 2.613 | NONE -> LIBM-F64-TRIG-QUICK | 2 | (b) quick sin/cos/tan in double, shared sincos table; (f) |
| sin | 1.982 | 2.219 | 2.154 | NONE -> LIBM-F64-TRIG-QUICK | 2 | (b) quick sin/cos/tan in double, shared sincos table; (f) |
| cos | 1.897 | 2.134 | 1.853 | NONE -> LIBM-F64-TRIG-QUICK | 2 | (b) quick sin/cos/tan in double, shared sincos table; (f) |
| pow | 2.575 | 3.351 | 3.585 | NONE -> LIBM-F64-POW-QUICK | 3 | (b) quick log (double+low) x y then quick exp, one test |
| powf | 2.738 | 4.466 | 4.858 | NONE -> LIBM-F32-ONEPASS | 4 | (a) one double evaluation, exhaustive proof, mode-independent |
| expf_qnan | 4.619 | 3.565 | 4.261 | NONE -> LIBM-F32-ONEPASS | 4 | (a) one double evaluation, exhaustive proof, mode-independent |
| exp10f | 1.606 | 2.600 | 3.055 | NONE -> LIBM-F32-ONEPASS | 4 | (a) one double evaluation, exhaustive proof, mode-independent |
| logf | 1.542 | 2.595 | 3.501 | NONE -> LIBM-F32-ONEPASS | 4 | (a) one double evaluation, exhaustive proof, mode-independent |
| log2f | 1.561 | 2.354 | 2.990 | NONE -> LIBM-F32-ONEPASS | 4 | (a) one double evaluation, exhaustive proof, mode-independent |
| exp2f | 1.667 | 2.307 | 2.593 | NONE -> LIBM-F32-ONEPASS | 4 | (a) one double evaluation, exhaustive proof, mode-independent |
| cosf | 1.769 | 2.280 | 3.122 | NONE -> LIBM-F32-ONEPASS | 4 | (a) one double evaluation, exhaustive proof, mode-independent |
| log1pf | 1.998 | 2.182 | 2.175 | NONE -> LIBM-F32-ONEPASS | 4 | (a) one double evaluation, exhaustive proof, mode-independent |
| expf | 1.629 | 2.114 | 2.598 | NONE -> LIBM-F32-ONEPASS | 4 | (a) one double evaluation, exhaustive proof, mode-independent |
| sinf | 1.777 | 2.047 | 3.282 | NONE -> LIBM-F32-ONEPASS | 4 | (a) one double evaluation, exhaustive proof, mode-independent |
| expm1f | 1.764 | 1.604 | 1.039 | NONE -> LIBM-F32-ONEPASS | 4 | (a) one double evaluation, exhaustive proof, mode-independent |
| exp10 | 3.904 | 5.887 | 5.213 | NONE -> LIBM-F64-EXP-FAMILY-QUICK | 5 | (b) exp quick table reused; exp2/exp10 if WIRE left them |
| exp2 | 3.825 | 5.485 | 3.510 | NONE -> LIBM-F64-EXP-FAMILY-QUICK | 5 | (b) exp quick table reused; exp2/exp10 if WIRE left them |
| expm1 | 2.085 | 2.038 | 2.086 | NONE -> LIBM-F64-EXP-FAMILY-QUICK | 5 | (b) exp quick table reused; exp2/exp10 if WIRE left them |
| atanh | 8.172 | 15.454 | 19.173 | NONE -> LIBM-F64-INVHYP-QUICK | 6 | (b) quick asin/acos/atan + log-quick-based acosh/asinh/atanh |
| acosh | 10.216 | 14.689 | 26.192 | NONE -> LIBM-F64-INVHYP-QUICK | 6 | (b) quick asin/acos/atan + log-quick-based acosh/asinh/atanh |
| asinh | 9.254 | 13.837 | 22.533 | NONE -> LIBM-F64-INVHYP-QUICK | 6 | (b) quick asin/acos/atan + log-quick-based acosh/asinh/atanh |
| asin | 3.922 | 9.792 | 8.393 | NONE -> LIBM-F64-INVHYP-QUICK | 6 | (b) quick asin/acos/atan + log-quick-based acosh/asinh/atanh |
| acos | 3.422 | 8.678 | 7.039 | NONE -> LIBM-F64-INVHYP-QUICK | 6 | (b) quick asin/acos/atan + log-quick-based acosh/asinh/atanh |
| cosh | 3.704 | 4.190 | 4.770 | NONE -> LIBM-F64-INVHYP-QUICK | 6 | (b) quick asin/acos/atan + log-quick-based acosh/asinh/atanh |
| hypot | 2.402 | 2.871 | 2.160 | NONE -> LIBM-F64-INVHYP-QUICK | 6 | (b) quick asin/acos/atan + log-quick-based acosh/asinh/atanh |
| tanh | 3.164 | 2.788 | 2.985 | NONE -> LIBM-F64-INVHYP-QUICK | 6 | (b) quick asin/acos/atan + log-quick-based acosh/asinh/atanh |
| atan | 1.979 | 2.591 | 3.203 | NONE -> LIBM-F64-INVHYP-QUICK | 6 | (b) quick asin/acos/atan + log-quick-based acosh/asinh/atanh |
| cbrt | 1.272 | 2.333 | 2.537 | NONE -> LIBM-F64-INVHYP-QUICK | 6 | (b) quick asin/acos/atan + log-quick-based acosh/asinh/atanh |
| sinh | 2.305 | 1.912 | 2.066 | NONE -> LIBM-F64-INVHYP-QUICK | 6 | (b) quick asin/acos/atan + log-quick-based acosh/asinh/atanh |
| atan2 | 1.256 | 1.797 | 1.648 | NONE -> LIBM-F64-INVHYP-QUICK | 6 | (b) quick asin/acos/atan + log-quick-based acosh/asinh/atanh |
| asinhl | 1177.956 | 410.470 | 209.899 | NONE -> LIBM-LD-EXPLOG-INT | 7 | D-10.52 integer fixed point; wide_dd as tier 2 |
| acoshl | 960.807 | 354.335 | 197.191 | NONE -> LIBM-LD-EXPLOG-INT | 7 | D-10.52 integer fixed point; wide_dd as tier 2 |
| atanhl | 749.566 | 312.135 | 188.027 | NONE -> LIBM-LD-EXPLOG-INT | 7 | D-10.52 integer fixed point; wide_dd as tier 2 |
| tanhl | 425.428 | 143.602 | 44.886 | NONE -> LIBM-LD-EXPLOG-INT | 7 | D-10.52 integer fixed point; wide_dd as tier 2 |
| sinhl | 335.596 | 122.878 | 39.917 | NONE -> LIBM-LD-EXPLOG-INT | 7 | D-10.52 integer fixed point; wide_dd as tier 2 |
| coshl | 378.146 | 117.928 | 47.871 | NONE -> LIBM-LD-EXPLOG-INT | 7 | D-10.52 integer fixed point; wide_dd as tier 2 |
| powl | 151.446 | 77.803 | 48.542 | NONE -> LIBM-LD-EXPLOG-INT | 7 | D-10.52 integer fixed point; wide_dd as tier 2 |
| log1pl | 31.404 | 11.671 | 6.235 | NONE -> LIBM-LD-EXPLOG-INT | 7 | D-10.52 integer fixed point; wide_dd as tier 2 |
| log10l | 26.358 | 11.581 | 6.221 | NONE -> LIBM-LD-EXPLOG-INT | 7 | D-10.52 integer fixed point; wide_dd as tier 2 |
| log2l | 26.327 | 11.359 | 6.224 | NONE -> LIBM-LD-EXPLOG-INT | 7 | D-10.52 integer fixed point; wide_dd as tier 2 |
| logl | 23.996 | 10.973 | 5.979 | NONE -> LIBM-LD-EXPLOG-INT | 7 | D-10.52 integer fixed point; wide_dd as tier 2 |
| exp2l | 18.788 | 6.834 | 2.740 | NONE -> LIBM-LD-EXPLOG-INT | 7 | D-10.52 integer fixed point; wide_dd as tier 2 |
| expm1l | 16.976 | 5.089 | 1.689 | NONE -> LIBM-LD-EXPLOG-INT | 7 | D-10.52 integer fixed point; wide_dd as tier 2 |
| exp10l | 14.411 | 4.968 | 2.019 | NONE -> LIBM-LD-EXPLOG-INT | 7 | D-10.52 integer fixed point; wide_dd as tier 2 |
| expl | 12.484 | 4.521 | 1.903 | NONE -> LIBM-LD-EXPLOG-INT | 7 | D-10.52 integer fixed point; wide_dd as tier 2 |
| asinl | 1683.064 | 817.355 | 475.766 | NONE -> LIBM-LD-TRIG-INVERSE-INT | 8 | D-10.52 integer kernels of LD-TRIG-INT |
| atanl | 7377.101 | 457.419 | 288.780 | NONE -> LIBM-LD-TRIG-INVERSE-INT | 8 | D-10.52 integer kernels of LD-TRIG-INT |
| atan2l | 3697.263 | 449.396 | 291.352 | NONE -> LIBM-LD-TRIG-INVERSE-INT | 8 | D-10.52 integer kernels of LD-TRIG-INT |
| acosl | 3934.333 | 395.671 | 265.302 | NONE -> LIBM-LD-TRIG-INVERSE-INT | 8 | D-10.52 integer kernels of LD-TRIG-INT |
| tanl | 82.264 | 117.058 | 90.153 | NONE -> LIBM-LD-TRIG-INVERSE-INT | 8 | D-10.52 integer kernels of LD-TRIG-INT |
| ynf | 641.378 | 763.086 | 912.498 | NONE -> LIBM-F64-BESSEL-N | 9 | recurrences on the fast j0/j1/y0/y1 tiers + test |
| yn | 438.185 | 554.980 | 628.331 | NONE -> LIBM-F64-BESSEL-N | 9 | recurrences on the fast j0/j1/y0/y1 tiers + test |
| jnf | 165.021 | 246.848 | 289.772 | NONE -> LIBM-F64-BESSEL-N | 9 | recurrences on the fast j0/j1/y0/y1 tiers + test |
| jn | 112.881 | 173.914 | 198.239 | NONE -> LIBM-F64-BESSEL-N | 9 | recurrences on the fast j0/j1/y0/y1 tiers + test |
| lgammaf | 7.769 | 9.178 | 9.084 | NONE -> LIBM-F32-REST | 10 | (a) as LIBM-F32-ONEPASS |
| asinf | 3.659 | 8.599 | 5.828 | NONE -> LIBM-F32-REST | 10 | (a) as LIBM-F32-ONEPASS |
| acosf | 3.158 | 6.619 | 4.741 | NONE -> LIBM-F32-REST | 10 | (a) as LIBM-F32-ONEPASS |
| atanf | 2.413 | 2.971 | 2.660 | NONE -> LIBM-F32-REST | 10 | (a) as LIBM-F32-ONEPASS |
| acoshf | 1.182 | 2.354 | 3.802 | NONE -> LIBM-F32-REST | 10 | (a) as LIBM-F32-ONEPASS |
| asinhf | 1.124 | 2.273 | 3.066 | NONE -> LIBM-F32-REST | 10 | (a) as LIBM-F32-ONEPASS |
| erfcf | 1.327 | 1.802 | 2.010 | NONE -> LIBM-F32-REST | 10 | (a) as LIBM-F32-ONEPASS |
| hypotf | 1.372 | 1.724 | 1.418 | NONE -> LIBM-F32-REST | 10 | (a) as LIBM-F32-ONEPASS |
| atan2f | 1.371 | 1.688 | 1.348 | NONE -> LIBM-F32-REST | 10 | (a) as LIBM-F32-ONEPASS |
| erff | 1.277 | 1.658 | 1.794 | NONE -> LIBM-F32-REST | 10 | (a) as LIBM-F32-ONEPASS |
| cbrtf | 1.178 | 1.243 | 1.373 | NONE -> LIBM-F32-REST | 10 | (a) as LIBM-F32-ONEPASS |
| atanhf | 0.868 | 0.984 | 1.163 | NONE -> LIBM-F32-REST | 10 | (a) as LIBM-F32-ONEPASS |
| erfc | 1.829 | 2.667 | 2.388 | NONE -> LIBM-F64-SPECIAL-QUICK | 11 | (b) quick phase ahead of today's fast tiers |
| erf | 1.388 | 2.063 | 2.040 | NONE -> LIBM-F64-SPECIAL-QUICK | 11 | (b) quick phase ahead of today's fast tiers |
| lgamma | 1.564 | 1.788 | 2.357 | NONE -> LIBM-F64-SPECIAL-QUICK | 11 | (b) quick phase ahead of today's fast tiers |
| tgamma | 1.144 | 1.489 | 1.738 | NONE -> LIBM-F64-SPECIAL-QUICK | 11 | (b) quick phase ahead of today's fast tiers |
| j1 | 1.093 | 1.326 | 1.659 | NONE -> LIBM-F64-SPECIAL-QUICK | 11 | (b) quick phase ahead of today's fast tiers |
| j0 | 1.073 | 1.274 | 1.681 | NONE -> LIBM-F64-SPECIAL-QUICK | 11 | (b) quick phase ahead of today's fast tiers |
| expf_upward | 4.758 | 18.337 | 17.883 | NONE -> LIBM-DIRECTED-INPLACE | 12 | quick phase evaluated in the caller's mode (own proof); P2 |
| sinf_upward | 4.374 | 14.503 | 18.497 | NONE -> LIBM-DIRECTED-INPLACE | 12 | quick phase evaluated in the caller's mode (own proof); P2 |
| log_upward | 4.482 | 14.115 | 16.151 | NONE -> LIBM-DIRECTED-INPLACE | 12 | quick phase evaluated in the caller's mode (own proof); P2 |
| exp_upward | 4.776 | 11.221 | 15.880 | NONE -> LIBM-DIRECTED-INPLACE | 12 | quick phase evaluated in the caller's mode (own proof); P2 |
| pow_upward | 4.575 | 7.130 | 8.899 | NONE -> LIBM-DIRECTED-INPLACE | 12 | quick phase evaluated in the caller's mode (own proof); P2 |
| sin_upward | 3.733 | 5.657 | 5.224 | NONE -> LIBM-DIRECTED-INPLACE | 12 | quick phase evaluated in the caller's mode (own proof); P2 |
| sqrtl | 2.767 | 3.581 | 2.216 | NONE -> LIBM-SQRT-ENTRY | 13 | entry/envelope only (one hardware instruction) |
| sqrtf | 1.823 | 2.091 | 1.135 | NONE -> LIBM-SQRT-ENTRY | 13 | entry/envelope only (one hardware instruction) |
| sqrt | 1.939 | 1.504 | 1.225 | NONE -> LIBM-SQRT-ENTRY | 13 | entry/envelope only (one hardware instruction) |
| lgammal | 979.649 | 450.873 | 271.032 | NONE -> LIBM-LD-SPECIAL | 14 | D-10.52 kernels |
| erfcl | 910.287 | 298.669 | 124.607 | NONE -> LIBM-LD-SPECIAL | 14 | D-10.52 kernels |
| erfl | 921.429 | 298.633 | 124.250 | NONE -> LIBM-LD-SPECIAL | 14 | D-10.52 kernels |
| tgammal | 612.485 | 250.644 | 123.293 | NONE -> LIBM-LD-SPECIAL | 14 | D-10.52 kernels |
| y1l | 191.050 | 231.125 | 167.639 | NONE -> LIBM-LD-SPECIAL | 14 | D-10.52 kernels |
| y0l | 191.141 | 230.823 | 166.767 | NONE -> LIBM-LD-SPECIAL | 14 | D-10.52 kernels |
| ynl | 94.158 | 114.701 | 83.581 | NONE -> LIBM-LD-SPECIAL | 14 | D-10.52 kernels |
| j0l | 50.060 | 76.097 | 58.473 | NONE -> LIBM-LD-SPECIAL | 14 | D-10.52 kernels |
| j1l | 49.981 | 76.095 | 58.491 | NONE -> LIBM-LD-SPECIAL | 14 | D-10.52 kernels |
| cbrtl | 231.019 | 71.538 | 44.387 | NONE -> LIBM-LD-SPECIAL | 14 | D-10.52 kernels |
| jnl | 24.306 | 35.936 | 28.375 | NONE -> LIBM-LD-SPECIAL | 14 | D-10.52 kernels |
| hypotl | 42.333 | 18.770 | 26.143 | NONE -> LIBM-LD-SPECIAL | 14 | D-10.52 kernels |
| atanf128 | 27.204 | 27.137 | 19.117 | NONE -> LIBM-F128-INT | 15 | (d) three-limb fixed point, D-10.52 |
| atan2f128 | 26.111 | 24.350 | 17.883 | NONE -> LIBM-F128-INT | 15 | (d) three-limb fixed point, D-10.52 |
| cosf128 | 11.367 | 20.102 | 13.640 | NONE -> LIBM-F128-INT | 15 | (d) three-limb fixed point, D-10.52 |
| sinf128 | 11.368 | 19.937 | 13.999 | NONE -> LIBM-F128-INT | 15 | (d) three-limb fixed point, D-10.52 |
| y0f128 | 13.065 | 19.374 | 13.624 | NONE -> LIBM-F128-INT | 15 | (d) three-limb fixed point, D-10.52 |
| y1f128 | 12.978 | 19.202 | 13.520 | NONE -> LIBM-F128-INT | 15 | (d) three-limb fixed point, D-10.52 |
| tgammaf128 | 14.804 | 18.620 | 13.043 | NONE -> LIBM-F128-INT | 15 | (d) three-limb fixed point, D-10.52 |
| asinf128 | 18.839 | 18.594 | 12.970 | NONE -> LIBM-F128-INT | 15 | (d) three-limb fixed point, D-10.52 |
| acosf128 | 18.450 | 18.496 | 12.996 | NONE -> LIBM-F128-INT | 15 | (d) three-limb fixed point, D-10.52 |
| tanf128 | 10.761 | 16.845 | 11.849 | NONE -> LIBM-F128-INT | 15 | (d) three-limb fixed point, D-10.52 |
| log2f128 | 10.483 | 16.610 | 11.860 | NONE -> LIBM-F128-INT | 15 | (d) three-limb fixed point, D-10.52 |
| logf128 | 9.071 | 15.845 | 10.690 | NONE -> LIBM-F128-INT | 15 | (d) three-limb fixed point, D-10.52 |
| log10f128 | 9.412 | 15.261 | 10.544 | NONE -> LIBM-F128-INT | 15 | (d) three-limb fixed point, D-10.52 |
| erfcf128 | 9.550 | 13.922 | 9.940 | NONE -> LIBM-F128-INT | 15 | (d) three-limb fixed point, D-10.52 |
| erff128 | 9.464 | 13.829 | 9.823 | NONE -> LIBM-F128-INT | 15 | (d) three-limb fixed point, D-10.52 |
| log1pf128 | 7.867 | 13.334 | 9.179 | NONE -> LIBM-F128-INT | 15 | (d) three-limb fixed point, D-10.52 |
| atanhf128 | 6.912 | 10.845 | 7.544 | NONE -> LIBM-F128-INT | 15 | (d) three-limb fixed point, D-10.52 |
| acoshf128 | 7.838 | 10.366 | 7.449 | NONE -> LIBM-F128-INT | 15 | (d) three-limb fixed point, D-10.52 |
| lgammaf128 | 7.898 | 10.053 | 6.915 | NONE -> LIBM-F128-INT | 15 | (d) three-limb fixed point, D-10.52 |
| ynf128 | 6.445 | 9.548 | 6.816 | NONE -> LIBM-F128-INT | 15 | (d) three-limb fixed point, D-10.52 |
| asinhf128 | 7.481 | 9.520 | 6.936 | NONE -> LIBM-F128-INT | 15 | (d) three-limb fixed point, D-10.52 |
| powf128 | 5.211 | 8.422 | 5.834 | NONE -> LIBM-F128-INT | 15 | (d) three-limb fixed point, D-10.52 |
| coshf128 | 5.860 | 7.182 | 5.404 | NONE -> LIBM-F128-INT | 15 | (d) three-limb fixed point, D-10.52 |
| expf128 | 5.023 | 6.784 | 5.079 | NONE -> LIBM-F128-INT | 15 | (d) three-limb fixed point, D-10.52 |
| j0f128 | 3.551 | 6.772 | 4.627 | NONE -> LIBM-F128-INT | 15 | (d) three-limb fixed point, D-10.52 |
| tanhf128 | 4.938 | 6.745 | 4.962 | NONE -> LIBM-F128-INT | 15 | (d) three-limb fixed point, D-10.52 |
| j1f128 | 3.522 | 6.713 | 4.572 | NONE -> LIBM-F128-INT | 15 | (d) three-limb fixed point, D-10.52 |
| exp2f128 | 4.682 | 6.336 | 4.826 | NONE -> LIBM-F128-INT | 15 | (d) three-limb fixed point, D-10.52 |
| sinhf128 | 4.216 | 6.080 | 4.434 | NONE -> LIBM-F128-INT | 15 | (d) three-limb fixed point, D-10.52 |
| expm1f128 | 3.676 | 5.928 | 4.248 | NONE -> LIBM-F128-INT | 15 | (d) three-limb fixed point, D-10.52 |
| cbrtf128 | 6.660 | 5.167 | 3.965 | NONE -> LIBM-F128-INT | 15 | (d) three-limb fixed point, D-10.52 |
| exp10f128 | 2.595 | 3.521 | 2.625 | NONE -> LIBM-F128-INT | 15 | (d) three-limb fixed point, D-10.52 |
| jnf128 | 1.632 | 3.005 | 2.111 | NONE -> LIBM-F128-INT | 15 | (d) three-limb fixed point, D-10.52 |
| sqrtf128 | 3.895 | 1.728 | 4.503 | NONE -> LIBM-F128-INT | 15 | (d) three-limb fixed point, D-10.52 |
| hypotf128 | 1.425 | 0.888 | 1.564 | NONE -> LIBM-F128-INT | 15 | (d) three-limb fixed point, D-10.52 |
| cpowf | 740.644 | 555.264 | 494.705 | NONE -> LIBM-CPLX-REST | 16 | (e) D-10.53 after LIBM-CPLX-EXP-TIER |
| cpow | 457.493 | 498.311 | 561.984 | NONE -> LIBM-CPLX-REST | 16 | (e) D-10.53 after LIBM-CPLX-EXP-TIER |
| csin | 285.789 | 366.469 | 406.067 | NONE -> LIBM-CPLX-REST | 16 | (e) D-10.53 after LIBM-CPLX-EXP-TIER |
| csinf | 380.868 | 322.069 | 304.444 | NONE -> LIBM-CPLX-REST | 16 | (e) D-10.53 after LIBM-CPLX-EXP-TIER |
| clogl | 780.743 | 307.599 | 222.574 | NONE -> LIBM-CPLX-REST | 16 | (e) D-10.53 after LIBM-CPLX-EXP-TIER |
| clogf | 264.957 | 241.181 | 213.697 | NONE -> LIBM-CPLX-REST | 16 | (e) D-10.53 after LIBM-CPLX-EXP-TIER |
| clog | 158.723 | 214.837 | 255.969 | NONE -> LIBM-CPLX-REST | 16 | (e) D-10.53 after LIBM-CPLX-EXP-TIER |
| cpowl | 226.544 | 165.151 | 103.156 | NONE -> LIBM-CPLX-REST | 16 | (e) D-10.53 after LIBM-CPLX-EXP-TIER |
| csqrt | 281.492 | 162.508 | 196.265 | NONE -> LIBM-CPLX-REST | 16 | (e) D-10.53 after LIBM-CPLX-EXP-TIER |
| csqrtf | 329.293 | 115.977 | 98.245 | NONE -> LIBM-CPLX-REST | 16 | (e) D-10.53 after LIBM-CPLX-EXP-TIER |
| cexpl | 101.012 | 106.120 | 61.042 | NONE -> LIBM-CPLX-REST | 16 | (e) D-10.53 after LIBM-CPLX-EXP-TIER |
| csqrtl | 216.053 | 82.307 | 96.769 | NONE -> LIBM-CPLX-REST | 16 | (e) D-10.53 after LIBM-CPLX-EXP-TIER |

Rows not slower on either class (excluded): sinhf tanhf tgammaf

| group | rank | precision | rows | median time IVB | max time IVB | median time N100 |
|---|---:|---|---:|---:|---:|---:|
| LIBM-F64-EXP-QUICK | - | f64 | 5 | 2.90 | 3.69 | 3.19 |
| LIBM-LD-TRIG-INT | - | long double | 2 | 112.38 | 112.72 | 88.40 |
| LIBM-CPLX-EXP-TIER | - | complex | 2 | 663.20 | 704.95 | 677.59 |
| re-time | - |  | 9 | 1.21 | 1.31 | 1.24 |
| LIBM-F64-LOG-QUICK | 1 | f64 | 4 | 4.12 | 5.17 | 4.71 |
| LIBM-F64-TRIG-QUICK | 2 | f64 | 4 | 2.52 | 4.58 | 2.38 |
| LIBM-F64-POW-QUICK | 3 | f64 | 1 | 3.35 | 3.35 | 3.58 |
| LIBM-F32-ONEPASS | 4 | f32 | 11 | 2.31 | 4.47 | 3.06 |
| LIBM-F64-EXP-FAMILY-QUICK | 5 | f64 | 3 | 5.49 | 5.89 | 3.51 |
| LIBM-F64-INVHYP-QUICK | 6 | f64 | 12 | 3.53 | 15.45 | 3.99 |
| LIBM-LD-EXPLOG-INT | 7 | long double | 15 | 11.67 | 410.47 | 6.24 |
| LIBM-LD-TRIG-INVERSE-INT | 8 | long double | 5 | 449.40 | 817.36 | 288.78 |
| LIBM-F64-BESSEL-N | 9 | f64/f32 | 4 | 400.91 | 763.09 | 459.05 |
| LIBM-F32-REST | 10 | f32 | 12 | 2.04 | 9.18 | 2.33 |
| LIBM-F64-SPECIAL-QUICK | 11 | f64 | 6 | 1.64 | 2.67 | 1.89 |
| LIBM-DIRECTED-INPLACE | 12 | f64/f32 | 6 | 12.67 | 18.34 | 16.02 |
| LIBM-SQRT-ENTRY | 13 | all | 3 | 2.09 | 3.58 | 1.23 |
| LIBM-LD-SPECIAL | 14 | long double | 12 | 172.76 | 450.87 | 103.44 |
| LIBM-F128-INT | 15 | f128 | 35 | 10.37 | 27.14 | 7.45 |
| LIBM-CPLX-REST | 16 | complex | 12 | 228.01 | 555.26 | 218.14 |

slower rows: 163 of 166
