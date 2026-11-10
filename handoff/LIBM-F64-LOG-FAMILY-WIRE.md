# LIBM-F64-LOG-FAMILY-WIRE: log2, log10, log1p on the log quick phase (tier 1)

State: branch `task/libm-f64-log-family-wire-3` (continuation -3, stacked on `-2` and `task/libm-f64-log-family-wire`),
head: see `handoff/reports/LIBM-F64-LOG-FAMILY-WIRE-3.json`. Role keke:~/w/n4 (Zen 1). Decisions in force: D-10.66
(this story), D-10.60 (log quick phase), D-10.51 (tiers), D-10.25/D-10.50 (two builds), D-10.3 (correct rounding),
D-X.20/D-X.32 (hot-text list).

## What is in

* `crates/math/src/fast64/quick_log.rs`: the macro has shared helpers `split` (reduction), `series`, `join`
  (log's evaluation unchanged in order) and per function `*_quick_parts`, `*_quick`, `*_tiered` for log2, log10,
  log1p in both builds; tier 2 = today's `fast64::{,fused::}log{2,10,1p}_nearest`, out of line and cold.
  * log2/log10: log's `(zh, zl)` times `1/ln b` as one double-double product, then Fast2Sum (FMA: 2MultFMA; SSE2:
    `zh` masked to 26 bits times the 26-bit `k26`, exact, plus two rounded terms). Scaled-table alternative weighed
    and rejected (module doc, D-10.66).
  * log1p: `2^-60 <= |x| < 2^-8` log's series with `r = x` exactly; `|x| >= 2^-8`: `1 + x = yh + yl` by Fast2Sum,
    log's reduction of `yh`, correction `d·(1 + r² − r)`, `d = yl·c·2^-e` from the bits (no division).
  * Exact results (run -2): `log2_tiered` answers `2^e` with `e`, `log10_tiered` answers `10^n` (`1 <= n <= 22`)
    from `POW10_BY_BINADE`, both before any arithmetic, so no inexact flag is raised.
* Wired: `crates/boundary/src/hot/exp_log.rs`; selftest hooks `log_family_quick`, `log_family_tiered`.
* Tests (clause s10_0_f_0): `quick_log/tests.rs`, `crates/boundary/tests/log_quick.rs`, the envelope test in
  `s10_math_symbols.rs` for the exact cases' flags.
* Run -3: `crates/libc-so/hot-text.list` regenerated (below).

## Proof numbers (unchanged since run 1)

Bounds: log2/log10 `2^-64.908`, log1p small `2^-67.587`, general `2^-64.346`; RHO `2^-64`. Largest error / bound:
SSE2 0.296 / 0.296 / 0.389, FMA 0.264 / 0.264 / 0.305. Fallback about 0.04% (log2, log10), 0.1% (log1p).

## Counts (run -2's counts2 on keke, read in run -3; also in D-10.66)

Instructions per call, cbench_libm `--compare --counters`, log2 / log10 / log1p: FMA 111.4 / 111.2 / 113.3, glibc
63.1 / 103.0 / 83.6 (before, LOG-QUICK-2 wl_percall: 195.2 / 205.4 / 245.0: no row slower). SSE2 (`-FMA` both
sides) 144.4 / 143.2 / 145.5, glibc 83.2 / 118.4 / 103.1. Still above glibc in instructions on every row.
The one-round ns on a loaded build host (not a timing verdict): FMA log2 29.2 vs 9.3, log10 28.8 vs 18.1, log1p
27.2 vs 22.5.

## Run -3: the CI red (startup-only program 24 page faults > 23)

Cause (perf `page-faults:u -d` after exec, no randomisation, merge base and head built at equal-length paths in
keke:~/work/LIBM-F64-LOG-FAMILY-WIRE-3/{b,h}): the hot-text list named the non-generic `runtime::start`, but since
D-27.10 the function is `start::<PIE>`; `start::<false>` and `apply_irelative` sat in plain `.text`, where other
code's size places them. On this branch `start::<false>` straddled the 64 KiB boundary at 0x6f0000 (faults at
0x6ef200 and 0x6f0016), one more window than on the merge base. origin/next has the same stale list.
Fix: `tools/hottext/regen.sh` on the head tree, fast then release profile (union). It adds 6 keys (the generic
start, its `map_or` closure, `apply_irelative`, `Zone::file_globals`, `iconv::codec_named`, `Conv::to_words`),
removes none, and moves 26 lines into first-touch order. User faults after exec, base / head / head with the new list:
startup 36 / 35 / 32, locale 60 / 60 / 56, tool 62 / 62 / 59. The bounds were not raised.

## Jobs (keke, run -3)

* `libm-f64-log-family-wire-3-lint0`: clippy + static on the inherited head: rc 0.
* `-diag`, `-regen`: the diagnosis and regen above (scratch trees; evidence, not final-head jobs).
* `-gate` (head b93420d57; superseded): full clippy, xtask clippy, static, `cargo xtask so`, `cargo test -p liella
  --test hot_text_products` (12 passed: startup 19 / 23, locale 59 / 68, tool 62 / 71, fork 7 / 9), then `affected
  --base origin/next --run`: rc 1, one failure, `popen_system_eintr system_signal_state_during_and_after` read
  `hits=0`. Cause: libtest runs the body on its own thread, so libtest's main thread (SIGCHLD not blocked) can take
  the process-directed SIGCHLD while `system` blocks it in the test thread, and its handler may run after the
  test's read on a loaded host. Fixed in the test with an iteration-budget wait (0f5dc4988), plant
  `sigchld-never-counted` (`handoff/LIBM-F64-LOG-FAMILY-WIRE-3-plants.txt`).
* `-plants` (b93420d57; superseded by -plants2): 16 CAUGHT, 0 SURVIVED, no WARN-RC-ONLY / WARN-NO-MSG.
* `-static`: static on an export of af5f983a9 (docs only on top of the gated tree): rc 0.
* `-gate2`, `-plants2` (final code head 0f5dc4988; plants2 = the -3 file, then the 16 log-family plants): queued
  at hand-back, results in jobs.tsv.

## Next

1. Timing (if the periodic sweep has not already timed these rows): Ivy Bridge (kotori or nico) and N100 (nozomi),
   `cargo xtask bench --stage` on the role, unpack whole, `./xtask-bin sweep run --only cbench_libm` under
   `~/bin/timing`. Run -3 left it to the sweep (no rep-string, x87 or division change; standing rule).
2. Instruction gap to glibc (log2 +48, log1p +30 FMA): a separate story, profile first (wl_percall phase split).

## Not to redo

The product-by-1/ln b design and its bounds; log1p's correction (one term `d·(1 − r)` is not enough: `2^-59.9`
relative near `|ln| = 2^-8`); the domain splits; the k0 plants on the SSE2 test (equivalent there by design); the
page-fault diagnosis above (the stale key was the cause, not the libm code).
