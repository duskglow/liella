# LIBM-F64-INVHYP-QUICK: binary64 acosh/asinh/atanh quick tier, then asin/acos/atan/cosh

Id: LIBM-F64-INVHYP-QUICK
Kind: code
Priority: 81
Size: L
Host: -
Model: opus
Effort: high
BUDGET: 90 tool calls / 180k
Stacks-on: LIBM-F64-LOG-QUICK (the quick log and log1p kernels)

## The change

Decision row: D-10.51. Rows (time IVB / N100): acosh 14.69 / 26.19, asinh 13.84 / 22.53, atanh 15.45 / 19.17, asin
9.79 / 8.39, acos 8.68 / 7.04, cosh 4.19 / 4.77, hypot 2.87 / 2.16, tanh 2.79 / 2.99, atan 2.59 / 3.20, cbrt 2.33 /
2.54, sinh 1.91 / 2.07, atan2 1.80 / 1.65. Counts (keke:libm-coverage-1107-percall, FMA): acosh 984 vs 86, asinh 1039
vs 102, atanh 1054 vs 115, asin 204 vs 78, acos 181 vs 83, cosh 279 vs 87.
Tier today: `crates/math/src/hyper.rs` `asinh`/`acosh`/`atanh` evaluate `log_dd`/`log1p_dd` with a double-double
sqrt and division on EVERY call (an accurate phase, ~10x glibc's instructions); asin/acos/atan run the invtrig fast
tier (LIBM-F64-INVTRIG-HYPOT-TIER) in double-double.

Part 1 (the largest gap, ~40 calls): acosh/asinh/atanh. METHOD (b): the argument in double + low part (sqrt(x²±1)
with one exact product and the hardware sqrt plus one Newton correction, or the Hull-Fairgrieve-Tang-style regime
split the code already proves), then LIBM-F64-LOG-QUICK's quick log/log1p on (hi, lo) to ~2^-62, `near` test; tier 2 =
today's dd code unchanged. atanh = ½·log1p(2x/(1−x)) with the division error in the bound.
Part 2: asin/acos/atan quick phase: atan by a table of atan(j/64) plus a short odd minimax polynomial of the reduced
argument (Tang-style; Muller, Elementary Functions 3rd ed. 2016, ch. 6), asin/acos via the atan2 form or via
asin(x) = x + x³·P(x²) for |x| < 0.5 and the 2·asin(sqrt((1−x)/2)) identity above; cosh/sinh/tanh from the exp quick
kernel (e^x and e^-x from one reduction). cbrt/hypot only if the budget allows (lower gap).
Split kernel/wiring if `hyper.rs`/`invtrig.rs` reads grow (grep -n, read by range).

## Tests and plants
Existing: `crates/math/tests/invtrig.rs`, `fast64.rs`, `elem_products.rs`, `exact_products.rs` (acosh(1) = 0 and the
other exact cases), `math_double_fast_entry_sweep`; new 10^8-operand accuracy job per function (x near 1 for acosh,
|x| near 1 for atanh/asin/acos, four modes). Plants: drop the low part of sqrt(x²±1), atanh division's error left out
(bound halved), asin regime boundary moved, `near` always true: each CAUGHT with msg:.

## JOBS
clippy + static, direct gate, plants, counts before/after (acosh asinh atanh asin acos atan cosh, both builds), booked
timing of the rows on Ivy Bridge and N100. Targets: acosh/asinh/atanh <= 2x time (from 14-26x), asin/acos <= 1.5x.

Hand-back first line: `COMPLETE: inverse hyperbolic quick tier <+ part 2>, IVB <x>x / N100 <y>x` or INCOMPLETE:/DO NOT LAND:.
