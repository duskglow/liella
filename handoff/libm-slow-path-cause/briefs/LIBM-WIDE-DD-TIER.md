# LIBM-WIDE-DD-TIER: a double-double fast tier for the x87 long double functions

Id: LIBM-WIDE-DD-TIER
Kind: code
Priority: 85
Size: L
Host: -
Stacks-on: LIBM-MP-CORE-SPEED, and the owner's ruling R2 (report §5: the yardstick for x87 rows)
Model: opus
Effort: high
BUDGET: 100 tool calls / 180k

## The change

**What was measured** (handoff/libm-slow-path-cause/report.md §3.7): every `long double` call is 384-bit mp, with
no fast tier (D-10.19).

| Function | Cost (instructions) | Reference | Where the time goes |
|---|---|---|---|
| expl | 41,092 | 83 | |
| logl | 88,955 | not quoted | |
| log10l | 195,076 | not quoted | `ln 10` recomputed per call; LIBM-MP-CORE-SPEED fixes that |
| sinl | 99,716 | not quoted | |
| atanl | 206,407 | 21 | eight 384-bit sqrt and division halvings |

x87 needs about 64 + 64 bits in the worst case, not 384.

**Build a fast tier** in double-double (106 bits) or triple-double, table driven, with a proven bound of about
2^-100. The rounding test then fails about 2^-35 of the time, so mp stays only as the fallback.

| Functions | Kernel |
|---|---|
| expl, exp2l, exp10l, expm1l | 2^(j/64) table, degree-11 polynomial in double-double |
| logl, log2l, log10l, log1pl | reciprocal table, atanh series |
| sinl, cosl, tanl | double-double Cody-Waite below 2^20; Payne-Hanek through `mp::reduce_wide` above |
| atanl, asinl, acosl, atan2l | `atan(j/64)` table |
| the hyperbolic functions | from those kernels |
| cbrtl, powl | Newton in double-double; `pow` via a triple-double log/exp |

Generalise the tier so that `_Float128` can follow with triple-double (bound about 2^-140) in a later story.

**Measure parity in time on a booked timing host** (cycles or ns) as well as instructions. The reference uses
microcoded x87 instructions, and a single `fpatan` counts as about one instruction.

**Target:** at most 2x in time per row. Record the instruction residual in docs/saturation with the count and the
x87 note.

**JOBS:**
* the gate (direct tier);
* plants:
  * the bound;
  * a table entry;
  * the reduction threshold;
  * the rounding test in a directed mode;
* the accuracy check (the long double differential and oracle suites, plus a random sweep against the mp path in
  all modes);
* counts before and after (`cbench_libm`: every `*l` row);
* a timing run (booked).
