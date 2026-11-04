# MATH-F64-FMA-PATH handoff (run 3 = MATH-F64-FMA-PATH-3)

Branch task/math-f64-fma-path-3 (stacked on task/math-f64-fma-path-2, from origin/next 125c0ccc). Role keke:~/w/r5.
Code head 298a64f6 (the handoff/report commit follows it). Decisions in force: **D-10.22**
(plan/07-decisions/s10-mathematics.md), ruling #2261 (UNSAFE(hot-kernel) category text widened).

## Run 3
- build3 (run 2) rc 1, cause: clippy on the new test's sample arithmetic (`as_conversions`, `indexing_slicing`,
  `arithmetic_side_effects`). Fixed with the file-level allow the other boundary tests use.
- build1 (run 3) rc 1, cause: `xtask static`: arch-audit (dd.rs `two_prod_fma` cfg + intrinsics `use`, fast64.rs
  `mod fused` cfg had no `// ARCH(x86_64):` tag), opaque-audit (test calls exp/log: needs `#![no_builtins]`; an
  `opaque-ok` note covers only allocator names), trace (test named no clause: now s10_3_c_0, s10_0_g_0). All fixed.
- Plant gap from run 2 closed: `selftest::math_fma()` returns the build in force; the test asserts that
  `set_math_fma(true/false)` took effect. New plant `select-ignores-arg` covers it.
- `s10_math_gen.rs` regenerated (math-gen) and committed: only exp/log (3 entries each) move to `crate::hot::*_nearest`.
- Jobs on keke (chain `~/work/math-f64-fma-path-3/verify.sh`): `math-f64-fma-path-3-build2` (math-gen, xtask clippy =
  `cargo clippy --workspace --all-targets --all-features -D warnings`, xtask static, math_fma_paths, liella-math lib)
  rc 0; `math-f64-fma-path-3-gate1` (direct tier, `--base origin/next`); `math-f64-fma-path-3-plants1`
  (`plants.sh 1`, RAM copy /dev/shm/claude/math-f64-fma-path-3/plants-1; fmadd-sign, fused-exp-ulp, fused-log-scale,
  select-ignores-arg; exits 1 on a survivor). Results: see handoff/reports/MATH-F64-FMA-PATH-3.json.
- Equivalent by design (not planted): `hot::{exp,log}_nearest` taking the baseline whatever the flag says returns the
  same bits (D-10.22 construction); only instruction counts can show it.

## Next step (exact)
1. Counts (original brief step 0, not started): `cargo xtask bench --compare --counters cbench_libm` for the exp and log
   rows, default and with `--tunables glibc.cpu.hwcaps=-FMA` (forces both glibc and Liella onto the baseline). Report
   instructions per call Liella FMA / Liella baseline / glibc both; update saturation register 13.5 #8. Keep only if no
   measured row is slower.
2. Rest of the family (exp2, exp10, log2, log10, pow): `fused::*_nearest` wrappers + selectors in hot.rs + tsv rows +
   math-gen, the test extended to them.

## Design (unchanged; see D-10.22)
`kernels!` builds mul, mul_f64, reduce, exp_kernel, log_kernel with `two_prod` and (in `fused`) `dd::two_prod_fma`;
exact products are unique, so results are bit-identical by construction. `dispatch::FMA` byte set in `cpu::init_with`
from `Features::fma()` and tunable `glibc.cpu.hwcaps=-FMA`; `selftest::set_math_fma` forces a build for tests,
`selftest::math_fma` reads it.

## Not to redo
The macro scope fix, D-10.22, the category widening, the lint/static fixes above, the gen-file regeneration.
