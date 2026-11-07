# LIBM-F64-EXP-FAMILY-QUICK: binary64 exp2, exp10 and expm1 on the exp quick table

Id: LIBM-F64-EXP-FAMILY-QUICK
Kind: code
Priority: 82
Size: M
Host: -
Model: opus
Effort: high
BUDGET: 80 tool calls / 160k
Stacks-on: LIBM-F64-EXP-QUICK-WIRE (it may already take exp2/exp10 "if the budget allows": read its hand-back and take
only what it left)

## The change

Decision row: D-10.51. Rows (time IVB / N100): exp2 5.49 / 3.51, exp10 5.89 / 5.21, expm1 2.04 / 2.09. Counts
(keke:libm-coverage-1107-percall, FMA): exp2 133 vs 54, exp10 150 vs 59, expm1 194 vs 89. Tier today: fast64 dd tier
on every call. The IVB time ratio is about twice the instruction ratio for exp2/exp10: check the SSE2 builds' counts
(keke:libm-coverage-1107-percall-sse2) and whether exp2/exp10 share the IVB exp regression (LIBM-F64-EXP-IVB-REGRESS).

METHOD: the quick kernel of LIBM-F64-EXP-QUICK (128-entry 2^(j/128) table, Taylor/minimax polynomial, `near`) with
its own reductions: exp2 needs no ln2 constant (k and j straight from x·128 by the shift; r = (x − k/128)·ln2 with one
exact product, FMA or Dekker on SSE2), exp10 multiplies by log2(10) in double-double (the 3-part constant), expm1 = the
exp kernel's T·(1 + r + q) − 1 assembled before rounding, with its own short polynomial for |x| < 2^-5 (cancellation)
[Tang, "Table-driven implementation of the expm1 function", ACM TOMS 18(2) 1992]. Estrin evaluation (f); SSE2 / VEX /
FMA builds (g); tier 2 today's kernels; exact cases (exp2 of integers, exp10 of small integers) ahead.

## Tests and plants
Existing: `crates/math/tests/fast64.rs`, `elem_products.rs`, `exact_products.rs` (exp2/exp10 exact results),
`math_double_fast_entry_sweep`; new 10^8-operand accuracy job (four modes) per function. Plants: exp2 j/k split off by
one, exp10 constant's low part dropped, expm1 small-|x| branch boundary moved, `near` always true: CAUGHT with msg:.

## JOBS
clippy + static, direct gate, plants, counts before/after (exp2, exp10, expm1, both builds), booked timing on Ivy Bridge
and N100. Targets <= 1.4x time.

Hand-back first line: `COMPLETE: exp2/exp10/expm1 quick, IVB <x>x / N100 <y>x` or INCOMPLETE:/DO NOT LAND:.
