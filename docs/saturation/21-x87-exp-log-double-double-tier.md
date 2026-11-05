## 21. x87 long double exp/log: the double-double first tier (LIBM-WIDE-DD-TIER, 2026-11-05, counted and timed)

`crates/math/src/wide_dd.rs`: a double-double approximation with a proved relative bound (`2^-100`; `expm1` where it
subtracts 1: `2^-92`) and Ziv's rounding test in front of the 384-bit route, for `expl`, `exp2l`, `exp10l`, `expm1l`,
`logl`, `log2l`, `log10l` and `log1pl`. Results stay correctly rounded in every mode (the tier refuses when the bound
cannot decide; the 384-bit route then runs). Spec §10.3, D-10.19.

**Counts.** `validation/percall/libm_wide_dd.c` (the `cbench_libm` operands; `*_full` = 64-bit significands),
`validation/wl_percall.sh`, Broadwell-EP build host (Xeon E5-2699 v4), instructions:u per call (sd 0). Base = 2c1e25b9 (`next`), new =
aa1eaeae. **Cycles.** The same binaries, cycles:u per call, `perf stat -r 5`, (C(N) − C(0))/N, Ivy Bridge timing host
booked and idle (load 0.00).

| Case | Instr base | Instr new | Instr glibc | Cycles base | Cycles new | Cycles glibc | new/glibc (cycles) |
|---|---:|---:|---:|---:|---:|---:|---:|
| `expl` | 33381.5 | 1054.4 | 87.0 | 23390.8 | 805.4 | 204.5 | 3.94 |
| `exp2l` | 34192.8 | 1207.9 | 68.0 | 23796.6 | 881.0 | 159.8 | 5.51 |
| `exp10l` | 34187.9 | 1205.9 | 87.0 | 23654.7 | 877.2 | 201.7 | 4.35 |
| `expm1l` | 33958.4 | 1213.9 | 75.0 | 23518.3 | 902.8 | 185.5 | 4.87 |
| `logl` | 71118.2 | 1161.5 | 52.2 | 51007.4 | 927.2 | 92.3 | 10.05 |
| `log2l` | 77937.5 | 1190.5 | 49.2 | 53726.1 | 957.7 | 89.6 | 10.69 |
| `log10l` | 78693.9 | 1194.5 | 49.2 | 53956.8 | 957.9 | 93.4 | 10.26 |
| `log1pl` | 70741.7 | 1451.0 | 50.9 | 50624.0 | 1065.5 | 93.4 | 11.41 |
| `expl_full` | 33162.4 | 1054.5 | 87.0 | 23176.2 | 800.6 | 202.9 | 3.95 |
| `logl_full` | 69442.6 | 1161.3 | 52.1 | 49999.4 | 926.2 | 90.6 | 10.22 |

28x to 66x fewer instructions and 27x to 56x fewer cycles than the base; still 4x to 11x glibc's cycles.

**What was tried.**

| Change | Result |
|---|---|
| Double-double tier with proved bounds, mp fallback | kept (above) |
| `log10`: exact-power test by `5^e` and one comparison in place of a 56-step `u128` scan of the powers of five | kept: `log10l` 2205.5 -> 1194.5 instructions (same results; exact 10^27 x87 and 10^48 `_Float128` tests) |

**Where one call goes now** (`validation/wl_trace.sh`, call 11, new): `expl` 1039 instructions: the kernel
(reduction, degree-11 double-double polynomial, table) about 560 (54%), `float::round::<F80>` 166 (16%), the
rounding test `eval`/`decide` 154 (15%), the boundary wrapper 71 (7%), `wide::exp` entry and decode 53 (5%).
`logl` 1147: kernel about 680 (59%), `float::round` 165, `decide` 154, wrapper 71, entry 53.

**Not saturated.** Open: the polynomial in full double-double (most high-order terms only need `double`); the test
and the generic rounder together (about 320, 31%) could round the double-double to the x87 bits directly. The glibc
figures come from a different method; matching them while staying correctly rounded is not claimed.
