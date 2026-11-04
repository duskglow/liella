# MATH-M6-F64-GAP-ENVELOPE (register 16.5 #3 + #4), CHECKPOINT: NOT READY, NOT VERIFIED

State: branch task/math-m6-f64-gap-envelope, cut from origin/next 432f1ef7 (LOG-TABLES-3 has landed there, so no
merge was needed). Role shiki:~/w/sh2 (synced at the head). No gate, plants, counts or sweep run yet.

## Done (code, builds with `cargo xtask so`; clippy/static NOT run)
- crates/math/src/fast64.rs: `#[inline(always)]` on the six `*_nearest` entries and on `exp_kernel`, `log_kernel`,
  `logb_kernel`, `log_pow` (#3). No numeric change: the same operations in the same order (Rust does no FP
  contraction), so results should be bit-identical. That is NOT yet confirmed by tests or a digest.
- crates/boundary/src/mathret.rs: `elem_fast` keeps only the read, the branch, `black_box(x)` and `fast`; the
  refused-attempt and non-default-mode fallback is `elem_fast_cold` (`#[cold] #[inline(never)]`, #4).
- Disassembly of target/liella/libm.a (shiki ~/work/math-m6-f64-gap-envelope/{base,after}/, script dis.sh):
  base prologue `push rbp; push r14; push rbx`, with calls to exp_kernel / logb_kernel / log_nearest shim /
  exp2_nearest shim. After: `push rbx; sub $0x10,%rsp` (one push left) and **pow is fully fused** (no PLT call but the
  cold fallback). **exp, exp2, log, log2, log10 now call the `<fn item as FnOnce>::call_once` shim** of
  `fast64::*_nearest`, which LLVM does not inline into `elem_fast`. That is a regression risk for those five
  until it is fixed.

## Exact next step
1. In crates/boundary/src/posix/s10_math.tsv, change every row that passes `math::fast64::{exp,exp2,log,log2,log10}_nearest`
   as a fn item to a closure (`|x| math::fast64::exp_nearest(x)`, like pow's row), then run `cargo xtask math-gen`
   (s10_math_gen.rs is generated: never edit it by hand). Rebuild, then rerun dis.sh: every entry should show no
   PLT call on the hot path.
2. Check the remaining `push rbx`: what lives in rbx across the near-miss `settle` call inside `finish`? If it is `saved`,
   try a `#[cold]` wrapper around that settle call.
3. Bit-identity: add a digest (FNV over the fast bits, MXCSR and errno, plus the directed bits) to
   `double_fast_entry_mismatches` in crates/boundary/tests/s10_math_symbols.rs. Print it in `math_double_fast_entry_sweep`
   and run it with LIELLA_FE_SWEEP_CHUNKS=10 on a base RAM copy and on an after RAM copy; the digests must be equal. Then
   start the full sweep detached (SLOT_SIZE=M).
4. Counts: `cargo xtask bench --compare --counters cbench_libm` at the default level and with
   `--tunables glibc.cpu.hwcaps=-AVX2`, base 432f1ef7 vs after. Keep only if no row is slower.
5. Plants (no bound changed, so no bound plants): skip-ftz-daz-check and wrong-rounding-dispatch must stay CAUGHT,
   plus one plant dropping `black_box(x)`'s ordering (expect EQUIVALENT unless a test catches it; say so).
6. clippy (--workspace --all-targets --all-features -D warnings) and static, then the direct gate, D-10.13 note, and the
   register 16.5 rows #3/#4 with the measured saving.

## RULING NEEDED
- #4's "MXCSR read inline (one stmxcsr)" and "black_box replaced by a register dependency on the read" both need inline
  assembly in liella-boundary. xtask/src/policy.rs permits exactly one asm site (tb-field, tb.rs; owner ruling
  2026-10-06, plan S2/S3: "machine code belongs in csys/"), and `_mm_getcsr` is an unsafe, deprecated intrinsic outside
  every unsafe category. So the read stays a call to `liella_machine_mxcsr_get` (call/ret plus the spill of x across it,
  about 4 instructions). Recommendation: a second, documented asm category `mxcsr-read` (one `stmxcsr` site in a
  boundary fpu module) if the counts show #3/#4 leave the read as a visible share; otherwise keep the call.

## Not to redo
- The base disassembly (base/ on shiki) is from 432f1ef7; reuse it.
- The Edit tool trims a trailing space in a replace_all new_string (`pub fn ` became `pub fn`): put the change
  mid-line instead.
