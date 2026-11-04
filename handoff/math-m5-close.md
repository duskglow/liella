# MATH-M5-CLOSE (runs 1-3): handoff

State: branch task/math-m5-close-3 (stacks on task/math-m5-close-2 5f78e3f5, which stacks on task/math-m5-close).
Not merged with origin/next (b0296d28 at the run 3 start). Decisions in force: D-10.3 (mathcmp), D-10.14 (flags/errno),
D-10.17, D-10.21.

## Done
- Run 1: symcov class m = 0, docs/spec-symbol-gaps.{tsv,md} regenerated, README milestone-5 qualifier removed, codemap
  lists the M5 tests.
- Run 2 (5f78e3f5): mathcmp probe calls the _Float128 special functions on the Liella side (measure2 rc 1 was the
  probe ending early on them); cmp.rs clamps the max-ulp reference to the round-toward-zero result (coshf128 mode U
  2.000 -> 1.000, a reference artefact). Measured on mei s2 (job math-m5-close-2-measure2 rc 0,
  ~/work/MATH-M5-CLOSE-2/rows.tsv, cosh.log).
- Run 3: docs/libm-accuracy.tsv gets the 32 q rows (erf..y1 x N U D Z), the corrected cosh q U row and a header note
  (reference clamp, undetermined records, names mathcmp cannot measure); differences-from-glibc.md has the
  milestone-5 _Float128 summary; plan M5 Delivered entry; s10_math_symbols.rs family tests check every f128 name
  (elementary, special) and the l/f64x/f128 complex names plus __clog10l, with value checks expf128(0),
  tgammaf128(5), erff128(-0), cexpf128(0). spec-symbol-gaps.md no longer names a lab host (static lab-names).
- Settled from CLOSE-2's cosh.log: the "1.0, Liella inexact, glibc none" records are mode N, tiny x (exponent
  ~2^-10000): 1.0 is CR and inexact is correct (cosh x != 1). In mode U Liella returns nextup(1) (CR); glibc returns 1.
  The 98-113 undetermined per 1000 in directed modes (cosh, erfc, j0, j1) are those tiny operands: 4096 oracle bits
  cannot decide them. Explained in the table header, not fixed (an analytic oracle rule would decide them).

## Open / not verified
- No plant and no unit test covers run 2's cmp.rs reference clamp (tool code). Plant to run: delete the `exact = z;`
  assignment; a test would need a cosh q tiny-x record in mode U (expects Liella max ulp 1.000, not 2.000).
- `cargo xtask status` (s10 at 0 pending except phase-15 compat names) not run in run 3.
- Run-3 job: shiki math-m5-close-3-check2 rc 0 on a5a97e78 (clippy clean, static ok, so built, the three s10 tests
  passed). The affected gate was not run (budget). check (run 3's first) failed
  on static lab-names (fixed in a5a97e78).

## Next step
Read check2's result (shiki ~/work/MATH-M5-CLOSE-3/*.log); if green, plant the cmp.rs clamp, run `cargo xtask status`,
then the direct gate (`cargo xtask affected --run --base origin/next`). Then the story can close.
Read: plan/06-phases/04-track-m-parallel-from-p2.md "M5-8" and the milestone 5 Delivered entry.
