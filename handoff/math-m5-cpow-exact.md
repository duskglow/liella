# MATH-M5-CPOW-EXACT handoff

State: branch task/math-m5-cpow-exact, cut from origin/next a92f1fac (ZIV-C landed); code head dac5281d.
Decisions in force: D-10.17 (Ziv ladder, 3072-bit assumption), D-10.23 (new: cpow integer powers beyond the exact width).

## What changed
- `complex_body.rs`: `pow_int` now falls back to `pow_int_far` when an exact `z^n` (0 < |n| <= 64) does not fit the
  current width. When one part dominates by 2^6 or more, each part is an alternating, decreasing series of exact terms
  (binomial for n > 0; negative binomial for n < 0 when the dominant part is a power of two). The exact partial sum A
  (<= B-4 bits) and the sign of the first term below 2^(ex A - B) decide every mode (bound in the fn comment). This
  decides at 384 bits, so no ladder step runs. Other inexact cases: the ladder as before (unchanged cost on the exact path).
- Test `complex::tests::cpow_integer_powers_beyond_the_exact_width`: f32, f64, F80, F128; n = 2, 3, 5, -2, -3;
  the parts 2^-100 to 2^-16000, swapped and scaled bases; all four modes; also checks nothing is undecided at 384.
- Confirmation (step 1): plant `old-cutoff` (fallback disabled = the old behaviour) makes this test fail (job below).

## Bound / limits (not verified beyond the above)
- Dense powers (no dominant part) that need > 3072 bits still rely on D-10.17's assumption, as before.
- n < 0 with a non-power-of-two dominant part: u^-m is not dyadic. It is left to the ladder (rounding a non-dyadic
  rational value, the real `pow` hard-case question).

## Jobs (kanon): math-m5-cpow-exact-gate (direct tier), math-m5-cpow-exact-plants (3 plants).
## Next step: CI landing; then MATH-M5-CLOSE.

Results: gate math-m5-cpow-exact-gate rc 0 (164 passed, 0 failed); plants math-m5-cpow-exact-plants rc 0 (3/3 CAUGHT).
