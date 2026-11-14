# WCTYPE-TRIE-FILE(-2..-7): lazily resolved class tables for compiled LC_CTYPE + the thread-block table word (D-5.10, D-5.11)

**State:** branch `task/wctype-trie-file-7` (continues `-6`..`-2`; origin/next merged in runs 3, 5 and 7). HEAD: see
`handoff/reports/WCTYPE-TRIE-FILE-7.json`. Decisions in force: **D-5.10**, **D-5.11** (records/decisions/D-5/; body
corrected in run 7 to the owned() gate), D-5.9, D-5.5, D-7.13, D-X.58 (next's ownership word). The holds are gone:
LAYOUT-STABILITY-RETHINK landed 11-10, TB-FOREIGN-SAFETY(-13) landed 11-14.

## What the story does (runs 1-2)
`ClassTable` (crates/core/src/locale/classes.rs): stage 1 of 4352 32-bit cells, built-in leaves (D-5.9) or pool leaves,
resolved per 256-character block on first touch; `ThreadBlock::wclass` (0x180) stored with `view` by every writer;
`isw*`/`iswctype` read it with one `%fs` load.

## D-5.11 safety, as of run 7 (ruling #4281: gate via owned(), never a 0x9c flag)
* `isw` reads `class_word()` = `wclass & thread::owned_mask()` (the `and` replaces the zero check's `test`); 0 ->
  `isw_view` (view path). `view()` / `with_view` / `wtrans` / `isw_int` are next's (`view_word()`, D-X.58).
* `store_view` returns unless `thread::owned()`; stores `wclass`, then `view`. `zone::with_period` gates on `owned()`.
* FLAG_VIEW, `views_stored`, `block_flags*`, `own_block` and the branch's `with_view`/`with_resolved_view` are gone.
* `tow*`/`towctrans` use next's frameless forms (ruling #4828); `isw*`/`iswctype` keep the class-table path.
* ThreadBlock: `wclass` 0x180, `tail` 0x188-0x280 (31 words), next's `dlerror` 0x280.
* Test `locale::tests::foreign_block_words_are_never_read_or_written` now skips when `owned()` (it runs in the test
  process, which the runtime did not start). Plants: handoff/wctype-trie-file-plants.txt (safety-isw-mask,
  safety-store-gate, safety-period-gate/-read/-write replace the FLAG_VIEW plants).

## Run 7 (this run)
* Step 0 on the inherited head (5124686): clippy + static green (keke `wctype-trie-file-7-lint0`).
* wtf6-gate (shiki, rc 1, 11-10): 3 failures, all start-up page-fault bounds: `heap_meta_products
  the_first_allocation_touches_few_pages`, `hot_text_products init_coreutils_shaped_...` and `init_short_programs_...`.
  The detailed log (role sh1 target/affected.log) had been overwritten by a later job in that role, so the numbers
  are lost; the handoff of run 6 had `localtime_r` 21 > 20. These are the layout-jitter bounds the owner's ruling
  #4347 covers; the layout now comes from a trace of each build (next), so the run-6 regen work is moot.
* Merged origin/next (1634 commits). Hot-text lists deleted as on next; xtask hottext.rs/layout.rs/coldbss.rs taken
  from next (no regen build there; next handles the `.0.llvm.<n>` suffix). Codemap views taken from next (generated).
* `crates/boundary/tests/s23_time_symbols.rs`: `tb::Flags` is gone on next; the test reads `tb::current().flags`.
* Gate / plants: see the json and the hand-back (keke `wctype-trie-file-7-gate`, `wctype-trie-file-7-plants`).

## Still open
* tow* counts after the merge (count.sh) and the isw* counts against the owned-mask gate.
* `_l` rows (ruling #4173: astral `_l` 20/21 vs 12; profile `view_of(loc)` first); period-cache own-process test; timing.

## Counts so far (run 4, kanon `wtf4-count`, c66487a; before the merge)
vs the -2 head: 432 faster, 450 equal, 54 slower (towupper/towlower/towctrans +2; now next's wtrans). Reference TSVs:
keke ~/work/WCTYPE-TRIE-FILE-2/after/count-{liella,glibc}.tsv; compare with handoff/wctype-trie-file/cmpcls.py;
counts by handoff/wctype-trie-file/count.sh <role> <label>.

## Next step
1. If the gate is red on a start-up page-fault bound: per ruling #4347, measure the same count on origin/next before
   anything else (this branch's start-up code: `store_view` in setlocale/publish). 2. Counts (isw*, iswctype, tow*).
3. `_l` rows. 4. timing.
Do not redo: the stage-1 encoding, the shared pool, `.bss` slots, the 32-bit cells, the Latin alignment experiment, the
cold out-of-line resolve-only path, `cold_path`; the run-7 merge and gate replacement.
