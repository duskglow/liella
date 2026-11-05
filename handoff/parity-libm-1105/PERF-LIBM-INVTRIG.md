# PERF-LIBM-INVTRIG: PERF-LIBM-INVTRIG: asin, acos, atan, atan2, hypot (f64, f32) run the accurate double-double path for every call

Id: PERF-LIBM-INVTRIG
Kind: code
Priority: 60
Size: M
Host: -
Model: sonnet
Effort: high
BUDGET: 70 tool calls / 140k

## Rows (cbench_libm, mei, origin/next 0005889c, `--runs 3`; instr ratio = Liella/glibc instructions:u, time ratio = Liella/glibc ns)

| Row | instr/op Liella | instr/op glibc | instr ratio | time ratio |
|---|---:|---:|---:|---:|
| asinf | 1549.7 | 53.5 | 28.96 | 83.97 |
| asin | 1558.5 | 75.0 | 20.79 | 63.95 |
| acosf | 1555.5 | 61.2 | 25.42 | 62.31 |
| acos | 1567.2 | 80.1 | 19.57 | 62.80 |
| atanf | 1379.4 | 66.6 | 20.70 | 45.40 |
| atan | 1391.1 | 78.7 | 17.67 | 41.80 |
| hypotf | 48.1 | 35.1 | 1.37 | 1.82 |
| hypot | 3811.8 | 63.8 | 59.75 | 49.14 |
| atan2f | 1619.4 | 147.7 | 10.96 | 24.28 |
| atan2 | 1616.3 | 161.7 | 10.00 | 21.82 |

## Profile and hypothesis

percall: atan2 1693.4 vs 163.0 (10.4x); asin/acos 19.6-20.8x, atan 17.7-20.7x, hypot 59.8x (f64; hypotf 1.37 has a fast
path since MATH-M6-F32-HYPOT). Every f64 row is in the 1.4-1.7k class (Liella about 1.5k instructions against glibc's 75-80 for asin/acos/atan),
the same constant as expm1/tanh: `real.rs` forwards to `elem`/`hyper` double-double functions with no `fast64`/`fast32`
dispatch (only exp/log/pow/sin/cos/tan have one). f32 rows (asinf 29x, acosf 25x, atanf 20.7x, atan2f 11x) are 1.5k
for the same reason: they widen to the accurate f64 double-double path instead of a f64 polynomial.
Hypothesis: (1) f32: evaluate a minimax polynomial in f64 (error <= 2^-40 relative) and round: no Ziv fallback needed
except within 2^-40 of a midpoint, so cost ~50-70 instructions, below glibc's 78 (counted, atanf 66); (2) f64: a fast64
kernel with a table-based argument reduction (atan: 64-entry breakpoints; asin/acos via atan2 of the sqrt form) and
the double-double rounding test, falling back to the existing accurate path; (3) hypot: scale-free fast path
(sqrtsd on the double-double sum of squares, exactly rounded by the fma-free correction), the f32 pattern from hypotf.

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
- `perf-libm-invtrig-gate`: direct-tier gate (`cargo xtask affected --run`), plus clippy and `cargo xtask static`.
- `perf-libm-invtrig-plants`: plants in RAM copies (new copy dir per run, `timeout 600` per plant): break the new fast path's rounding test,
  its range filter and its dispatch condition; the accuracy tests must catch each (CAUGHT/SURVIVED, nonzero on SURVIVED).
- `perf-libm-invtrig-accuracy`: the accuracy check named above for each touched function (exhaustive for f32 where feasible, else the
  suite's sample plus the hard-case lists), before/after identical results.
- `perf-libm-invtrig-counts`: `cargo xtask bench --compare --counters --runs 3 cbench_libm` before (origin/next) and after, whole group;
  report instructions:u and time for every row of the group and a no-row-slower statement for the rest of the file.
