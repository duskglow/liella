# WCTYPE-TRIE-FILE(-2, -3): lazily resolved class tables for compiled LC_CTYPE + the thread-block table word (D-5.10, D-5.11)

**State:** branch `task/wctype-trie-file-3` (continues `task/wctype-trie-file-2`; origin/next merged at the start of
run 3, including XTASK-WALK-BUILD-OUTPUT). HEAD: see `handoff/reports/WCTYPE-TRIE-FILE-3.json`. Decisions in force:
**D-5.10**, **D-5.11** (records/decisions/D-5/; D-5.11 now carries the owner's safety rule), D-5.9, D-5.5, D-7.13.
The chain is HELD until the coordinator reviews step 0 (below).

## What the story does (runs 1-2, unchanged)
`ClassTable` (crates/core/src/locale/classes.rs): stage 1 of 4352 32-bit cells, built-in leaves (D-5.9) or pool leaves,
resolved per 256-character block on first touch; `ThreadBlock::wclass` (0x180) stored with `view` by every writer;
`isw*`/`iswctype` read it with one `%fs` load. Details: D-5.10/D-5.11 records, run 2's report in git history of this file.

## Run 3 (WCTYPE-TRIE-FILE-3)
* **Step 0, D-5.11 safety (owner note ~/state/notes/D-5.11-SAFETY.md):** the words at 0x138-0x187 (`view`, cache
  line, `wclass`) are Liella's only in a block with `FLAG_OWN`, tested per block:
  - `locale::own_block()` (`%fs:0x9c` bit 0). `isw` tests it first, then reads `wclass` with NO zero check: a block the
    runtime makes gets `ClassTable::missing()` (the walking table, stage 1 never written: every test misses) in
    `wclass` when it is marked `FLAG_OWN` (`own_block_made`, runtime.rs and thread.rs). Foreign block -> view path.
  - `view()` reads the `view` word only in an own block; `store_view` returns on a block without `FLAG_OWN` (as well as
    the process-wide `owned()` gate in `store_words`); `zone::with_period` neither reads nor stores the cache words in a
    foreign block (starts from no period).
  - Test `locale::tests::foreign_block_words_are_never_read_or_written` (boundary lib; the test process's block is
    glibc-made, i.e. the D-22.10 foreign-loader case): non-canonical garbage in all ten words, `isw`/`iswctype`/`view`/
    `store_view`/`with_period` give the right answers and write nothing. `s23_time_symbols::tz_period_words_per_thread_and_fork`
    now checks that a foreign block's cache words stay untouched; its per-thread and fork checks run only in an own
    block, so in cargo test they are skipped (printed): **coverage of the period cache moved out of cargo test; an
    own-process (conformance) test of it is a follow-up.**
  - Plants (handoff/wctype-trie-file-plants.txt, shiki `wtf3-plants`): 13 CAUGHT, 1 EQUIVALENT (publish-order, x86 TSO),
    **1 SURVIVED: `safety-missing-table`** (store 0 instead of the missing table: `s05_ctype_words` passed, so in that
    program every own block had its view stored before the first wide test). Next run: either a test that reaches `isw`
    in an own block before its words are stored (a thread's first call before it is listed, if that can happen) or
    proof it cannot happen (then mark it equivalent with that reason). All five safety gates were CAUGHT.
  - Audit of the other words in the range: `view` (0x138) and the cache line (0x140-0x17f, time zone period, its only
    owner) fixed as above. Outside the note's range and NOT changed (escalation): Liella also reads and writes
    0x9c-0x137 (`flags`, `thread`, `locale`, `ctype_b/toupper/tolower`, `lconv`, `h_errno`, `res`) in a foreign block,
    and the `FLAG_OWN` test itself trusts bit 0 of 0x9c there (D-22.10's premise).
  - **Hot path counts (shiki `wtf3-count` vs keke `wctype-trie-file-2-count`, the -2 head; merge of next included):**
    432 rows faster (iswctype 18 -> 17 x216, isw 16 -> 15 x180, 15 -> 14 x36), 456 equal, **54 slower: towupper/towlower
    +3 (15 -> 18, 40 -> 43), towctrans +4**: the `view()` gate (flag test plus the kept zero check). Not yet compared with
    the D-5.9 base (its counts were not on keke). Fix idea: store a view in every own block when it is made (the C view
    at startup, the global's at thread creation) so `view()` drops its zero check like `isw`.
* **Step 1:** origin/next merged (clean).
* **Step 2, page-fault bounds (NOT FIXED):** shiki r sh2, head as is: first `localtime_r` 21 faults (bound 20), combined
  21 (bound 20), every run identical; with `Latin`'s `repr(align(4096))` removed: still 21 -> not the cause. Untried:
  the built-in tables' `.data` growth (C_TABLE/C_UTF8_TABLE 2 x 17 KiB of 32-bit cells); compare with origin/next's
  products (`nm -n` of the static test program around the zone code's data) to find the moved page. Never raise the bound.
  The direct-tier gate does not select `hot_text_products`; run it by hand (`cargo xtask so` + `cargo test -p liella
  --test hot_text_products`).
* **Step 3 (en_US astral `_l` rows, ruling #4173):** not started this run (budget).
* Timing (Ivy Bridge, N100): not done.

## Jobs (shiki sh2)
`wtf3-lint3` rc 0 (clippy + static); `wtf3-gate` rc 1 (direct tier, 6285 passed, 1 failed: the tz period test above);
`wtf3-gate2` rc 0 (the fixed suite `s23_time_symbols` 73 passed + clippy, on the final code head); `wtf3-plants` rc 1
(one survivor above); `wtf3-count` rc 0 (outputs shiki ~/work/WCTYPE-TRIE-FILE/wtf3-after/); `wtf3-exp1` (fault experiment).

## Next step
1. Fix the `view()` +3 (pre-stored view) and re-count; 2. settle `safety-missing-table`; 3. page faults (above);
4. step 3 `_l` rows; 5. full direct-tier gate on the final head; 6. timing.
Do not redo: the stage-1 encoding, the shared pool, `.bss` slots, the 32-bit cells, the Latin alignment experiment.
