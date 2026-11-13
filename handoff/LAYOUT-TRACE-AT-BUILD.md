# LAYOUT-TRACE-AT-BUILD (LT-1): `cargo xtask so` traces its own build, opt-in

State: branch `task/layout-trace-at-build`, cut from origin/next (b888f8ff0). Plan: `plan/layout-rethink.md`
section 4 (on `task/layout-rethink`), rulings #5190-#5194. Decision: `records/pending/LAYOUT-TRACE-AT-BUILD-1.toml`
(series D-X, pending). Plants: `handoff/LAYOUT-TRACE-AT-BUILD-plants.txt`.

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

## Jobs (shiki)

ltab-fast (fast table, rc 0), ltab-lint-2 (clippy, static, xtask hotlists unit tests: rc 0), ltab-release
(release table, rc 0; its next-release hot_text_products fails on next itself, see above), ltab-gate (affected
--run --base origin/next, default mode), ltab-plants (3 plants, trace mode), ltab-optout (no-tracer message,
ungrouped opt-out, plain build removes the marker). Superseded: ltab-lint (static records: bare `|` in the record).
Logs and products: `~/work/LAYOUT-TRACE-AT-BUILD/{m1,m2,optout}` on shiki.

## Open / next

* INFORMATIONAL: next (no switch) fails `init_short_programs_stay_within_their_page_fault_bounds` in the release
  profile on this host: setlocale + printf 53 > 51. With the switch it reads 50.
* NOT VERIFIED at hand-back: ltab-gate, ltab-plants, ltab-optout were still queued (results in jobs.tsv).
* Next step: when the three jobs are green, land; then LT-2 (clusters by probe phase), LT-3 (ptrace tracer),
  LT-4 (host-canonical, before the default changes). Do not redo: the fast/release tables and determinism above.
