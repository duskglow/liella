# TB-FOREIGN-WORDS-LOW: errno, TID, dtor, heap and node words (0x80-0x9b) in a block Liella did not make

State: branch `task/tb-foreign-words-low-7` (stacks on -6; origin/next merged at cef760b0a, next = d6918f2c3 with
trace-at-build the default, LAYOUT-TRACE-DEFAULT-4). Code head 55807c5c4, plants file 7f69b4cc2, HEAD = the handoff
commit after it. Decisions in force: D-X.58, D-X.60, D-X.62 (STARTUP-DATA-WINDOWS), rulings #4835, #4849, #4851,
#4779 (<= +2 per access vs next), #4889 (judge on Release), #5325, #5378, #4347. Older reports:
handoff/TB-FOREIGN-WORDS-LOW-report.md (-1..-4), git history of this file (-5, -6).

## What -7 did (mei:~/w/s1)
- **cef760b0a** merge of next: the hand lists next deleted (hot-text.list/.absent, probe-text.keys,
  startup-data.keys) are dropped with this chain's hand keys in them (the trace places after_fork_child and the
  errno entry now); next's all-ones `OWNED_HERE` word kept with this chain's `DEBUG.clear_foreign()` (D-X.60).
- **55807c5c4** `heap_cache_slot()` (inlined into the heap's start-up allocation) tested `is_spare()` there, so it
  addressed `thread::SPARE` and STARTUP-DATA-WINDOWS counted SPARE's page. The spare test moved into a cold,
  out-of-line `foreign_heap_cache_slot()`. Written start-up pages: fast 14 -> 12 (budget 12, next 11), release
  15 -> 14 (budget 14, next 14). The fast 14 had two more causes besides SPARE: `.data.rel.ro` bytes that start-up
  functions address are not clustered by the trace (link order), so they spread over 8 pages vs next's 6. With
  the fix they take 7.
- **7f69b4cc2** handoff/TB-FOREIGN-WORDS-LOW-7-plants.txt: -6's four plants (msg fixed) + two on the split.

## Results
- step1 (merge head): clippy + static green; `cargo xtask so` fast RED: STARTUP-DATA-WINDOWS 14 > 12.
- rel (merge head): release so RED: 15 > 14. Both superseded by 55807c5c4.
- checks (7f69b4cc2): clippy, static, fast so (STARTUP-DATA-WINDOWS ok 12/12), heap_meta_products 2/2 green;
  **hot_text_products (fast) 22/25: RED**; release half (so + hot_text_products + heap_meta_products) GREEN.
  The three fast failures share one cause: the start-up text group ends 0x41810c, **268 bytes past** the first
  fault-around window 0x408000..0x418000 (liella_sys_fstat last at 0x418100), so startup-only 12 > 11 and
  coreutils-shaped 51 > 50 (FIRST-WINDOW). Next (nextfast, same host, fast): ends 0x417e4c, margin +436,
  startup-only 11, coreutils-shaped 50. The branch's start-up group is ~704 bytes larger than next's.
- sizes (fast libc.a, branch vs next, the 103 traced start-up keys by substring): net +629 bytes; the growth is
  the heap: `malloc_debug::Debug::alloc_checked` +332 (4188 vs 3856), `Debug::release_perturbed` +124,
  `Debug::drain_held` +102, `heap::malloc_general` +66, `malloc_compat::aligned_alloc_secure` +53;
  `__liella_libc_malloc_stats` -249. KernelPages->OwnPages renames of the memalign family net 0.
- plants (7f69b4cc2): 6 plants, 5 CAUGHT with their message (errno x3, both heap-cache-split plants),
  **1 SURVIVED: slow-path-own-copy** (heap slow path on OwnPages instead of KernelPages): under trace-at-build
  `init_heap_metadata_of_new_size_classes_stays_dense` no longer sees the -5 difference, likely because the trace
  now groups whichever copy runs. Not checked further (budget). percall NOT RUN (rc 2: base d6918f2c3 not primed on mei).
- FULL tier NOT RUN: the fast start-up tests are red on this head.

## Next step (not jitter: this chain changes start-up code, so ALWAYS FORWARD applies)
1. Get ~704 bytes (>= 268 to pass, more for margin) out of the start-up text group: the D-X.60 additions to
   `Debug::alloc_checked` / `release_perturbed` / `drain_held` / `malloc_general` (the foreign-block tests and
   the KernelPages slow path of -5). Move the foreign-process branches into `#[cold] #[inline(never)]` helpers the
   way 55807c5c4 did for heap_cache_slot; recheck with the checks job (~/work/TB-FOREIGN-WORDS-LOW-7/checks.sh on
   mei) and sizediff.py there.
2. slow-path-own-copy SURVIVED: map the 14-size-class run with OwnPages vs KernelPages on trace-at-build. If they
   fault the same, -5's KernelPages split is no longer needed (dropping it may also shrink start-up text); else find
   the test that sees the difference and point the plant at it.
3. Then prime next's base (`bash validation/wl_prime_base.sh mei <next sha>` from a git checkout), percall
   foreign_words.c (#4779: errno <= +2), plants rerun, FULL tier on the final head.

## Not to redo
-1's 0x80-0x9b audit; the registry/FOREIGN/OwnPages/asm design; the heap slow-path KernelPages fix (-5); no
full regeneration of lists (gone with trace-at-build); the next-vs-branch page tables and size table above.
