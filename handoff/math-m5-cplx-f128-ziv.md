# MATH-M5-CPLX-F128-ZIV-C-2: handoff (one page)

State: branch task/math-m5-cplx-f128-ziv-c-2, code head 6ef07138 (on ZIV-C's 63fb8a2f). Checkpoint hand-back at the
context ceiling: NOT READY (no plants, no counts, origin/next not merged yet). Decisions: D-10.17 (updated), #1719, #1758, #1905.

## Done this run (ZIV-C items 1-4)
- mpx.rs (24 limbs, 1536) and mpy.rs (48 limbs, 3072): mp_core!/mp_kernels!; MPX_/MPY_ PI, LN2, EULER from
  `cargo xtask tables` at 1600/3150 bits; TWO_OVER_PI 272 -> 312 words (word 307 at e=16383, RW=51).
- mp.rs: `NEWTON` from BITS (max(4, k: 49*2^k >= B+8)): 4 at 384/768 (unchanged), 5 at 1536, 6 at 3072.
- complex_body.rs: `WIDTH_SLACK` = log2(B/768)+1 bits on every bound above 800 bits (series term counts scale with B;
  each bound is a sum over terms). Proof is the scaling argument in mpx.rs/D-10.17; not re-derived kernel by kernel.
- complex.rs: modules wx, wy; `ziv` -> cold `ladder` (768) -> cold `wider` (1536) -> 3072; counters
  retries/wide_retries/wider_retries/residual. TODO(ZIV-C) markers gone. cpow starts at 384 (start-step choice
  from ex(c), ex(d) NOT done: only saves the narrow evaluations; recorded in D-10.17).
- Tests: `forced_undecided_climbs_each_step_of_the_ladder` (forced offset 2^4000: 2^1000 decides at 1536+),
  `cpow_huge_exponents_decide_on_the_ladder` (decides before the residual), products count test per step.
  kanon unit run: complex:: + mpw:: 46 passed.

## Exact next step
1. Read gate `math-m5-cplx-f128-ziv-c-2-gate` (kanon; clippy --all-features, static, affected --base origin/next).
2. Plants (RAM copy, new dir): NEWTON fixed at 4; WIDTH_SLACK 0; drop the `wider` step (y never called);
   TWO_OVER_PI_WORDS back to 272 (expect the 3072 reduce at huge e to differ: may need a test with e near 16383).
3. Counts job math-m5-cplx-f128-ziv-c-counts: products count test + random_sample_retry_count (--ignored
   --nocapture), per step; fast-path instructions:u vs B2-3 (validation/percall/cplx_f128.c).
4. Merge origin/next once, final gate, report.

## Earlier run (ZIV-C, Step 0)
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

