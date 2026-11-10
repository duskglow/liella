# WCTYPE-TRIE-FILE(-2, -3, -4): lazily resolved class tables for compiled LC_CTYPE + the thread-block table word (D-5.10, D-5.11)

**State:** branch `task/wctype-trie-file-4` (continues `-3`, `-2`; origin/next merged in run 3). HEAD: see
`handoff/reports/WCTYPE-TRIE-FILE-4.json`. Decisions in force: **D-5.10**, **D-5.11** (records/decisions/D-5/; D-5.11
carries the owner's safety rule and, from run 4, `FLAG_VIEW`), D-5.9, D-5.5, D-7.13. The chain is HELD: it does not land
before TB-FOREIGN-SAFETY (coordinator, 11-10).

## What the story does (runs 1-2)
`ClassTable` (crates/core/src/locale/classes.rs): stage 1 of 4352 32-bit cells, built-in leaves (D-5.9) or pool leaves,
resolved per 256-character block on first touch; `ThreadBlock::wclass` (0x180) stored with `view` by every writer;
`isw*`/`iswctype` read it with one `%fs` load.

## D-5.11 safety (run 3, reworked in run 4)
* The words at 0x138-0x187 (`view`, cache line, `wclass`) are read and written only in a block the runtime made.
  Run 4 replaced run 3's always-missing table and `view()`'s zero check with a second flag bit:
  `FLAG_VIEW` (0x9c bit 1, crates/rt/src/tls.rs), set by `store_view` after the first store of both words into a
  `FLAG_OWN` block (only in a listed thread of a process the runtime started). `isw`/`iswctype`/`view()` test
  `FLAG_VIEW`, then read their word with no zero check; anything else takes the view path. `own_block()` (`FLAG_OWN`)
  still gates the time zone cache. All flag reads go through `block_flags()` / `block_flags_of(b)` in
  crates/boundary/src/locale.rs: **TB-FOREIGN-SAFETY replaces only those two bodies.**
* Why: run 3's `safety-missing-table` plant SURVIVED (cause of `wtf3-plants` rc 1): every own block is listed, so
  its words are stored, before it runs library code (new threads before `clone`, the initial thread in
  `init_initial`). The only own blocks that are never stored are startup before `init_initial` and a process whose
  initial thread could not be listed (`adopt()`'s mmap failed: `OWNED` false, nothing stores the view). The flag covers
  exactly those without the table; no test reaches them (no plant: it would survive for the same reason).
* Test `locale::tests::foreign_block_words_are_never_read_or_written` now also checks the flag word is not written.
  Black-box probe (glibc 2.39, x86-64 host): `%fs:0x9c` read 0 in the initial thread and three created threads, before
  and after `setlocale`, so bit 1 is as trustworthy as bit 0 (no more: TB-FOREIGN-SAFETY).
* Plants (handoff/wctype-trie-file-plants.txt): `stale-thread-word`, `safety-isw-gate`, `safety-view-gate`,
  `safety-store-gate` updated to the new code; `safety-missing-table` removed with its code; new `safety-own-gate`.

## Counts (wcprobe, 936 rows; kanon `wtf4-count`, c66487a = final code)
* vs the -2 head (keke `wctype-trie-file-2-count`): 432 faster, 450 equal, **54 slower: towupper/towlower/towctrans
  +2** (run 3: +3/+4). vs the D-5.9 base (keke ~/work/WCTYPE-TRIE-BUILTIN/after): 872 faster, 18 equal, 46 slower
  (towupper/towlower x18 +2, the 10 accepted/unaccepted astral isw/isw_l rows of ruling #4173).
* Where the +2 comes from (objdump of towupper): `mov %fs:0x9c; test $2; jne` to a block holding `mov %fs:0x138; jmp`:
  the compiler lays out the resolve path as the fall-through. The flag load is the gate's own +1; the jump is +1.
  **Tried, worse (reverted):** resolve path `#[cold] #[inline(never)]` (towupper became non-leaf: push/sub, +4/+5,
  `wtf4-count2`); `core::hint::cold_path()` (layout fixed but the frame setup moved above the test, objdump `wtf4-exp1`).
  Untried: a `tb` primitive that tests the flag with a memory operand (`testb $2, %fs:0x9c`, inline asm) so the
  gate is one instruction; making the tow* bodies' slow paths frame-free so a cold call costs nothing.

## Still open
* **Page faults (NOT FIXED):** `hot_text_products` first `localtime_r` 21 > 20 and combined 21 > 20 (kanon, every run
  identical; the direct tier does not select this test: run `cargo xtask so` + `cargo test -p liella --test
  hot_text_products`). Not Latin's alignment (run 3). Next: `nm -n` the static test program on origin/next vs this
  head around the zone code's data; suspect the built-in tables' `.data` growth (2 x 17 KiB of 32-bit cells). Never
  raise the bound.
* **`_l` rows (ruling #4173):** astral `_l` 20/21 vs glibc 12, not started. The `_l` entry is `view_of(loc)` (null and
  alignment tests, first-word load, zero test) then `classes` from the view: profile it per phase first (brief rule).
* Period-cache coverage moved out of cargo test in run 3 (own block only): an own-process (conformance) test is a follow-up.
* Timing (Ivy Bridge, N100): not done.

## Jobs (kanon kn1)
`wtf4-lint0` rc 0 (inherited head e96e6ff); `wtf4-lint1` rc 0 (clippy, static, locale tests, s05_ctype_words; c66487a);
`wtf4-count` rc 0 (counts + hot_text: 2 page-fault failures above; c66487a); `wtf4-count2` (6eef295, the worse
out-of-line attempt); `wtf4-exp1` rc 141 (diagnostic objdump, SIGPIPE from `head`); `wtf4-plants`, `wtf4-plants2`
killed (superseded heads); final head: `wtf4-gate`, `wtf4-plants3` (see the report json).

## Next step
1. tow* +2 (memory-operand flag test, above); 2. page faults; 3. `_l` rows; 4. direct gate; 5. timing.
Do not redo: the stage-1 encoding, the shared pool, `.bss` slots, the 32-bit cells, the Latin alignment experiment, the
cold out-of-line / `cold_path` resolve path.
