# LIBM-F64-POW-QUICK: binary64 pow quick phase from the quick log and quick exp

Id: LIBM-F64-POW-QUICK
Kind: code
Priority: 84
Size: M
Host: -
Model: opus
Effort: high
BUDGET: 80 tool calls / 160k
Stacks-on: LIBM-F64-LOG-QUICK, LIBM-F64-EXP-QUICK-WIRE

## The change

Decision row: D-10.51. Row (time IVB / N100): pow 3.35 / 3.59 (pow_upward goes to LIBM-DIRECTED-INPLACE). Counts
(keke:libm-coverage-1107-percall, FMA): 319 vs 143; SSE2 494 vs 193 (LIBM-RETHINK-2). Tier today: fast64 pow (log in
double-double, product with y, exp in double-double, saturation §16 candidates already applied) on every call.

METHOD: log x in "double + low part" to ~2^-68 from LIBM-F64-LOG-QUICK's table (one extra table column or one extra
polynomial term over the plain quick log), t = y·log x with the exact product (FMA, or Dekker's product with Veltkamp's
split on SSE2: the one place an exact product pays off, since |t| up to 709 amplifies the log error), then the quick exp
kernel of LIBM-F64-EXP-QUICK on (th, tl) with tl folded into r; one Ziv `near` test on the result with a bound from the
composed errors; tier 2 = today's pow, tier 3 = mp. Exact cases (integer y, x a power of two, results exactly
representable: the detectors that exist) stay ahead of the quick phase. Builds SSE2 / VEX / FMA (method g).
Profile first (wl_trace.sh, both builds): today's split between log, product and exp.

## Tests and plants
Existing: `crates/math/tests/fast64.rs` pow cases, `exact_products.rs` (exact pow results), `elem_products.rs`,
`math_double_fast_entry_sweep`; new 10^8-operand job over (x in [0.01, 10], y in [-10, 10]), x near 1 with large y,
results near overflow/underflow, four modes, 0 mismatches. Plants: drop the low part of log x, inexact y·log x product,
`near` always true, exact-case detector bypassed: each CAUGHT with msg:.

## JOBS
clippy + static, direct gate, plants, counts before/after (pow, both builds), booked timing of pow on Ivy Bridge and
N100. Target pow <= 170 instructions FMA, time <= 1.5x.

Hand-back first line: `COMPLETE: pow quick phase, IVB <x>x / N100 <y>x` or INCOMPLETE:/DO NOT LAND:.
