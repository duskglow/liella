# LIBM-F32-REST(-3): one-pass inverse trig; lgammaf profiled; merged with next (handoff, one page)

State: branch `task/libm-f32-rest-3` (continues `task/libm-f32-rest-2`, base origin/next), role shiki:~/w/sh1. Run 1
moved asinf, acosf, atanf, atan2f to the one-pass `float` form (D-10.82; code, proofs and per-call spec as below). Run 2
(LIBM-F32-REST-2) changed no product code: it settled run 1's two plant survivors and profiled lgammaf. Run 3
(LIBM-F32-REST-3, merge only) resolved CI's merge conflict; see the last section.

## Run 1 (LIBM-F32-REST), unchanged
- `crates/math/src/fast32/inv_trig.rs`: `atan_red_any` (direction-independent table index), `onepass_abs`,
  `onepass_finish` (margin 64), `asin_/acos_/atan_/atan2_onepass_any`; bound ≤ 21u in every direction (doc comments).
- `crates/boundary/src/mathret.rs` `elem32_onepass2`; `hot/f32_invtrig.rs` (asinf/acosf pass `sqrtsd`); `s10_math.tsv`
  rows + regenerated `s10_math_gen.rs`; tests in `crates/boundary/tests/s10_math_symbols.rs` (sampled 16-MXCSR in the
  gate; ignored exhaustive sweeps and the 10^9-pair atan2f job); `validation/percall/math_f32_invtrig.c`.
- Verified on mei (coordinator 11-10): `f32rest-exh` rc 0 (3 sweeps 2^32 × 4 modes, 0 mismatches; atan2f 10^9 pairs),
  `f32rest-gate` rc 0, `f32rest-percall-2` rc 0; plants 5 CAUGHT, 2 SURVIVED (the two below).

## Run 2: the two survivors (handoff/LIBM-F32-REST-2-plants.txt, job `f32rest2-plants1` on kanon)
- `red-lowpart` (the fit's u^7 term dropped): moves results by ≤ |AT7|·(1/64)^7 ≈ 2^-44.8 ≈ 300 double ulp, above
  the margin 64, so only operands within ~300 ulp(double) of a `float` boundary change; the sampled gate test never
  meets one. Not dead code. The plant now runs the atanf exhaustive sweep (every third operand, 4 directions).
- `asin-partial-write`: run 1's mutant `f64::from(x).abs()` compiled to `pand %xmm0,%xmm1; cvtss2sd %xmm1,%xmm1`
  (disassembly of the mutant product): the conversion's destination was written two instructions earlier, which is
  not D-10.67/D-10.68's hazard, so `cargo xtask partial-write` passing it was correct; the checker needs no change.
  The new mutant converts the signed operand (`f64::from(x)`, sign removed afterwards by `0.0 - a`), which stays live
  for the envelope, so the conversion merges into a register the export has not written: the hazard the plant names.

## lgammaf profile (job `f32rest2-lgprobe`, scratch probe, instructions:u per call per operand)
cbench_libm's lgammaf row has 256 fixed operands in [-10, 10]. Liella mean 2257.2 instr/call, glibc 274.1. ONE
operand, x = -3.14382577 (bits c0493471, next to lnΓ's zero near -3.1435), costs 499 513 instructions (the 384-bit
path); every other operand ≤ 354.4. Without it Liella's mean is ≈ 307 (1.12x). The time ratio 9.18x is this one
mp call, not latency: the 1110 table's instr column for lgammaf is 7.765 (the "1.04"/1.001 in the brief is the
change column). Cause: `special::lgamma` sends a `float` that `fast32::lgamma_core` refuses (|y| < 2^-8 beside the
negative zeros, |x| < 2^-20) straight to `lgamma_accurate`; the `double` tier (`fast_gamma`) is used for `double` only.

## Run 3 (LIBM-F32-REST-3): merge with next only (coordinator scope 11-10)
- CI stopped on a conflict in `crates/math/src/fast32.rs`: LIBM-F128-TRIG-INT widened the `trig` re-export
  (`cos/sin/tan_nearest_widen`) on the line beside this branch's `inv_trig` re-export of the one-pass kernels.
  Merge `9db84d304` keeps both lines; nothing else changed. `s10_math.tsv` merged cleanly (next moved sinf/cosf/tanf
  to `crate::hot`, this branch moved asinf/acosf/atanf/atan2f: disjoint rows), and `cargo xtask math-gen` on the merged
  tree reproduces the merged `s10_math_gen.rs` byte for byte (job `libm-f32-rest-3-lint`: MATHGEN, clippy, static ok).
- On the merged head `9b3103cdf` (shiki:~/w/sh1): `libm-f32-rest-3-gate` = `cargo xtask so`, `cargo xtask static`
  (partial-write: 790 exports walked, 0 findings) and `affected --run --base origin/next`: rc 0, 4085 passed /
  0 failed, conformance 20/20, peak 12G, oom_kill 0 (2 SKIPPED lines: tool or capability absent, not this change).
  `libm-f32-rest-3-plants` (handoff/LIBM-F32-REST-3-plants.txt = run 1's five caught plants + run 2's two): rc 0, 7 CAUGHT, 0 SURVIVED,
  0 WARN (asin-partial-write is caught by `cargo xtask partial-write`'s finding, which names no test).
- No exhaustive job rerun: the merge changed no line of the one-pass kernels or their exports (only the `pub use`
  line), and next's trig change is next's own (proved on its own branch).

## Next step (a new story the coordinator queues after this lands; not this branch)
1. lgammaf: in `lgamma_core`, replace the near-zero `None` (and gamma_core's |x| < 2^-20 refusal) by a `#[cold]`
   call to `fast_gamma::lgamma_kernel(xd)` when `fast_gamma::domain(xd, false)`: y (double-double) with absolute
   bound eps (tables 2^-75 relative, D 2^-99); return (RN(y.hi + y.lo), ceil(eps/ulp + 1/2) + 1) as the margin in ulp
   of y, so `eval`'s existing rounding test and status logic apply. Float operands beside the zeros have |y| ≳ 2^-26,
   so the margin is a few ulp. Zeros below -16 have no float in their |y| < 2^-8 window except the poles. Prove the
   margin in the doc comment, re-run `lgammaf_fast_path_agrees_exhaustively` (≈ 600 s; its near-zero tally should go
   to ~0), plant the cold call, count with a per-call spec using the bench's 256 operands. Expected ≈ 310 vs 274.
2. Booked timing (Ivy Bridge: nico or kotori, the less busy; N100) of asinf, acosf, atanf, atan2f, Liella vs glibc
   (instruction mix changed: `sqrtsd`); target ≤ 1.3x IVB. 3. erff/erfcf/hypotf one-pass.
Not to redo: integer kernels for f32 (DESIGN 3.5 B); a partial-write conversion (D-10.67); the round-to-nearest
index `s·32 + 1.5·2^52` in a one-pass kernel (use `atan_red_any`). Plan rows: D-10.63, D-10.67, D-10.68, D-10.72, D-10.82.
