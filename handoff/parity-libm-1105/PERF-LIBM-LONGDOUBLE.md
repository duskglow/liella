# PERF-LIBM-LONGDOUBLE: PERF-LIBM-LONGDOUBLE: x87 long double rows: every libm function pays the 384-bit wide kernel

Id: PERF-LIBM-LONGDOUBLE
Kind: code
Priority: 60
Size: L
Host: -
Model: sonnet
Effort: high
BUDGET: 70 tool calls / 140k

## Rows (cbench_libm, mei, origin/next 0005889c, `--runs 3`; instr ratio = Liella/glibc instructions:u, time ratio = Liella/glibc ns)

| Row | instr/op Liella | instr/op glibc | instr ratio | time ratio |
|---|---:|---:|---:|---:|
| expl | 40218.1 | 82.5 | 487.42 | 134.64 |
| exp2l | 40839.8 | 63.4 | 644.33 | 208.69 |
| exp10l | 136505.3 | 82.5 | 1654.81 | 488.82 |
| expm1l | 41105.2 | 70.5 | 582.87 | 140.17 |
| logl | 86873.2 | 47.3 | 1837.34 | 691.29 |
| log2l | 95210.4 | 44.3 | 2149.17 | 726.10 |
| log10l | 191375.6 | 44.3 | 4323.99 | 1497.90 |
| log1pl | 84795.8 | 46.0 | 1842.55 | 637.46 |
| cbrtl | 45958.7 | 157.7 | 291.44 | 85.13 |
| sinl | 96973.8 | 1081.7 | 89.65 | 123.08 |
| cosl | 96972.9 | 1081.8 | 89.64 | 123.15 |
| tanl | 105696.0 | 1046.1 | 101.04 | 129.68 |
| asinl | 218887.2 | 102.9 | 2126.42 | 959.53 |
| acosl | 222476.6 | 44.6 | 4993.64 | 417.05 |
| atanl | 201682.0 | 21.5 | 9398.92 | 474.43 |
| sinhl | 51766.1 | 126.4 | 409.43 | 131.62 |
| coshl | 51755.0 | 112.2 | 461.30 | 128.27 |
| tanhl | 60402.6 | 115.9 | 521.34 | 154.33 |
| asinhl | 107321.9 | 73.4 | 1462.05 | 478.45 |
| acoshl | 101810.6 | 85.4 | 1192.45 | 412.75 |
| atanhl | 94229.8 | 101.3 | 929.84 | 379.08 |
| erfl | 212094.1 | 189.1 | 1121.89 | 340.76 |
| erfcl | 212801.1 | 191.8 | 1109.58 | 340.75 |
| tgammal | 624436.6 | 751.0 | 831.51 | 315.64 |
| lgammal | 585792.5 | 437.0 | 1340.43 | 581.44 |
| j0l | 142468.1 | 2420.2 | 58.87 | 86.64 |
| j1l | 142518.2 | 2426.6 | 58.73 | 86.95 |
| y0l | 595545.8 | 2469.7 | 241.14 | 266.75 |
| y1l | 595892.9 | 2470.7 | 241.18 | 266.90 |
| powl | 123343.1 | 664.9 | 185.50 | 83.42 |
| hypotl | 4478.4 | 105.7 | 42.37 | 19.52 |
| atan2l | 208802.1 | 44.5 | 4692.50 | 472.56 |
| jnl | 142671.7 | 4999.0 | 28.54 | 41.78 |
| ynl | 599018.8 | 5044.6 | 118.74 | 132.83 |

## Profile and hypothesis

percall: expl 40964.0 vs 87.0 (470.9x); the group runs 29x to 9399x. `wide.rs` is the 384-bit kernel documented for x87 `long
double` and `_Float128` ("the 384-bit one for x87 long double and _Float128", crates/math/src/lib.rs), so the
long double entry points use the same kernel as the binary128 ones; the f128 rows are 4-34x because glibc's f128 is a software
format, while the long double rows compete with x87 hardware (fyl2x, f2xm1: 87 instructions).
Hypothesis: for the 64-bit significand a 128-bit fixed-point tier (u128 or two-limb double-double of f64 pairs gives only
~106 bits, enough: a 64-bit target needs ~2^-75 for a Ziv margin) is sufficient for the common case; keep the 384-bit
path as the fallback (decided by a rounding test). First count, per function, the share of calls the fast tier would decide.
Start with the exp/log/trig/pow kernels that the other long double rows (sinh via exp, asin via atan, ...) build on, so one kernel
moves many rows. Parity <= 1.05x may be unreachable against x87 microcode instructions (glibc's expl is 87 instructions
of which fyl2x/f2xm1 count as one each); state the counted floor and propose a budget row for the rows that stay
above.

## The change

Target: every row above at <= 1.05x instructions:u against the host C library (the cbench_libm row, `cargo xtask bench --compare
--counters --runs 3 cbench_libm`), accuracy unchanged and proven (the existing correctly-rounded accuracy suites and the
oracle comparison for every function touched, on the full operand ranges plus the special values and all four rounding modes),
no row slower (time and instructions, every cbench_libm row, not only the group's). Hardware-options rule: run-time
dispatch (FMA, x86-64-v3) is allowed; a dispatched path must return bit-identical results wherever the spec fixes the result
(correct rounding in round-to-nearest, the directed modes, errno and flags). If a row cannot reach 1.05x, say so with the
counted floor (the method of docs/saturation/16: a per-phase minimum against the counted total) and add a row to
docs/saturation/ rather than leaving it undocumented. Start from a call-level profile (validation/wl_percall.sh with
validation/percall/parity_libm1105.c, or a probe per function: the group's rows share a kernel, so one kernel change may
move several).

## JOBS (name each by its label, all on the final head)
- `perf-libm-longdouble-gate`: direct-tier gate (`cargo xtask affected --run`), plus clippy and `cargo xtask static`.
- `perf-libm-longdouble-plants`: plants in RAM copies (new copy dir per run, `timeout 600` per plant): break the new fast path's rounding test,
  its range filter and its dispatch condition; the accuracy tests must catch each (CAUGHT/SURVIVED, nonzero on SURVIVED).
- `perf-libm-longdouble-accuracy`: the accuracy check named above for each touched function (exhaustive for f32 where feasible, else the
  suite's sample plus the hard-case lists), before/after identical results.
- `perf-libm-longdouble-counts`: `cargo xtask bench --compare --counters --runs 3 cbench_libm` before (origin/next) and after, whole group;
  report instructions:u and time for every row of the group and a no-row-slower statement for the rest of the file.
