# PERF-LIBM-TRIG: PERF-LIBM-TRIG: sin, cos, tan (f64, f32), the sin_huge and directed-rounding rows

Id: PERF-LIBM-TRIG
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
| sin_huge | 1789.4 | 365.7 | 4.89 | 6.29 |
| sin_upward | 955.6 | 109.2 | 8.75 | 11.87 |
| sinf_upward | 246.6 | 41.9 | 5.88 | 22.54 |
| sinf | 167.3 | 41.9 | 3.99 | 5.53 |
| sin | 340.2 | 100.2 | 3.39 | 6.38 |
| cosf | 167.3 | 42.1 | 3.97 | 5.57 |
| cos | 335.7 | 104.7 | 3.21 | 6.00 |
| tanf | 173.3 | 106.3 | 1.63 | 1.60 |
| tan | 496.1 | 107.6 | 4.61 | 7.85 |

## Profile and hypothesis

percall: sin 360.5 vs 104.6 (3.45x). `trig::sin` already has `fast::sin` (53-bit only) then `sin_accurate`; the bench
operands [-10,10] take the fast path, so the 3.4x is the fast kernel's own cost, the same shape docs/saturation/16 counted for exp
(argument reduction in double-double, double-double assembly, the settle/rounding test, the MXCSR read and range filter).
sin_huge (4.9x) is the Payne-Hanek-class reduction row; sin_upward / sinf_upward (8.8x / 5.9x) are the directed-mode envelope
(round-up leaves the fast path). The f32 rows (sinf 3.99, cosf 3.97, tanf 1.63) take `fast32`, whose excess is larger than
exp's (expf 1.8): the f32 sin/cos kernel is the cheapest place to start (a degree-7 polynomial in f64 needs no double-double).
Hypothesis: (1) f32: reduction by a single f64 multiply-subtract for |x| < 2^20 and a minimax polynomial, rounding-checked
by the f64 result's distance to a f32 midpoint; (2) f64: the saturation-16 changes applied to sin/cos (skip the
double-double reduction for |x| < pi/4, table-free polynomial of the Estrin form, cheaper rounding test); (3) directed
modes: a fast path that returns the faithful bracket (result and the settle test) instead of leaving for the accurate path.

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
- `perf-libm-trig-gate`: direct-tier gate (`cargo xtask affected --run`), plus clippy and `cargo xtask static`.
- `perf-libm-trig-plants`: plants in RAM copies (new copy dir per run, `timeout 600` per plant): break the new fast path's rounding test,
  its range filter and its dispatch condition; the accuracy tests must catch each (CAUGHT/SURVIVED, nonzero on SURVIVED).
- `perf-libm-trig-accuracy`: the accuracy check named above for each touched function (exhaustive for f32 where feasible, else the
  suite's sample plus the hard-case lists), before/after identical results.
- `perf-libm-trig-counts`: `cargo xtask bench --compare --counters --runs 3 cbench_libm` before (origin/next) and after, whole group;
  report instructions:u and time for every row of the group and a no-row-slower statement for the rest of the file.
