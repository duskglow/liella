# LIBM-F64-TRIG-QUICK: binary64 sin/cos/tan quick phase with a shared reduction

Id: LIBM-F64-TRIG-QUICK
Kind: code
Priority: 85
Size: L
Host: -
Model: opus
Effort: high
BUDGET: 100 tool calls / 200k
Stacks-on: LIBM-F64-EXP-QUICK-WIRE

## The change

Decision row: D-10.51. Rows (time IVB / N100): sin 2.22 / 2.15, cos 2.13 / 1.85, tan 2.83 / 2.61, sin_huge 4.58 / 5.98.
Counts (keke:libm-coverage-1107-percall, FMA): sin 152 vs 105, cos 156 vs 107, tan 181 vs 112; SSE2 sin 232 vs 127
(LIBM-RETHINK-2). Tier today: the fast64 trig tier in double-double on every call; sin_huge runs Payne-Hanek each call.

METHOD: Cody-Waite reduction by π/2 in three parts for |x| < 2^20 (k from the 1.5·2^52 shift), Payne-Hanek [Payne &
Hanek, SIGNUM Newsl. 18(1) 1983] above, kept; then a Tang-style table of sin/cos(j/64) as double + low part (Gal's
accurate-table points so the low part can be dropped where the bound allows [Gal & Bachelis 1991]), |r| < 2^-7, and
short minimax polynomials for sin r − r and cos r − 1 (fpminimax-style rigorous fits, Estrin evaluation, method f);
sin and cos share the reduction and table lookups (sincos returns both: also the building block LIBM-CPLX-EXP-TIER and
csin need). tan = sin/cos with one division, or a dedicated table of tan(j/64) if the division's latency dominates
(measure). Quick bound ~2^-62; Ziv `near` test; tier 2 today's trig tier, tier 3 mp. For sin_huge, cache nothing
across calls (reentrancy); make Payne-Hanek's product 3 u64 multiplies against the 2/π bits (integer, method d-style).
Builds SSE2 / VEX / FMA by D-10.50 dispatch.
Profile first: `wl_trace.sh` one call of sin and tan (both builds): where the 47-70 extra instructions are.

## Tests and plants
Existing: `crates/math/tests/fast64.rs`, `elem_products.rs`, the trig tier's hard-case lists (LIBM-F64-TRIG-TIER
handoff), `math_double_fast_entry_sweep`; new 10^8-operand accuracy job (uniform [-10,10], bit patterns up to 2^1023
for the huge path, four modes) with 0 mismatches against the CR result. Plants: quadrant off by one, table index
shifted, `near` constant halved (accepts too much), Payne-Hanek dropping the third product: each CAUGHT with msg:.

## JOBS
clippy + static, direct gate, plants, counts before/after (sin, cos, tan, sin_huge cases, both builds), booked timing
of sin/cos/tan/sin_huge on Ivy Bridge and N100. Targets: sin <= 110 instructions FMA, time <= 1.3x.

Hand-back first line: `COMPLETE: trig quick phase, IVB <x>x / N100 <y>x` or INCOMPLETE:/DO NOT LAND:.
