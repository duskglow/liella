# MATH-M5-CPLX-F128-ZIV-C: handoff (one page)

State: branch task/math-m5-cplx-f128-ziv-c, cut from origin/task/math-m5-cplx-f128-ziv-b2-3 (fed89fc5, the whole chain).
Step 0 is done and committed (63fb8a2f). The ladder (items 1-5 of the brief) is NOT started: this run handed back at
its budget checkpoint. Decisions in force: D-10.17 (s10-mathematics.md), rulings #1719, #1758, #1905; ZIV-CHAIN-PLAN §1.

## Step 0 result (settled; do not redo)
- Reproduced B2-3's count: 2219 parts undecided at 768 bits, all in cexp, csinh, ccosh, csin, ccos, ctan, ctanh
  (cpow 0 on the suite). Printed inputs (count job, `ziv-c-input` lines): every one had a subnormal or minimum-normal
  sine/cosine operand. Class: **bound too loose**. The trig factor's absolute bound `2^(10-B)` (`trig_abs`) exceeds
  `|sin t|` for such `t`, so the part was undecided at every width. These were not hard cases and not exact parts.
- Fix: `small_trig` in complex_body.rs. Below `2^(-B/2)`, sin = operand within `2^(max(a, 3e-2)+1)` and cos = 1
  within `2^(max(a, 2e-1)+1)` (alternating Taylor remainder; both functions 1-Lipschitz). Used by `Op::trig` (cexp,
  circ, tan_core) and by cpow's `sin_cos_any`. tan_core no longer computes sin_cos twice.
- After it (job math-m5-cplx-f128-ziv-c-diag3, keke, rc 0): **0 parts undecided at 768** on the suite; 105 of 84 308
  calls still retry at 384 (cexp 9, ccos 12, ccosh 12, csin/csinh/ctan/ctanh 18 each), all decided at 768. Those
  105 are not diagnosed: they cost one 768-bit evaluation each and are correct. The eight differential shards against
  glibc pass. The rate is still above the ~2^-250 per part a random input would give, so a second loose bound may
  remain (candidate: huge/tiny `e^x` clamps or results at the subnormal edge). Worth one diagnostic in the next run.
- Test: `complex::ziv_tests::tiny_trig_operands_decide_at_384` (7 families x 3 tiny operands x 3 others x 4 modes, plus cpow).

## Exact next step (the ladder, ZIV-C items 1-5)
1. New modules for 1536 and 3072 bits like mpw.rs (`mp_core!`/`mp_kernels!` with LIMBS 24 and 48). Constants
   `MP?_PI/LN2/EULER` from `cargo xtask tables` at >= 1600 / 3150 bits. The 2/pi table must cover `RW = LIMBS + 3`
   words past bit 16383: about 285 / 309 words (it holds 272 now).
2. Re-prove the fixed iteration counts in mp.rs for larger B. Newton `recip`/`sqrt` `for _ in 0..4` holds only for
   B <= 800: 1536 needs 5, 3072 needs 6, so derive the count from LIMBS. Also check the loops at mp.rs ~396 (0..6),
   ~585 (0..20), ~674 (12 squarings) and the atan halvings (~738) against B. The mpw.rs header lists each bound.
3. Expand complex_body.rs twice more, entered through `#[cold] #[inline(never)]` functions so the fast frame does
   not grow. Chain `ziv()` with one counter per step. At the top, return the 3072-bit value rounded, count it, and
   add a test-only hook that forces it. cpow (#1905): choose the start step from ex(c), ex(d) (needs more than
   1000 + 113 bits plus the margin).
4. D-10.17: the guarantee as ruled in #1758 (every step proven; one assumption: no binary128 input needs more than
   3072 bits). Remove every TODO(ZIV-C) marker (complex.rs, complex_body.rs, tests).
5. Counts job `math-m5-cplx-f128-ziv-c-counts`: per step, suite plus random sample. Compare the fast path's
   instructions:u with B2-3 (validation/percall/cplx_f128.c).
Merge origin/next once before the final gate. Merge no other chain branch.

## Jobs of this run (keke)
See the report below and handoff/reports/MATH-M5-CPLX-F128-ZIV-C.json.
JOBS: math-m5-cplx-f128-ziv-c-gate (rc 0, 129 passed), math-m5-cplx-f128-ziv-c-plants2 (skip-small-trig,
sine-value-off, threshold-wide; sine-exact/cosine-exact EQUIVALENT), math-m5-cplx-f128-ziv-c-diag3 (counts, rc 0).
SUPERSEDED: diag0 (pre-fix reproduction, rc 101 by design: the old assertion), diag1/diag2 (script errors: per-package
clippy, missing products), plants (rc 1: sine-exact and cosine-exact survived, equivalent by design: at e <= -B/2 the
Taylor term is a relative <= 2^(-B-1), below Approx::mul's +2 slack; replaced by value plants in plants2).

## Report
- Commit 63fb8a2f "Bound the sine of a tiny operand relatively in the f128 complex Ziv step".
- Gate math-m5-cplx-f128-ziv-c-gate (direct tier, base ziv-b2-3) and plants math-m5-cplx-f128-ziv-c-plants
  (skip-small-trig, sine-exact, cosine-exact): results in the json.
- INFORMATIONAL: `cargo clippy -p liella-math` alone fails in liella-boundary (`set_sysinfo_ehdr`, `start` never used
  under the package-only feature set). `cargo xtask clippy` is clean. This is pre-existing and not caused by this change.
