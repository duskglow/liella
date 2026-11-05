# LIBM-F32-HYPERBOLIC: handoff (float hyperbolic family on the fast32 pattern)

State: branch `task/libm-f32-hyperbolic-4` (on -3's `f77fb221`), code head `aa2375f4`; handoff commit on top.
Role mei:~/w/s2. Report: handoff/reports/LIBM-F32-HYPERBOLIC-4.json. Earlier runs: -1 `872c0c88` (six kernels),
-2 `0b446d69` (log1p64 shorter, six exhaustive sweeps), -3 `1f68110c` (in-place nearest entry for all six).

## Done (-4)
* `faa059c0`: ruling #2989. The safe inline square root from task/libm-f64-invtrig-hypot-tier-2 (`sqrtsd_sse2`, a
  `#[target_feature(enable = "sse2")]` fn, and its `sqrtsd` caller in hot.rs) was **cherry-picked from `9fc38c3f`**
  (`-n`, conflict resolved: that commit's invtrig macro, kernels and table test are not ours and were dropped; the two
  functions are the same text). `fast32::asinh_core_with` / `acosh_core_with` take the root as a parameter (as the f64
  branch's invtrig kernels do); `asinh_nearest_sqrt` / `acosh_nearest_sqrt` are the entries; `hot::asinhf_nearest` /
  `acoshf_nearest` pass `sqrtsd`; s10_math.tsv `f` rows point at them (s10_math_gen.rs regenerated with
  `cargo xtask math-gen`). The directed-mode path (`eval`) keeps `sqrt64`. Proof: a correctly rounded root (≤ 0.5u)
  is inside sqrt64's 2u bound and raises only inexact on the cores' normal operands, so the bounds, deltas and
  rounding test hold unchanged.
* `aa2375f4`: `// ARCH(x86_64)` tag on the `core::arch` use (xtask static arch-audit refused it untagged).
* Tests: the core-bound test checks both root variants; `hyperbolic_hardware_sqrt_agrees_sampled` (every 16411th
  encoding, gate); `hyperbolic_hardware_sqrt_agrees_exhaustively` (ignored, 2^33 calls, nearest).

## Jobs (mei)
* `libm-f32-hyperbolic-4-sweep-sqrt` rc 0 on faa059c0 (liella-math identical at aa2375f4): 3 603 461 982 of 2^33 calls
  taken, no mismatch against the accurate path, 272 s.
* `libm-f32-hyperbolic-4-plants` rc 0 on faa059c0: 4 CAUGHT (asinh-core-root, acosh-core-root, hot-sqrtsd,
  hot-acosh-route), 0 WARN. `-plants-2` rc 0 on aa2375f4: 4 CAUGHT, 0 WARN. `-percall-2` rc 0 on aa2375f4: the table below.
* `libm-f32-hyperbolic-4-gate-2` on aa2375f4: affected escalated itself to FULL (s10_math_gen.rs is a generated source);
  stubs and clippy ok; left running at hand-back for the dispatcher.
* `libm-f32-hyperbolic-4-gate` rc 1 on faa059c0: xtask static arch-audit (untagged seam), fixed by aa2375f4.

| row (instr/call, wl_percall, base 1f68110c) | glibc | -3 | -4 | ratio |
|---|---|---|---|---|
| asinhf | 93.1 | 150.7 | 108.7 | 1.17 |
| acoshf | 78.0 | 139.0 | 97.0 | 1.24 |
| sinhf / tanhf / atanhf / coshf | | unchanged | 99.5 / 106.0 / 103.2 / 79.0 | 0.68 / 0.81 / 0.88 / 0.96 |
The root cost 42 instructions per call (inferred from the -3 to -4 delta, not traced). The change swaps ~20 dependent
multiplies and adds for one `sqrtsd` (latency 15-20 cycles): the instruction mix changed, so the timing sweep must
confirm the row (brief exception "square root").

## Open / next
* MERGE NOTE: both this branch and the f64 invtrig branch add after `log_nearest` at the end of hot.rs, so whichever
  lands second has one textual conflict: keep one copy of `sqrtsd_sse2` / `sqrtsd` (same text). The f64 branch also needs
  the `// ARCH(x86_64)` tag (aa2375f4) or its xtask static fails arch-audit.
* asinhf (+15.6) and acoshf (+19.0) remain above 1.05x. Next: wl_trace.sh one call of each (base primed on mei) to split
  log1p64 (~35) / rounding test / entry, then shorten log1p64 for w ≥ some bound (a log table step on 1 + w instead of the
  series), proving the bound and rerunning both sweeps (`eval` and the hardware-root sweep).
* Optional: the same in-place entry for sinf/asinf/atanf/log1pf/expm1f/cbrtf (prove their subnormal-operand flags).
* Not to redo: D8 (-2), the #2889 oracle for this family, the six `eval` sweeps (kernels unchanged since 0b446d69).
* Plan rows: rulings #2760 (A), #2783, #2784, #2889, #2989; handoff/libm-f32-double-kernels.md PATTERN.
