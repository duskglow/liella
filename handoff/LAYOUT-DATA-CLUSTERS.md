# LAYOUT-DATA-CLUSTERS (brief B of LAYOUT-STABILITY-RETHINK): data clusters — DO NOT LAND as is

**State (2026-11-11):** branch `task/layout-data-clusters`, base task/layout-text-pin-9 81c825fdf (it holds
LAYOUT-STARTUP-RODATA-2/-3; neither those nor TEXT-PIN-9 nor RODATA-7 are in origin/next, so nothing merged).
Code 7c7c12411 + fixes bce76b0f5, the EPOLL split-name fix and 53f39d39d (test scope); role mei:~/w/s2.
Decision **D-X.64** (`records/decisions/D-X/D-X.64.toml`, records only). Plants: `handoff/LAYOUT-DATA-CLUSTERS-plants.txt`.

## What the code does
1. `.data1` cluster (`xtask/src/coldbss.rs`): + `timer::EPOLL` (fixed list, coordinator note #4493) and every `rw` key
   of `startup-data.keys` whose `.data.<s>`/`.bss.<s>` section is <= 4 KiB. ThinLTO split names (`<s>.0.llvm.<n>`) matched.
2. Relro head (`xtask/src/startup_ro.rs`): `.data.rel.ro*` sections the start-up functions reach, or holding an `rw`
   key, renamed `.data.rel.ro.local.<rest>` (sorted first by GNU ld's default script).
3. Alignment: entry.S has empty `.rodata1`, `.data.rel.ro.local.liella_cluster`, `.data1` aligned to
   `LIELLA_{RODATA1,RELRO,DATA1}_ALIGN`; `so` passes pow2 >= archive bytes, 4 KiB..64 KiB (`cluster_aligns`).
   Measured fast: RODATA1 65536 (cluster 48064 B linked, 59968 B archive bound: >32 KiB, so no change), RELRO 8192, DATA1 4096.
4. Tests: `init_written_startup_objects_are_in_the_data1_cluster`, `init_data_clusters_hold_at_every_text_phase`
   (pre/cold pads 0x800/0x2000/0x8000/0xF000; equal true/tool/fork at the cold pads; FORK_BOUND 7), EPOLL added to
   `init_startup_state_statics_are_in_data1`; xtask unit tests in coldbss and startup_ro (9 pass, job ldc-check-2).

## Measured (fast, mei, tools: scratch ldc_measure.py; jobs ldc-base-measure, ldc-after-fast-2)
| fast, orig link | true | locale | tool | fork | localtime | lt+mktime | float | tool T/R/Dr/D/B/O | tool rwPages |
|---|---|---|---|---|---|---|---|---|---|
| base 81c825fdf | 15 | 55 | 58 | 6 | 20 | 20 | 3 | 10/2/1/4/7/33 | 9 |
| this branch | 13 | 56 | 59 | 7 | 21 | 21 | 4 | 10/2/**2**/4/7/33 | 9 |
Cold pads: identical counts at every pad in both trees (base already phase-stable there). Pre pads: T +1 at
0x800/0x2000 in both (base text phase, not data). S_w: `.data1` = 3992 B after (672 before): 1 page at every pad.
Release after: NOT MEASURED (job killed for the final head's sync). Expected total 44-48: NOT reached (59).

**Why worse:** Dr 1 -> 2. Before, the start-up relro reads shared `.got`'s 64 KiB window (0x850000); at the head of
`.data.rel.ro` (0x84e000) they take a window of their own. Written pages did not drop: the remaining written start-up
pages are `.lbss` (heap::HEAP 3 pages, EXIT_HANDLERS, global_locale::SLOTS: large, D-X.22), assembly `.data`
(liella_machine_cmp_sse_null, set_mid), global_locale::C and intl::SETTINGS (.data, >4 KiB or unkeyed), environ.

## Open / next step
1. Drop or move the relro part (item 2): put the cluster at the TAIL of `.data.rel.ro` next to `.got`, or leave relro
   alone; then re-measure. The `ldc-no-relro` plant's test log prints the counts without it.
2. Written pages: the clustering needs the `.lbss` writers (HEAP head, EXIT_HANDLERS) and the asm `.data` words; that
   is beyond "rename keyed statics". `__environ` sits in `.bss.environ` (C section name): not picked by key.
3. Bounds untouched (RATCHET); per-class re-derivation is the owner's (#4348). localtime 21 > 20 here: gate expected red.

## Jobs (mei)
ldc-base-measure (rc 0), ldc-lint (clippy red, fixed), ldc-lint-2/ldc-check-2 (clippy+static+unit rc 0),
ldc-after-fast-2 (rc 0), ldc-gate-2/ldc-plants-2 (stale head: plants baseline red on `__environ`; stopped),
**ldc-gate-3, ldc-plants-3 (final head 53f39d39d, running at hand-back)**: `ssh mei '~/bin/jobwait ldc-gate-3 ldc-plants-3'`.
