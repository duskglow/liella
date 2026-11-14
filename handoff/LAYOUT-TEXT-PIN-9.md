# LAYOUT-TEXT-PIN-9 (brief A, continuation 9): the regen tool can no longer shrink the list silently

**State (2026-11-11):** branch `task/layout-text-pin-9` = task/layout-text-pin-8 2a5e301a4 + the tool fix
(ee000a3d2) + a merge of origin/next a43eb7c3c (964a64e53, clean, no conflict) + the two-profile regen (bf529fd2f)
+ this note and the report. Role shiki:~/w/sh2 (Haswell-EP, E5-2697 v3). Decisions in force: D-X.63, D-X.56 (regen),
D-X.47 (tiers), D-X.49 (start-up windows), D-4.53; rulings #4539 (accepted) and #4541. Earlier notes:
handoff/LAYOUT-TEXT-PIN-8.md ... LAYOUT-TEXT-PIN.md.

## Done this run
1. **Ruling #4539, regen.sh:** page_faults.c is linked as hot_text_products.rs links it before the probes, and its
   `locale` and `tool` modes are traced on EVERY run (LANG=en_US.UTF-8, stdin /dev/null, as the bound test runs
   them), before the extra programs. A plain `regen.sh . --build` can no longer lose the tool-path keys.
2. **Ruling #4539, no silent drop:** merge.py `--dropped <file>` names every old key it drops (also on stderr);
   regen.sh merges startup-text.keys and the hot list into scratch first and, if either drops a key, prints each
   one (`regen:   DROPPED <key> <tier> <profiles>`) and writes NOTHING unless `--allow-drop` is given.
   probe_text.sh and startup_data.sh still run after (startup_data.sh reads the new startup-text.keys).
3. **#4541:** the section sed is gone; `keys.py --sections <readelf -SW>` derives the function name with
   xtask's `function_of` rules (cold rename `.text.liella_cold.<f>.liella_cold`, `unlikely.`, `startup.`, `exit.`,
   `sorted.h<n>.`, `hot.`); `--aliases` uses the same function. A function once renamed cold re-enters by trace.
4. Tests (xtask `hottext::tests`, no spec clause: build tooling): `a_cold_renamed_function_reenters_the_list_by_trace`
   (keys.py --sections == function_of on every section shape, cold rename stripped),
   `merge_names_every_key_it_drops`, `regen_always_traces_the_bounded_tool_and_locale_modes`.
5. Merged origin/next (the brief asks for the bounds "on the merged head"; the branch was 69 commits behind).
   After the merge the generated lists equal next's except startup-text.keys (2 lines). -8's "next keeps
   builtin_charset" no longer holds: neither next a43eb7c3c's list nor its libc.a sections have it now.
6. Regen both profiles with the fixed tool on the merged head (job layout-text-pin-9-regen-both): **no key dropped**
   in either profile; only 5 list lines changed (getenv, `_IO_fflush` + its flush helper now traced, tiers tool /
   locale, instead of untraced `extra -`; getrusage start-up keys gain profiles); startup-data.keys: heap::DEBUG
   release only.

## Measurements (fast products, the bound tests, shiki, same host, same procedure)
| tree | start-up only (17) | setlocale + printf (55) | coreutils-shaped (58) |
|---|---|---|---|
| origin/next a43eb7c3c (RAM copy, job layout-text-pin-9-next-ref) | 16 | 53 | 56 |
| branch before the merge, after a fast regen (job layout-text-pin-9-regen-fast) | 15 | 57 | 61 |
| merged head after both regens (job layout-text-pin-9-regen-both) | 15 | 55 | 58 |
The merged head meets the bounds (at the limit), but it reads +2 / +2 over next on the same host (start-up -1). The
regen changed only the order of 5 keys, so the +2 comes from this branch's layout code (D-X.63 cluster alignment,
cold renames), not from the list. Not diagnosed this run (budget).

## Open (next step, in order)
1. If the coordinator wants the branch at or below next's level (ALWAYS FORWARD reading): fault-map the merged head
   against next on one host (-8's fmap.sh / classify.py, ~/work/LAYOUT-TEXT-PIN-8 on keke) to find which windows the
   two extra tool/locale faults are in (cluster padding pushing a tool-tier key into another 64 KiB window is the
   likely suspect).
2. The drop guard's refusal path is checked by a source-text test only (running regen.sh needs gdb and products).

## Not to redo
The tool fix; the merge with next a43eb7c3c; both regens (no drops); the same-host next reference above.
