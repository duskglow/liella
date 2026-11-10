# WCTYPE-TRIE-FILE(-2..-5): lazily resolved class tables for compiled LC_CTYPE + the thread-block table word (D-5.10, D-5.11)

**State:** branch `task/wctype-trie-file-5` (continues `-4`, `-3`, `-2`; origin/next merged in runs 3 and 5). HEAD: see
`handoff/reports/WCTYPE-TRIE-FILE-5.json`. Decisions in force: **D-5.10**, **D-5.11** (records/decisions/D-5/; D-5.11
carries the owner's safety rule and `FLAG_VIEW`), D-5.9, D-5.5, D-7.13. The chain is HELD: it does not land before
TB-FOREIGN-SAFETY (coordinator, 11-10). **Run 5 ended INCOMPLETE: the merged tree does not build in the release
profile (below); nothing after the merge is gated.**

## What the story does (runs 1-2)
`ClassTable` (crates/core/src/locale/classes.rs): stage 1 of 4352 32-bit cells, built-in leaves (D-5.9) or pool leaves,
resolved per 256-character block on first touch; `ThreadBlock::wclass` (0x180) stored with `view` by every writer;
`isw*`/`iswctype` read it with one `%fs` load.

## D-5.11 safety (runs 3-5)
* The words at 0x138-0x187 (`view`, cache line, `wclass`) are read and written only in a block the runtime made.
  `FLAG_VIEW` (0x9c bit 1, crates/rt/src/tls.rs) is set by `store_view` after the first store of both words into a
  `FLAG_OWN` block. `isw`/`iswctype`/`view()`/`with_view()` test `FLAG_VIEW`, then read their word with no zero check;
  anything else takes the view path. `own_block()` (`FLAG_OWN`) gates the time zone cache. All flag reads go through
  `block_flags()` / `block_flags_of(b)` in crates/boundary/src/locale.rs: **TB-FOREIGN-SAFETY replaces only those two bodies.**
* Test `locale::tests::foreign_block_words_are_never_read_or_written` covers isw, iswctype, view(), with_view() (run 5),
  the zone cache and the store gate. Black-box probe (glibc 2.39, x86-64): `%fs:0x9c` reads 0 in glibc threads.
* Plants (handoff/wctype-trie-file-plants.txt): run 5 anchored `safety-view-gate` to `view()` with `perl:` (its `lit:`
  matched with_view too) and added `safety-with-view-gate` and `coldbss-part-before-llvm`. **Not run yet on run 5's code.**

## Run 5 (this run)
* Merged origin/next (ruling #4253); `cargo xtask codemap` output committed (96379be). `wtf5-lint0` (clippy + static on
  the merged tree before any code change) rc 0.
* tow* +2: `with_view(f)` (locale.rs): the stored case inline, everything else in one `#[cold] #[inline(never)]`
  function that runs `f` itself (a tail jump, no frame in the caller). towupper/towlower/towctrans/__towctrans use it
  (513057e). **Not measured yet** (the count jobs never reached it). Expected: -1 (the layout jump); the flag load's +1
  remains. Parity needs a memory-operand flag test (`testb $2, %fs:0x9c; jz`), i.e. a second inline-asm site, which
  the owner's one-tb-field-site rule (xtask/src/policy.rs) forbids: RULING NEEDED.
* `cargo xtask so` stopped in xtask's start-up-state check: LLVM now emits SYSINFO_EHDR as
  `.data.<sym>.0.llvm.<hash>`, which the parser in xtask/src/coldbss.rs rejected. Fixed (e72ad2d, unit test extended).
* Then the hot-text check: 3 keys match no fast-profile section (`locale::publish_global`, `getopt::next<..>`,
  `nscache::real_secs<..>`, inlined or renamed). `tools/hottext/regen.sh . --build` (fast) ran in the role (job
  `wtf5-regen`), then `LIELLA_FULL_PROFILE=1 regen.sh . --build` FAILED: `layout: STARTUP-TEXT-WINDOWS BROKEN: start-up
  text spans 5 windows > budget-release 4` (D-X.49), and `hot-text.absent: 1 stale release line`. The role's
  hot-text.list is half-regenerated and NOT committed; regenerate from scratch.

## Still open
* **Release build (blocks everything):** find which start-up function left the release hot group (STARTUP-TEXT-WINDOWS
  lists keys in crates/libc-so/startup-text.keys; 107 of 111 traced in fast). Check whether origin/next alone passes
  in release (if yes, it is this branch's code generation: locale.rs grew ~200 lines). Never raise the budget.
* **Page faults (NOT FIXED, since run 3):** `hot_text_products` first `localtime_r` 21 > 20 and combined 21 > 20
  (direct tier does not select it: `cargo xtask so` + `cargo test -p liella --test hot_text_products`). Tool ready:
  handoff/wctype-trie-file/{pfmark.c,diag1.sh,resolve.py} (diag1.sh expects them in ~/work/WCTYPE-TRIE-FILE-5) print every fault
  of the first localtime_r between two marker pages, with section and symbol (perf page-faults, paranoid 1 is enough).
  It has not run yet (each build stopped first). The hot-text regen above may move this fault: re-measure after it.
* **`_l` rows (ruling #4173):** astral `_l` 20/21 vs glibc 12, not started: profile `view_of(loc)` per phase first.
* Period-cache own-process (conformance) test; timing (Ivy Bridge, N100): not done.

## Counts so far (run 4, kanon `wtf4-count`, c66487a)
vs the -2 head: 432 faster, 450 equal, 54 slower (towupper/towlower/towctrans +2). Reference TSVs: keke
~/work/WCTYPE-TRIE-FILE-2/after/count-{liella,glibc}.tsv (-2 head), kanon ~/work/WCTYPE-TRIE-FILE-4/a1 (run 4);
compare with handoff/wctype-trie-file/cmpcls.py; counts by handoff/wctype-trie-file/count.sh <role> <label>.

## Next step
1. Release build (above); commit the regenerated hot-text.list / hot-text.absent. 2. `wtf5`-style diag: page faults
(diag1.sh) and counts (count.sh) on the built head. 3. clippy + static, direct gate, plants
(handoff/wctype-trie-file-plants.txt). 4. `_l` rows. 5. timing.
Do not redo: the stage-1 encoding, the shared pool, `.bss` slots, the 32-bit cells, the Latin alignment experiment, the
cold out-of-line resolve-only path, `cold_path`; the codemap and coldbss fixes of run 5.
