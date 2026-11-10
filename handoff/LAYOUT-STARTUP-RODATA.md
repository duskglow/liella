# LAYOUT-STARTUP-RODATA(-2,-3): hot read-only data in one 64 KiB-aligned .rodata1 (D-X.62, amends D-X.50) - NOT READY

**State (2026-11-10, run -3):** branch `task/layout-startup-rodata-3` (stacks on -2 and -1; base origin/next 2e33a3cb9,
next has moved since, not merged). Build role mei:~/w/s1. Scratch on mei: ~/work/LAYOUT-STARTUP-RODATA-3/ (exp.sh = fast
products + faults, rel.sh = release products + windows over 5 text paddings + faults, who.py, prod-fast/, prod-release/),
older: ~/work/LAYOUT-STARTUP-RODATA{,-2}/ (romap.py, pad*.o, prod-base-{fast,release} = next 2e33a3cb9 products).
Ruling #4313 (option 1) in force: group ALL read-only data the fault-bounded probes read; never raise a fault bound.

## What the branch does
- Run 1: `xtask/src/startup_ro.rs` renames start-up read-only sections to `.rodata1`; entry.S aligns `.rodata1` to 64 KiB;
  STARTUP-DATA-WINDOWS ro budgets 1/1; product test with 16 text paddings.
- Run 2: start functions = startup-text.keys + hot-text.list keys of tiers startup/stdio/strings/locale/tool (`HOT_TIERS`),
  by symbol and by section name; transitive selection (text -> .rodata*/.data.rel.ro* -> .rodata*).
- Run 3 (the open bug, FIXED): `readelf -SW` cuts section names at 256 bytes (symbol names are whole); `plan` chose the
  hot switch table `.rodata..Lswitch.table.<v0 name of an intern_locked closure>` (read by global_locale::replace<Glue>),
  but objcopy `--rename-section <cut name>` matched nothing and renamed it silently never. Now `section_names` reads
  the whole names from the archive's ELF members (own ar + ELF64 reader, GNU long names, >0xff00 sections) and
  `whole_names` replaces readelf's (error if not a prefix); relocation sections are matched by name AND file offset
  (two names can be cut alike). New `cargo xtask startup-ro-plan <libc.a> [<filter>]`: prints the plan without changing
  the archive; with a filter, each matching section's start status and relocation targets (renamed or not). On built
  products it must print "0 sections to rename". Three unit tests added (whole names, cut-alike rela, existing ones).
  D-X.62 body corrected to the run-2/3 selection (record, not view). Plants file rewritten for the current code (not run).

## Measured (Skylake-SP host mei, role s1)
| products | group bytes | ro windows, any text pad | tool | locale | startup | localtime / lt+mktime |
|---|---|---|---|---|---|---|
| next 2e33a3cb9 fast | - | 2-4 | 56 | 53 | 17 | 20 / 20 |
| run 3 fast (lsr3-exp1) | 60037 | 1 | **56** | **53** | **17** | 20 / 20 pass |
| run 3 release (lsr3-rel1) | 49385 | 1 (plain + 5 paddings) | 56 | 53 | 15 | **21 / 21 FAIL (bound 20)** |
Fast meets the ratchet (<= 56/53/17, bounds untouched). Release localtime fails; next's release localtime NOT measured.

## Open
1. Release localtime 21 > 20. The zone functions (localtime_r, mktime, implicit, ...) are all in hot-text tier `extra`,
   which the selection leaves out (all of `extra` = 160 KiB, too big). The localtime probe is fault-bounded, so ruling
   #4313 covers its data. Next: (a) measure next's release localtime (build next's release products, or run the test on
   ~/work/LAYOUT-STARTUP-RODATA/prod-base-release if the test can take a products dir) to see if this is a +1 of the
   branch; (b) fault-map it (tools/hottext/faultmap.py, as -2's fmap.sh) to name the page; (c) likely fix: add the
   localtime probe's functions (the `4zone` keys of `extra`) as starts, generically: a start set per fault-bounded
   probe rather than per tier, and check release + fast group bytes stay <= 65536.
2. The RW shift (64 KiB gap moves the RW segment: +1 `.data.rel.ro` fault in tool/locale, paid back by the grouped
   straggler today). Making the writable start-up pages independent of the ro size (the `.data1` half, #4315) would
   give tool 55 / locale 52: then tighten the bounds.
3. Then: plants (handoff/LAYOUT-STARTUP-RODATA-plants.txt, 4 unit + 2 product plants), direct-tier affected gate.

## For LAYOUT-STABILITY-RETHINK
- The placement plan is one list: `plan()` = starts (hot keys by tier) + transitive relocation closure + ro keys. To
  take it over, replace `starts()`/`HOT_TIERS` with the rethink's per-probe hot object list; keep `section_names` /
  `whole_names`: any objcopy-by-name pass fed from `readelf -S` names has the 256-byte cut (xtask/src/coldbss.rs
  reads `.bss.*` names from `readelf -SW` the same way: INFORMATIONAL, not checked whether any of its names are cut).
- The 64 KiB alignment gap shifts every later segment; any window contract on RW data must account for it.

Not to redo: 16 KiB alignment (bound 33 KiB fast), "all tiers" (160 KiB), symbol-only matching, hunting the straggler.
Plan rows: D-X.50, D-X.62 (records/decisions/D-X/), D-X.48/49 (hot text), contract STARTUP-DATA-WINDOWS.
