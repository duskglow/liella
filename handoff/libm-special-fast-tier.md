# LIBM-SPECIAL-FAST-TIER handoff (run 6: LIBM-SPECIAL-FAST-TIER-6, merge only)

State: branch `task/libm-special-fast-tier-6` = -5's head 77aab541 + `origin/next` c7c320a2 merged in (merge 70db150c),
KERNELS.md regenerated (daa183b4), plants file (04678bec); after it: handoff only. Role shiki:~/w/sh2 (Haswell-EP,
FMA). Report: handoff/reports/LIBM-SPECIAL-FAST-TIER-6.json. Accuracy contract unchanged (correct rounding, ruling
#2760 option A); bound 2^-71 (ruling #2916). No code change beyond paths (coordinator note, 11-05).

## Merge resolution (70db150c)
* `crates/boundary/src/hot.rs` was split by family on next (LIBM-HOT-PER-FAMILY). The erf/erfc FMA dispatch
  wrappers (`erf_nearest`, `erfc_nearest`) moved unchanged into `crates/boundary/src/hot/erf.rs` (the family file,
  flow rule); `hot/mod.rs` untouched (it already re-exports `erf::*`); `hot/mem.rs` is next's version.
* `s10_math.tsv`: `f` rows on next's `math::fast32::erf`/`erfc` (LIBM-F32-ERF), `d` rows on this branch's in-place
  paths (`crate::hot::erf_nearest`/`erfc_nearest`), `lq` rows on `math::special`.
* `crates/math/src/fast64.rs`: next's `fused` import list (a superset: it already has `reduce_pow`); this branch's
  `exp_dd` and its `pub(crate)` items (RHO_EXP, C_EXP, finish) merged without conflict.
* `crates/boundary/tests/math_fma_paths.rs`: next's `both2` + exp2/log2/log10/pow test and this branch's erf/erfc test.
* Generated: `math-gen` output (s10_math_gen.rs, csys adapters) was already current after git's merge; KERNELS.md
  regenerated with `cargo xtask kernel-index`. math-gen --check, tables --check (5 files), kernel-index --check: current.

## Verified on head 04678bec (shiki, Haswell-EP)
* lint `libm-special-fast-tier-6-lint`: math-gen --check, tables --check, `cargo clippy --workspace --all-targets
  --all-features -- -D warnings`, `cargo xtask static`: rc 0.
* Plants `libm-special-fast-tier-6-plants` (cargo xtask plants, handoff/libm-special-fast-tier-6-plants.txt,
  /dev/shm/claude/libm-special-fast-tier-6/plants-1): 9 CAUGHT, 0 SURVIVED, 0 WARN. Runs 3-5's 7 kernel plants, plus
  `hot-erfc-fused` and `hot-erf-baseline` on hot/erf.rs (both builds of the wrappers at their new site).
* Gate `libm-special-fast-tier-6-gate`: `cargo xtask affected --run`, Direct tier (15 changed files): see the report
  json for the result line.

## Counts (instr/call, validation/percall/libm_erf.c; label libm-special-fast-tier-6-counts)
Base = origin/next c7c320a2 (primed with validation/wl_prime_base.sh). Sorted as printed.
| row | glibc | base (next) | new | new/glibc | -5 (Skylake-SP) |
|---|---|---|---|---|---|
| erfc above 1 | 194.3 | 565.0 | 248.0 | 1.28 | 247.0 |
| erf series | 63.0 | 294.0 | 113.0 | 1.79 | 112.0 |
| erfc [-5,5] | 162.4 | 452.9 | 201.4 | 1.24 | 200.4 |
| erfc below 1 | 162.4 | 396.8 | 177.4 | 1.09 | 176.4 |
| erf [-5,5] | 157.9 | 376.0 | 166.0 | 1.05 | 165.0 |
| erf table | 159.3 | 368.0 | 166.0 | 1.04 | 165.0 |
Every row faster than base (-187 to -317). +1.0 on every row against -5's counts (other host, other base): common to
all six rows, so in the shared entry path rather than a kernel; not diagnosed (merge-only run). The erfc row's check
MISMATCH is the reference's last bit (known: below).

## Next steps (optional follow-ups; this branch should land)
1. erf series (113 vs 63): the |x| < 2^-5 path's envelope and dd steps; profile it first (wl_trace) before a design.
2. erfc above 1 (248 vs 194): the e^(-x^2) dd product and the exp kernel; profile first.
3. The +1 per row against -5: compare one call's trace on next's boundary entry (mret::elem_fast, dispatch) if it matters.
4. Baseline build kernel (from -4): per-slot bounds could drop the exact product in the innermost dd step.
5. gamma/Bessel are their own stories.

## Things not to redo
* Degree/slot layout (run 1); the 2^-71 bound; the erfc percall MISMATCH: erfc(0x1.b12e88p+0) is
  0x1.11c9655380463p-6 (true value 0.305 ulp below it, Decimal oracle at 200 digits); the reference returns ...462.
* The identical FMA/-FMA counts of -4 were the libm dispatch byte, fixed in next; do not re-diagnose.
* Run 6's merge resolution above.
