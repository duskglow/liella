# LIBM-F32-REST: f32 inverse trig one-pass (landed), lgammaf edge path (LIBM-F32-REST-NEXT); handoff, one page

State: branch `task/libm-f32-rest-next` (base origin/next `c1ae12c2d`), head `722d7775a`, role shiki:~/w/sh1.
Runs 1-5 (LIBM-F32-REST..-5, landed) moved asinf, acosf, atanf, atan2f to the one-pass `float` form (D-10.82;
`crates/math/src/fast32/inv_trig.rs`, `mathret.rs` `elem32_onepass2`, exhaustive 2^32 × 4 sweeps and the 10^9-pair
atan2f job green; plants in handoff/LIBM-F32-REST-5-plants.txt).

## LIBM-F32-REST-NEXT (this run): lgammaf beside lnΓ's negative zeros (D-10.87)
- Cause (profiled in run 2): `lgamma_core_signed` refused |y| < 2^-8 (its absolute bound, up to 7260 ulp of y in
  the band above, is too wide there) and |x| < 2^-20 (`gamma_core`'s domain); those operands went to the 384-bit
  path. cbench_libm's lgammaf x = -3.14382577 (bits c0493471) cost about 500k instructions.
- Change (`crates/math/src/fast32/gamma.rs`): `#[cold] lgamma_edge`: x in `fast_gamma::domain` (2^-60 ≤ |x| < 16,
  not a pole) by `fast_gamma::lgamma_kernel` (double-double Y, proved absolute ε), v = RN(Y.hi + Y.lo), margin
  ⌊ε/ulp(v)⌋ + 2 (refused at 2^26); 0 < |x| < 2^-60 by -ln|x| (≤ 2.04 ulp, margin 72). Edge results carry
  `Status::hw` = HW_ANY (as the `double` tier for the same kernel). `eval(Fast::Lgamma)` now returns
  `eval_lgamma`'s value, so the exhaustive sweep checks the exported path. Proofs in the doc comments.
- Tests: `lgammaf_beside_the_negative_zeros_takes_the_fast_path` (1024 encodings around -3.1438258 and -2.4570248
  plus 7 tiny operands, every mode, value/flags/signgam against the accurate path, refusals ≤ 4);
  `gamma_cores_are_within_their_bounds` checks the edge margins. Per-call case `lgammaf_zero` in
  `validation/percall/lgammaf.c`.
- Counts (wl_percall, instructions per call, glibc / Liella; `percall2` on 83de6229e, the same product code as the
  head): lgammaf_zero 543 / 1049 (was ≈ 500k per such operand), lgammaf_pos 138.4 / 307.8, lgammaf_neg 394.7 /
  355.7, lgammaf_r_neg 398.7 / 363.7. The bench row (256 operands) should go from mean 2257 to ≈ 310 (glibc 274):
  job `libm-f32-rest-next-bench4` measures it.
- Plants (handoff/LIBM-F32-REST-NEXT-plants.txt): run `libm-f32-rest-next-plants` on 3387fa4c4 (same plant sites):
  4 CAUGHT (edge-near-zero-none, edge-gamma-refusal-none, edge-sign, tiny-ln-sign, all by the new test with msg),
  1 EQUIVALENT (edge-lowpart: Y.hi = RN(Y.hi + Y.lo) since the kernel ends in a fast two-sum), 0 SURVIVED.
  Not planted: the margin's size (the kernel's actual error is far below its proved ε, so a shrunk margin
  survives every test; soundness rests on the proof and the exhaustive sweep) and the shared rounding test.
- Final-head jobs on shiki (detached): `libm-f32-rest-next-gate4` (so, clippy, static, affected --run --base
  origin/next), then `-exh4` (lgammaf exhaustive, every float × 4 modes, with the near-zero tally), then
  `-bench4` (`cargo xtask bench --compare --counters cbench_libm`); `-plants4`, `-percall4`.
- Superseded on earlier heads: gate (clippy: doc list), gate2 (clippy: doc list, float literal), gate3 (static
  float-casts: RANGE comment); exh/exh2/exh3, plants2/plants3 stopped when the head moved; percall (base missing).

## Open / next step
1. Read `-exh4` (rc and the "near-zero refusals" figure; expected ~0) and `-bench4`'s lgammaf row.
2. Booked timing (Ivy Bridge: nico or kotori, the less busy; N100) of asinf, acosf, atanf, atan2f and lgammaf,
   Liella vs glibc, via `cargo xtask bench --stage`; target ≤ 1.3x IVB. Not done this run (budget).
3. erff, erfcf, hypotf onto the one-pass form (D-10.67), each with its exhaustive proof (hypotf: bound + 10^9
   pairs), plants: low part dropped, mode test skipped, partial-write conversion. Not started.
4. Optional: x < -42 (non-pole) still takes the 384-bit path (reflection formula needed), as do
   rounding-test refusals in the 2^-8 ≤ |y| < 1 band (margin 11264); `lgamma_edge` could serve as a second try.
Not to redo: integer kernels for f32 (DESIGN 3.5 B); a partial-write conversion (D-10.67); the round-to-nearest
index `s·32 + 1.5·2^52` in a one-pass kernel (use `atan_red_any`). Plan rows: D-10.13, D-10.63, D-10.67, D-10.82,
D-10.87, D-10.95.
