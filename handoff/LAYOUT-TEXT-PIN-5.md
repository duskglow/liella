# LAYOUT-TEXT-PIN-5 (brief A, continuation 5): start-up keys at the list head; release regen done; all bounds hold with @place end

**State (2026-11-10):** branch `task/layout-text-pin-5` (= task/layout-text-pin-4 cea5abcb1 + this run's commits);
HEAD = the commit carrying this note. Role shiki:~/w/sh2 (Haswell host). Decision D-X.63 (no new record). Earlier notes:
handoff/LAYOUT-TEXT-PIN-4.md, -3, -2, LAYOUT-TEXT-PIN.md (not to redo).

## Done this run
1. Step 0: clippy + static on the inherited head rc 0 (job layout-text-pin-5-lint0).
2. Merged task/layout-startup-rodata-4 7b0d311b7 (brief PIN-4 #4396; it had handed back). Clean merge.
3. **Ruling #4417, list order (this is LAYOUT-FAULT-CONTRACTS' item, done here; C need not redo it):**
   `tools/hottext/merge.py --startup-head <startup-text.keys>` gives every key of that file the merged list holds the
   tier `startup` and moves the start-up tier to the head (stable order within both parts; keys the list lacks are not
   added). regen.sh passes it. `crates/libc-so/hot-text.clusters`: strings now ends at `__printf_chk` (its old last key,
   vfprintf, and the printf body after it are start-up keys and moved to the head). Tests:
   `hottext::tests::startup_text_keys_lead_the_hot_text_list` (the committed list) and
   `merge_moves_startup_text_keys_to_the_head` (merge.py on a fixture).
4. Regen: `regen.sh . --build` (fast) then `LIELLA_FULL_PROFILE=1 regen.sh . --build` (release), both rc 0 (job
   layout-text-pin-5-regen-measure2). The release regen alone could not start (job layout-text-pin-5-regen-measure,
   rc 1): its first build stops at STARTUP-TEXT-WINDOWS 6 > 4 on the old list and writes no `.profile`, so regen's
   non-`--build` mode cannot run either (#4397; not worked around). The fast regen's reorder covers both profiles' keys,
   after which every release build passes. hot-text.list: 594 keys, start-up tier = positions 1-100, no start-up key
   after it. hot-text.absent: unchanged, and the plain release `so` passes the dead-key check (#4419's stale line gone).
5. layout/contracts.tsv STARTUP-TEXT-WINDOWS budget-fast 5 -> 4 (#4418); release stays 4.

## Counts per variant (Haswell host, shiki, hot_text_products 5 runs each, every run identical; bound in parentheses)
| variant | profile | windows (4/4) | localtime (20) | lt+mktime | mktime/strings/rebind | tool (58) | locale (55) | true (17) | fork (7) | float (5) |
|---|---|---|---|---|---|---|---|---|---|---|
| **end** (committed) | fast | **3** | 20 | 20 | 0/0/0 | 58 | 55 | 15 | 6 | 4 |
| **end** (committed) | release | **3** | 18 | 18 | 0/0/0 | 58 | 55 | 14 | 6 | 4 |
| in-place | fast | 3 | **21 FAIL** | - | 0/0/0 | 57 | 54 | 15 | 6 | 4 |
| in-place | release | not captured | 18 | - | - | 56 | 53 | 14 | 6 | - |
PIN-4 (Skylake-SP host, mei, before the merge and the regen) read end fast localtime 19, locale 54, float 3: this run is
another host with RODATA-4 merged and both lists regenerated, so those deltas are not attributable to one cause.
Windows: 0x400000 and 0x410000 hold positions 1-100; the third is the strings cluster's head (0x480000), where a
start-up key absent from the list (no list position; an alias of a strings-cluster function, most likely strlen) sits.

## Padding per cluster (bytes of gap ahead of each cluster; .text start not captured this run)
| variant/profile | strings | time | rebind | rebind-tail | float | gaps |
|---|---|---|---|---|---|---|
| end fast | 29,495 | 11,366 | 15,318 | 12,370 | 14,708 | **83,257** |
| end release | 6,194 | 11,750 | 15,126 | 13,587 | 11,374 | **58,031** |
| in-place fast | 26,785 | 12,242 | 19,391 | 12,370 | 5,391 | 76,179 |
| in-place release | 29,092 | 20,818 | 23,167 | 13,587 | 21,967 | 108,631 |
The gap ahead of strings (fast) depends on where the plain text ends modulo 32 KiB: it moved 4,340 -> 29,495 with the
new list order. Size vs faults stays the owner's decision (#4418) after LAYOUT-DATA-CLUSTERS.

## Open (next step, in order)
1. Gate + plants on this head (jobs layout-text-pin-5-gate, layout-text-pin-5-plants; results in the report).
2. For C (LAYOUT-FAULT-CONTRACTS): regen --build cannot bootstrap when the current list breaks a contract (#4397).
3. Start-up windows 3 -> 2: map the start-up key at the strings cluster head (alias) to its list key, so that function
   joins the head (the strings mode then finds it in an already faulted window). Recommend after A and B land.
4. Bounds: windows read 3 in both profiles; tightening to 3 and the fault bounds waits for #4348 (after A and B land).

## Not to redo
Fault-mapping fast localtime 21 (PIN-3); the 64 KiB rebind cluster; readelf names; the release window map (PIN-4);
`@place in-place` with this list (fast localtime 21). Scripts: run2.sh / build_probe.sh / winmap.py under
~/work/LAYOUT-TEXT-PIN-5 on shiki. Plan rows: D-X.63, D-X.49 (STARTUP-TEXT-WINDOWS), D-X.56 (regen), D-X.47 (tiers).
