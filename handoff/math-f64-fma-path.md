# MATH-F64-FMA-PATH handoff (WIP, NOT VERIFIED)

Branch task/math-f64-fma-path (from origin/next 125c0ccc). State: code for exp/log written, never compiled (no local
rustc; keke build job math-f64-fma-path-build1 queued >15 min when the run hit its context budget).

## Design (decision row needed: D-10.48 is a PROVISIONAL id, check plan/07-decisions for the next free D-10.n and add it)
- crates/math/src/dd.rs: `two_prod_fma` (safe `#[target_feature(enable="fma")]` fn, `_mm_fmsub_sd`), same pair as Dekker
  `two_prod` bit for bit wherever the product is exact.
- crates/math/src/fast64.rs: `kernels!` macro builds `mul`, `mul_f64`, `reduce`, `exp_kernel`, `log_kernel` twice: plain
  (`two_prod`) and `fused` (FMA). Only the exact products change, so both builds give identical `y`, `eps`, `None`
  branches and results (bit identity by construction; the error bounds are unchanged and per path the same). Plain
  exp/exp10/pow now call the macro's `reduce` (same arithmetic as exp_log::reduce).
- Dispatch: `dispatch::FMA` AtomicBool + `fma()`/`select_fma()` (same one-byte startup mechanism as AVX2, Rust-only reader
  so not in fold.S), set in `cpu::init_with` from `Features::fma()` (includes OS AVX state) and tunable
  `glibc.cpu.hwcaps=-FMA` (new `Tunables::fma`, config.rs). `selftest::set_math_fma(bool)` is the test-only switch.
- crates/boundary/src/hot.rs: `exp_nearest`/`log_nearest` selectors, UNSAFE(hot-kernel) calls (as hot.rs's AVX2 calls).
- s10_math.tsv: exp d / log d point at crate::hot::{exp,log}_nearest. s10_math_gen.rs NOT yet regenerated in the commit.
- Test: crates/boundary/tests/math_fma_paths.rs (500k-point sample through both builds, bit compare, every 64th vs Real).
- s01_startup_clauses.rs:1467 literal gets `fma: None`.

## Next step (exact)
1. On keke r5: read ~/work/math-f64-fma-path/build1.log (job math-f64-fma-path-build1: math-gen, clippy, static, the new
   test, math lib tests). Fix compile errors (likely spots: glob/`use super::{t,...}` in `fused`, macro doc links,
   `features_cached` visibility, test crate deps liella_math/liella_rt). Commit the regenerated s10_math_gen.rs.
2. Plants: (a) in `two_prod_fma` use `_mm_fmadd_sd` (sign wrong) -> math_fma_paths must fail; (b) selector always plain
   (`if false`) -> counts unchanged (EQUIVALENT for tests, caught by counts); (c) in fused::exp_nearest drop `reduce`'s
   x.lo term -> bit-compare fails.
3. Counts (brief step 0, not started): glibc exp/log instructions:u default and with GLIBC_TUNABLES=glibc.cpu.hwcaps=-FMA
   (also try -FMA,-AVX2_Usable); Liella both paths (force plain with the same tunable). Update register 13.5 #8.
4. Rest of family (exp2, exp10, log2, log10, pow) not done: they only need `fused::*_nearest` wrappers + selectors.

## Open / ruling
- RULING NEEDED: UNSAFE(hot-kernel) now also covers calls to the math FMA builds (category text says SSE2 loads/stores).
  Recommend: widen the category description to "calls to CPU-variant builds behind a dispatch flag" (hot.rs only).
- `glibc.cpu.hwcaps=-FMA` now also switches Liella's FMA path (glibc-compatible name); no new public interface.
