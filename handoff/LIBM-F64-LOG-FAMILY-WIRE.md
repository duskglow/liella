# LIBM-F64-LOG-FAMILY-WIRE: log2, log10, log1p on the log quick phase (tier 1)

State: branch `task/libm-f64-log-family-wire-2` (continuation -2 on `task/libm-f64-log-family-wire`), head: see the
report json. Role keke:~/w/n2 (Zen 1). Decisions in force: D-10.66 (this story, reserved id), D-10.60 (log quick
phase), D-10.51 (tiers), D-10.25/D-10.50 (two builds), D-10.3 (correct rounding).

## What is in

* `crates/math/src/fast64/quick_log.rs`: the macro has shared helpers `split` (reduction), `series`, `join`
  (log's evaluation unchanged in order) and per function `*_quick_parts`, `*_quick`, `*_tiered` for log2, log10,
  log1p in both builds; tier 2 = today's `fast64::{,fused::}log{2,10,1p}_nearest`, out of line and cold.
  * log2/log10: log's `(zh, zl)` times `1/ln b` as one double-double product, then Fast2Sum (FMA: 2MultFMA; SSE2:
    `zh` masked to 26 bits times the 26-bit `k26`, exact, plus two rounded terms). Scaled-table alternative weighed
    and rejected (module doc, D-10.66): it still needs the double-double product on `r − r²/2`.
  * log1p: `2^-60 <= |x| < 2^-8` log's series with `r = x` exactly; `|x| >= 2^-8`: `1 + x = yh + yl` by Fast2Sum,
    log's reduction of `yh`, correction `d·(1 + r² − r)`, `d = yl·c·2^-e` from the bits (no division), into the
    low sum. Tiny `|x|`, `±0`, `x <= −1`, `x >= 2^960` go to tier 2.
  * **Exact results and flags (run -2):** the fast entry (`mathret::elem_fast`) returns a fast-path answer with
    MXCSR as the arithmetic left it, so an answer must be inexact. At `x = 2^e` (log2) and `x = 10^n`, `1 <= n <=
    22` (log10), the quick product gave the exact value but raised inexact (the gate's
    `math_double_fast_entry_matches_the_envelope_in_every_mode`: 136 mismatches, flags `0x1fa0` vs `0x1fc0`).
    `log2_tiered` now answers `2^e` with `e` (fraction bits 0, one shift and test on the common path) and
    `log10_tiered` answers `10^n` from `POW10_BY_BINADE` (71 entries by biased exponent, built by a const loop),
    both before any arithmetic. `x = 1` stays on the quick phase (all `+0`, no flag). log1p has no exact result
    in its quick domain.
* Wired: `crates/boundary/src/hot/exp_log.rs` (`log2_nearest`, `log10_nearest`, `log1p_nearest` call the tiered
  functions); selftest hooks `log_family_quick`, `log_family_tiered`.
* Tests (clause s10_0_f_0): `quick_log/tests.rs` (`log_family_constants_and_bounds`, `..._error_within_bound`,
  `..._matches_correctly_rounded` (env `LIELLA_QUICK_LOG_N`, default 10^6 per function),
  `log_family_exact_and_hard_cases`, now also the tiered exact paths and the table's shape);
  `crates/boundary/tests/log_quick.rs` (FMA bound + exactness, both builds through the entries). Flags of the exact
  cases: the existing envelope test in `s10_math_symbols.rs` (edges 2, 0.5, 4, 2^-60, 2^-1022, 10, 100, 1e22).

## Proof numbers (inherited run, Skylake-SP host; values unchanged by run -2's exact paths)

* Bounds: log2/log10 `2^-64.908`, log1p small `2^-67.587`, general `2^-64.346`; RHO `2^-64`.
* Largest error / bound: SSE2 log2 0.296, log10 0.296, log1p 0.389; FMA 0.264, 0.264, 0.305.
* 10^7 per function, SSE2 unit test (run -2's first gate on keke, before the fix): log2 9990240 answered, fallback
  3884 of 4·10^6; log10 9990409, 3859; log1p 8547994, 4007 (0.1%, model 0.098%).

## Run -2 (this run)

1. Static findings fixed: `// UNSAFE(hot-kernel) SAFETY:` directly above the `#[allow(unsafe_code)]` of
   `log_family_quick`'s FMA arm; KERNELS.md regenerated. LOG-QUICK's `near-dropped` plant anchored to `log_quick`.
2. Inherited plants job (mei) had 2 SURVIVED: `log{2,10}-scale-one-ulp` moved `k0`, which only the FMA build's
   product uses (the SSE2 build multiplies terms below `2^-25` by it); they now run the FMA test (CAUGHT on keke),
   new `log{2,10}-k26-one-ulp` cover the SSE2 high product; log1p plants' msg regex fixed (WARN-NO-MSG before).
3. The gate found the exact-result flag defect above; fixed; plants `log2-exact-path-dropped`,
   `log10-exact-path-dropped`, `pow10-table-short` added.

## Jobs (keke, code head 72cce56e6; later commits touch handoff/ only)

* `libm-f64-log-family-wire-2-gate2`: full clippy, xtask clippy, static, the 10^7 accuracy runs (SSE2 unit test;
  both builds through the boundary), `affected --base origin/next --run`; writes
  `~/work/LIBM-F64-LOG-FAMILY-WIRE-2/gate.done`. rc 0: `affected: ok`, peak 11G, envelope test green.
* `libm-f64-log-family-wire-2-plants2`: rc 0, 16 plants, 14 CAUGHT, 0 SURVIVED, 2 WARN-NO-MSG
  (`log{2,10}-tier2-skipped`: caught rc 101 by the new tiered-path assertion before the old message; the plants
  file's regex now names both messages).
* `libm-f64-log-family-wire-2-counts2`: waits for gate.done, then `bench --compare --counters --runs 1
  cbench_libm` (FMA build), the same with `--tunables glibc.cpu.hwcaps=-FMA` (SSE2 build, both sides), then
  `bench --stage ~/work/LIBM-F64-LOG-FAMILY-WIRE-2/libm-stage.tar --only cbench_libm` (the timing tarball).
  Numbers NOT READ by run -2. Before (LOG-QUICK-2 wl_percall, FMA): log2 195.2, log10 205.4, log1p 245.0; glibc
  66.0 / 105.9 / 86.0. The exact paths add a shift and test (log2) or shift, subtract, compare (log10) per call.
* Superseded: `-kidx` (rc 128: a trailing `git status` in a role without .git; KERNELS.md was written), `-gate`,
  `-plants`, `-counts`, `-stage` (killed: the gate found the flag defect; `-stage` also used a wrong `--stage`
  syntax), `-t1`/`-t2` (compile error; stale KERNELS.md), `-t3` (green quick check), `-kidx2`.
* Timing bookings 132 (kotori) and 133 (nozomi) cancelled: the tarball comes after gate and counts, past the budget.

## Next

1. Read counts2 on keke (`grep -E "log(2|10|1p)" ~/logs/libm-f64-log-family-wire-2-counts2.log`) into this file
   and D-10.66; check no row is slower than before.
2. Timing, Ivy Bridge (kotori or nico) and N100 (nozomi): copy `~/work/LIBM-F64-LOG-FAMILY-WIRE-2/libm-stage.tar`
   from keke, unpack whole, `./xtask-bin sweep run --only cbench_libm` under `~/bin/timing`, as LOG-QUICK-2 did.
   Timing hosts are down from 20:00 UTC 11-07 until about midweek.

## Not to redo

The product-by-1/ln b design and its bounds; log1p's correction (one term `d·(1 − r)` is not enough: `2^-59.9`
relative near `|ln| = 2^-8`); the domain splits; the k0 plants on the SSE2 test (equivalent there by design).
