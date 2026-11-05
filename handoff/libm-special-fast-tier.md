# LIBM-SPECIAL-FAST-TIER handoff (run 5: LIBM-SPECIAL-FAST-TIER-5, merge with next, re-verify, recount)

State: branch `task/libm-special-fast-tier-5` = LIBM-TABLES-PER-FAMILY's head (stacks-on) + `task/libm-special-fast-tier-4`
merged in (clean) + `origin/next` f2157504 merged in (merge 2bf99a9d). Code head 4473888e (after it: handoff only).
Role mei:~/w/s1 (Skylake-SP). Report: handoff/reports/LIBM-SPECIAL-FAST-TIER-5.json. Accuracy contract unchanged
(correct rounding, ruling #2760 option A); bound 2^-71 (ruling #2916). No new design work this run (brief).

## Merge resolution (2bf99a9d)
* `crates/boundary/src/hot.rs`: both sides kept: the erf/erfc FMA dispatch entries and next's asinhf/acoshf
  entries with their inline `sqrtsd`.
* `crates/math/src/fast64.rs`: this branch's `exp_dd` and next's `reduce_exp10`, both in `kernels!`; the `fused`
  module imports the union (`reduce_pow` plus next's exp10 names).
* `crates/math/src/fast32.rs` (LIBM-TABLES-PER-FAMILY split vs next's LIBM-F32-HYPERBOLIC-4): the split core kept;
  next's change moved by hand, otherwise unchanged: `asinh_core_with` / `acosh_core_with` into fast32/hyperbolic.rs,
  `asinh_nearest_sqrt` / `acosh_nearest_sqrt` after the nearest entries in fast32.rs, the two `check(..._with...)`
  lines, `agree_sqrt` and the two `hyperbolic_hardware_sqrt_agrees_*` tests into fast32/tests.rs.
* Generated files: none conflicted; `math-gen --check` and `tables --check` (5 files) current on the merged tree.
* The erf/erfc kernels stay in their own module, crates/math/src/fast_special.rs.

## Verified on head 4473888e (mei, Skylake-SP)
* lint `libm-special-fast-tier-5-lint1`: math-gen --check, tables --check, `cargo clippy --workspace --all-targets
  --all-features -D warnings`, `cargo xtask static`: rc 0.
* Plants `libm-special-fast-tier-5-plants` (cargo xtask plants, handoff/libm-special-fast-tier-5-plants.txt,
  /mnt/zram/claude/libm-special-fast-tier-5/plants-1): 9 CAUGHT, 0 SURVIVED, 0 WARN. Run 3/4's 7 erf/erfc plants plus
  two on the merge's sites: `hot-erfc-fused` (hot.rs erfc FMA entry calls erf: math_fma_paths) and
  `asinh-sqrt-moved` (moved hyperbolic code: hyperbolic_hardware_sqrt_agrees_sampled).
* Gate `libm-special-fast-tier-5-gate`: `cargo xtask affected --run`, escalated to FULL by the tool (s10_math.tsv is
  a generator input). Result: see the report JSON.

## Counts (instr/call, validation/percall/libm_erf.c; label libm-special-fast-tier-5-counts-2)
Base = origin/next f2157504 (has -2's erf, not -3/-4's in-place entries). Liella now runs its FMA build from libm
(LIBM-F64-FAST-KERNELS landed in next: libm sets its own dispatch byte), which -4 could not measure.
| row | glibc (FMA host) | base (next) | new | new/glibc | -4 (keke, baseline build) |
|---|---|---|---|---|---|
| erf [-5,5] | 157.9 | 376.0 | 165.0 | 1.05 | 238.0 |
| erf table | 159.3 | 368.0 | 165.0 | 1.04 | 238.0 |
| erf series | 63.0 | 294.0 | 112.0 | 1.78 | 156.0 |
| erfc [-5,5] | 162.4 | 452.9 | 200.4 | 1.23 | 310.0 |
| erfc below 1 | 162.4 | 396.8 | 176.4 | 1.09 | 252.1 |
| erfc above 1 | 194.3 | 565.0 | 247.0 | 1.27 | 425.0 |
Every row faster than base (-189 to -318). Target <= 1.05x reached for erf and erf table, not for erf series or the
erfc rows. The erfc row's `check` MISMATCH is the reference's last bit (known: see below). The baseline (non-FMA)
build was not recounted on this head (-4's numbers are from the same kernels before the merge; NOT VERIFIED here).

## Next steps (optional follow-ups; this branch should land)
1. erf series (112 vs 63): the |x| < 2^-5 path's envelope and dd steps; profile it first (wl_trace) before a design.
2. erfc above 1 (247 vs 194): the e^(-x^2) dd product and the exp kernel; profile first.
3. Baseline build kernel (from -4): per-slot bounds could drop the exact product in the innermost dd step.
4. erff/erfcf (float, still mp) and gamma/Bessel are their own stories.

## Things not to redo
* Degree/slot layout (run 1); the 2^-71 bound; the erfc percall MISMATCH: erfc(0x1.b12e88p+0) is
  0x1.11c9655380463p-6 (true value 0.305 ulp below it, Decimal oracle at 200 digits); the reference returns ...462.
* The identical FMA/-FMA counts of -4 were the libm dispatch byte, fixed in next; do not re-diagnose.
