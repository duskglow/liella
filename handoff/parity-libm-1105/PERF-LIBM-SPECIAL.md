# PERF-LIBM-SPECIAL: PERF-LIBM-SPECIAL: erf, erfc, tgamma, lgamma, j0, j1, y0, y1, jn, yn (f64, f32): 100x to 2000x on every row

Id: PERF-LIBM-SPECIAL
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
| erff | 212107.7 | 135.3 | 1567.34 | 2270.58 |
| erf | 212121.4 | 153.8 | 1379.00 | 2323.17 |
| erfcf | 212798.8 | 138.7 | 1534.37 | 2240.88 |
| erfc | 212830.0 | 157.8 | 1349.11 | 2276.35 |
| tgammaf | 626252.4 | 476.5 | 1314.31 | 1426.43 |
| tgamma | 624450.5 | 609.5 | 1024.56 | 1082.64 |
| lgammaf | 587622.1 | 277.2 | 2119.67 | 2375.05 |
| lgamma | 585823.9 | 370.6 | 1580.75 | 1693.70 |
| j0f | 143198.9 | 278.5 | 514.21 | 932.84 |
| j0 | 142476.1 | 414.9 | 343.40 | 707.81 |
| j1f | 143244.2 | 283.0 | 506.23 | 921.06 |
| j1 | 142535.0 | 420.1 | 339.30 | 706.38 |
| y0f | 596419.1 | 295.0 | 2021.78 | 2739.53 |
| y0 | 595575.4 | 435.7 | 1366.94 | 2063.16 |
| y1f | 596753.9 | 296.9 | 2009.66 | 2701.04 |
| y1 | 595927.4 | 435.4 | 1368.81 | 2073.87 |
| jnf | 143419.5 | 697.3 | 205.69 | 355.19 |
| jn | 142679.9 | 975.1 | 146.32 | 282.48 |
| ynf | 599879.2 | 698.0 | 859.38 | 1095.55 |
| yn | 599035.2 | 980.4 | 611.04 | 892.49 |

## Profile and hypothesis

percall: erf 223222.1 vs 158.9 (1404x); same class for erfc (1349x), tgamma (1024x), lgamma (1581x), j0/j1 (340x),
y0/y1 (1370x), jn/yn (146x/611x). These are 140k-620k instructions per call: the 384-bit `wide` or `mp` multiprecision
kernel is run for every operand (no double-double fast tier), the cost of a series in 384-bit arithmetic. The f32 rows
(erff 1567x, lgammaf 2120x) pay the same, which proves the fast tier is simply absent, not the rounding fallback.
These percall totals do not break down by function; the first step is a call-count profile of `mp`/`wide` entry points
(add counters per probe) to confirm which kernel is hit. Hypothesis: a double-double tier (Ziv step 1) at ~2^-90 for erf/erfc
(series |x|<2, continued fraction/asymptotic beyond), lgamma/tgamma (Stirling with a shift, Lanczos not allowed:
use the classic Stirling series with exact argument shift in double-double), and j/y (Hankel asymptotic for large |x|,
power series small |x|), with the existing wide path kept as the fallback. The f32 rows can use plain f64 evaluation with a
2^-35 margin and round. This is a large group: split it by function family into sub-stories if one run cannot finish
all, but start with erf/erfc and the f32 rows, whose margin is cheapest. The target for this group is the instruction
class (<= 1.05x is the final goal; a first landing at <= 3x per row is acceptable if the remaining floor is counted).

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
- `perf-libm-special-gate`: direct-tier gate (`cargo xtask affected --run`), plus clippy and `cargo xtask static`.
- `perf-libm-special-plants`: plants in RAM copies (new copy dir per run, `timeout 600` per plant): break the new fast path's rounding test,
  its range filter and its dispatch condition; the accuracy tests must catch each (CAUGHT/SURVIVED, nonzero on SURVIVED).
- `perf-libm-special-accuracy`: the accuracy check named above for each touched function (exhaustive for f32 where feasible, else the
  suite's sample plus the hard-case lists), before/after identical results.
- `perf-libm-special-counts`: `cargo xtask bench --compare --counters --runs 3 cbench_libm` before (origin/next) and after, whole group;
  report instructions:u and time for every row of the group and a no-row-slower statement for the rest of the file.
