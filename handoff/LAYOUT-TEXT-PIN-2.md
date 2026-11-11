# LAYOUT-TEXT-PIN-2 (brief A, continuation): INCOMPLETE, fast localtime 21 > bound 20

**State (2026-11-10):** branch `task/layout-text-pin-2` (= task/layout-text-pin + merge of
task/layout-startup-rodata-3, ruling #4363 (1)); HEAD = the commit carrying this note. Role shiki:~/w/sh2.
Decision D-X.63 (records/decisions/D-X/D-X.63.toml, unchanged this run). Plants file
`handoff/LAYOUT-TEXT-PIN-2-plants.txt` (3 inherited + 4 new). Run 1's note: handoff/LAYOUT-TEXT-PIN.md.

## Done this run
1. Step 0: clippy + static on the inherited head rc 0 (job layout-text-pin-2-lint0).
2. Merged origin/task/layout-startup-rodata-3 (b5aea1d6e; -4 is running, not pushed). One conflict: two tests
   appended at the end of crates/core/tests/hot_text_products.rs, both kept. STARTUP-DATA-WINDOWS now passes in `so`
   (the run-1 blocker): `.rodata1` 48,096 B at a 64 KiB boundary.
3. #4364: coldbss.rs reads section names from the archive in memory (`hottext::ar_members`, GNU long names) and each
   member's ELF headers (`hottext::elf_headers`, now with sh_type, so the NOBITS check stays); no readelf -SW.
   Every rename of the hot-text grouping, the cold .bss pass and the start-up state pass is checked against the
   archive read back (`hottext::check_renames`; missing target or leftover source fails `so`). Fast build:
   "hot-text grouping: 735 section renames, all in effect"; coldbss and state passes return their checked counts.
4. Ruling #4363 (2): rebind split at catalog_of into `rebind` (positions 424..443, 19,856 B) and `rebind-tail`
   (444..462, 17,600 B); both start with code the first bindtextdomain+gettext ran. The CLUSTER-WINDOW test now
   prints PADDING lines.

## Padding per variant (fast products, probe link of page_faults.c, shiki)
| variant | .text start | head gaps (time/strings/rebind[/tail]/float) | gap total | address padding | probe file |
|---|---|---|---|---|---|
| next (no clusters) | 0x401000 | - | 0 | 0 | not measured |
| V0: rebind one cluster (64 KiB) | 0x410000 (+60 KiB) | 28,562 / 12,193 / 51,775 / 4,111 | 96,641 | 158,081 | 5,899,864 |
| V1: rebind split (32 KiB max) | 0x408000 (+28 KiB) | 28,562 / 12,193 / 19,007 + 12,370 / 4,111 | 76,243 | 104,915 | 5,834,328 |
A 3-way split (16 KiB) cannot lower the maximum alignment: time (17,840 B) and strings (24,224 B) need 32 KiB.

## Counts (fast; bound in parentheses)
| | true | locale | tool | fork | mktime | localtime | lt+mktime | float | strings | rebind |
|---|---|---|---|---|---|---|---|---|---|---|
| next, run 1 (mei) | 16 | 52 | 55 | 7 | 0 | 20 | 20 | 2 | 0 | 0 |
| V0 (shiki) | 16 | 53 | 55 | 6 | 0 | **22** | **22** | 3 | 0 | 0 |
| V1 (shiki, HEAD) | 16 (17) | 53 (55) | 56 (58) | 6 (7) | 0 | **21 (20)** | **21 (20)** | 3 (5) | 0 | 0 |
Pad test (pre/cold 0x800..0xF000): mktime/strings/rebind 0 and every cluster in one window, both variants.
TEXT-PREFIX holds (only crt1's `_start` ahead of the group). Release profile: NOT measured.

## Open (next step, in order)
1. Fast localtime 21 > 20 (FAIL). Not known whether this branch or the rodata merge causes it: rodata-3 alone
   measured 20 on mei; never measured on shiki. Measure origin/task/layout-startup-rodata-3 alone on the same host
   first (and next), then fault-map the localtime probe (tools/hottext/faultmap.py) on V1 to name the page. A likely
   suspect is the 28,562 B gap ahead of the time cluster pushing zone-load text or data across a window.
   ALWAYS FORWARD: no bound change.
2. Then the release profile counts, the direct gate (`cargo xtask affected --run --base origin/next`), plants result.
3. Rebase onto rodata-4 when it lands (the merge here is of -3).

## Not to redo
The 64 KiB single rebind cluster (V0, more padding, same localtime failure); readelf-based section names.

## INFORMATIONAL
- xtask/src/startup_ro.rs (rodata chain) has its own ar + ELF readers (`section_names`, `elf_section_names`): could
  reuse `hottext::ar_members`/`elf_headers` once both land (left alone here to avoid a conflict with -4).
- so.rs::sections_of still uses readelf -SW.
