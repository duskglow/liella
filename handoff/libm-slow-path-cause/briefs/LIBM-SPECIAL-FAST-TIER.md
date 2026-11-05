# LIBM-SPECIAL-FAST-TIER: double-double fast tiers for erf/erfc, lgamma/tgamma and the Bessel functions

Id: LIBM-SPECIAL-FAST-TIER
Kind: code
Priority: 95
Size: L
Host: -
Stacks-on: LIBM-MP-CORE-SPEED, and the owner's ruling R1 (report §5)
Model: opus
Effort: high
BUDGET: 120 tool calls / 200k (design-heavy: split per family if the first checkpoint shows it)

## The change

**What was measured** (handoff/libm-slow-path-cause/report.md §3.6): every special-function call is 384-bit mp,
with no fast tier (D-10.15).

| Function | Cost (instructions) | Reference |
|---|---|---|
| erf | 228,029 | 159 |
| lgamma | 592,426 | not quoted |
| tgamma | 631,999 | not quoted |
| j0 | 149,669-303,982 | not quoted |

The reference itself is not correctly rounded on any of them: it differs on 2-54% of inputs, by up to 4 ulp, and
by 791 ulp on j0.

**Build Ziv's first step per family** (double, plus a double-double head where needed, a bound of about 2^-66, a
rounding test in every mode, fallback to the existing mp path):
* **erf, erfc** (start here; it should reach about 1.0-1.4x): piecewise minimax polynomials on about 8 intervals
  of [0, 6] for erf. For erfc beyond about 1: `e^(−x²)` with an exact `x²` split, times a polynomial in `1/x`.
* **lgamma, tgamma**:
  * Stirling in double-double above about 8;
  * a shifted polynomial on [1, 3);
  * expansions around the positive zeros (1, 2) and the first negative zeros;
  * reflection with a fast `sin πx`;
  * tgamma = exp of lgamma for large x, exact products for small integers.
* **Bessel**: expansions around the first N tabulated zeros (the zeros come from the oracle), and Hankel's
  asymptotic form above with a double-double phase. The mp path stays the fallback near untabulated zeros.

**Rules:**
* Coefficients and zero tables come from `cargo xtask tables` / the Remez tool, with the derivation recorded.
* Proof in the module docs.
* Fallback-rate tests with ceilings derived from the bounds.

**This brief replaces the held PERF-LIBM-SPECIAL.** If R1 picks option B for a family, implement that family's ulp
bound instead and record it (a differences row and a compat option per CLAUDE.md rule 2, with the coordinator's
reserved ids).

**Target:**
* erf/erfc: at most 1.05x (FMA);
* lgamma/tgamma: below 2x;
* Bessel: below 4x;
* each residual goes into docs/saturation with its count.

**JOBS:**
* the gate (direct tier);
* plants:
  * an interval boundary;
  * a coefficient;
  * a zero-table entry;
  * the reflection sign;
  * the rounding test;
* the accuracy check (the oracle suites and a 10^7 random sweep per function in all modes against the mp path);
* counts before and after (`cbench_libm` special rows, f32 and f64).
