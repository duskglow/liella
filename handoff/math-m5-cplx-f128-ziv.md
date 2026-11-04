# MATH-M5-CPLX-F128-ZIV-C-3: handoff (one page)

State: branch task/math-m5-cplx-f128-ziv-c-3 (continues ZIV-C-2), code head 5e088b66 (merge of origin/next at
8a7e9e6a). Decisions in force: D-10.17, owner rulings #1719, #1758, #1905. Role kanon:~/w/kn1.

## This run
- ZIV-C-2's gate2 rc 101 was NOT a test panic: clippy `indexing_slicing` (-D warnings, lib test) on `&f[..k]` in
  complex.rs `force()`; the build stopped before any test. Fixed with `f.iter().take(k)` (commit 1a fix).
- Merged origin/next once (48 commits, no conflicts). `s10_math_gen.rs` auto-merged; regenerated with
  `cargo xtask math-gen` on the role: byte-identical.
- New tests (mp.rs `wide_width_tests!`, expanded in mpx.rs and mpy.rs): recip/sqrt relative bounds (3, 5 ulp of
  2^-B) and `reduce_wide` at e = 4000 and 16 271 (largest binary128 exponent) against the oracle at B+500 bits. Before
  them nothing tested the 1536/3072 kernels' width-dependent parts: NEWTON fixed at 4 and the 2/pi table cut back to
  272 words both passed every test (the 768 tests cannot see them). 4 tests, 0.09 s.
- clippy --workspace --all-targets --all-features -D warnings and xtask static: clean on 5e088b66 (kanon).
- Jobs on 5e088b66 (kanon): gate `math-m5-cplx-f128-ziv-c-3-gate2`, plants `math-m5-cplx-f128-ziv-c-3-plants`
  then `math-m5-cplx-f128-ziv-c-3-plants2` rc 0: newton, wider, twopi CAUGHT by the named tests; slack EQUIVALENT
  (proof margin: WIDTH_SLACK widens a proven bound by <= 4 bits; the kernels' true errors lie far below the bound, and
  the boundary test is written in terms of WIDTH_SLACK, so no test can see it; coordinator: judge). The gate2 job
  escalated to FULL (the merged next files are in its changed list); it was still running at hand-back (detached).
  Superseded: `-gate` (stopped by me: on 8a7e9e6a, before the new tests), `-plants` rc 1 (slack counted SURVIVED
  before the EQUIVALENT classification; same head).

## Plants (script ~/work/math-m5-cplx-f128-ziv-c-3/plants.sh on kanon; RAM copy, restores by cat)
newton (NEWTON = 4), slack (WIDTH_SLACK = 0), wider (1536 step never climbs to 3072), twopi (2/pi table read as 272
words). Tests run: `cargo test -p liella-math --lib -- complex:: width_tests`.

## Not done (next run)
1. Counts job math-m5-cplx-f128-ziv-c-counts: products count test + random_sample_retry_count (--ignored
   --nocapture), per step; fast-path instructions:u vs B2-3 (validation/percall/cplx_f128.c). Budget ran out here.
2. Optional (perf only): cpow start step from ex(c), ex(d) (D-10.17).
3. The 105 calls of 84 308 that retry at 384 (ZIV-C Step 0) are still undiagnosed (correct; one 768 evaluation each).

## Do not redo
Step 0 (small_trig fix, settled), the ladder code (ZIV-C-2), the origin/next merge, the gate2 diagnosis.

## Earlier runs
ZIV-C Step 0: 2219 parts undecided at 768 were all subnormal/min-normal sine/cosine operands (bound too loose);
`small_trig` in complex_body.rs fixed it; 0 undecided at 768 after it. ZIV-C-2: mpx (1536), mpy (3072), NEWTON from
BITS, WIDTH_SLACK, cold `ladder` -> `wider` steps, TWO_OVER_PI 312 words, ladder tests.
