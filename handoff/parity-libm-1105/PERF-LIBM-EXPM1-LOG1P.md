# PERF-LIBM-EXPM1-LOG1P: PERF-LIBM-EXPM1-LOG1P: expm1, log1p, exp10 and cbrt (f64 and f32) have no fast path

Id: PERF-LIBM-EXPM1-LOG1P
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
| exp10f | 69.1 | 43.1 | 1.61 | 3.23 |
| exp10 | 419.3 | 56.1 | 7.48 | 16.40 |
| expm1f | 161.2 | 91.4 | 1.76 | 1.38 |
| expm1 | 1371.3 | 86.4 | 15.88 | 26.05 |
| log1pf | 174.1 | 87.1 | 2.00 | 2.62 |
| log1p | 1335.9 | 83.4 | 16.01 | 33.13 |
| cbrtf | 161.8 | 137.3 | 1.18 | 1.42 |
| cbrt | 1069.3 | 148.3 | 7.21 | 13.02 |

## Profile and hypothesis

percall (instructions:u per call, mei, 6a36d693): expm1 1428.9 vs glibc 89.3 (16.0x). The f64 sibling `exp` has a counted fast path
(`fast64::exp`, 181 vs 67 in the bench), but `exp_log::expm1` (crates/math/src/exp_log.rs) goes straight to the
double-double `expm1_small` / `exp_dd` path for every operand with |x| > TINY; there is no `fast::expm1` the way
`trig::sin` calls `fast::sin`. log1p (16.0x) and exp10 (7.5x) show the same ratio class as expm1 (about 1.3-1.4k
instructions); cbrt (7.2x) is the same shape at 148 vs 1069. The f32 rows (expm1f 1.76, log1pf 2.0, exp10f 1.61, cbrtf 1.18)
are narrow gaps: the f32 path widens to f64 and has a fast32 kernel, so its excess is the envelope/rounding wrapper.
Hypothesis: a fast64 kernel (table-driven reduction, degree-5..7 polynomial, rounding test, Ziv fallback to the existing
double-double code) for expm1, log1p, exp10 and cbrt, built from the pieces `fast64::exp` / `fast64::log_kernel` already
have, takes the common case from ~1.4k to ~150-250. First-principles floor: docs/saturation/16 (exp 1.35x without FMA),
so ratios near 2x for the f64 rows without FMA are the realistic result unless the FMA path (math-f64-fma-path) is on;
state the dispatch used for the 1.05x claim.

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
- `perf-libm-expm1-log1p-gate`: direct-tier gate (`cargo xtask affected --run`), plus clippy and `cargo xtask static`.
- `perf-libm-expm1-log1p-plants`: plants in RAM copies (new copy dir per run, `timeout 600` per plant): break the new fast path's rounding test,
  its range filter and its dispatch condition; the accuracy tests must catch each (CAUGHT/SURVIVED, nonzero on SURVIVED).
- `perf-libm-expm1-log1p-accuracy`: the accuracy check named above for each touched function (exhaustive for f32 where feasible, else the
  suite's sample plus the hard-case lists), before/after identical results.
- `perf-libm-expm1-log1p-counts`: `cargo xtask bench --compare --counters --runs 3 cbench_libm` before (origin/next) and after, whole group;
  report instructions:u and time for every row of the group and a no-row-slower statement for the rest of the file.
