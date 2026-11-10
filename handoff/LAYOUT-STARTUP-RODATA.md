# LAYOUT-STARTUP-RODATA(-2,-3,-4): hot read-only data in one 64 KiB-aligned .rodata1 (D-X.62, amends D-X.50)

**State (2026-11-10, run -4):** branch `task/layout-startup-rodata-4` (stacks on -3, -2, -1; base origin/next 2e33a3cb9,
next has moved 38 commits since, not merged). Build role shiki:~/w/sh1. Scratch on shiki: ~/work/LAYOUT-STARTUP-RODATA-4/
(build.sh = release products of branch + next tip, probe.py = the hot_text_products fault harness outside cargo,
fmap.sh/who.sh/who.py = fault map + link-map owner of each read-only fault, exp.sh = trace + both profiles + faults;
prod-next = next tip 9af6dff86 release, prod-fast2/prod-rel2 = this head). Older: mei ~/work/LAYOUT-STARTUP-RODATA{,-2,-3}/.
Ruling #4313 (option 1) in force: group ALL read-only data the fault-bounded probes read; never raise a fault bound.

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

## Open / next step
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
