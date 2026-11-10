# WCTYPE-TRIE-FILE(-2): lazily resolved class tables for compiled LC_CTYPE + the thread-block table word (D-5.10, D-5.11)

**State:** branch `task/wctype-trie-file-2` (continues `task/wctype-trie-file`, base origin/next `2f72b2638`, not merged:
the XTASK-WALK-BUILD-OUTPUT fix #4134 was still in the CI queue at hand-back). Commits: `6c2e57f30` (code, tests,
plants), `101c32c1f` (walk-test fix, counts job), `4452fee13` (first report); run 2: `36fc43dbf` (D-5.10/D-5.11 as
records, the generated view back to the base's copy), `b84851700` (codegen fixes below), then this report.
Decisions in force: **D-5.10**, **D-5.11** (records/decisions/D-5/; D-5.11 provisionally accepted, ruling #4133);
D-5.9, D-5.5 and D-7.13 still hold.

## What the story does (run 1)
* `crates/core/src/locale/classes.rs`: `ClassTable`, one type for every locale. Stage 1 = 4352 cells (block `c >> 8`):
  0 unresolved, `1..=148` built-in leaf `v-1` (D-5.9 `LEAVES`), `0x8000+k` pool leaf, `0xffff` no leaf (walk).
  `C_TABLE` / `C_UTF8_TABLE` are statics built at compile time; compiled views take one of 16 `.bss` tables
  (`for_file`), a 17th view gets `WALK_TABLE` (walks every call). Pool: 256 leaves, shared by all compiled locales.
  First touch of a block: 256 `w_classes_file` words, an equal built-in leaf else `pool_add`, published by
  `compare_exchange(0, v, Release, Relaxed)`.
* `ThreadBlock::wclass` (0x180) stored with `view` by every writer (`boundary::locale::store_view`); `locale::isw` /
  `iswctype` read it with one `%fs` load; 0 or a miss tail-calls the view path. `_l` uses `view_of(loc).isw_int`.

## Run 2 (WCTYPE-TRIE-FILE-2)
* Counts of run 1's head (mei `wctype-trie-file-count`) vs the D-5.9 base: 620 rows faster, 76 slower. The disassembly of
  Liella's own `iswalpha`/`iswgraph`/`towupper` showed three causes, all fixed in `b84851700`:
  stage-1 cells `AtomicU16` -> `AtomicU32` (same values; the compiler does not fold the zero extension into a 16-bit
  atomic load: one `movzwl` per call); `ClassTable::test(c, mask)` makes the mask test in each leaf arm (a merged word
  cost `iswgraph` a second `movzwl`); `Latin` `repr(align(4096))` (3 KiB slots made the slot address an `imul`).
  Hot path now 19 instructions in the export (`iswalpha` was 20, `iswgraph` 21).
* Counts of `b84851700` (keke `wctype-trie-file-2-count`; outputs keke ~/work/WCTYPE-TRIE-FILE-2/after/; glibc's
  counts equal mei's on all 936 rows, so the hosts compare): **vs the D-5.9 base 850 faster, 76 equal, 10 slower; vs
  run 1's head 0 slower.** Medians (before/after/glibc): en_US `isw*` cjk 31/16/28, bmpmix 38/16/31, ascii 16/16/5;
  en_US `iswctype` bmpmix 50/18/25; C and C.UTF-8 `isw*` 17/16, `iswctype` 22/18, `isw*_l` 23/21.
  The 10 slower: en_US.UTF-8 astral blank/cntrl/digit/space/xdigit, `isw` 11 -> 15/16 (glibc 15), `isw_l` 17 -> 20/21
  (glibc 12): the replaced per-class walk stopped early at the class's bound; a uniform table has no per-class bound
  and getting it back costs a branch on every row (RULING NEEDED in the report).
* Test: `file_class_tables_equal_the_file_walk` also checks `test()` (one standard mask per code point in turn) on the
  shared and the pool-only tables. Plants: handoff/wctype-trie-file-plants.txt, patterns updated, two new
  (`test-builtin-arm`, `test-pool-arm`).

## Jobs (head `b84851700`, keke r5)
* `wctype-trie-file-2-lint1` rc 0 (clippy -D warnings all features, xtask clippy, static, records check).
* `wctype-trie-file-2-count` rc 0. Plants: `wctype-trie-file-2-plants` rc 0 (8 CAUGHT, publish-order EQUIVALENT,
  test-builtin-arm WARN-NO-MSG: my mutant did not compile, `w` is `&u16` there), `wctype-trie-file-2-plants2` rc 0
  (the fixed `*w != 0` plant alone: CAUGHT with its message).
* **Gate `wctype-trie-file-2-gate` rc 1 (RED).** The new records/decisions/*.toml are "not a path the selection
  knows", so `affected` escalated to FULL (`cargo xtask test`, 56 min): 6268 passed, **2 failed**, both in
  `liella::hot_text_products`: `init_first_localtime_r_and_mktime_stay_within_their_combined_page_fault_bound`
  (21 faults > 20) and `init_first_mktime_after_localtime_r_takes_no_fault`. Static did not fail this time.
  Likely cause (NOT VERIFIED): this run's layout change: the two built-in tables in `.data` doubled (8.7 -> 17.4 KiB
  each, +17 KiB) and `Latin`'s 4 KiB alignment pads `.bss`, moving the time zone code's data pages. Run 1's direct
  gate may not have selected these tests.
* SUPERSEDED: mei `wctype-trie-file-gate2` (rc 1 only from `cargo xtask static` private-paths on
  conformance/build/s27_*.key, the tooling defect #4134; a gate on any head that changes tb.rs selects those s27
  programs and fails the same way until XTASK-WALK-BUILD-OUTPUT lands).

## Open / next
* **First: the page-fault bound.** On keke r5 run `cargo xtask so` + `cargo test -p liella --test hot_text_products`
  on (a) this head with `Latin`'s `repr(align(4096))` removed, (b) with the stage-1 cells back to `AtomicU16`, to find
  which change moves the faults (compare `nm -n --size-sort` / section sizes of libc.so.6 against run 1's head).
  Fit the change to the bound (ALWAYS FORWARD), never loosen it. Idea if the `.data` growth is the cause: keep the
  32-bit cells but put the built-in tables in `.bss` and resolve their blocks lazily from the generated `u8` stage 1
  (a static is `.bss` only when all its bytes are zero, so the mode must not carry a pointer).
* When XTASK-WALK-BUILD-OUTPUT has landed (`git log origin/next --oneline | grep -i 'conformance/build'`): merge
  origin/next, sync, rerun the direct-tier gate (`affected --run --base origin/next`).
* Timing (Ivy Bridge, N100): not done (handoff/wctype-rethink/trun.sh). The change alters no instruction mix.
* The publish-order plant cannot be caught on x86-64 (equivalent); an aarch64 run would.
* Follow-ups: the same table for `towupper`/`towlower` (rethink section 8 item 4); `towupper` in C.UTF-8 is 53 vs
  glibc 40 instructions (cbench_locale `towupper_utf8`, unchanged here).
* Do not redo: the stage-1 value encoding, the shared pool, `.bss` slots (heap tables cost pages in every
  `setlocale(LC_ALL, "")`), the 32-bit cells (measured above).
