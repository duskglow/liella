# WCTYPE-TRIE-FILE(-2..-6): lazily resolved class tables for compiled LC_CTYPE + the thread-block table word (D-5.10, D-5.11)

**State:** branch `task/wctype-trie-file-6` (continues `-5`..`-2`; origin/next merged in runs 3 and 5). HEAD: see
`handoff/reports/WCTYPE-TRIE-FILE-6.json`. Decisions in force: **D-5.10**, **D-5.11** (records/decisions/D-5/; D-5.11
carries the owner's safety rule and `FLAG_VIEW`), D-5.9, D-5.5, D-7.13. The chain is HELD: it does not land before
TB-FOREIGN-SAFETY (coordinator, 11-10). Run 6 report: this file (section "Run 6") and the json.

## What the story does (runs 1-2)
`ClassTable` (crates/core/src/locale/classes.rs): stage 1 of 4352 32-bit cells, built-in leaves (D-5.9) or pool leaves,
resolved per 256-character block on first touch; `ThreadBlock::wclass` (0x180) stored with `view` by every writer;
`isw*`/`iswctype` read it with one `%fs` load.

## D-5.11 safety (runs 3-5)
* The words at 0x138-0x187 (`view`, cache line, `wclass`) are read and written only in a block the runtime made.
  `FLAG_VIEW` (0x9c bit 1) is set by `store_view` after the first store of both words into a `FLAG_OWN` block. All
  flag reads go through `block_flags()` / `block_flags_of(b)` in crates/boundary/src/locale.rs: **TB-FOREIGN-SAFETY
  replaces only those two bodies** (ruling #4281: 0x9c is the other library's data in a foreign block; the gate's
  source becomes that story's ownership helper, the process-level owned() byte).
* Test `locale::tests::foreign_block_words_are_never_read_or_written` covers isw, iswctype, view(), with_view(), the
  zone cache and the store gate. Plants: handoff/wctype-trie-file-plants.txt.

## Run 6 (this run)
* Inherited head: `cargo xtask static` red (codemap stale after run 5's with_view): regenerated, a8edf36.
* **Release build cause (wtf5-regen rc 1):** not this branch's code. The fast-profile regen (merge.py: the new trace
  wins, an old key goes after its old predecessor) moved `RawVecInner::finish_grow` of liella_boundary, a start-up
  function only the RELEASE start-up runs, from list position 50 to 255; with `__fprintf_chk` (171) and `getrusage`
  (587, the list's tail, `extra -`) that made 5 release windows. The release regen build (`--hottext-regen`) then
  stopped in STARTUP-TEXT-WINDOWS before the release trace could restore the order: the contract blocked its own
  repair. Fix 0bc791f: in the regen build only, that one contract kind is reported, not enforced
  (`waived_in_regen`, xtask/src/layout.rs, unit test + 2 plants); regen.sh's final plain build enforces it. Budget
  unchanged. Then regen.sh in both profiles as its usage says (job wtf6-regen2 rc 0): finish_grow back at 50; release
  4 of 4 windows, fast 5 of 5; the 3 stale keys re-traced; stale hot-text.absent line (CtypeFile::wide) removed (974927c).
* Diagnosis method (reusable): scratch lay.py rebuilt the release group layout from libc.a's `.text.sorted.h<n>`
  sections (matched the link's group end exactly) and printed each start-up key's window.
* tow* (ruling #4281): no `%fs:0x9c` test; `with_view` (run 5) stays; its count is still unmeasured.

## Still open
* **Page faults (since run 3):** `hot_text_products` first `localtime_r` 21 > 20. NOT run in run 6 (out of budget;
  its chained job was stopped before it started). Ready: shiki ~/work/WCTYPE-TRIE-FILE-6/diag.sh (diag1.sh without the
  build, with pfmark.c/resolve.py beside it) runs the test, then prints every fault of the first localtime_r with section
  and symbol: run it as a jobrun job in the role after a fast build.
* Plants: `coldbss-part-before-llvm` (run 5) printed WARN-NO-MSG in wtf6-plants (caught by rc only): fix its msg regex. Note the regen moved
  `zone::localtime_r` to `release` only (fast now inlines it) and added `time::load` in fast.
* getrusage sits at the list tail (`extra -`, no probe traces it) yet is a start-up key: one window in both profiles.
  A tooling fix (start-up probe keys first in the order) would free a window: own story, not this one.
* tow* counts after with_view (count.sh); `_l` rows (ruling #4173: astral `_l` 20/21 vs 12; profile `view_of(loc)`
  first); period-cache own-process test; timing.

## Counts so far (run 4, kanon `wtf4-count`, c66487a)
vs the -2 head: 432 faster, 450 equal, 54 slower (towupper/towlower/towctrans +2). Reference TSVs: keke
~/work/WCTYPE-TRIE-FILE-2/after/count-{liella,glibc}.tsv, kanon ~/work/WCTYPE-TRIE-FILE-4/a1; compare with
handoff/wctype-trie-file/cmpcls.py; counts by handoff/wctype-trie-file/count.sh <role> <label>.

## Next step
1. Read wtf6-gate / wtf6-plants (shiki); fix a red. 2. Page fault: run diag.sh. 3. tow* counts.
4. `_l` rows. 5. timing.
Do not redo: the stage-1 encoding, the shared pool, `.bss` slots, the 32-bit cells, the Latin alignment experiment, the
cold out-of-line resolve-only path, `cold_path`; the codemap and coldbss fixes of run 5; the release-build diagnosis
and the hot-text regeneration of run 6.
