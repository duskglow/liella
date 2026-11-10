# WCTYPE-TRIE-FILE: lazily resolved class tables for compiled LC_CTYPE + the thread-block table word (D-5.10, D-5.11)

**State:** branch `task/wctype-trie-file` (from origin/next `2f72b2638`). Commits: `6c2e57f30` (code, tests, plants,
rows), `101c32c1f` (walk-test fix, counts job), then this report. Decisions in force: **D-5.10** (proposal 2),
**D-5.11** (proposal 3) in plan/07-decisions/s05-character-classification.md; D-5.9, D-5.5 and D-7.13 still hold.

## What changed
* `crates/core/src/locale/classes.rs` (new): `ClassTable`, one type for every locale. Stage 1 = 4352 `AtomicU16`:
  0 unresolved, `1..=148` built-in leaf `v-1` (D-5.9 `LEAVES`), `0x8000+k` pool leaf, `0xffff` no leaf (walk).
  `C_TABLE` / `C_UTF8_TABLE` are statics built at compile time from the generated `u8` stage 1 (in `.data` now:
  +8.5 KB file size for the two). Compiled views take one of 16 `.bss` tables when built (`for_file`; no page touched
  until a wide class test); a view past 16 gets `WALK_TABLE` (never written, walks every call). The pool (256
  leaves, 128 KiB `.bss`) is shared by all compiled locales (a leaf is a pure function of its words).
  `hit(c)` (inline: stage-1 Acquire load, leaf load) and `word(c, wide)` (out of line: resolve or walk).
  Resolve: 256 `w_classes_file` words, an equal built-in leaf else `pool_add` (dedup; words written, ready flag
  Release), stage 1 published by `compare_exchange(0, v, Release, Relaxed)` (the loser keeps the winner's value).
* `LocaleView`: `classes: &'static ClassTable`; `isw_int` = `classes.hit(c)` + mask, else `isw_wide` (tail call) =
  `class_word(c)`. The Latin-1 cache lost its class array (case maps and widths stay). `class_table()` accessor.
* `ThreadBlock::wclass` (0x180; `tail` now 0x188, 47 words). `boundary::locale::store_view` stores `wclass` with
  `view` for every writer (store_words: `uselocale`, `setlocale`, thread creation, first use; `refresh_views_of`:
  `newlocale` in place). `locale::isw` / `locale::iswctype` read `wclass` (one `%fs` load) + `hit`; 0 or a miss tail-
  calls the view path. All `isw*` and `iswctype`/`__iswctype` exports use them; `_l` keeps `view_of(loc).isw_int`.
  `iswctype_view` lost its built-in arm (one path for all views now).

## Tests and proofs
* `locale::tests::file_class_tables_equal_the_file_walk` (s5_2_g_0, s5_2_c_0): en_US.UTF-8 and the system
  `/usr/lib/locale/C.utf8/LC_CTYPE` (SKIPPED line if absent), every code point 0..=U+10FFFF + 5 edge values, word ==
  `w_classes_file`, tables sharing built-in leaves and pool-only; then through a view, `isw` == `is_w_file` x 12
  classes. Census on mei: both locales resolve **all 4352 blocks to built-in leaves**; pool-only: 4352 pool blocks,
  0 walked (the 256-leaf pool held every distinct leaf of both).
* `class_table_first_touch_race`: 16 threads, barrier, one unresolved block, 4 cases (built-in and pool leaves).
* `walking_class_tables_equal_the_file_walk`: blocks marked no-leaf and a no-slot table answer by the walk, never written.
* `conformance/c/s05_ctype_words.c` (existing) covers the thread word after `uselocale`/`setlocale`/threads/`newlocale`.
* Plants: handoff/wctype-trie-file-plants.txt (leaf index, unresolved marker, resolved test, pool offset, walk marker,
  publish order = `equivalent:` (x86-TSO: Release and Relaxed cmpxchg are the same instruction), stale thread word).
  Job mei:`wctype-trie-file-plants2` rc 0: **6 CAUGHT, 0 SURVIVED, 1 EQUIVALENT (publish-order), 0 WARN**.

## Jobs (head `101c32c1f`)
* Lint: mei:`wctype-trie-file-lint4` rc 0 (clippy -D warnings, static) on `6c2e57f30`; the gate re-ran clippy+static first.
* Gate mei:`wctype-trie-file-gate2` (`affected --run --base origin/next`): suites 8 ok / 0 failed, 10254 passed,
  0 failed, 1160 ignored; conformance 28 passed; peak 16G (page cache), oom_kill 0; **rc 1 only from `cargo xtask
  static` private-paths**: it flagged `conformance/build/s27_static_pie_load_offset.key` and
  `conformance/build/s27_dl_find_object.key` (home path), build outputs the same gate's conformance step wrote into
  the role; not files of this change (my diff has no such path; standalone static on the role passed before the run).
  INFORMATIONAL for the static/affected tooling. SUPERSEDED: `wctype-trie-file-gate` (killed, old head),
  `wctype-trie-file-plants` (baseline red: a wrong assertion in the new walk test, fixed in `101c32c1f`).
* Counts mei:`wctype-trie-file-count` (count.sh, started after the gate): PENDING at hand-back. Outputs
  ~/work/WCTYPE-TRIE-FILE/after/ on mei: `count-liella.tsv`, `count-glibc.tsv`, `touch` lines (first-touch
  instructions: (all - one) / 4351 = mean cost of resolving a block), `bench.log` (cbench_locale --compare --counters).
  Before = mei ~/work/WCTYPE-TRIE-BUILTIN-2/after2/count-liella.tsv (`5a6969c1d`, the D-5.9 head, in this base).
  Compare: `python3 handoff/wctype-trie-builtin/cmp.py before.tsv after.tsv glibc.tsv`. Expected (not measured):
  en_US `isw*` from 16-38 to about 12-14 instructions, C/C.UTF-8 17 to about 13.

## Open / next
* Timing (Ivy Bridge, N100): PENDING (hosts down, brief: after 19:30 UTC PENDING with counts is fine). Use
  handoff/wctype-rethink/trun.sh.
* The publish-order plant cannot be caught on x86-64; an aarch64 run (or a weak-memory model checker) would.
* Possible follow-up: the same table for `towupper`/`towlower` (rethink section 8 item 4); `ctype::wclasses` callers
  (scanf, text.rs) could read the view's table.
* Do not redo: the u16 encoding, the shared pool, `.bss` slots (heap tables cost pages in every `setlocale(LC_ALL, "")`).
