# LAYOUT-TEXT-PIN-6 (brief A, continuation 6): start-up labels resolved to their sections; start-up text 2 windows

**State (2026-11-10):** branch `task/layout-text-pin-6` (= task/layout-text-pin-5 d9a7cb6ee + merge of origin/next
8253a107c + this run's commits); HEAD = the commit carrying this note. Role shiki:~/w/sh1 (Haswell host). Decision
D-X.63 (amended in place, part 3; the record is new on this branch chain). Earlier notes: handoff/LAYOUT-TEXT-PIN-5.md,
-4, -3, -2, LAYOUT-TEXT-PIN.md (not to redo). Plants: handoff/LAYOUT-TEXT-PIN-6-plants.txt (4/4 CAUGHT).

## Done this run
1. Step 0 (job layout-text-pin-6-lint0, rc 1): clippy ok; static red only on `codemap` (docs/codemap/symbols.tsv out
   of date). Fixed by the brief's merge of origin/next (clean; symbols.tsv is now next's, byte for byte).
2. Last run's red gate (layout-text-pin-5-gate): `conformance: FAIL s04_classed_memset` (status 1). Its stderr log was
   gone (sh2 reused). On this run's tree it passed alone (job layout-text-pin-6-classed1), and job
   layout-text-pin-6-release loops it 10 times on the final fast products after the gate (see "Open" 1).
3. The merge left a dead hot-text key (`locale::builtin`, renamed on next): `cargo xtask so` refused. Fixed by the
   regen in both profiles (below), not by hand.
4. **#4467 (alias start-up keys):** the start-up trace names labels inside function sections
   (`liella_machine_move_avx2`, `liella_machine_move_mid`, `liella_internal_memmove`, `liella_machine_fill_avx2_entry`,
   aliases such as `bcmp`, `__memcmpeq`), which have no list key, so `merge.py --startup-head` skipped them and their
   sections stayed in later tiers. `tools/hottext/keys.py --aliases <objdump -t of libc.a>` maps every symbol inside a
   function section to that section's key (prefix `.text.sorted.h<n>.`/`hot.`/`unlikely.` stripped; a name in two
   sections of different keys is left out); regen.sh writes the map and passes `merge.py --alias`, which adds the
   mapped section keys to the start-up head. Test: `hottext::tests::a_startup_label_brings_its_section_to_the_head`.
5. Regen: `regen.sh . --build` (fast) rc 0, then `LIELLA_FULL_PROFILE=1 regen.sh . --build` (job
   layout-text-pin-6-regen2): the list was rewritten, then its closing `cargo xtask so` refused 5 stale release lines
   of hot-text.absent (functions the release archive has again after the merge; same in the pre-alias baseline run).
   Removed them (hand-kept file, as `so` asks); lists copied back from the role and committed.
   Result: `liella_machine_memmove` is now list position 89 (start-up tier); `liella_machine_memset` stays at 116
   (strings tier). Why the alias map did not move it is NOT established (not checked: whether
   `liella_machine_fill_avx2_entry` lies in memset's section or another one); the window count, with every start-up
   function found, is 2 either way. See Open 3.

## Counts (Haswell host, layout check of `cargo xtask so`, STARTUP-TEXT-WINDOWS, bound 4/4)
| list | fast | release |
|---|---|---|
| after merge, before #4467 (job layout-text-pin-6-regen, killed after both regens) | 3 | 3 |
| with #4467 (job layout-text-pin-6-regen2) | **2** | **2** |
107 of 107 (fast) / 101 of 101 (release) start-up functions found; all 12 layout contracts held in the release
regen build. Fault bounds (hot_text_products): fast in the gate; release in job layout-text-pin-6-release.

## Open (next step, in order)
1. Read jobs layout-text-pin-6-gate (direct tier, `--base origin/next`; escalated to FULL) and
   layout-text-pin-6-release (release hot_text_products + s04_classed_memset x10) on shiki. If s04 fails again, its
   stderr is conformance/build/s04_classed_memset.log.stderr in ~/w/sh1: read it before the role is reused.
2. Bounds: windows read 2 in both profiles; tightening STARTUP-TEXT-WINDOWS (4/4 -> 2/2) waits for the #4348
   re-derivation (ruling #4464), not done here.
3. Check `objdump -t target/liella/libc.a | grep fill_avx2_entry` and the regen's alias.keys: if the label is in
   memset's section, find why merge.py did not move it (one call); optional, the windows are already 2.
4. For C (LAYOUT-FAULT-CONTRACTS): regen --build cannot bootstrap when the current list breaks a contract (#4397);
   regen's closing `so` also stops on stale hot-text.absent lines after a merge (this run).

## Not to redo
The regens of this run; fault-mapping fast localtime 21 (PIN-3); `@place in-place`; the release window map (PIN-4).
Scripts: run1.sh/run2.sh/run3.sh, build_probe.sh, winmap.py under ~/work/LAYOUT-TEXT-PIN-6 on shiki. Plan rows:
D-X.63, D-X.49, D-X.56, D-X.47.
