# STARTUP-FAULTS-RESTORE: static start-up back to 61 page faults, and contracts that hold it

State: branch `task/startup-faults-restore-3` (continues `-2` and `-1`), merged with origin/next 03c9a4972 (RECORDS
cutover; coordinator-allowed merge). Role shiki:~/w/sh1 (Haswell-EP). Decisions in force: D-X.20, D-X.22, D-X.32,
D-X.47, **D-X.53** (this story: bounds = count + at most 1, lowered when a count falls; a dead hot-text key fails
`so` unless allow-listed with a reason; start-up state words in `.data1`); `records/decisions/D-X/D-X.53.toml` passes
`cargo xtask records check` on the merged head. Plants: `handoff/STARTUP-FAULTS-RESTORE-plants.txt` (5 plants).
Report json: `handoff/reports/STARTUP-FAULTS-RESTORE-3.json`.

## What the story does (runs 1-2)
* **Dead keys fail the build** (`xtask/src/hottext.rs`): in a fast or release products build every key of
  `crates/libc-so/hot-text.list` must match a function section, any tier, except a key traced only in the other
  profile or a line of `crates/libc-so/hot-text.absent` (`<key>\t<profile>\t<reason>`); a stale allow line fails
  too. `so` writes `target/product/<profile>/hot-text.matched`; `tools/hottext/prune.py <list> <fast> <release>`
  drops keys no build matches.
* **Start-up state on one page** (`xtask/src/coldbss.rs::move_startup_state`): listed statics go to `.data1`, which
  ends on the page where `.bss` starts (execve dirties it when it zero-fills the bss tail). `scan.S` puts
  `scan_level` there.
* **Bounds** (`crates/core/tests/hot_text_products.rs`) at count + 1; `init_startup_path_lies_in_the_hot_text_range`
  and `init_startup_state_statics_are_in_data1` check the structure (the +1 slack lets a one-fault regression through).
* `tools/hottext/faultmap.py`: page faults of one run -> section and nearest symbol.

## Run 3
* **gate3's red, cause:** `init_startup_state_statics_are_in_data1` compared raw symbol names; libc.a defines
  `..._SYSINFO_EHDR.0` (LLVM split part number), so "no definition". The test now drops `.llvm.<n>` / `.<n>` tails
  like coldbss.rs does (af317606a). plants3 (mei) had the same failure in its baseline: no plant ran there.
* **Merge with next:** `hottext.rs` keeps this story's all-tier contract and routes its error through next's
  `dead_key_verdict` (regen mode `--hottext-regen`, HOTTEXT-REGEN-UNBLOCK); the remedy text names
  `regen.sh . --build`. `hot-text.list` took next's regenerated hunks, minus three `Ms1_` keys of
  `liella::locale::Locale` (with, string, item_parsed): next renumbered that impl `Ms2_` (its keys are listed), so
  they match no build in either profile (what prune.py drops).
* **Merge cost a fault, fixed:** startup-only 17 -> 18 after the merge. Pre-merge head and merged head built side by
  side on shiki (setarch -R, perf page-fault maps, `~/work/STARTUP-FAULTS-RESTORE-3/fmap-*` there): the extra fault
  is `liella_legacy::nscache::NSCACHE` (608 bytes, written by `nscache::start` in every process) alone on a `.data`
  page. NSCACHE joined the start-up state (bbd593199): startup 18 -> 17; setlocale+printf 56, tool 59, fork 6,
  localtime 19, localtime_mktime 19, float 2 all unchanged (every run identical).
* FORK_BOUND 8 -> 7 (count 6 on Haswell-EP here; 6 on Zen 1 in HOTTEXT-REGEN-UNBLOCK's base column).
* Plant 5 (`nscache-not-moved`): NSCACHE dropped from the list; the structural test must name it.

## Counts (fast products, `hot_text_products --nocapture`, every run identical)
| mode | bound | Zen 1 (run 1) | Skylake-SP (run 2) | Haswell-EP (run 3, merged) |
|---|---|---|---|---|
| startup only | 18 | 17 | 17 | 17 |
| setlocale + printf | 57 | 56 | 56 | 56 |
| tool | 60 | 59 | 59 | 59 |
| fork | 7 | 7 (pre-merge) | 7 (pre-merge) | 6 |
| localtime / localtime+mktime | 20 / 20 | - | - | 19 / 19 |
| float | 6 | - | - | 2 |

Kit printf faults 62 -> 61 (run 1, Zen 1). wl_count vs main (run 2, mei): page faults printf-loop 1258745 ->
1240079, seq 126 -> 123, wc 264 -> 259, md5sum 146 -> 144; instructions within 0.01%.

NEGATIVE (keep): run 1's m3 (cpu COPY_MIN/FILL_MIN/ERMS_LARGE_FROM, fold.S cmp_sse, global_locale::MAPS to .data1)
gained nothing; the freed pages got other first writers. Run 3 shows the same effect from the other side: a
`.data` reorder alone moved NSCACHE onto its own page. A hand-kept list holds only while `.data` order holds.

## Jobs
Run 3 (shiki): -lint1 rc 1 (`so` refused the 3 dead Ms1_ keys; clippy, static, records check passed), -check2 rc 0,
-fmap-pre / -fmap-new rc 0 (diagnosis), -check3 rc 0 (NSCACHE move, all 14 tests). Final head: -gate1 (clippy,
static, affected --run --base origin/next) and -plants1. Superseded: mei startup-faults-restore-2-gate3 (rc 1, the
test bug above) and -plants3 (rc 2, same bug in its baseline).

## Open / next
* FLOAT_BOUND 6 and LOCALTIME 20: float reads 2 on Haswell-EP; ratchet to 3 once a Skylake-SP (CI class) run prints
  its count (the gate log of this branch on CI does).
* Structural fix for `.data`/`.bss` start-up faults: generate the `.data1` placement from a trace of first-touched
  small statics (as regen.sh does for text), instead of the hand-kept `STATE_KEY_SUFFIXES`.
* regen.sh `--build`: run prune.py once both profiles' `hot-text.matched` come from the same tree (a stale other
  profile file would drop live keys, so it needs a tree-hash guard).
