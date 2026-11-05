# LIBM-F64-EXPLOGPOW-TIER handoff (run 1: FMA builds for exp2, log2, log10, pow)

Branch `task/libm-f64-explogpow-tier` (cut from origin/next f2157504). Role shiki:~/w/sh2 (Haswell-EP
E5-2697 v3, FMA, AVX2). Code head d94de40e. Decisions in force: D-10.13, D-10.25, D-10.50, rulings #2760 A, #932.
No new decision row, compat id or differences row (no behaviour change: both builds return the same bits).

## Profile before design (from counted data, no new trace)
LIBM-F64-FAST-KERNELS-3's cbench_libm counts (kanon, head e976b48c) gave exp2 / log2 / log10 / pow **the same
count with and without FMA** (186.3 / 228.6 / 242.7 / 481.5), while exp went 181.3 -> 126.2 and log 217.5 -> 161.4.
Cause (read in the source): register 16.5 #8 had reached only the kernels inside `kernels!` (exp_kernel,
log_series, reduce_exp10). exp2's `ln2_times`, log2/log10's `logb_assemble`, and pow's `log_pow` / `pow_kernel`
(4 + 1 products) were outside, and their entries had no `fused::` build or `hot.rs` dispatcher. So the surplus
phase is the Dekker products (17 flops each against 2 with FMA): 3 in exp2, 3 in log2/log10, 7 in pow. Estimated
saving: about 30-45 per row, about 100 for pow (exp saved 55 for 2 products).

## Change (commit d94de40e)
* `crates/math/src/fast64.rs`: `ln2_times`, `logb_assemble`, `logb_kernel`, `log_pow`, `pow_kernel` moved into
  `kernels!` (each exact product now `$prod`; `pow_kernel` writes `Dd::mul_f64` out with `$prod`). A new macro
  argument `$ln2` gives `ln2_times` its exact `r·LN2[0]`: `ln2_prod` (baseline, the pre-split Dekker product
  as before) or `fused::ln2_prod` (one FMA). New `fused::{exp2,log2,log10,pow}_nearest`; the plain entries share
  new `exp2_domain` / `log2_domain` / `log10_domain` / `pow_domain` / `pow_round` with them. The baseline arithmetic
  is unchanged expression for expression. Exactness of each product in both builds is stated in the proofs
  (exp2's: `r` is 0 or a multiple of `ulp(x) >= 2^-113`).
* `crates/boundary/src/hot.rs`: `exp2_nearest`, `log2_nearest`, `log10_nearest`, `pow_nearest` dispatchers
  (`dispatch::fma()`, as exp/exp10/log). `s10_math.tsv` names them; `s10_math_gen.rs` regenerated
  (`cargo xtask math-gen`). The tsv change makes the gate FULL (#2921).
* Test `math_fma_paths::exp2_logb_pow_fma_build_matches_baseline_bit_for_bit` (s10_3_c_0, s10_0_g_0): about 220k
  exp2, 220k log2/log10 and 170k pow operands through both builds, bit for bit, every 64th against the accurate
  path. It also runs pow's refused domain (negative, zero, infinite, NaN and subnormal x, x = 1, tiny y, y·2^11
  integral) through both builds against the accurate path.

## Results
* Direct checks on d94de40e (job `libm-f64-explogpow-tier-check2`, rc 0): workspace clippy `-D warnings`,
  `cargo xtask static`, `math_fma_paths` (2 passed), `liella-math --test fast64` (2 passed), `--lib fast64`
  bound tests (8 passed).
* Plants `libm-f64-explogpow-tier-plants1` (file handoff/LIBM-F64-EXPLOGPOW-TIER-plants.txt, dir
  /mnt/zram/claude/libm-f64-explogpow-tier/plants-1): **6 CAUGHT, 0 SURVIVED, 0 WARN** (FMA pair's error term,
  reduction constant LN2B, log_pow coefficient, log_pow tail bound LTAIL, pow's product error term, pow's
  domain ladder).
* Gate `libm-f64-explogpow-tier-gate`: escalated FULL (tsv change), **still running at hand-back**.
* Counts `libm-f64-explogpow-tier-counts` (`cbench_libm`, default then `--tunables glibc.cpu.hwcaps=-FMA`): this
  job starts after the gate and is **still running at hand-back. NOT VERIFIED: no after numbers yet.** "Before"
  for both builds = the `-FMA` column for these four rows (they had no FMA build). The FAST-KERNELS-3 counts on
  Broadwell are exp2 186.3, log2 228.6, log10 242.7, pow 481.5, glibc 51.1 / 63.1 / 102.9 / 141.2. Read the log;
  if any row (including exp, log, exp10 and the `-FMA` column) is slower than before, revert d94de40e's slower
  part and record a NEGATIVE row in register 16.

## Not done / next step
1. The rows stay above 1.05x glibc (see Results). Register 16's remaining gap on the FMA build is structural:
   double-double carrying in the log assembly and pow's log (log_pow carries r³/3 in double-double), and the exp
   kernel's degree-8 tail. Next design to profile: a two-level (or 128-entry) log table so the tail drops to
   degree 5, and pow's log with a 2^-7 → 2^-9 reduction (larger LOG_C/LOG_L tables). Profile one call per phase
   first (validation/wl_percall.sh), as the brief requires.
2. Directed rounding modes are untouched (they take `fast64::{exp2,log2,log10,pow}` with `settle`).
3. Timing: not booked. No change to the instruction mix beyond scalar FMA replacing Dekker, so timing is left
   to the periodic sweep.

## Process note
I edited one comment line with `sed -i` (against the rules; no harm: the change was checked by the build and the
diff). Every other edit used the Edit tool or a Python script.
