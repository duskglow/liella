# LIBM-CPLX-SINCOS handoff: csinh/csin (double, float) quick tier before the 384-bit route

State: branch task/libm-cplx-sincos-2 (from task/libm-cplx-sincos, base origin/next d1b3a596c); code head 12f4a0891
(later commits: handoff/ and records/pending only). Role keke:~/w/r5. Run 2 stopped at the context checkpoint.
Decisions in force: D-10.53 (tier structure), D-10.25 (FMA builds). New: records/pending/LIBM-CPLX-SINCOS-2-1.toml
(D-NEW, series D-10: the method, bounds and bands; the coordinator assigns the id).

## Done (verified on keke, code head 12f4a0891)

* Code (run 1, b7f59d625): `crates/math/src/complex_fast_sinh.rs` (tier, proof in its module comment),
  `fast32/csinh.rs`, complex.rs wiring (`csinh`/`csin` = tier then `csinh_route`/`csin_route`), s10_math.tsv rows;
  run 2 committed the regenerated s10_math_gen.rs (`cargo xtask math-gen`).
* Run 2 fixes: two `expect()` in tests (workspace denies expect_used and panic); `compare` now shapes the HYPERBOLIC
  argument for csin too (it fed csin's trig argument the tiny series values, which the tier routes below 2^-30 by
  design: that was the only failure); sweep threaded (8 x 125k per regime, 4 modes) and registered in
  xtask/src/longsweeps.rs; bound wording at the series cut (2^-69 coth 2^-3 < 2^-65.9).
* Job libm-cplx-sincos-2-check4 rc 0: math-gen --check, `cargo xtask clippy`, `cargo xtask static`, the 4 unit tests,
  the release sweep: csinh and csin each 10^6 operands per regime in 4 modes, every answer equal to the route bit for
  bit; nearest answers per regime [999729, 999676, 999663, 950661, 896581] (typical, series, cancellation, large |x|
  incl. overflowing results, y near k*pi/2).

## Red (not caused by a test of this story; attribution NOT yet shown)

* libm-cplx-sincos-2-gate rc 1 and libm-cplx-sincos-2-plants rc 2 (baseline): `xtask so`/`xtask test` stop on
  "hot-text.list: 1 key(s) match no function section of the fast-profile libc.a: _RNvNtC6liella6locale7builtin
  (locale, traced in fast,release)". This branch touches no locale code. NEXT STEP 1: check whether origin/next
  d1b3a596c fails the same way (percall's base build of d1b3a596c on keke, or `cargo xtask so` on a next role); if
  next is green, the new libm code changed an inlining decision: follow the message (`tools/hottext/regen.sh . --build`
  per profile, or a hot-text.absent line with the reason), never loosen the check.
* libm-cplx-sincos-2-percall was still running at hand-back (validation/percall/cplx_sin.c: csin, csinh, csinf,
  csinhf typical, csin small-|y| and large-|y|, csinf small-|y|, bench operands; base d1b3a596c primed on keke).

## Bands and routes (double, a = |hyperbolic argument|, t = trig argument)

| band | route |
|---|---|
| 2^-60 <= a < 2^-3, 2^-30 <= abs(t) < 2^20 | tier: series sinh, exp/reciprocal cosh |
| 2^-3 <= a <= 40 | tier: E +- Q |
| 40 < a <= 746, result normal | tier: Q dropped; a product that stays finite while cosh/sinh overflow is answered |
| result overflows/underflows, a > 746, abs(t) >= 2^20 (Payne-Hanek), abs(t) < 2^-30, a < 2^-60, axes, zeros, inf, NaN | 384-bit route |

## Next steps (in order)

1. The hot-text attribution above; then rerun the gate (`cargo xtask affected --run`) and plants
   (handoff/LIBM-CPLX-SINCOS-2-plants.txt, 7 plants, `--dir /dev/shm/claude/<story>/plants-<n>`).
2. percall: read libm-cplx-sincos-2-percall (or rerun); record before/after/glibc in the handoff.
3. Follow-ups: abs(t) < 2^-30 (sin t = t - t^3/6, cos t = 1 - t^2/2 in double-double) and abs(t) >= 2^20 (the
   existing large-argument reduction) in the tier, the FMA build, ccos/ccosh (ccos(z) = ccosh(iz)).

Do not redo: the design and its bounds (module comment), the test fixes above.
