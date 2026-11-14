# LAYOUT-TEXT-PIN-15: the traced default removes the start-up +1 on its own; the generator fix is reverted

**State (2026-11-14):** branch `task/layout-text-pin-15` = task/layout-text-pin-14 556dfadb7 + ed640faf6 (merge of
origin/next 8f3e2993e) + f5043280a (revert of the startup_data.py operand scan) + this note and its report. **The
branch's code is identical to origin/next**; it differs only in handoff/ (-12..-15 notes, plants files, reports).
Role shiki:~/w/sh2 (Haswell-EP). Scripts in shiki:~/work/LAYOUT-TEXT-PIN-15/ (ab.sh, patchB.py), outputs in
ab-A/ and ab-B/ there. The chain is finished: nothing is left to do on it.

## Done
1. Inherited head 556dfadb7: clippy clean, static red as -14 said (job layout-text-pin-15-lint0, rc 1: hottext
   `check_pinned` refuses the 29 regen-added keys). The merge removes that check (LT-5), so it is fixed by step 2.
2. **Merge of origin/next** (ed640faf6), per the coordinator's 11-10 notes. Next now traces every product build
   (LAYOUT-TRACE-DEFAULT) and has deleted the four committed lists and the dead-key / absent / unpinned machinery
   (LT-5, LAYOUT-DELETE-LISTS). Resolution: the four lists stay deleted (no key pinned); hottext.rs and regen.sh take
   next's side: the #4778 unreferenced-key check and its per-profile filter (10e20c108, "the cross-profile key fix")
   lived inside the deleted dead-key check, and a traced list holds only functions its own build ran, so nothing is
   left for them to guard. thread.rs (the -12 robust-list detour, a committed-list placement fix) and
   hot_text_products.rs (FORK_BOUND 5) take next's side too. startup_data.py kept the -13 absolute-operand scan.
   Merged head: clippy + static clean (job layout-text-pin-15-lint1, rc 0).
3. **A/B of the generator fix under the traced default** (RAM copies of the merged tree on shiki; A = with the scan,
   B = the scan removed = next's code exactly; both profiles; per pre pad min of 3 runs after a warm-up, no ASLR):
   * B (job layout-text-pin-15-abB, rc 0): `__libc_single_threaded` is in the traced startup-data.keys in **both
     profiles without the fix**; hot_text_products 26/26 in fast and release.
   * A (job layout-text-pin-15-abA, rc 1): the scan adds 11 keys to the fast startup-data.keys (81 -> 92 lines):
     `_IO_2_1_stdout_`, `stdout`, ctype_tables::C_TOLOWER, global_locale::SLOTS, cpu::COPY_MIN, runtime::EARLY_DONE,
     liella_machine_{chr_limit,fill_erms_from,fold_limit,move_long,set_mid}: objects the start-up functions address
     on paths start-up never runs. The larger cluster breaks STARTUP-DATA-WINDOWS (`heap::RESERVE_MEM` and
     `global_locale::SLOTS` each alone on a page: 8 written pages > rw-budget-fast 7), and `cargo xtask so` fails.
   * Coordinator rule: the fix lands only if a test shows it changes the traced lists. It does change them, but for
     the worse, so f5043280a reverts startup_data.py to next's version (a change that measured worse is reverted).
4. Per-pad counts of next's code under the traced default (B; `pre` pads, the padded start file of hot_text_products):

   | profile | true | fork | tool | locale |
   |---|---|---|---|---|
   | fast | 11 at 0, 0x8000; 12 elsewhere | 5; 6 at 0x2000-0x6000 | 50-51 | 46-48 |
   | release | 10 at 0, 0x8000; 11 elsewhere | 5; 6 at 0x2000-0x5000, 0xe000 | 49-51 | 46-48 |

   The true count has the same shape -14 measured on next with the committed lists (11 at 0 and 0x8000, 12
   elsewhere): that is the text phase of the `pre` pads (they move the 64 KiB text windows), which the data-phase
   test excludes on purpose. It is not the data-page +1 the chain chased. With the code now equal to next there
   is no head-vs-next difference at any pad.

## Open / informational
* The fork child reads 6 at some `pre` pads (above FORK_BOUND 5, which bounds the unpadded probe; the tests do not
  measure fork at `pre` pads). This is start-up layout jitter under ruling #4347. Not chased; a candidate for LT-2/LT-3.
* `cargo xtask affected --run --base origin/next` selected 0 suites for a change to tools/hottext/startup_data.py
  (job layout-text-pin-15-gate on ed640faf6: "suites: 0"). A change to the trace generators reaches no direct test:
  the tier should map tools/hottext/*.py and *.sh to hot_text_products and the layout check.

## Not to redo
The merge, the A/B (scripts above), the per-pad table. The absolute-operand scan: under the traced default it only
adds objects start-up does not touch. If a start-up object is ever missing from the traced list again, take the
missing touch from the trace (perf), not a static operand scan.

## Next step
None on this chain: land the handoff record, or close the story. Plan rows: D-X.88, D-X.92 (trace-at-build), #4347.
