# LIBM-F32-HYPERBOLIC: handoff (float hyperbolic family on the fast32 pattern)

State: branch `task/libm-f32-hyperbolic-3` (on -2's `0383207a`), code head `1f68110c`; handoff commit on top.
Role keke:~/w/n1. Report: handoff/reports/LIBM-F32-HYPERBOLIC-3.json. Earlier runs: -1 `872c0c88` (six kernels),
-2 `0b446d69` (log1p64 one degree shorter, plants 5/5, six exhaustive sweeps green on the kernels).

## Profile (-3, validation/wl_trace.sh, call 11, head 0383207a)
| row | Liella total | entry+envelope (`elem`, mxcsr_get, alias) | kernel (`fast32::<f>`: core + rounding test) | glibc total |
|---|---|---|---|---|
| asinhf | 183 | 50 | 133 | 93 |
| coshf | 123 | 50 | 73 | 82 |
The surplus sat in the entry: the float hyperbolic exports used the generic `mret::elem` envelope (MXCSR switch,
status merge, exit write), while expf/logf already used the in-place `elem32_fast` entry.

## Done (-3, 1f68110c): in-place nearest entry for sinhf..atanhf
* `fast32::eval_nearest` answers for the six hyperbolic cores (same cores and rounding test as `eval`; the doc
  comment says why only inexact is raised where they answer: |x| ≥ 2^-26 / x > 1, normal intermediates, divisors
  normal and ≥ 2^-24). `nearest_fn!` `sinh_nearest`..`atanh_nearest`; s10_math.tsv rows split `f` (elem32_fast)
  / `dlq` (elem); s10_math_gen.rs regenerated with `cargo xtask math-gen`. No kernel changed, so -2's exhaustive
  sweeps still cover the values (not rerun).
* Test `math_float_hyperbolic_fast_entry_matches_the_envelope_in_every_mode` (s10_math_symbols.rs, s10_0_f_0):
  every 2^16th float of each sign plus the domain edges, in-place vs the DAZ-forced envelope (bits, flags,
  errno), directed modes against liella-math.
* NEGATIVE (not landed): ruling #2783's `_mm_sqrt_sd` does not compile safely on 1.99.1. rustc E0133: "the sse2
  target feature being enabled in the build configuration does not remove the requirement to list it in
  `#[target_feature]`". liella-math forbids `unsafe`, so the change was reverted before it was committed.

## Jobs (keke) on 1f68110c
* `libm-f32-hyperbolic-3-plants` rc 0: entry-cosh-value, entry-atanh-sign: 2 CAUGHT, 0 WARN.
* `libm-f32-hyperbolic-3-percall` rc 0 (wl_percall, spec ~/work/LIBM-F32-HYPERBOLIC-3/hyp.c, a copy is in the
  report). `libm-f32-hyperbolic-3-gate` (clippy -D warnings, xtask static, affected --base origin/next): see report.

| row (instr/call) | glibc | -2 (0383207a) | -3 (1f68110c) | ratio |
|---|---|---|---|---|
| sinhf | 146.9 | 154.5 | 99.5 | 0.68 |
| tanhf | 130.5 | 162.0 | 106.0 | 0.81 |
| atanhf | 117.7 | 159.2 | 103.2 | 0.88 |
| coshf | 82.0 | 133.0 | 79.0 | 0.96 |
| asinhf | 93.1 | 193.7 | 150.7 | 1.62 |
| acoshf | 78.0 | 184.0 | 139.0 | 1.78 |
(Controls: expf 73.0 vs 42.0, sinf 152.0 vs 42.0. sin/asin/atan/log1p/expm1/cbrt float still use `elem`: the
same entry change should cut ~50 from each, once their subnormal-operand flags are proven.)

## Open / next
* asinhf, acoshf are still above 1.05x. Their kernels are about 130 instructions: sqrt64 (3 Newton steps + Heron, ~20
  dependent ops), log1p64 (~35), the rounding test. Next: a hardware square root without `unsafe` in liella-math.
  RULING NEEDED: try `f64::sqrt` from core first (safe, if it is stable on 1.99.1), else a sqrt hook from
  liella-boundary (UNSAFE(hot-kernel)), as `hot::exp_nearest` does. Rerun asinhf/acoshf/asinf/acosf sweeps after.
* Optional: give sinf/asinf/atanf/log1pf/expm1f/cbrtf the same in-place entry (prove their flags on subnormal x).
* Not to redo: D8 (settled in -2), the #2889 oracle for this family (no mp tier), the exhaustive sweeps (kernels
  unchanged since 0b446d69).
* Plan rows: ruling #2760 (A), #2783, #2784, #2889; handoff/libm-f32-double-kernels.md PATTERN.
