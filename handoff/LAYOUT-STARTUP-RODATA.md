# LAYOUT-STARTUP-RODATA(-2): hot read-only data in one 64 KiB-aligned .rodata1 (D-X.62, amends D-X.50) - NOT READY

**State (2026-11-10, run -2):** branch `task/layout-startup-rodata-2` (on `task/layout-startup-rodata`, from origin/next
2e33a3cb9; next has moved since, not merged). Build role mei:~/w/s1. Scratch scripts and saved products:
mei:~/work/LAYOUT-STARTUP-RODATA-2/ (run.sh, exp.sh, fmap.sh) and ~/work/LAYOUT-STARTUP-RODATA/ (romap.py, pad*.o).
Ruling #4313 (option 1) is in force: group ALL read-only data the fault-bounded probes read; never raise a fault bound.

## What the branch does now
- Run 1 (see D-X.62 body): `xtask/src/startup_ro.rs` renames start-up read-only sections to `.rodata1`; entry.S aligns
  `.rodata1` to 64 KiB; STARTUP-DATA-WINDOWS ro budgets 1/1; product test with 16 text paddings.
- Run 2: inherited head failed `cargo xtask clippy` (type_complexity in `plan`): fixed (`type Plan`).
- Run 2 (ruling #4313): the start functions are now startup-text.keys PLUS the hot-text.list keys of tiers startup,
  stdio, strings, locale, tool (`HOT_TIERS`; fork and extra stay out), matched by symbol and by section name
  (`.text.sorted.h<n>.<f>`); the selection is transitive: text -> `.rodata*` and `.data.rel.ro*` -> `.rodata*`.
  `hottext::parse_list`/`Entry` made `pub(crate)`. Unit test `a_function_section_names_its_function_grouped_or_not`.

## Measured (mei, fast profile; release NOT measured with the run-2 selection)
| products | group bytes (bound) | ro windows, any text pad | tool | locale | startup | localtime / lt+mktime |
|---|---|---|---|---|---|---|
| next 2e33a3cb9 | - | 2-4 | 56 | 53 | 17 | (pass) |
| run 1 (start-up only) | 33441 | 1 | 59 | 55 | 17 | - |
| run 2, tiers direct (lsr2-m1) | 53691 | 1 | 58 | 55 | 17 | 21 / 21 FAIL |
| run 2, + transitive (lsr2-exp2, lsr2-exp4) | 60037 | 1 | 57 | 54 | 17 | 20 / 20 pass |
| all tiers incl. extra (lsr2-exp2 "all") | 160593 | - | build fails (> 64 KiB) | | | |

Fault maps (tools/hottext/faultmap.py via fmap.sh; jobs lsr2-fmap1, lsr2-fmap3, lsr2-exp4) of tool/locale, next vs run 2:
- next: ro windows = head of .rodata (0x756000, read by `liella_machine_strcmp`), `unicode::tables::RECORDS`, and the
  .rodata tail window shared with STARTUP_NAMES (.rodata1 sat right after .rodata); `.data.rel.ro` 0 faults.
- run 2: head of .rodata (same), ONE straggler `anon.e930a032...100.llvm.*` (2.3 KiB, read by
  `liella_legacy::global_locale::replace<Glue>`, a locale-tier key at hot position h00180, also by zone::localtime_r),
  `.rodata1` (1 window): same 3 ro windows as next, PLUS one `.data.rel.ro` fault: the 64 KiB gap shifts the RW
  segment (+0xd000), so `.data.rel.ro`'s hot page (0x83e000) no longer shares the 64 KiB window of the `.got` fault.
- So the +1 (tool 57, locale 54) is: straggler not grouped (0) and RW shift (+1). Grouping the straggler should give
  tool 56 / locale 53 (= next, passes the ratchet); the RW shift needs the `.data1`/relro half (#4315).
- OPEN BUG: the straggler's section `.rodata.anon.e930...` IS addressed by R_X86_64_PC32 relocations (readelf -rW of the
  fast libc.a) from hot sections, yet `plan` does not choose it (section count stayed 226 with section-name matching
  added). Not found by reading: next step is a debug print in `plan` of whether `.text.sorted.h00180.*replace*` is a
  start and which relocations it yields (suspect the `readelf -SrsW` parse for this member, e.g. symbol or relocation
  lines it skips).

## Next step (in order)
1. Find why `plan` misses the straggler (above), group it; re-measure tool/locale/startup/localtime (fast) with
   `sh ~/work/LAYOUT-STARTUP-RODATA-2/exp.sh` (prints group bytes, windows, faults) + fmap.sh. Target <= 56/53/17.
2. RW shift: the hot `.data.rel.ro`/.got pages (#4315, the `.data1` half): make their window independent of the
   rodata size, or show the +1 is unavoidable with numbers (then RULING NEEDED).
3. Release profile: measure group bytes (must stay <= 65536 or the build fails) and faults.
4. Then: plants file (handoff/LAYOUT-STARTUP-RODATA-plants.txt is for run 1's code: the `chosen.extend(at)` plant line no
   longer matches; rewrite), affected gate, D-X.62 body (run-2 selection) via a new record or correction.

Not to redo: 16 KiB alignment (bound 33 KiB in fast), "all tiers" (160 KiB), symbol-only matching.
Plan rows: D-X.50, D-X.62 (records/decisions/D-X/), D-X.48/49 (hot text), contract STARTUP-DATA-WINDOWS.
