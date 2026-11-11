# LAYOUT-DATA-CLUSTERS-9: the locale / tool +1 named and removed; the plants file fixed

**State (2026-11-11):** branch `task/layout-data-clusters-9` = -8's `9f96cab25` + `ede0eef1e` (list entries,
bounds, plants file) + this handoff. Role mei:~/w/s2 (Skylake-SP). Scratch on mei: `~/work/LAYOUT-DATA-CLUSTERS-9/`
(`m2.sh <tag>`: `cargo xtask so`, the fault-count tests, then `map9.sh` maps of the locale and tool probes;
`map9.sh`/`resolve.py` = -8's `map.sh`/`resolve.py` with this run's paths; maps `m2-*`, `m3-*`, `head2-*`).

## Decisions in force
D-X.70 (the declared list, ruling #4694), D-X.64 (clusters), D-X.50. No new record: entries added under D-X.70.

## Cause of the locale / tool +1 (#4774)
-8 put `program_invocation_name` in the `.data1` cluster, so start-up no longer writes the last `.bss` page.
The first setlocale reads, then writes, several global_locale statics on that page: the first one there costs
a read fault (zero page) and a write fault (copy), against the single start-up fault next pays (net +1 in the
counted child). Each map (perf `-e page-faults -d`) shows only the first read and the first write on a page, so
declaring one static moved the faults to the next: NAME_COUNT (-8's map), then `process::SECURE`
(head2 = 4aec9013d), then GENERATION and MAPS, then QUERY_HANDLE and QUERY_SEQ. Declared as a set
(MAPS, COUNT, NAME_COUNT, FILE_COUNT, GENERATION, QUERY_SEQ, QUERY_HANDLE, QUERY_SLOT, process::SECURE).

## Results (mei, fast products, 5 runs each, every run identical)
| tree | true | locale | tool | fork |
|---|---|---|---|---|
| next 07a4db6ac (-8, keke = mei) | 12 | 52 | 55 | 6 |
| -8 head 38253a532 (`ldc9-measure-1` with NAME_COUNT + SECURE) | 11 | 53 | 56 | 5 |
| + counters (`ldc9-measure-2`) | 11 | 53 | 56 | 5 |
| + query statics = ede0eef1e (`ldc9-measure-3`) | 11 | **51** | **55** | 5 |
LOCALE_BOUND 55 -> 52, TOOL_BOUND 58 -> 56 (count plus 1). Release products NOT measured in this run (-8
read release = fast for locale and tool on every tree, both hosts).

## Plants
`handoff/LAYOUT-DATA-CLUSTERS-9-plants.txt` (replaces -8's file, whose line had the equivalence note in the
command field: baseline rc 2). ldc9-locale-set (the set dropped; LOCALE_BOUND 52 must catch it),
ldc8-time-compat (equivalent: release-only level, no fast-profile guard). Results: see the report json / `ldc9-plants`.

## Jobs (mei)
ldc9-lint0 (inherited head: clippy + static, rc 0); ldc9-lint (final head: workspace clippy -D warnings,
xtask clippy, static, rc 0); ldc9-gate (direct tier, --base origin/next); ldc9-plants. Diagnostics:
ldc9-map-head2-mei, ldc9-map-m1, ldc9-measure-1/-2/-3. keke ldc9-map-head2 cancelled while queued (slots full).

## Next step
1. Release profile: measure locale / tool on release products (expect 51 / 55) before the train's release pass.
2. From -7: fold the two `.data1` list tests into one list-driven test; regen.sh header note.
3. A guard that sees all first touches at once: extend `init_startup_data_accesses_are_in_the_data1_cluster`
   (isolated 64 KiB sections) to trace `relay locale` and `relay tool`, not only `true`: one pass names every
   static these modes touch, instead of one per map.
