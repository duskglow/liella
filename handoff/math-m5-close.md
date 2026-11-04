# MATH-M5-CLOSE (runs 1-4): handoff

State: branch task/math-m5-close-4 (stacks on task/math-m5-close-3 16fb2b58 -> close-2 -> close). HEAD: see
handoff/reports/MATH-M5-CLOSE-4.json. Not merged with origin/next. Decisions in force: D-10.3, D-10.14, D-10.17, D-10.21.

## Done (runs 1-3, unchanged)
symcov class m = 0, spec-symbol-gaps regenerated, codemap; mathcmp probe calls the f128 special functions; cmp.rs clamps the
max-ulp reference to the round-toward-zero result (coshf128 mode U 2.000 -> 1.000); libm-accuracy.tsv q rows + header note;
differences summary; plan M5 Delivered; s10_math_symbols.rs f128/complex family tests. Run-3 check2 rc 0 on a5a97e78.

## Run 4 (WIP commit 5ae3c690, NOT GREEN)
- Moved the clamp into `pub fn cmp::ulp_reference` (crates/mathtools/src/cmp.rs) and added
  crates/mathtools/tests/cmp_reference.rs (cosh f128, x = 2^-10000, mode U).
- shiki sh1 jobs on 5ae3c690, both red:
  - math-m5-close-4-gate rc 1: clippy missing-docs on `pub fn is_nan`. The new fn was inserted between is_nan's
    existing `///` doc comment and is_nan, so that doc now sits above ulp_reference's doc. FIX: move those original
    lines back directly above `pub fn is_nan`.
  - math-m5-close-4-plants rc 2 (baseline): the test's precondition fails - cosh(2^-10000) at 2p+40 bits is NOT below
    1, so this operand does not reach the clamp. The handoff-3 explanation (tiny x, mode U) of which records the clamp
    changed is therefore unproven for this operand. FIX: find a record the clamp actually changes (CLOSE-2's
    ~/work/MATH-M5-CLOSE-2/cosh.log on mei s2: a q mode U record where Liella's ulp went 2.000 -> 1.000; or add a temporary
    eprintln in ulp_reference while a local mathcmp run for cosh q U executes) and use that operand in the test. If no
    clamped record can be found, the clamp may not be what changed the row: re-check before keeping it.
- Job scripts: shiki ~/work/MATH-M5-CLOSE-4/{gate,plants}.sh (plant = drop the `exact = z;` line in a RAM copy, expects
  the test to fail on its assertion messages). Use a NEW plants-<n> dir next time.

## Next step
1. Fix the is_nan doc placement. 2. Find a real clamped operand and fix the test. 3. Sync to the role, run gate.sh and
plants.sh (plants-2 dir), then `cargo xtask status`. Then the story can close.
Read: plan/06-phases/04-track-m-parallel-from-p2.md "M5-8" and the milestone 5 Delivered entry.
