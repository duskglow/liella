# PERF-LIBM-HYPERBOLIC: PERF-LIBM-HYPERBOLIC: sinh, cosh, tanh, asinh, acosh, atanh (f64, f32): the accurate path for every call

Id: PERF-LIBM-HYPERBOLIC
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
| sinhf | 1520.4 | 145.4 | 10.46 | 15.49 |
| sinh | 1548.6 | 144.7 | 10.70 | 17.80 |
| coshf | 1505.4 | 78.8 | 19.09 | 47.66 |
| cosh | 1525.9 | 84.5 | 18.07 | 45.03 |
| tanhf | 1521.4 | 131.2 | 11.59 | 18.95 |
| tanh | 1548.2 | 131.6 | 11.77 | 21.92 |
| asinhf | 1482.6 | 88.9 | 16.68 | 31.51 |
| asinh | 1510.0 | 99.3 | 15.21 | 27.29 |
| acoshf | 1445.0 | 72.9 | 19.81 | 42.34 |
| acosh | 1449.9 | 83.6 | 17.35 | 32.53 |
| atanhf | 1495.0 | 115.6 | 12.93 | 24.48 |
| atanh | 1512.6 | 112.6 | 13.43 | 26.50 |

## Profile and hypothesis

percall: tanh 1591.6 vs 134.7 (11.8x); the group is 10.5-19.8x on every row (f32 and f64 alike, sinhf 10.5, coshf 19.1, acoshf
19.8). `hyper.rs` computes each from `exp_dd`/`ln_dd` double-double kernels with the accurate result assembly and
no fast64/fast32 entry. They cost about one `exp_dd` + a double-double division/log (about 1.4-1.6k).
Hypothesis: build them on the fast exp / log kernels: sinh, cosh, tanh from one fast `exp` evaluation (as double-double e^x
with e^-x by the reciprocal, tanh by one division; the small-|x| range by expm1 series, which depends on PERF-LIBM-EXPM1-LOG1P,
so land that first or share its kernel), asinh/acosh/atanh from the fast log kernel. For f32 widen to f64 and use a plain
exp-based formula with the f64 result then rounded (error 2^-45 is enough off midpoints; Ziv fallback to the present
path). Expect ~150-250 instructions per call.

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
- `perf-libm-hyperbolic-gate`: direct-tier gate (`cargo xtask affected --run`), plus clippy and `cargo xtask static`.
- `perf-libm-hyperbolic-plants`: plants in RAM copies (new copy dir per run, `timeout 600` per plant): break the new fast path's rounding test,
  its range filter and its dispatch condition; the accuracy tests must catch each (CAUGHT/SURVIVED, nonzero on SURVIVED).
- `perf-libm-hyperbolic-accuracy`: the accuracy check named above for each touched function (exhaustive for f32 where feasible, else the
  suite's sample plus the hard-case lists), before/after identical results.
- `perf-libm-hyperbolic-counts`: `cargo xtask bench --compare --counters --runs 3 cbench_libm` before (origin/next) and after, whole group;
  report instructions:u and time for every row of the group and a no-row-slower statement for the rest of the file.
