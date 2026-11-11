# LAYOUT-DATA-CLUSTERS-4: static PIE kept to one writable segment (brief item 0); items 1-3 open

**State (2026-11-11):** branch `task/layout-data-clusters-4` (on -3's `4a7ceb9ec`; -2 and -3 underneath). Code
5bc08ae90 (fix + test), 0c7c62a74 and 22febab34 (D-X.68 record, plants file), then the relro-cap removal. Role mei:~/w/s1 (Skylake-SP).
Decisions: D-X.68 (records/decisions/D-X/D-X.68.toml; amends D-X.64/D-X.67 for `liella-rcrt1.o`). Earlier:
`handoff/LAYOUT-DATA-CLUSTERS-3.md` (its "Open / next step" list is still the plan for items 1-3).

## Item 0: -3's red (ldc3-gate-2, shiki): s2_7_c_1/2/3_static_pie exit 127
- Cause (job ldc4-spie, readelf -lW of a static PIE built from -3's products, fast): **two writable PT_LOADs**,
  `RW 0x460000` (.data.rel.ro .. .data) and `RW 0x498000 align 0x4000` (.data1, .bss, .lbss). `.data` ended at
  0x494f66 and the 16 KiB-aligned `.data1` started at 0x498000: a 3-page gap, so GNU ld began a new segment.
  `crates/ld/src/boot.rs` `image` accepts exactly one writable PT_LOAD, so `relocate_self` failed. So the trigger
  was -2's `.data1` alignment, at -3's text/data phase (it did not show on -2's gate).
- Fix (D-X.68): `csys/src/machine/entry.S` built with `LIELLA_STATIC_PIE` (liella-rcrt1.o only) sets
  `LIELLA_DATA1_ALIGN` to 4096: the gap stays under a page, and the written cluster still takes ceil(size / 4 KiB)
  pages. liella-crt1.o (the measured probes) is unchanged, so -2/-3's counts are untouched. A relro cap was tried
  too, but its plant (rcrt1 relro 65536) SURVIVED: the test program with `.tdata`/`.init_array` kept one writable
  segment, so that cap was removed (head 2 of this run).
- Test: `init_static_pie_keeps_one_writable_segment_at_every_data_phase` (hot_text_products.rs): static PIE with
  `.tdata`, a constructor and relro pointers through liella-rcrt1.o, `.data` padded 0/0x800/0x1000/0x2000/0x3000
  (new pad region `data`, helper `padded_start`); one writable PT_LOAD and exit 0. Plants
  (`handoff/LAYOUT-DATA-CLUSTERS-4-plants.txt`): data1 cap 4096 -> 16384, relro cap 4096 -> 65536.
- Before the gate: ldc4-spie-2 (rebuilt products): s2_7 static-PIE 3/3 and the new test pass.

## Results on the final head 22febab34 (mei)
See `handoff/reports/LAYOUT-DATA-CLUSTERS-4.json` (gate, plants, hot_text_products lines are filled in there).

## Not done (budget/context: the item-0 diagnosis came first, as the brief required)
1. #4602: regenerate `crates/libc-so/startup-data.keys` (fast `bash tools/hottext/startup_data.sh . --replace`, then
   release `LIELLA_FULL_PROFILE=1 cargo xtask so` + `startup_data.sh .`, or regen.sh --build per profile); add region
   `data` (helper `padded_start` already exists) to `init_data_clusters_hold_at_every_text_phase`; plant: drop the
   `_IO_2_1_stdout_` key. Note: `init_written_startup_objects_are_in_the_data1_cluster` reads the keys file, so it
   cannot catch a removed key; the catch must come from a count or a perf data-address check (-3's suggestion).
2. Release `strings` 1 vs 0 on next (#4631): not mapped.
3. Counts both profiles against next: not run in this run (-3's table in its handoff is the latest).

## INFORMATIONAL / RULING
- The one-writable-segment rule (`boot::image`) also refuses a static PIE whose OWN data is aligned above 4 KiB
  (a user `aligned(65536)` object), and ld.so (`crates/boundary/src/ld.rs` `run`) refuses any dynamic program with
  two writable PT_LOADs, before it checks dependencies. Present on next too. Recommend a relocator story that
  accepts several writable segments (a view per segment), with a ruling since it changes D-27.10's contract.
- A dynamic program linked with liella-crt1.o would carry the 16 KiB `.data1` / size-aligned relro and could hit
  that ld.so refusal; not measured here.

## Next step
Item 1 above on this branch, then item 2 (perf -e page-faults -d, release, head vs next on one host), then item 3.
