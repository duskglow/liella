# MATH-M5-CPLX-F128-ZIV-C-4: handoff (one page)

State: branch task/math-m5-cplx-f128-ziv-c-4 (continues ZIV-C-3). Code is unchanged since 5e088b66; this run added
only this handoff and the report. Decisions in force: D-10.17, owner rulings #1719, #1758, #1905. Role kanon:~/w/kn1.

## This run (counts; no code change)
- ZIV-C-3's jobs: FULL gate `math-m5-cplx-f128-ziv-c-3-gate2` rc 0 (2691 s) and plants `-plants2` rc 0, both on
  this code. The FULL gate was not rerun (the code did not change).
- clippy --workspace --all-targets --all-features -D warnings and xtask static: clean on kanon (this head's tree).
- Counts job `math-m5-cplx-f128-ziv-c-4-counts` (kanon, rc 0; script ~/work/math-m5-cplx-f128-ziv-c-4/counts.sh):
  - Accuracy suite (`accuracy_suite_retry_count`, 84 308 calls): 105 calls retried at 384 bits (cexp 9, ccos 12,
    ccosh 12, csin 18, csinh 18, ctan 18, ctanh 18; the others 0). Calls reaching each step: 768: 0, 1536: 0, 3072: 0,
    residual: 0. Every retry decided at 768 bits.
  - Random sample (`random_sample_retry_count`, 32 000 calls, 16 families x 500 x 4 modes): 0 retried; reaching
    768: 0, 1536: 0, 3072: 0 (#1761 expected about zero).
  - cpow with huge |c|,|d| (#1905) climbs to 1536/3072: `cpow_huge_exponents_decide_on_the_ladder` (unit test) checks it.
- Fast-path count job `math-m5-cplx-f128-ziv-c-4-percall2` (kanon, rc 0, `validation/wl_percall.sh --base
  fed89fc58` (ZIV-B2-3 head, primed with wl_prime_base.sh) `--spec validation/percall/cplx_f128.c`), instructions:u
  per call, sd 0, as B2-3 / this head: cexp 153 289.4 / 153 322.4 (+33), csinh 164 035.9 / 164 054.9 (+19), csin
  154 699.9 / 154 715.9 (+16), ccos 154 742.4 / 154 756.4 (+14), clog 312 522.3 / 312 531.3 (+9), ctan 174 606.7 /
  174 611.7 (+5), csqrt 75 760.6 / 75 751.6 (-9), catan 319 940.2 / 319 930.2 (-10), casin 387 145.8 / 387 144.8 (-1).
  All within 0.022%. The delta is not ZIV-C's alone: the base has neither ZIV-C Step 0's `small_trig` correctness fix
  nor the 48 merged origin/next commits. Not split further (RULING NEEDED in the report).
- Diagnosis of the 105 retries at 384 (scratch-only RAM copy, job `math-m5-cplx-f128-ziv-c-4-diag`; it printed one
  line per retried call): all 98 lines are in directed modes (D 33, U 32, Z 33), none to nearest, with operands that
  are tiny powers of two (2^-140, 2^-160) or the smallest subnormal. So each part is x or 1 to within about 2^-280
  relative (sin x, tanh x near x; cos x, e^x near 1). Hypothesis (NOT VERIFIED): the 384-bit bound on these
  tiny-but-normal sine/tangent parts is absolute or loose like the subnormal case ZIV-C Step 0 fixed in `small_trig`, so
  it cannot separate x from its neighbour at 384 bits. The results are correct (decided at 768).

## Not done (next run)
1. Optional (perf only, hard cases only): extend the `small_trig` style relative bound to tiny normal operands
   (|x| <= 2^-128 or so), so these 105 suite calls decide at 384. Check first that the bound is the cause (print the
   384-bit bound beside |part - nearest representable|).
2. Optional (perf only): cpow start step from ex(c), ex(d) (D-10.17).

## Do not redo
Step 0 (small_trig fix), the ladder code (ZIV-C-2), the origin/next merge, the gate2 diagnosis, the width tests and
plants (ZIV-C-3), the counts and percall (this run; base fed89fc58 is now cached on kanon).

## Earlier runs
ZIV-C Step 0: 2219 parts undecided at 768 were all subnormal/min-normal sine/cosine operands (bound too loose);
`small_trig` in complex_body.rs fixed it. ZIV-C-2: mpx (1536), mpy (3072), NEWTON from BITS, WIDTH_SLACK, cold
`ladder` -> `wider` steps, TWO_OVER_PI 312 words. ZIV-C-3: clippy fix, next merge, 1536/3072 width tests, plants
newton/wider/twopi CAUGHT, slack EQUIVALENT (proof margin).
