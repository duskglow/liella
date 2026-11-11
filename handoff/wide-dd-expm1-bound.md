# WIDE-DD-EXPM1-BOUND: expm1's double-double tier exceeded its bound; the sweep's check was coarse

State: branch `task/wide-dd-expm1-bound`, code commit fa1689021 on origin/next f841a00b3 (next is an
ancestor: this head is the merged head). Decisions in force: D-10.19 (the tier), ruling #2760 option A.
No decision record: no rule changes (D-10.98 reserved, unused).

## Finding (both numbers, the operands)

The sweep failure LIBM-INVALID-CONVERT-SCAN-5 saw (#4741), `Expm1 x = +11193749041240441045·2^-71:
error 2^-100 against the bound 2^-100`, was the test's coarse estimate `ex(d) − ex(want) + 1`, which can
overstate an error by up to 4×. The exact error there is **0.4196 of the bound** (2^-101.25).
With an exact check, the same sweep then found a real excess:
**x = −12783431649032223825·2^-71 (≈ −0.0054141, just inside −ln2/128): error 2^-99.317 against the
claimed 2^-100.** So it was both cases from the brief: the test's check was not the proof's claim, AND the
implementation exceeded what the proof assumed.

Cause: on `|x| < ln2/128` (the `k = 0` branch and the `|x| < 2^-8` branch) `expm1` does not subtract 1. Its
error counts against `e^x − 1 ≈ x`, but `expm1_poly` held the `r^6` term in `f64`, sized for exp's
scale (2^-54 of 1). Relative to `r` that term is 2^-47.1 at `|r| = ln2/128`, so each of its four
`f64` roundings (the coefficient, the sum, the product, `r.hi` for `r`) can cost up to 2^-100.1.

Fix (implementation, the bound unchanged at b = 100): `expm1_poly_rel` evaluates the `r^6` term in
double-double too (new dd constant `E6 = 1/720`, checked by `constants_are_the_rounded_values`), used by
the two branches without the subtraction. `exp`, `exp2`, `exp10` and the subtracting `expm1` branch
(b = 92) keep `expm1_poly`, so their instruction counts are unchanged. Proof, in the module doc: `r` within
10·2^-106 relative (the product's 7·2^-106 is from Joldes, Muller and Popescu, ACM TOMS 44(2) 2017), the
final sum 3·2^-106, the `f64` terms from `r^7` below 2^-108, giving a total below 2^-102.2.
Measured worst after the fix, over 40 000 operands: **0.0388 of the bound** (2^-104.7; before, 1.6×).
The pinned operands now sit at 0.004 of the bound.

Test: `sweep` compares `|f − v·2^scale| < 2^-b·|v·2^scale|` exactly in 384 bits. That is the
claim `kernel` states and `decide` uses. It prints each kernel's worst error as a fraction of
the bound and names the operand. The 40 000-operand sweep takes about 4 s at liella-math's test
opt-level 2, so it replaces the 300-operand test as `tier_agrees_with_the_wide_route` and is no longer
ignored (refused: 0). `expm1_without_subtraction_stays_within_its_bound` pins both operands,
both signs, for the fast tier.

## Results (kanon, role kn1)

- lint `wide-dd-expm1-bound-lint` (`cargo xtask clippy && cargo xtask static`): rc 0.
- gate `wide-dd-expm1-bound-gate` (`affected --run --base origin/next`, direct tier): rc 0; 2 suites,
  209 passed, 0 failed; peak 1.4G.
- plants `wide-dd-expm1-bound-plants` (`handoff/WIDE-DD-EXPM1-BOUND-plants.txt`): rc 0; 3 CAUGHT
  (r6-f64-k0 by the sweep, r6-f64-pinned by the pinned test, last-correction-lo), 1 EQUIVALENT
  (last-term-e11), 0 SURVIVED, no WARN lines.
- `wide-dd-expm1-bound-hottext` (`cargo xtask so && cargo test -p liella --test hot_text_products` on the
  merged head): rc 0, 14 passed.
- Diagnostics (evidence, not on the final head): `wide-dd-expm1-bound-sweep-next` (origin/next, the
  coarse failure), `wide-dd-expm1-bound-sweep-diag` (exact check, old polynomial: 2^-99.317),
  `wide-dd-expm1-bound-wide-dd-dev` (the fix in the dev profile: 0.0388).

## Open / next

- INFORMATIONAL: the other wide tiers (`wide_int.rs`, `wide_int_el.rs`, `wide_int/quad.rs`,
  `wide_int/quad/arc.rs`, `wide_int/arc.rs`) have `#[ignore]`d `*_long` sweeps "run by the accuracy job".
  No xtask or validation script runs them (grep), so nothing runs them. Next step: a story that runs
  each one, as this one did, and un-ignores the cheap ones.
- The `last-term-e11` plant is declared equivalent: r^11/11! is 2^-100.5 of r at the top of the range,
  and with the other errors (2^-102.2) it is still below the claimed 2^-100. Dropping it would cost all
  the margin, so it stays.
- Nothing else to redo. Plan rows: D-10.19.
