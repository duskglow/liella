# LIBM-F64-LOG-FAMILY-WIRE: log2, log10, log1p on the log quick phase (tier 1)

State: branch `task/libm-f64-log-family-wire` (on LIBM-F64-LOG-QUICK-2's head 615d6a4f9), code head 1b72cd30c,
handoff commit on top. Role mei:~/w/s2. Decisions in force: D-10.66 (this story, reserved id), D-10.60 (log quick
phase), D-10.51 (tiers), D-10.25/D-10.50 (two builds), D-10.3 (correct rounding).

## What is in

* `crates/math/src/fast64/quick_log.rs`: the macro now has shared helpers `split` (reduction), `series`, `join`
  (log's evaluation unchanged in order) and per function `*_quick_parts`, `*_quick`, `*_tiered` for log2, log10,
  log1p in both builds; tier 2 = today's `fast64::{,fused::}log{2,10,1p}_nearest`, out of line and cold.
  * log2/log10: log's `(zh, zl)` times `1/ln b` as one double-double product, then Fast2Sum (FMA: 2MultFMA; SSE2:
    `zh` masked to 26 bits times the 26-bit `k26`, exact, plus two rounded terms). Scaled-table alternative weighed
    and rejected (module doc, D-10.66): it still needs the double-double product on `r − r²/2`.
  * log1p: `2^-60 <= |x| < 2^-8` log's series with `r = x` exactly; `|x| >= 2^-8`: `1 + x = yh + yl` by Fast2Sum,
    log's reduction of `yh`, correction `d·(1 + r² − r)`, `d = yl·c·2^-e` from the bits (no division), into the
    low sum. Tiny `|x|`, `±0`, `x <= −1`, `x >= 2^960` go to tier 2.
* Wired: `crates/boundary/src/hot/exp_log.rs` (`log2_nearest`, `log10_nearest`, `log1p_nearest` call the tiered
  functions); selftest hooks `log_family_quick`, `log_family_tiered`.
* Tests (clause s10_0_f_0): `quick_log/tests.rs` (SSE2: `log_family_constants_and_bounds`, `..._error_within_bound`,
  `..._matches_correctly_rounded` (env `LIELLA_QUICK_LOG_N` sets the count, default 10^6 per function),
  `log_family_exact_and_hard_cases`); `crates/boundary/tests/log_quick.rs` (FMA bound + exactness, both builds
  through the entries, same env var).

## Proof numbers (job t4 on 1b72cd30c's code, Skylake-SP host)

* Bounds recomputed by the test: log2/log10 `2^-64.908`, log1p small `2^-67.587`, general `2^-64.346`; RHO `2^-64`.
* Largest error / bound: SSE2 log2 0.296, log10 0.296, log1p 0.389; FMA 0.264, 0.264, 0.305.
* Fallback (uniform groups): SSE2 log2 374, log10 384, log1p 376 of 400000 (0.094%; model 0.098%); through the
  entries both builds 31-43 of 40000 per function.
* Exact: `log2(2^k)` = k for every k in [−1022, 1023], `log10(10^n)` = n for n in 0..22, answered by the quick
  phase in both builds. Hard cases (1 ± k·2^-53, 10^-n, log1p tiny/±2^-8/near −1, misrounded unchecked samples)
  declined or right; tier 2 answers more than half of the quick misses.
* 10^7 operands per function in both builds: in the gate job (`LIELLA_QUICK_LOG_N=10000000`), NOT VERIFIED at
  hand-back.

## Jobs (mei)

* `libm-f64-log-family-wire-gate`: full clippy, xtask clippy, static, the 10^7 accuracy runs (SSE2 unit test; both
  builds through the boundary), `affected --base origin/next --run`; writes `~/work/LIBM-F64-LOG-FAMILY-WIRE/gate.done`.
* `libm-f64-log-family-wire-plants`: 11 plants (`handoff/LIBM-F64-LOG-FAMILY-WIRE-plants.txt`): per function miss
  returned, tier 2 skipped, `1/ln b` one ulp off (log2, log10), log1p correction cut to one term, log1p `yl`
  dropped, FMA product's `p'` dropped.
* `libm-f64-log-family-wire-counts`: waits for gate.done, then `cargo xtask bench --compare --counters cbench_libm`
  into `~/work/LIBM-F64-LOG-FAMILY-WIRE/counters.txt` (the host has FMA, so this is the FMA build; SSE2 build count
  NOT VERIFIED). Before (LOG-QUICK-2 wl_percall, FMA): log2 195.2, log10 205.4, log1p 245.0; glibc 66.0 / 105.9 / 86.0.
* Superseded: `-t1` (clippy: one unsafe block with 6 calls), `-t2` (clippy: manual range contains), `-t3` (log1p
  tier-2 check on the misrounded subset only, which tier 2 also declined), `-t4` (green; test-only run).

## Not done / next

1. Timing (Ivy Bridge, N100) not run: context budget, and the timing hosts go down from 20:00 UTC 11-07. Next run:
   `cargo xtask bench --stage` on the role, then booked `sweep run --only cbench_libm` as LOG-QUICK-2 did.
2. SSE2-build instruction counts (force the SSE2 build in the bench), and a `wl_percall` split of one call.
3. The old `handoff/LIBM-F64-LOG-QUICK-plants.txt` `near-dropped` plant's literal now matches four sites (log2/log10/
   log1p reuse the text): the plants tool will refuse that file if rerun; anchor it to `log_quick` with `perl:`.

## Not to redo

The product-by-1/ln b design and its bounds; log1p's correction (one term `d·(1 − r)` is not enough: `2^-59.9`
relative near `|ln| = 2^-8`); the domain splits.
