# LAYOUT-TEXT-PIN-11 (brief A, continuation 11): merged with next 07a4db6ac, within every bound, tool/locale below next

**State (2026-11-11):** branch `task/layout-text-pin-11` = task/layout-text-pin-10 04ad010cb + merge of origin/next
07a4db6ac (one-line conflict in the generated hot-text.list, resolved by the regen) + f631921e7 (both-profile regen)
+ e81201c73 (test fix) + this note. Role shiki:~/w/sh1 (Haswell-EP). Decisions in force: D-X.63, D-X.56, D-X.47,
D-X.49, D-4.53; rulings #4539, #4541, #4608. Earlier notes: handoff/LAYOUT-TEXT-PIN-10.md ... LAYOUT-TEXT-PIN.md.

## Done this run
1. Step 0: clippy + static on the inherited head 04ad010cb green (job layout-text-pin-11-lint0).
2. Merged origin/next 07a4db6ac (LAYOUT-DATA-CLUSTERS-4c landed). One conflict: one line of hot-text.list.
3. Regen of both profiles:
   - Run 1 (layout-text-pin-11-regen, rc 1, by design) refused to drop `_RNvNtC9liella_rt5start7program`
     (startup, fast). The symbol is still in libc.a. But in the linked page_faults probe, `objdump -d` shows **no call
     site**, on the merged head and on origin/next alike. The fast build inlines it into `runtime::initial_thread`
     and keeps an out-of-line copy that never runs. So the key is dead in next's list too: next only pins a dead copy.
   - Run 2 (layout-text-pin-11-regen2): fast with `--allow-drop`. The script exits 3 unless that one key is the whole
     drop list, and it was. Release without `--allow-drop` dropped nothing. The trace re-added `find_cached` /
     `attach_charmap <Files, Glue>` (out of line again after the merge). `locale::builtin` is back to `release`.
     `cargo xtask so` passed the dead-key check. startup-data.keys / probe-text.keys / startup-text.keys rewritten too.
4. `init_startup_path_lies_in_the_hot_text_range` then failed: the dropped dead copy of `program` lies in cold text.
   The test assumed an out-of-line copy runs. Fixed (e81201c73): an optional start-up function outside the hot
   range fails the test only if an instruction in the program names its address. Required ones are checked as before.
   Plants: handoff/LAYOUT-TEXT-PIN-11-plants.txt (filter inverted; `#[inline(never)]` program with no key).

## Measurements (fast products, minflt from hot_text_products, Haswell-EP host, same procedure)
| tree | start-up (bound 17) | locale (bound 55) | tool (bound 58) | fork (bound 7) |
|---|---|---|---|---|
| origin/next 07a4db6ac, RAM copy (job layout-text-pin-11-next-ref, 21/21 pass) | 12 | 52 | 55 | 6 |
| merged head after both regens (job layout-text-pin-11-regen2) | 13 | **51** | **54** | 6 |
-10's +1/+1 on tool/locale is gone: both are now 1 below next. Start-up is +1 against next, still 4 under its bound.
Fault map of start-up (perf -e page-faults -d, ~/work/LAYOUT-TEXT-PIN-11/{next,merged}.true.cls on shiki):
perf records 20 faults on next and 19 on the merged head. The only difference is in `.data`. Next touches the
`stderr` and `_IO_2_1_stderr_` pages (2). The merged head touches the `__libc_single_threaded` page (1). So the
merged head touches one fewer user page that perf attributes. Its +1 in minflt is not in a page perf attributes
(exec-time or kernel-side). **Cause not found**; follow-up below. No bound was changed.

## Open (next step)
1. Follow-up (optional): find the start-up minflt +1 (13 vs 12). perf's user-page map shows the merged head one
   page better, so look at exec-time faults: segment count/sizes, `readelf -lW` next/merged in
   ~/work/LAYOUT-TEXT-PIN-11/{next,merged}/segments.txt, and the .data/.bss page split.
2. IMPROVEMENT for the dead-key check: `xtask so` checks only that a key has a section. A key whose function is
   never called (inlined copy) passes that check but pins dead code in hot text. Checking call sites, as the test
   now does, would catch it at build time.

## Not to redo
The merge with next 07a4db6ac, the two-profile regen and its drop check, the next reference and both fault maps.
