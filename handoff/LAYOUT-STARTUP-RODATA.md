# LAYOUT-STARTUP-RODATA: start-up read-only data in one aligned section (D-X.62, amends D-X.50) - DO NOT LAND

**State (2026-11-10):** branch `task/layout-startup-rodata` from origin/next 2e33a3cb9; code commit 8ed7c7eb7, WIP commit
after it (test fix + this note). Build role mei:~/w/s1. Record `records/decisions/D-X/D-X.62.toml` (hand-written in the
`records new` shape, `amends = ["D-X.50"]`; `cargo xtask static` NOT run on it).

## What the branch does
- `xtask/src/startup_ro.rs` (run by `build_archive` after the hot-text grouping): renames to `.rodata1` every read-only
  `.rodata*` section that the relocations of a start-up function (startup-text.keys, by symbol, incl. `__liella_libc_`
  names) address, plus the sections of the `ro` keys of startup-data.keys. It fails the build if the archive-wide
  upper bound exceeds `ALIGN`.
- `entry.S` (start object, first in every link): empty `.rodata1` with `.balign 65536`, so the output section starts on
  a window boundary. 16 KiB was tried first: the bound is 33441 bytes (fast: the single codegen unit's whole
  `.rodata.str1.1`, ~19 KiB, moves with the start-up literals).
- STARTUP-DATA-WINDOWS ro budgets 3/4 -> 1/1, row decision D-X.62. New product test
  `init_startup_rodata_stays_in_one_window_for_any_text_size` (16 text paddings). `init_startup_names_sit_in_one_window`
  bound 4096 -> 65536 (the group now shares .rodata1).

## Measured (mei, fast unless noted; scratch scripts in ~/work/LAYOUT-STARTUP-RODATA/)
- Read-only start-up windows over 0..64 KiB of extra text in 4 KiB steps: next fast 2-4, release 4-6 (lsr-base-measure);
  branch 1 in every link of both profiles (lsr-after-4). Alignment gap in the probe: fast 32680 B, release 29612 B.
- **Page faults (hot_text_products, 5 identical runs each), tool / setlocale+printf / startup-only:**
  next 56 / 53 / 17; branch 59 / 55 / 17 (lsr-ptest-5: tool test FAILS, bound 58); variant without merge sections
  (`!flags.contains('M')`) 57 / 54 / 17, and its contract reads 2 windows (lsr-exp-7). **Item 4 is violated: do not land.**
- Likely cause (not verified with a perf map): before, the start-up constants shared .rodata windows that other reads
  of these programs touch anyway; the group now sits in a window of its own (the 64 KiB gap keeps it from sharing
  `.rodata`'s last window), while those .rodata windows are still faulted in by non-start-up reads. The contract counts
  only start-up objects, so it improves while real faults rise.
- rw pages did not move across the shifts (fast 20, release 21); the `.data1` half was not done (see D-X.62 body).

## Next step
Decide the design (RULING NEEDED, below). Then: `perf record -e page-faults -d` of the tool and locale programs, base vs branch
(tools/hottext/faultmap.py), to confirm which .rodata windows remain. Not run: clippy, static, affected gate, plants
(plants file `handoff/LAYOUT-STARTUP-RODATA-plants.txt` is written for the current design), cbench_startup counts
(instruction counts cannot change with section placement; not measured).

## RULING NEEDED
- The contract's goal (ro window count independent of text size) and item 4 (no fault bound worse) conflict for this
  design. Recommendation: group ALL the read-only data the fault-bounded programs read (start-up + tool + locale traces,
  as hot text is grouped from several probes), not only start-up's, and place it at the head of `.rodata1` with the
  64 KiB alignment; or, cheaper, leave the layout and make the contract count windows relative to the windows the same
  link's other reads already touch. Until then TB-FOREIGN-SAFETY cannot land on top of this branch as ruled in #4297.
