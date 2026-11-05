# LIBM-F64-FAST-KERNELS: double-only fast kernels for sin/cos/tan and exp/log/pow, with FMA dispatch

Id: LIBM-F64-FAST-KERNELS
Kind: code
Priority: 90
Size: L
Host: -
Stacks-on: LIBM-MP-CORE-SPEED (the direct double-double rounder, item 4)
Model: opus
Effort: high
BUDGET: 90 tool calls / 180k

## The change

LIBM-SLOW-PATH-CAUSE (handoff/libm-slow-path-cause/report.md §3.1, §3.2, §4) measured the existing `double` fast
tiers:

| Function | Fast share | Cost (instructions) | Reference |
|---|---|---|---|
| sin, cos | 99.4% | 314-320 | 100 |
| tan | 98.9% | 481 | 105 |
| exp, log, pow | 99.999% | 221-233 (fast kernel only) | 67-84 |

The kernels evaluate everything in double-double with Dekker products and no FMA. sin's bound is loose (about
2^-61, from the 0.64% fallback rate). In directed modes, a passing test still runs `elem::finish`
(sin upward: 865).

**Redesign the kernels** (`trig::fast`, `fast64`):
* Cody-Waite reduction in double plus a single double-double correction.
* The polynomial in double (Estrin), with only the leading term or terms in double-double.
* A proven bound of about 2^-66, or tighter where cheap.
* A rounding test that costs the same in all four modes.
* `exp10` shares the exp kernel through a `log10(2)/64` split.

Add the FMA variant through the existing run-time dispatch (ruling #932 allows it). Both variants are correctly
rounded, so their results are bit-identical: test both on the same operands.

**Rules:**
* Proof in the module docs, term by term (PRINCIPLES §4).
* The accurate path stays as the fallback, unchanged.
* The fallback-rate tests (`stats::check_fallback`) get ceilings from the new bound: about 1 in 2^12 to nearest.
* Coordinate with the TRIG work on `task/perf-libm-trig` (the in-place nearest entries): build on its head if it
  has landed.

**Target:** at most 1.05x instructions with FMA, at most 1.3x without, per row (sin, cos, tan, sincos, exp, exp2,
exp10, log, log2, log10, pow, and their `_upward` rows).

**JOBS:**
* the gate (direct tier);
* plants:
  * the bound halved;
  * a reduction constant off by one ulp;
  * a directed-test sign flip;
  * the FMA-dispatch condition inverted;
* the accuracy check (the oracle suites, plus a 10^8 random sweep per function in all modes comparing against the
  accurate path);
* counts before and after (`cbench_libm`, the rows above), both on an FMA host and with the FMA variant disabled.
