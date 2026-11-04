# MATH-M5-CLOSE (runs 1-5): handoff

State: branch task/math-m5-close-5 (on close-4 626595c7) (stacks on task/math-m5-close-3 16fb2b58 -> close-2 -> close). HEAD: see
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
  the test to fail on its assertion messages). Use a NEW plant## Run 5 (commit ffd8b5af)
- is_nan doc moved back above `pub fn is_nan` (the missing-docs red).
- Clamp precondition, derived: with an oracle that rounds monotonically, the 2p+40-bit value of v can never lie below
  RZ(v) (RZ(v) is on the finer grid and <= v), so `exact = z;` is reached only if the oracle errs. Probe (temporary scan
  test on kanon kn1, jobs math-m5-close-5-scan, -scan2): cosh at 2^-k (k 100..200, 1000..16300, 4 mantissas) and the six
  logged seed-1 operands (CLOSE-2 cosh.log on mei s2): the 2p+40-bit value is never below 1. No operand reaches the clamp.
  So the clamp is not what changed the cosh q U max ulp 2.000 -> 1.000; the handoff-3 explanation is unproven.
- cmp_reference.rs now asserts what holds on those operands: reference = 2p+40-bit value, >= 1, nextup(1) within 1 ulp.
- Plant `drop exact = z;` is EQUIVALENT on every input found (unreachable branch); no plants job run.
- Gate kanon kn1 math-m5-close-5-gate: clippy and static green; `cargo xtask affected` ESCALATED TO FULL (running at hand-back).

## RULING NEEDED
Keep or drop the clamp in cmp::ulp_reference. Recommend: drop it (dead for a sound oracle, costs one RZ Ziv evaluation per
record) and re-measure cosh q U to find what really moved the max ulp (likely the ulp_error definition at a reference of
exactly 1, a power of two). Coordinator note forbade further refactoring this run, so it is left in.

## Next step
1. Read math-m5-close-5-gate on kanon (~/work/MATH-M5-CLOSE-5/affected.log). 2. Rule on the clamp; if dropped, revert it and
re-run the cosh q U measurement. 3. The story can then close.
Read: plan/06-phases/04-track-m-parallel-from-p2.md "M5-8" and the milestone 5 Delivered entry.
