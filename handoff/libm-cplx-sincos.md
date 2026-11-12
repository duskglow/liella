# LIBM-CPLX-SINCOS handoff: csinh/csin (double, float) quick tier before the 384-bit route

State: branch task/libm-cplx-sincos-3 (= task/libm-cplx-sincos-2 + merge of origin/next a5636ce6b, merge commit
53474a07a; later commits handoff/ only). Role mei:~/w/s3. Code unchanged since run 2's 12f4a0891.
Decisions in force: D-10.53 (tier structure), D-10.25 (FMA builds), D-10.99 (cexp's middle tier, reused below).
Pending: records/pending/LIBM-CPLX-SINCOS-2-1.toml (D-NEW, series D-10; the coordinator assigns the id).

## Run 3: still red on the product build (hot-text), now other keys

* Merged origin/next a5636ce6b (LAYOUT-HOTKEY-INLINE-STABLE-4 landed; locale::builtin now survives). Job
  libm-cplx-sincos-3-check1 (mei, rc 1): `cargo xtask clippy` and `cargo xtask static` pass; `cargo xtask so` stops on
  "hot-text.list: 2 key(s) match no function section of the fast-profile libc.a":
  `_RNvNtC15liella_boundary7catalog10textdomain` (extra, no trace) and
  `_RINvNtC6liella6getopt4nextNtNtC15liella_boundary7options5CArgsNtNtC15liella_boundary7options6CLongsEC15liella_boundary`
  (tool, traced in fast).
* Attribution: origin/next a5636ce6b alone passed CI's gate on mei (ci-gate-a5636ce6b-1794512176 rc 0); this branch
  touches no catalog/getopt code. So the libm growth again moves fat-LTO inlining in the fast profile, now for two other
  keys. Per the coordinator's note no workaround here (no regen, prune or hot-text.absent line from this branch).
  RULING NEEDED: pin these two keys like locale::builtin, or (better) make the pin cover every hot-text key so that
  unrelated code growth cannot drop one. That needs its own layout story before this branch can build.

## Decline rates: diagnosis (run 3, from the code; no new measurement)

Sweep regimes (complex_fast_sinh_tests.rs `operand`, index 0..4) and run 2's nearest answers of 10^6:
0 typical [-3,3]^2 999729; 1 series 999676; 2 cancellation [1/8,1] 999663; 3 large |x| in [30,746], |y|<40 950661;
4 y = k*pi/2 + U(-1e-9,1e-9), |x|<5: 896581. Target from the brief: <= 2^-20 (about 1 per 10^6) in EVERY regime, so
regimes 0-2 (2.7e-4..3.4e-4, about 2^-11.7) miss it too.
* Regime 3: the declines are overflowing results, not loose bounds: cosh x·|sin y| > DBL_MAX for x > ~709.8, i.e.
  (746-709.8)/716 = 5.06% of the regime (4.93% declined). Fix: in the tier, when the part's lower bound
  `(|P.hi| - |P.lo| - eps)·2^(m-1) >= 2^1024`, answer the overflow value by sign and mode (nearest/away: inf;
  toward zero: DBL_MAX) with status OVERFLOW|INEXACT; values between DBL_MAX and 2^1024 still decline. The in-place
  `*_nearest` entries keep declining it (they raise only inexact), so the test's `nearest == Some` assertion and its
  `rs == INEXACT` assertion must compare the tier's status with the route's instead.
* Regime 4: `trig::eps` with table sine 0 is `A_RED + B_REL·|T| = 2^-89 + 2^-63·|T|`; with |T| ~ 2^-30 the absolute
  2^-89 is 2^-59 relative, so the rounding test declines ~10%.
* Regimes 0-2: the quick bound (~2^-66 relative) leaves ~2^-12 declines; only a middle tier reaches 2^-20.

## Middle tier design (next run implements it; reached only by quick-tier declines)

Per declined part, as cexp's D-10.99 middle: `H ⊗ T` with settle at eps ~2^-95 (decline ~2^-42 per part).
* T: complex_fast's `sin_or_cos` (Cody-Waite with pi/2 in 3 doubles, double-double Taylor). Its documented bound is
  absolute 2^-100; for small |T| re-derive it relative from the same terms: the three Joldes-Muller-Popescu additions
  (3u^2 relative to each partial sum; the first partial sum is r + n·P2 with |n·P2| <= 2^-34.5), RN(n·P3) 2^-142.6,
  n·delta 2^-143.6 give |r~ - r| <= 2^-102.8·|r| + 2^-138.5; sine branch r~·S(z) relative ~2^-100.6, cosine branch
  >= 0.70 so 2^-100 absolute is 2^-99.5 relative: |T - t| <= 2^-99·|T.hi| + 2^-137 (check it by a test against the
  384-bit arithmetic on y near k·pi/2, as the TRIG_ABS test does). Doubles stay >= 2^-62 from k·pi/2 (Muller 2016 §11.3).
* H, a >= 1/8: `wide_dd::exp_dd` (2^-100 relative, |x| < 2^14); e^-a by the quick tier's reciprocal step (r0 =
  RN(1/V.hi), Sterbenz residual, Fast2Sum: within 2^-103 relative; factor it into a helper shared with `hyp`), dropped
  above 40 (e^-2a < 2^-115); sinh = (E - Q)/2 loses at most coth(1/8) ~ 8 (2^-97), cosh no cancellation.
* H, a < 1/8: sinh a = a·sum z^k/(2k+1)!, cosh a = sum z^k/(2k)!, z = two_prod(a, a) exact, by `dd::horner` with the
  coefficients of SIN_DD/COS_DD with the odd-k signs flipped (all terms positive, no cancellation).
* eps = A·B·2^-95 + A·2^-136 (A = |H.hi|, B = |T.hi|). Overflow/underflow of the middle result still declines (scale).
Then rerun the 10^6/regime sweep in 4 modes and report per-regime rates before/after.

## Bands and routes (quick tier, double; a = |hyperbolic argument|, t = trig argument)

| band | route |
|---|---|
| 2^-60 <= a < 2^-3, 2^-30 <= abs(t) < 2^20 | tier: series sinh, exp/reciprocal cosh |
| 2^-3 <= a <= 40 | tier: E +- Q |
| 40 < a <= 746, result normal | tier: Q dropped |
| result overflows/underflows, a > 746, abs(t) >= 2^20, abs(t) < 2^-30, a < 2^-60, axes, zeros, inf, NaN | 384-bit route |

## Next steps (in order)

1. Blocked: the hot-text keys above (layout story). Then `cargo xtask so` must pass on this branch unchanged.
2. Overflow answers in the tier, then the middle tier above; sweep rates before/after; update the pending D-10 record.
3. Gate (`cargo xtask affected --run`), plants (handoff/LIBM-CPLX-SINCOS-2-plants.txt plus plants on the middle tier,
   `--dir /mnt/zram/claude/<story>/plants-<n>` on mei), percall validation/percall/cplx_sin.c against next and glibc.
4. ccos/ccosh on the shared kernel (ccos(z) = ccosh(iz)); abs(t) < 2^-30 and >= 2^20 in the tier; FMA build.

Do not redo: the quick-tier design and its bounds (module comment), run 2's test fixes, the diagnosis above.
