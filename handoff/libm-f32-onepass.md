# LIBM-F32-ONEPASS: handoff (one-pass `float` exp family, first slice: expf, exp2f)

State: branch `task/libm-f32-onepass` (cut from origin/next 5a12901d), code commit a8d5f4a27, plants 4d8f8123f,
this note and the report on top. Role mei:~/w/s4.

## Done (D-10.63, reserved id)
* `fast32::exp_onepass` (crates/math/src/fast32/exp_log.rs) and the entries `exp_onepass_any`/`exp2_onepass_any`:
  for 2^-26 <= |x| < 87.33 (exp) / 126 (exp2), tested on the bits BEFORE any floating-point instruction, the existing
  Tang kernel (`exp_z`: 2^(j/128) table + degree-4 Estrin) runs once in double in the caller's rounding direction;
  the margin is proved for every direction (eps <= 2^-43.71, test margin EXP_ONEPASS_DELTA = 1024 ulp of y; the proof
  is the doc comment), then one `cvtsd2ss` rounds in the caller's direction and raises inexact. No MXCSR read.
* `mathret::elem32_onepass` (boundary): the fast result is returned as is; a refusal goes to a cold out-of-line
  function: NaN -> `x + x` (no envelope, no kernel: the expf_qnan row), else the unchanged `elem` envelope.
* tsv rows `exp f` / `exp2 f` switched (expf, expf32, exp2f, exp2f32); `cargo xtask math-gen` and `kernel-index`
  regenerated s10_math_gen.rs and crates/math/KERNELS.md.
* expf hot path in libm.so.6 (objdump of Liella's own build): 40 straight-line instructions, no call, no stack
  (before: about 58 in expf plus a call through the GOT to the MXCSR peek). The integer range check precedes cvtss2sd.

## Why it is correct in the caller's environment (the argument the tests check)
* Inexact is raised only for an inexact result: e^x is exact only at 0 (refused by the bit test) and 2^x at the
  integers, where every kernel operation is exact (z = 128x, r = 0, p = 1, y = S); a refusal after the kernel has
  run therefore leaves inexact set only when the envelope's result is inexact too.
* Every intermediate is a normal double, so FTZ/DAZ (D-10.22) change nothing; subnormal/NaN/inf/zero x never reach
  a floating-point instruction (no denormal-operand, no invalid).
* Traps: with an unmasked inexact trap the trap now fires inside the kernel instead of at the envelope's exit, for
  operands whose result is inexact (exact results raise nothing before the envelope reads MXCSR). INFORMATIONAL.

## Tests
* `math_expf_onepass_entries_match_the_envelope` (s10_math_symbols.rs, in the direct tier): both functions, MXCSR
  {plain, FTZ, DAZ, both} x 4 directions, 4 samples per 2^20 encodings + 20 edge operands (range edges, exact exp2
  integers, NaNs, infinities, zeros, subnormals): bits, errno, flags against `mathret::elem` (the old entry's path).
* `math_expf_onepass_sweep`, `math_exp2f_onepass_sweep` (ignored, exhaustive): all 2^32 encodings x 4 directions,
  chunks from an atomic counter, asserts every chunk checked once; `LIELLA_SWEEP_STRIDE=k` for sizing.
* Plants: handoff/LIBM-F32-ONEPASS-plants.txt (conversion in RN, rounding test dropped, NaN unquieted, range
  widened into overflow).

## Results (mei, final code a8d5f4a27; JOBS mei:f32op-gate, f32op-plants, f32op-sweep, f32op-counts)
* Gate (direct tier, --base origin/next): 7 suites ok, 1909 passed, 0 failed, conformance 15/15, peak 3.9G, rc 0.
* Plants: 4 CAUGHT, 0 SURVIVED, 0 WARN. Baseline 2 (expf sweep, stride 4) had 0 mismatches.
* Exhaustive: expf and exp2f, 17,179,869,184 calls each (2^32 x 4 directions), 0 mismatches (bits, errno, flags).
* Counts (cbench_libm, instr/op Liella vs glibc): expf 47.2 vs 39.1, exp2f 47.2 vs 37.1, expf_qnan 27.1 vs 23.1;
  unconverted controls exp10f 69.2, logf 77.2, log2f 78.2, sinf/cosf 90.2, powf 249.6 (keke before: expf/exp2f 73).
  Instructions: -22 per call, target (<= glibc) NOT reached (1.21x / 1.27x).
* Time on mei (unbooked, shared, indicative only): expf 2.18x, exp2f 2.30x, expf_qnan 0.71x glibc.
  LIKELY CAUSE (to verify first): the new hot path converts with `cvtss2sd %xmm0,%xmm1` and no
  `xorps %xmm1,%xmm1` (the old path had one), a false dependency on the upper half of xmm1, which holds the previous call's y, so
  back-to-back calls form one latency chain (47 instructions taking as long as the 69-instruction exp10f). Fix ideas:
  let x die before the conversion (the cold call re-making x from integer bits) so LLVM converts in place, or build
  the double from the bits with integer operations; check the disassembly, then book IVB/N100 timing.

## Not done (next steps, in order)
0. Break the cvtss2sd false dependency above, re-count, book timing (nico/kotori + N100) for expf/exp2f/expf_qnan.
1. exp10f: an integer x in 1..10 has an exact result, and x·RN(128 log2 10) is inexact, so the kernel would raise a
   spurious inexact before refusing. Fix: refuse integer x in [1, 10] on the bits first (or a split constant).
2. logf: log(1) exact and kernel exact there; log2f/log10f need a table form where 2^k (10^k) stay exact
   (y = e + log2 table + poly; log10: refuse 10^k on the bits). Same entry `elem32_onepass`.
3. sinf/cosf/tanf: only x = 0 exact (cos 0 = 1): the bit test refuses |x| < 2^-26; the margin needs re-proof for
   directed intermediates (TRIG_DELTA 40 in RN).
4. powf: method (b) of the brief (online test + fallback); its exact cases are many (refuse before arithmetic or
   prove the kernel exact on them).
5. Booked timing on Ivy Bridge (nico/kotori) and N100 for expf/exp2f/expf_qnan: not run in this story.
Plan rows to read: D-10.63, D-10.51, D-10.14, D-10.22, D-10.13.
