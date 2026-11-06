# LIBM-F64-TRIG-TIER(-3) handoff: double sin/cos/tan/sincos fast kernels, wired (run 3: merge only)

Branch task/libm-f64-trig-tier-3 (continues task/libm-f64-trig-tier-2, which continues task/libm-f64-trig-tier).
origin/next c7c320a2 merged in (merge e67b69d0), which includes LIBM-HOT-PER-FAMILY-2. Role mei:~/w/s2 (Skylake-SP,
FMA). HEAD and job results: handoff/reports/LIBM-F64-TRIG-TIER-3.json.
Decisions in force: D-10.13 (envelope), D-10.25 (FMA builds), D-10.50 (libm decides FMA at load). No new D-row, no compat id.

## Run 3: the merge (CI could not merge TIER-2: conflict in crates/boundary/src/hot/mem.rs)

next split `crates/boundary/src/hot.rs` into `hot/<family>.rs`, and git mapped TIER-2's hot.rs additions onto
`hot/mem.rs` (the conflict covered next's whole moved libm block, even with `-X no-renames`). Resolution:

* `hot/mem.rs` taken exactly as next has it (`git diff origin/next -- crates/boundary/src/hot/mem.rs` is empty).
* `sin_nearest`, `cos_nearest`, `tan_nearest` (macro `trig_nearest!`) and `sincos_nearest` moved to the trig family
  file `hot/trig.rs` (flow rule: family file only; `hot/mod.rs` untouched, it already has `pub(crate) use trig::*`).
  Diff of the entries, old hot.rs block vs new file: only the module doc line `//! The double sin, cos, tan, sincos
  family's boundary wrappers.` and the two intra-doc links `[exp_nearest]` -> `[exp_nearest](super::exp_nearest)`
  (exp_nearest now lives in hot/exp_log.rs; invtrig.rs uses the same form). Code unchanged.
* `s10_math.tsv` / `s10_math_gen.rs` as merged; `cargo xtask math-gen` regenerated them byte-identical (sha256 before
  = after for s10_math_gen.rs and csys/src/adapter/math_*.c), `math-gen --check` ok (job libm-f64-trig-tier-3-check).
* New plant `trig-hot-dispatch` (hot/trig.rs: the FMA build of sin/cos/tan calls the cosine kernel). It SURVIVED
  the trig entry test (plants-1): the Rust test processes never set the FMA dispatch byte (only the libm product's
  constructor does, D-10.50), so no direct test reached the FMA branch of the boundary entries. New test
  `libm_fma_select.rs::libm_product_trig_fma_builds_return_the_baseline_bits` (no spec clause; D-10.25/D-10.50):
  a static `-lm` program prints sin/cos/tan/sincos bits over every binade 2^-36..2^20 (both signs) and n*pi/2,
  run with the FMA builds and with `glibc.cpu.hwcaps=-FMA`; the outputs must be equal. The plant now runs it.
  The same gap holds for every family's FMA branch (exp/log, invtrig, ...): INFORMATIONAL in the report.

## Done in run 2 (unchanged here)

Boundary entries above; `mathret.rs::sincos_fast` (+ cold path); s10_math.tsv sin/cos/tan/sincos `d` rows on
elem_fast / sincos_fast; tests `s10_math_symbols.rs::math_double_trig_fast_entry_matches_the_envelope_in_every_mode`,
`math_fma_paths.rs::trig_fma_build_matches_baseline_bit_for_bit`, `trig.rs::trig_assembly_error_is_within_the_stated_bound`
(clauses s10_0_f_0 / s10_3_c_0, s10_0_g_0). Bound derivation in the module comment of crates/math/src/trig.rs.

## Jobs (mei)

* libm-f64-trig-tier-3-check (rc 0): math-gen (no change), math-gen --check, xtask clippy, xtask static.
* libm-f64-trig-tier-3-check2: xtask clippy, xtask static, xtask so, the new libm_fma_select test.
* libm-f64-trig-tier-3-gate2: `cargo xtask affected --run` under ~/bin/gate, base origin/next (direct tier).
* libm-f64-trig-tier-3-plants2: handoff/LIBM-F64-TRIG-TIER-plants.txt (7 plants), dir
  /mnt/zram/claude/libm-f64-trig-tier-3/plants-2.
* libm-f64-trig-tier-3-counts2: cbench_libm --compare --counters, default then `glibc.cpu.hwcaps=-FMA`, after a
  green gate2 (script ~/work/libm-f64-trig-tier-3/counts2.sh; it exits 4 without building if the gate is not green).
* Results: handoff/reports/LIBM-F64-TRIG-TIER-3.json.
* Superseded: libm-f64-trig-tier-3-plants (rc 1, trig-hot-dispatch SURVIVED, 6 CAUGHT: led to the new test),
  libm-f64-trig-tier-3-gate (rc 143, stopped by me: older head), libm-f64-trig-tier-3-counts (rc 4, never built).
* TIER-2's counts (libm-f64-trig-tier-2-counts2, mei, same code): Liella / glibc instructions per call, FMA:
  sin 158.4 / 100.2, cos 156.3 / 104.7, tan 175.9 / 107.6; -FMA: sin 242.6 / 122.3, cos 239.5 / 126.1,
  tan 353.2 / 124.2. Before (FAST-KERNELS-3): sin 338.6, cos 336.0, tan 494.9. Faster in both builds; 1.05x not reached.

## Exact next step

The 1.05x target is not reached by this design: the later story is the minimax polynomial from the D-10.3 Remez tool
and a pre-split table (fewer Dekker products on the -FMA build; tan -FMA is the furthest row at 2.84x).

Do not redo: the bound derivation, the kernels (tested: fast == accurate, all modes), the merge above.
