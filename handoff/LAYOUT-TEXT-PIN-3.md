# LAYOUT-TEXT-PIN-3 (brief A, continuation 3): INCOMPLETE, fast fixed; release regen blocked by STARTUP-TEXT-WINDOWS

**State (2026-11-10):** branch `task/layout-text-pin-3` (= task/layout-text-pin-2 fa3b23f66 + 2 commits); HEAD = the
commit carrying this note. Role mei:~/w/s2. Decision D-X.63 (unchanged). Plants file
`handoff/LAYOUT-TEXT-PIN-3-plants.txt` (3 new, on so.rs); run 2's: handoff/LAYOUT-TEXT-PIN-2.md (padding V0/V1, not to redo).

## Done this run
1. Step 0: clippy + static on the inherited head rc 0 (job layout-text-pin-3-lint0).
2. Why layout-text-pin-2-v1-fast failed (shiki, rc 101): only the two localtime bounds (21 > 20); same 21 on mei
   (job layout-text-pin-3-fmap-v1), so not host-specific. Fault map of `page_faults localtime` (faultmap.py):
   during the call, text pages 0x443000 (localtime_r, hot), 0x450000 (time cluster), 0x492000
   (liella_legacy's RawVecInner::finish_grow, tool tier at position 517), 0x4d4000 (rule::Cursor::number, NOT in
   the list) and 0x502000 (Zone::at_cached, NOT in the list).
   **Cause:** regen.sh traced p_cold's localtime_r/mktime in the host's zone (UTC on mei), so the trace never ran the
   POSIX rule-string parse, the period cache or that vector growth, which the America/New_York probes do.
3. Fix (traced, not hand-picked): regen.sh runs p_cold with `TZ=America/New_York`; fast regen
   (`tools/hottext/regen.sh . --build`) put Cursor::number (269), finish_grow/liella_legacy (277) and at_cached (283,
   inside the time cluster 276..288) on the zone path. The regen also re-sorted unrelated keys (the list had drifted
   from the code; 3 keys added or removed in total) and rewrote startup-text.keys / startup-data.keys.
4. #4375: so.rs::sections_of reads the ELF section headers (hottext::elf_headers), not readelf -SW; unit test
   `so::tests::alias_sections_keep_whole_names_at_their_index`.

## Counts (Skylake-SP host, mei; bound in parentheses; commit 62fcaac7e's message says "Zen 1 host" in error)
| products | localtime | lt+mktime | mktime | STARTUP-TEXT-WINDOWS | other |
|---|---|---|---|---|---|
| inherited head, fast | 21 (20) FAIL | 21 (20) FAIL | 0 | ok | - |
| this head, fast (job layout-text-pin-3-final: 18 of 18 hot_text_products pass) | **18** | **18** | 0 | ok (all 12 contracts) | tool 58 (58), locale 54 (55), true 16 (17), fork 6 (7), float 3 (5), strings/rebind 0 |
| inherited head, release (rel-base) | 21 FAIL | 21 FAIL | 0 | **5 > 4 BROKEN** | tool 58 (58), locale 55 (55), true 15 (17), fork 6 (7), float 3 |
| fast-regen list, release (regen1's build) | not run | | | **7 > 4 BROKEN** | |
Bounds untouched (rulings #4347/#4348: no re-derivation before A and B land).

## Padding per cluster (fast, this head, page_faults.c probe link; file 5,834,328 B as V1)
| cluster | address | bytes | gap ahead |
|---|---|---|---|
| strings | 0x418000 | 24,732 | 12,385 |
| float | 0x428000 | 15,690 | 4,111 |
| time | 0x450000 | 18,602 | 31,186 |
| rebind | 0x480000 | 20,398 | 17,727 |
| rebind-tail | 0x488000 | 18,060 | 12,370 |
Gaps 77,779 B + .text start moved 0x401000 -> 0x408000 (28,672 B) = **106,451 B address padding**. That is above
~32 KB: the size-vs-faults trade goes to the owner (RULING NEEDED in the report). Options not yet measured: (a) order
the clusters so their 32 KiB alignments fall back to back (time and rebind are both preceded by ~30 KB of plain
text: moving plain keys after the clusters would cut most of the 31 KB and 18 KB gaps); (b) align only the clusters
whose probes need it (mktime/strings/rebind: 0 faults; float has margin 3 of 5); (c) a single 64 KiB-aligned
zone+time cluster would save one more localtime fault but costs up to 64 KiB.

## Open (next step, in order)
1. **Release regen blocked.** `LIELLA_FULL_PROFILE=1 tools/hottext/regen.sh . --build` stops in its own product build
   at STARTUP-TEXT-WINDOWS (release 7 > budget 4 with the fast-regen list; the inherited list already gave 5 > 4,
   so the PIN-2 cluster branch never passed release). Find why the release start-up functions spread over 5-7
   windows (likely the 32 KiB cluster alignments inside the group behind release-only start-up keys; check the
   positions of startup-text.keys' release keys against the cluster heads), fix that, then run the release regen and
   measure release counts. Do not loosen budget-release (ALWAYS FORWARD).
2. Padding options above; direct gate (`cargo xtask affected --run --base origin/next`).
3. Rebase onto rodata-4 when it lands.

## Not to redo
Fault-mapping the fast 21 (cause above). The 64 KiB single rebind cluster. readelf-based section names.
Plan rows: D-X.63, D-X.49 (STARTUP-TEXT-WINDOWS), D-X.56 (regen).
