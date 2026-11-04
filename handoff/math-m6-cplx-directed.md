# MATH-M6-CPLX-DIRECTED (M6-10): handoff (one page)

State: branch task/math-m6-cplx-directed-2 (continues task/math-m6-cplx-directed; from origin/next 442429ab). Code head
74920905 (test + D-10.17 text only; no product code changed). Role shiki:~/w/sh1. Decisions: D-10.17. Ruling #2486:
M6-11 (float/double) may go ahead before the wide-format findings close.

## Done
- crates/math/tests/complex_directed.rs: the 16 two-operand families against the oracle (Ziv loop to 2^14 bits) on a grid
  of part exponents over each format's range, both signs, all four modes; float and double pass (0 differing, 0
  undecided). Plants on complex.rs's ladder (noladder, nowider) CAUGHT.
- Run 2: gate3's red was clippy `absurd_extreme_comparisons` (`undecided <= UNDECIDED_MAX` with the constant 0; the
  toolchain on the role flags it): now `assert_eq!(undecided, 0)`. The trace check found no clause ID on the
  macro-generated tests: they name s10_5_c_0, s10_0_g_0. Module comment corrected to the probe below.

## Open findings (the follow-up story; long double/_Float128 grids are #[ignore]d, run with --ignored)
Probe on 13381a9f, `--ignored --test-threads=1` (exact counters, wide oracle), shiki ~/work/math-m6-cplx-directed-2/probe.log:
- ONLY catan and catanh (p64 and p113) differ: 102 / 126 parts, all at the grid's y = 2^4096 (catan part 1, catanh
  part 0), Up/Down/Zero only (58 each at catan p64), never Nearest. They are also the only families that reach the
  residual path (catan 174, catanh 162 per format). Every other family: 0 residual, 0 differing. The earlier "many
  families reach the residual path" was the global counters under parallel threads; ctan/ctanh's differences were
  the narrow oracle (gone with complex_wide).
- Diagnosis (by series, not yet proven in code): for catan(x + i·y), y = 2^4096, Im = (1/2)·atanh(2y/(x²+y²+1)) =
  1/y + (1/3 − x²)/y³ + O(y^-5). The correction is ~2^-8192 relative, beyond the 3072-bit ladder, so only its SIGN
  decides directed rounding: below 1/y when x² > 1/3, above when x² < 1/3. Observed: x = ±1, ±(1+ulp), 2^32 ... get
  the "above" answer (Up = succ(2^-4096), Down/Zero = 2^-4096); the oracle wants 2^-4096 / pred. The tiny-x operands
  (x = 2^-16445 ...) reach the residual path and come out right. So: an exact sign rule for this asymptotic part is
  missing (the residual path assumes the x = 0 sign). catanh mirrors it.
- Oracle undecided (beyond 2^14 bits) at the subnormal end, e.g. x = y = 2^-16445 (ctan 624, csqrt 182, casin 102 ...
  per format, clog10 0): corrections ~2^-32890 relative. Liella's parts there are unverified, not known wrong.

## Next step
1. complex.rs catan/catanh: for |y| (resp. the catanh analogue) so large that 1/y³ is below the ladder, decide the
   directed rounding from sign(1/3 − x²) (exact in the format: compare x² with 1/3 by exact arithmetic), plus the
   general |z| ≫ 1 form Im ≈ y/|z|² with its next term; prove with the grid (drop the catan/catanh shortfall).
2. Oracle: raise the cap or add exact-correction handling at the subnormal end so `oracle undecided` reaches 0, then
   drop the #[ignore]s (UNDECIDED is asserted 0).
Plan rows to read: D-10.17 (plan/07-decisions/s10-mathematics.md), ZIV-C notes in handoff/math-m5-cplx-f128-ziv.md.

## Jobs (shiki, head 74920905)
math-m6-cplx-directed-2-gate2 (direct tier: complex_directed), math-m6-cplx-directed-2-plants2. Superseded: -gate and
-plants (rc 0 on 13381a9f, before the comment fix), -probe (rc 101 by design: the ignored wide grids; informational),
kanon gate3 (rc 4, clippy).
