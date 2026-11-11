# LAYOUT-TEXT-PIN-4 (brief A, continuation 4): INCOMPLETE, clusters placed together at the end; release still broken

**State (2026-11-10):** branch `task/layout-text-pin-4` (= task/layout-text-pin-3 c5e1810c2 + this run's commits);
HEAD = the commit carrying this note. Role mei:~/w/s2. Decision D-X.63 (unchanged; this run adds no record). Earlier
notes: handoff/LAYOUT-TEXT-PIN-3.md, handoff/LAYOUT-TEXT-PIN-2.md (not to redo).

## Done this run
1. Step 0: clippy + static on the inherited head rc 0 (job layout-text-pin-4-lint0).
2. Release reproduced (job layout-text-pin-4-rel1, `LIELLA_FULL_PROFILE=1 cargo xtask so --hottext-regen`, diagnostic
   only): STARTUP-TEXT-WINDOWS **7 > 4**. A plain release `cargo xtask so` stops earlier (job layout-text-pin-4-rel0):
   `hot-text.absent: 1 stale release line` (`..ctype9CtypeFile4wide`), left by PIN-3's fast-only regen; the release
   regen rewrites it (#4397: not hand-edited here).
   **Cause of the 7 windows (window map, `page_faults true` start-up keys by list position):** the start-up keys are
   not contiguous in hot-text.list. They sit at positions 1-91, inside the strings cluster (111, 123, 124), 125-132,
   166 (`__fprintf_chk`), 255 (release only: `RawVecInner::finish_grow`, it straddled 0x450000) and 585-586
   (`getrusage`). Every 32 KiB cluster alignment between them (strings at 99, float at 145, which is 21.6 KB and so
   32 KiB in release) pushes the next start-up key into a new window, and the .text start moves 28 KiB (0x408000).
3. New: `@place<TAB>end` in crates/libc-so/hot-text.clusters (xtask/src/hottext.rs `clusters_at_end`,
   `cluster_section`): every cluster, in file order, is renamed `.text.sorted.h9<cluster 2 digits><position 5>.<f>`,
   so it sorts after the last list position and keeps the `h<digits>.` shape every reader expects (position_of,
   function_of, layout.rs is_hot_section, hot_text_products.rs cluster_ranges updated). `@place<TAB>in-place` (or no
   line) gives the old layout. Unit test in `clusters_are_aligned_to_their_size`.

## Counts per variant (Skylake-SP host, mei; bound in parentheses; bounds untouched)
| variant | profile | STARTUP-TEXT-WINDOWS | localtime | lt+mktime | mktime/strings/rebind | tool | locale | true | fork | float |
|---|---|---|---|---|---|---|---|---|---|---|
| in-place (PIN-3 head) | fast | ok, <= 5 (5) | 18 (20) | 18 | 0/0/0 | 58 (58) | 54 (55) | 16 (17) | 6 (7) | 3 (5) |
| in-place (PIN-3 head) | release | **7 (4) BROKEN** | not run | | | not run | | | | |
| **end** (this head, job layout-text-pin-4-measure-end) | fast | **4** (5) | 19 (20) | 19 | 0/0/0 | 58 | 54 | 16 | 6 | 3 |
| **end** | release | **6 (4) BROKEN** | 20 (20) | 20 | 0/0/0 | **59 (58) FAIL** | **56 (55) FAIL** | 16 | 6 | 4 |
(Release in-place per-class counts: PIN-3's inherited release head, an older list: tool 58, locale 55, true 15, fork 6,
float 3, localtime 21. The release list is stale in both variants until the release regen runs, so release per-class
deltas are not attributable to the placement alone.)

## Padding per cluster (address bytes of gap ahead of each cluster; .text start 0x401000 -> 0x408000 = 28,672 in all)
| variant/profile | strings | time | rebind | rebind-tail | float | gaps | + .text start |
|---|---|---|---|---|---|---|---|
| in-place fast (PIN-3) | 12,385 | 31,186 | 17,727 | 12,370 | 4,111 | 77,779 | **106,451** |
| end fast | 4,340 | 8,036 | 14,166 | 12,370 | 14,708 | 53,620 | **82,292** |
| end release | 13,684 | 7,981 | 13,974 | 13,587 | 11,374 | 60,600 | **89,272** |
Probe file size is unchanged by placement (fast 5,834,328 B; release 4,495,808 B): the cost is address space plus
the padding inside the text segment. Still above ~32 KB: the size-vs-faults trade stays with the owner. What remains
is structural: five clusters of 15-25 KB in 32 KiB slots cost about 55 KB however they are ordered (no two fit one
32 KiB slot; 64 KiB slots would move .text's start by 60 KiB, ruling #4363), plus the 28 KiB .text start.

## Open (next step, in order)
1. **RULING NEEDED / design:** the start-up keys must be contiguous for STARTUP-TEXT-WINDOWS release 4 to hold with
   any cluster alignment. Recommend: regen (tools/hottext/regen.sh) puts every startup-text.keys key in the start-up
   tier at the head of hot-text.list (they are traced already), and the strings cluster's start-up members
   (positions 111, 123, 124) either join that tier or the strings cluster goes first in the end block next to the
   list's tail. That is a list-order change (regen / LAYOUT-FAULT-CONTRACTS territory, #4397), not done here.
2. Release regen (`LIELLA_FULL_PROFILE=1 tools/hottext/regen.sh . --build`) once 1 lets it pass its own contracts;
   then release counts both variants. Fast localtime 19 with `end` vs 18 in-place: re-check after the regen; if it
   stays, compare with `@place in-place` (one line in hot-text.clusters).
3. Merge LAYOUT-STARTUP-RODATA-4's head when it hands back (#4396). Direct gate and plants on the final code
   (plants for clusters_at_end / cluster_section: drop `@place end` handling -> unit test; drop the h9 decode in
   position_of -> CLUSTER-WINDOW test).

## Not to redo
Fault-mapping fast localtime 21 (PIN-3). The 64 KiB single rebind cluster. readelf-based section names. The window
map of release start-up (above). Scripts: build_probe.sh / winmap.py / measure.sh under ~/work/LAYOUT-TEXT-PIN-4 on mei.
Plan rows: D-X.63, D-X.49 (STARTUP-TEXT-WINDOWS), D-X.56 (regen).
