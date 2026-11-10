# LAYOUT-STARTUP-RODATA(-2..-5): hot read-only data in one 64 KiB-aligned .rodata1 (D-X.62, amends D-X.50)

**State (2026-11-10, run -5):** branch `task/layout-startup-rodata-5` (stacks on -4..-1; base origin/next 2e33a3cb9, not
merged). Build role keke:~/w/n1. Scratch on keke: ~/work/LAYOUT-STARTUP-RODATA-5/ (run.sh = link the cbench_startup child
against a tree's products and single-step it under gdb, trace.py = the gdb tracer, cg.rs/cg.sh = the scan variants measured
with perf stat, check.sh = so + trace + counters, next/ = next tip 0d0d82e5d built). Older: shiki ~/work/LAYOUT-STARTUP-RODATA-4/.
Ruling #4313 (option 1) in force: group ALL read-only data the fault-bounded probes read; never raise a fault bound.

## Run 5: CI red startup_instr_c 2159 > 2150 (fixed forward, limit untouched)
- Cause (single-step trace of the `true` child, branch vs next tip, products of each): only `liella_rt::start::program`
  differed, 152 vs 139 steps. The 64 KiB alignment gap after `.rodata` spans whole pages, so the linker opens a new R
  `PT_LOAD` for `.rodata1` (8 program headers, next 7), and the header scan cost 13 instructions per header.
- Fix (crates/rt/src/start.rs, D-X.62 body): one walk over the `phnum` headers' slice (split_first_chunk; no second
  counter) and one compare passing over every type below `PT_PHDR`. Same results (first header of a type wins, phnum /
  truncated header limit the read; new assertion for first-wins). Standalone copy, perf instructions:u per call: 8 static
  headers 126 -> 100, 7 -> 90 (was 112), PIE 11 headers 163 -> 141, 9 with TLS 138 -> 121.
- Product, keke (Zen 1), bench child `CBENCH_COUNTERS=1 CBENCH_ROWS=startup_instr_c`, min of 5: **branch 2229, next tip
  2241** (before the fix the branch was +13 steps over next in the trace; now 2300 vs 2309). The CI harness reports lower
  absolute numbers (its env); the delta is what moved.
- INFORMATIONAL: startup_kinstr_c (user+kernel, not gated) branch 186044 vs next 173091 (+13k, about 7%): the extra
  segment's exec/mmap work and the 64 KiB file-offset gap. Not from run 5's change; a layout cost of the group. The
  `.data1`/RW half or LAYOUT-DATA-CLUSTERS should weigh it (an R segment that starts 64 KiB-aligned without a split).

## Run 5 hand-back status
- Jobs (keke): lint rc 0 (clippy all-targets, xtask clippy, static on 440d1fd0e), plants 5/5 CAUGHT, counts rc 0 (2229 vs
  next 2241), gate `layout-startup-rodata-5-gate` (direct tier) still running at hand-back (the rt change reaches a wide tier).
- handback-check red on a contradiction: it calls docs/codemap/symbols.tsv train-owned (restored to the base copy in
  a660692d6), while `static` on the head alone (the branch conflicts with next, so no merged run) says symbols.tsv is out
  of date. Next step: the train's regen/merge settles it; or a ruling on which check wins for an unmerged branch.

## What the branch does
- Run 1: `xtask/src/startup_ro.rs` renames start-up read-only sections to `.rodata1`; entry.S aligns `.rodata1` to 64 KiB;
  STARTUP-DATA-WINDOWS ro budgets 1/1; product test with 16 text paddings.
- Run 2: start functions = startup-text.keys + hot-text.list keys of tiers startup/stdio/strings/locale/tool (`HOT_TIERS`);
  transitive selection (text -> .rodata*/.data.rel.ro* -> .rodata*).
- Run 3: whole section names read from the archive's ELF members (readelf cuts at 256 bytes); `cargo xtask startup-ro-plan`.
- Run 4: release first localtime_r read 21 (bound 20). **Measured next first** (same harness as the test, ADDR_NO_RANDOMIZE,
  env LANG only, min of 5): next tip 9af6dff86 and base 2e33a3cb9 release both read localtime 19 / lt+mktime 19, the
  branch 21 / 21, identical on the Skylake-SP host (mei) and the Haswell-EP host (shiki). So the +2 was the branch's.
  Fault map: the two extra faults are the `TZ` (2 bytes) and `TZDIR` (5 bytes) getenv name literals (ip env_find), which
  in next shared windows start-up had already faulted; the group moved their neighbours out and left them alone in two cold
  `.rodata` windows. The zone functions are hot-text tier `extra`, outside `HOT_TIERS`. Fix (generic, trace-built):
  `tools/hottext/probe_text.sh` traces each fault-bounded probe mode that no tier covers (`MODES="localtime
  localtime_mktime"`) as the test runs it into `crates/libc-so/probe-text.keys` (@generated, both profiles merged with
  merge.py, tier column `extra`; regen.sh calls it), and `startup_ro::inputs` adds those keys (`PROBE_KEYS`) to the starts.

## Measured (this head 56504e2a1, same harness; next = tip 9af6dff86)
| products | group bytes | true | locale | localtime | lt+mktime |
|---|---|---|---|---|---|
| next release | - | 15 | 54 | 19 | 19 |
| run 3 release | 49385 | 15 | 53 | 21 | 21 |
| run 4 release | 49412 | 15 | 53 | **18** | **18** |
| run 4 fast | 60064 | 17 | 53 | 20 | 20 |
Fast is unchanged from run 3 (17/53/20, at the bounds 17/55/20). Bounds untouched (ALWAYS FORWARD: release localtime is now
one under next; tightening LOCALTIME_BOUND needs the fast profile at 19 too, not yet).

## Open / next step (from run 4; run 5 jobs: layout-startup-rodata-5-gate, -plants, -lint, -counts on keke)
1. Jobs on this head: gate `layout-startup-rodata-4-gate` (direct tier), plants `layout-startup-rodata-4-plants`
   (handoff/LAYOUT-STARTUP-RODATA-plants.txt, 7 plants incl. new lsr-probe-keys-dropped, release profile). See the report.
   lsr-reloc-not-followed (run 3's) printed WARN-NO-MSG: the tool rebuilds the products with an xtask mutant and that
   build already fails on it (logs/lsr-reloc-not-followed.so.log), before the unit test runs: caught by the build, not by
   the named message. Its msg should also accept the build failure (not changed in this run).
2. The RW shift (64 KiB gap moves the RW segment; +1 `.data.rel.ro` fault in tool/locale paid back by grouped data). The
   `.data1` half (#4315) would give tool 55 / locale 52: then tighten the bounds.
3. Fast localtime 20 (= bound): if the fast trace's ro data is all in the group, the remaining faults are text/zone file.

## For LAYOUT-DATA-CLUSTERS / the layout rethink
- The placement plan is one list: `plan()` = starts (startup-text.keys + `HOT_TIERS` hot keys + probe-text.keys) +
  transitive relocation closure + ro keys. To take it over, replace `starts()` inputs with the rethink's per-probe hot
  object list (probe-text.keys is already per probe mode; add modes to `MODES` in probe_text.sh). Keep `section_names` /
  `whole_names` (readelf's 256-byte cut; xtask/src/coldbss.rs reads `.bss.*` names the same way, unchecked).
- The 64 KiB alignment gap shifts every later segment; any window contract on RW data must account for it.
- Group headroom: fast 60064 of 65536 bytes; a new probe mode's data may not fit (the archive-wide check fails `so`).

Not to redo: 16 KiB alignment (bound 33 KiB fast), "all tiers" (160 KiB), symbol-only matching, hunting the straggler,
measuring next's release localtime (19, above).
Plan rows: D-X.50, D-X.62 (records/decisions/D-X/), D-X.48/49 (hot text), contract STARTUP-DATA-WINDOWS.
