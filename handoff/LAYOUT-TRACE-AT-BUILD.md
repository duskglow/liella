# LAYOUT-TRACE-AT-BUILD (LT-1): `cargo xtask so` traces its own build, opt-in

State: branch `task/layout-trace-at-build-4` (LAYOUT-TRACE-AT-BUILD-3's head 37009447e plus a merge of
origin/next and a test fix; HEAD in handoff/reports/LAYOUT-TRACE-AT-BUILD-4.json). Plan: `plan/layout-rethink.md`
section 4, rulings #5190-#5194. Decision: D-X.88 (`records/decisions/D-X/D-X.88.toml`). Plants:
`handoff/LAYOUT-TRACE-AT-BUILD-plants.txt`.

## What it does

* Switch: `LIELLA_LAYOUT_TRACE=1` (trace at build), `LIELLA_LAYOUT_TRACE=ungrouped` (explicit opt-out: ungrouped
  product, failed layout contracts printed as a WARNING), unset/empty/`0`: unchanged. Any other value is an error.
  Only `run_so_as` reads it; repair, bench (x86-64-v3) and regen builds keep the committed lists; with
  `--hottext-regen` it is an error.
* Trace mode (`xtask/src/so.rs` `trace_layout`, `finish_archive`): keeps `hot-text.pre-group.a`, writes empty lists
  into `target/product/<profile>/`, forces that directory for the process (`hotlists::force_dir`), finishes an
  ungrouped product from the pre-group copy into `target/product/<profile>/trace` (libc.a, crt1, .profile), runs
  `tools/hottext/regen.sh <root> --replace --all-modes --products <trace>` with `LIELLA_HOT_LISTS=<lists>` (log
  `target/product/<profile>/trace.log`), then groups the real archive from the fresh lists. No usable gdb
  (`gdb -nx -batch -ex 'python print(1)'`, `HOTTEXT_GDB` honoured) fails with a message naming the opt-out.
* `regen.sh --all-modes`: also traces `page_faults` fork (child), strings, float, mktime, rebind, appended after
  `tool` (earlier first-touch order unchanged). With localtime / localtime_mktime (probe_text.sh) and true / locale /
  tool, every mode `hot_text_products.rs` bounds is traced.
* Products marker `target/liella/.hot-lists`: line 1 the lists directory, then each list's FNV hash (the layout
  verdict cache hashes the products dir, so a list change invalidates it). `layout-check` follows it
  (`hotlists::follow_products`); `hot_text_products.rs` reads lists via `generated_list()`. Plain `so` removes it.
* `group()` takes the `Layout`: Traced ignores `hot-text.absent` (no history) and rewords a missing
  `hot-text.clusters` key as "trace-at-build: ... this build's trace does not reach that function ... until LT-2";
  Ungrouped reads no list and aligns no cluster.
* Build tag: unchanged (`crates/ csys/ include/`); lists live under `target/`, so the trace never recompiles.

## Measurements (Haswell-EP build host, role shiki:sh2, relay harness, 5 runs, every run identical)

| probe | next fast | trace fast | next release | trace release |
|---|---|---|---|---|
| start-up `true` (bound 17) | 11 | 11 | 10 | 10 |
| setlocale + printf `locale` (bound 51) | 51 | 49 | **53 (red on next)** | 50 |
| `tool` (bound 56) | 54 | 53 | 56 | 53 |
| fork child | 5 | 5 | 5 | 5 |
| localtime / localtime + mktime | 19 / 19 | 17 / 17 | 19 / 19 | 18 / 18 |
| float | 4 | 3 | 3 | 3 |
| 0-fault mktime, strings, rebind | 0, 0, 0 | 0, 0, 0 | 0, 0, 0 | 0, 0, 0 |
| puts / cat (`perf stat -e minor-faults`) | 17 / 23 | 16 / 23 | 16 / 21 | 16 / 21 |
| hot group in the probe link | 605,770 B (545 sections) | 604,170 B (484) | 644,426 B | 636,810 B (434) |
| layout-check (12 contracts) / hot_text_products | pass / pass | pass / pass | pass / **FAIL** | pass / pass |

Every row equal or lower with the switch, both profiles. Determinism: a second trace-mode `so` gave byte-identical
`hot-text.list`, `startup-text.keys`, `probe-text.keys`, `startup-data.keys` and a byte-identical `libc.a`, both
profiles. Cost with a warm cargo cache: fast `so` 233 s -> 262 / 247 s; release 409 s (next) -> 447 / 396 s.
Fast `tool`: the plan's 52 -> 53 (base b31418181) does not reproduce on today's next on the same host class (next
54, trace 53), so the window phase was not diagnosed; the trace set now also holds every bound mode (`--all-modes`).

## Trace-set coverage check (LAYOUT-TRACE-AT-BUILD-2)

The first run's plants that dropped the `locale` or `tool` probe survived: a dropped probe could still pass the
fault bounds by luck of placement. `regen.sh` now records every command `trace()` runs (`$work/traced`), and under
`--all-modes` (what `so` passes) checks after the traces that every mode of `page_faults.c` was traced: the
`strcmp(argv[1], "<mode>")` modes minus `relay`, parsed as `init_startup_data_accesses_are_in_the_data1_cluster`
parses them (that test also checks they are all in `STARTUP_PROBE_MODES`, which the bounds measure), plus `true`
(`startup.names`), with localtime / localtime_mktime counted from probe_text.sh's `probe/names.<mode>`. A missing
one fails the trace and `so` with `regen: --all-modes: the trace set misses page_faults mode(s): <modes> (...)`;
a parse that finds fewer than 3 modes fails too. The traced commands, their order and environment are unchanged
(the record is a side file), so the lists are the first run's; not re-measured in this run.

Plants (mei, ltab2-plants): ltab-drop-locale-probe, ltab-drop-tool-probe and the new ltab-drop-probe-text-mode
CAUGHT by that message; ltab-rename-skips-first CAUGHT as before. The tool notes "caught by no test the log
names" for the first three: they are caught by `so` itself, before the test runs, which is the intent.

## Merge with next (LAYOUT-TRACE-AT-BUILD-3, shiki role sh2)

CI's merge_next stopped on `xtask/src/so.rs`: XTASK-SO-NOCHANGE-PROFILE moved the libc `cargo rustc` call into
`LIBC_BUILD.command` (one flag set and one `LIELLA_BUILD_TAG` per shared target directory; the interpreter in
`target/product/ld-pie`). The one conflict was `build_archive_locked`'s signature: it keeps the `layout` parameter
and takes next's body, so every layout builds through `LIBC_BUILD.command` with the same tag. Both intents hold:
the trace stays out of the tag (its lists are under `target/product/<profile>`, outside `crates/ csys/ include/`),
and no xtask step is skipped on an input hash (cargo's fingerprint covers compilation only; the grouping reruns from
lists rewritten empty, then traced, on every trace-mode build; the layout verdict cache hashes the products and the
marker carries the lists' hashes). Next's dead-key change (LAYOUT-DEADKEY-WARN) merged without conflict; Traced still
ignores `hot-text.absent`. On the merged head: ltab3-so (clippy, static, then `LIELLA_LAYOUT_TRACE=1 so` and
hot_text_products in fast, then release: both 23 passed; every page_faults mode traced in both; fast 101/503/153
startup/hot/probe keys, release 102/436/154; rc 0), ltab3-plants (4 plants, trace mode, fast: 4 CAUGHT, 0 WARN,
rc 0), ltab3-gate (affected --run --base origin/next, default mode: 32 suites, 11710 passed, 0 failed, conformance
132/0, peak 16G, rc 0). Also ltab3-lint0 (clippy + static on the inherited head 5edb1b197: rc 0).

## Merge with next, again (LAYOUT-TRACE-AT-BUILD-4, keke role n3)

CI's merge_next stopped on `xtask/src/hottext.rs`: LAYOUT-FIRST-WINDOW (next) reads `hot-text.clusters` once at
the top of `group()` and links the `hot-text.first-window` keys right after the start-up tier
(`first_window_positions`). Resolution: the cluster file is read once for every layout; `Ungrouped` takes
`positions()` of its empty list (no first-window key, no cluster); `Traced` applies the first-window keys to its
own trace, and a first-window or cluster key the trace lacks (or places in the start-up tier / inside a cluster)
fails with the `trace-at-build:` message naming both hand-kept files until LT-2. Regen builds keep next's fallback.
The plant `ltab-rename-skips-first` moved from `positions()` to `link_positions()`, where `group()` now takes its
link positions (a plant on `positions()` no longer reaches the grouping).

Next's new test `init_startup_group_ends_inside_the_first_fault_around_window` failed in trace mode (fast): it
counted the start-up tier of the COMMITTED `hot-text.list` (95 keys) while the trace-mode product is grouped from
its own list (91 start-up keys; fstat at link position 91, 0x417980..0x41798c, inside the window 0x408000..0x418000).
It counted puts and fread into the group and read a margin of -804 bytes. It now reads `generated_list("hot-text.list")`
like the other list readers in that file (a plain build still reads the committed list). Not a layout regression:
the real trace-mode group ends ~1.6 KiB inside the window.

Jobs (keke, role n3): ltab4-lint0 (clippy + static on the inherited head 37009447e: rc 0); ltab4-so2 (clippy,
static, xtask hottext unit tests 24 passed, then `LIELLA_LAYOUT_TRACE=1 so` + hot_text_products in fast, then
release: 24 passed each; fast 101/482/152 startup/hot/probe keys, release 102/433/153; rc 0); ltab4-plants2
(4 plants, trace mode, fast: 4 CAUGHT, 0 WARN, rc 0); ltab4-gate (affected --run --base origin/next, default
mode; FULL by escalation: 32 suites, 11714 passed, 0 failed, conformance 132/0, peak 12G, rc 0). Superseded: ltab4-so and ltab4-plants (rc 101 / 2:
the first-window test above, before the fix).

Second merge in the same run (next moved during the gate): LAYOUT-TRACE-HOST-CANONICAL(-3) landed (LT-4's
host-canonical tracing). Conflict in `tools/hottext/regen.sh` `trace()`: keep this branch's `$work/traced` record,
call next's `trace_tagged` form (LANG, optional `tz=`). Next's `--replace` step prunes the COMMITTED
`crates/libc-so/hot-text.absent` against the rewritten list; trace-at-build (`--all-modes`, list under target/,
Traced ignores the allow-list) now skips it (`if [ $all_modes = 1 ]; then absent=; fi`; next's guard line is
unchanged because its unit test pins the text). New guard in `so.rs` `trace_layout`: the files of crates/libc-so
are recorded before the trace; any the trace changed are put back and the build fails naming them
(`tracked_files` / `restore_tracked`; plant ltab-prune-committed-absent).

Jobs on the final code head 9237acb84 (keke, n3): ltab4-so3 (clippy, static, xtask hottext tests 26 passed; trace
mode fast 103/510/155 and release 104/455/156 startup/hot/probe keys, hot_text_products 24 passed each, no tracked
file changed; rc 0); ltab4-plants3 (5 plants, trace mode, fast: 5 CAUGHT, 0 WARN, rc 0); ltab4-gate2 (affected
--run --base origin/next, FULL by escalation, ~77 min; running at hand-back). The jobs of the first merge
(ltab4-so2, ltab4-plants2, ltab4-gate) ran on 7635ff45e and are superseded by these. The head merges cleanly with
next as of 12:15Z (62 commits ahead).

## Jobs, LAYOUT-TRACE-AT-BUILD-2 (mei, role s1)

ltab2-lint0 (clippy + static on the inherited head: rc 0); ltab2-plants (4 plants, trace mode, fast: 4 CAUGHT,
rc 0; baseline = trace-mode `so` with the check + hot_text_products green); ltab2-gate (clippy, static, affected
--run --base origin/next, default mode: 32 suites, 11692 passed, 0 failed, conformance 130/0, rc 0);
ltab2-release (release trace-mode `so`: `every page_faults mode traced: true float fork locale localtime
localtime_mktime mktime rebind strings tool`, then hot_text_products 23 passed, rc 0).

## Jobs, first run (shiki)

ltab-fast (fast table, rc 0), ltab-lint-2 (clippy, static, xtask hotlists unit tests: rc 0), ltab-release
(release table, rc 0; its next-release hot_text_products fails on next itself, see above), ltab-gate (affected
--run --base origin/next, default mode), ltab-plants (3 plants, trace mode), ltab-optout (no-tracer message,
ungrouped opt-out, plain build removes the marker). Superseded: ltab-lint (static records: bare `|` in the record).
Logs and products: `~/work/LAYOUT-TRACE-AT-BUILD/{m1,m2,optout}` on shiki.

## Open / next

* INFORMATIONAL: next (no switch) fails `init_short_programs_stay_within_their_page_fault_bounds` in the release
  profile on this host: setlocale + printf 53 > 51. With the switch it reads 50.
* First run's jobs: ltab-gate and ltab-optout rc 0; ltab-plants rc 1 (the two survivors, fixed above).
* Do not redo the merge: a later conflict in `build_archive_locked` resolves the same way (keep `layout`, take the
  build call from next); one in `group()` keeps `layout` (Ungrouped: no list, no first-window, no cluster;
  Traced: the `trace-at-build:` message) and next's single read of the hand-kept inputs.
* Any new hot_text_products test that reads a list must use `generated_list()`, not `crates/libc-so/...`
  directly, or it reads the wrong list for trace-mode products (the hand-kept inputs, clusters / first-window /
  startup-state, stay in crates/libc-so).
* Next step: land; then LT-2 (clusters by probe phase), LT-3 (ptrace tracer), LT-4 (host-canonical, before the
  default changes). A new `page_faults.c` mode now needs a trace in regen.sh (or probe_text.sh MODES) or the
  trace-mode build fails by name. Do not redo: the fast/release tables, determinism, the coverage check.
