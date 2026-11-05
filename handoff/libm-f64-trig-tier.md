# LIBM-F64-TRIG-TIER(-2) handoff: double sin/cos/tan/sincos fast kernels, wired (run 2)

Branch task/libm-f64-trig-tier-2 (continues task/libm-f64-trig-tier; origin/next c6e5db9d merged in, which includes
LIBM-TABLES-PER-FAMILY-2). Role mei:~/w/s2 (Skylake-SP, FMA). HEAD and job results: handoff/reports/LIBM-F64-TRIG-TIER-2.json.
Decisions in force: D-10.13 (envelope), D-10.25 (FMA builds), D-10.50 (libm decides FMA at load). No new D-row, no compat id.

## Per-family layout (LIBM-TABLES-PER-FAMILY-2)

Nothing to move: the trig kernels live in their own module `crates/math/src/trig.rs` and read generated tables
through `crate::coeffs::tables` (still the one re-export path; `coeffs/trig.rs` is the generated family file). This
story adds no table, so `cargo xtask tables` rewrote nothing (run in job libm-f64-trig-tier-2-check2).

## Done in run 2 (commits 4fe2e85a, the math-gen commit, 2bba70c0)

1. `crates/boundary/src/hot.rs`: `sin_nearest`, `cos_nearest`, `tan_nearest` (macro `trig_nearest!`) and
   `sincos_nearest`, each picking `liella_math::trig::kernel_fma::*` under `dispatch::fma()` (exp's shape and
   UNSAFE(hot-kernel) comment).
2. `crates/boundary/src/mathret.rs`: `sincos_fast` (out-parameters checked first with `ret::must(conv::req(..))`, then
   `fpu::peek` and the in-place attempt; cold `sincos_fast_cold` = `elem_fast_cold` for a pair, else full `sincos`).
3. `s10_math.tsv`: sin/cos/tan/sincos split into `flq` (unchanged) and `d` (elem_fast / sincos_fast); math-gen output
   committed. The tsv change makes the gate FULL (#2921).
4. Tests (clause s10_0_f_0 / s10_3_c_0, s10_0_g_0):
   `s10_math_symbols.rs::math_double_trig_fast_entry_matches_the_envelope_in_every_mode` (new; in-place vs full
   envelope bits/flags/errno, directed modes vs the accurate path; args: 2^-1000, 2^-360, 2^-339, 2^-200, 2^-36,
   2^-30, 2^20 +-3 ulps, n*pi/2 +-2 ulps (n < 400 and up to 42e6), cell edges (2j+1)*pi/128 +-1 ulp, 4000 random
   2^-36..2^23, both signs); the trig exports also join `math_double_fast_entry_stays_out_of_place_in_a_non_default_mode`
   (subnormal x skipped for trig: the result is subnormal and FTZ flushes it); `math_fma_paths.rs::
   trig_fma_build_matches_baseline_bit_for_bit` (200k sample: every binade 2^-36..2^23, near cell edges and n*pi/2;
   sincos pair too). `fast_entry_mismatches` now takes the entry list (the old function is a wrapper).
5. `# Safety` doc on the four in-place entries (clippy missing_safety_doc on the FMA build; doc only).

## Jobs (mei)

* libm-f64-trig-tier-2-check2 (rc 0): math-gen, tables, workspace clippy -D warnings, xtask static, liella-math trig
  (10 passed), s10_math_symbols math_double (4 passed), math_fma_paths (3 passed, FMA present, no SKIP).
* libm-f64-trig-tier-2-gate: FULL (escalated by the tsv change); see the report json for its state.
* libm-f64-trig-tier-2-plants: handoff/LIBM-F64-TRIG-TIER-plants.txt, dir /mnt/zram/claude/libm-f64-trig-tier-2/plants-1
  (P3 reduction constant, a coefficient C6, the bound E_ABS -> 0, the large-argument switch LIMIT_BITS -> 2^30, the
  small edge SMALL_BITS -> 2^-400, the sincos wiring).
* libm-f64-trig-tier-2-counts: `cargo xtask bench --compare --counters cbench_libm`, default then
  `--tunables glibc.cpu.hwcaps=-FMA`; starts after the gate. Before (kanon, FAST-KERNELS-3, both builds alike):
  sin 338.6, cos 336.0, tan 494.9; glibc 100.2 / 104.7 / 107.7 (FMA), 122.5 / 126.2 / 124.2 (-FMA). Projection
  (handoff run 1, not measured): sin/cos ~180 (-FMA) / ~125 (FMA), tan ~260 / ~190.
* Superseded: libm-f64-trig-tier-2-gen (rc 128: a `git status` in the script, the role is not a git tree; math-gen
  itself ran), libm-f64-trig-tier-2-check (rc 101: missing `# Safety` docs, fixed in 2bba70c0).

## Exact next step

1. Read the counts log (`grep -E '^\| *`?(sin|cos|tan|sincos)`? ' ~/logs/libm-f64-trig-tier-2-counts.log`, both
   tunables): if any row is slower than before, revert that part and record a NEGATIVE row in register 16.
2. The 1.05x target is not reached by this design (projection above): the later story is the minimax polynomial from
   the D-10.3 Remez tool and a pre-split table (fewer Dekker products on the -FMA build), per run 1's analysis.

Do not redo: the bound derivation (module comment of trig.rs), the kernels (tested: fast == accurate, all modes).
