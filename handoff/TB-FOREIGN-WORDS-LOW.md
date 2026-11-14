# TB-FOREIGN-WORDS-LOW: errno, TID, dtor, heap and node words (0x80-0x9b) in a block Liella did not make

State: branch `task/tb-foreign-words-low-8` (stacks on -7; origin/next merged at cef760b0a by -7, merge base
d6918f2c3; next has moved on 10 commits since, not merged: the brief allows no merge). Code head f5b8f7320, HEAD = the
handoff commit after it. Decisions in force: D-X.58, D-X.60, D-X.62 (STARTUP-DATA-WINDOWS), rulings #4835, #4849,
#4851, #4779 (<= +2 per access vs next), #4889 (judge on Release), #5325, #5378, #4347. Older reports:
handoff/TB-FOREIGN-WORDS-LOW-report.md (-1..-4), git history of this file (-5..-7).

## What -8 did (shiki:~/w/sh1, Haswell-EP)
- **34aa7fb9e** the D-X.60 test on the heap's general paths (`KernelPages::cache_word` / `cache_slot`: owned(), the
  %fs load, a cold call) was inlined at every site, a dozen of them inside start-up functions
  (`Debug::alloc_checked`, `release_perturbed`, `drain_held`, `malloc_general`). Now `thread::heap_cache_word()` and
  `thread::heap_cache_slot()` are `#[inline(never)]`: one call per site. The fast paths keep `OwnPages` (one
  %fs-relative load, no test). `foreign_heap_cache` reads `foreign_heap_cache_slot` directly.
- **f5b8f7320** `selftest::heap_cache_word` reads through `KernelPages`; `foreign_block_low_words_neither_trusted_nor_written`
  asserts the general paths never read the planted 0x90 word. Plants file handoff/TB-FOREIGN-WORDS-LOW-8-plants.txt:
  -7's six (slow-path-own-copy re-aimed at `init_startup_group_ends`, since heap_meta no longer sees a second copy
  under trace-at-build) + two on the cache word.

## Results (fast products unless named)
- lint0 (inherited head 3cb54a4af): clippy + static green.
- chk1 (34aa7fb9e): STARTUP-TEXT-WINDOWS 2/2, STARTUP-DATA-WINDOWS ro 1/1, written 12/12 (fast), 14/14 (release).
  First window: start-up group ends **0x417f4c, margin +180** (-7: 268 past; next as merged: 0x417e4c, +436).
  Release: ends 0x417e0c, margin +500, hot_text_products 25/25, heap_meta 2/2. Fast heap_meta 2/2; startup-only 11
  (bound 11), setlocale+printf 47 (47), fork 5 (5). Fast **coreutils-shaped 51 > 50**: see below.
- pfbase / pfbr1: the merge base (next as merged, RAM copy) built and measured on the same host reads
  **coreutils-shaped 51 as well**, and `perf record -e page-faults -d` of one `tool` run gives the same fault set for
  both (9 text windows 0x40-0x4a except 0x44/0x45, 3 rodata, 7 data pages; one rodata window 0x77 vs 0x78 shifted).
  So on this Haswell-EP host the 51 is next's own count, not this branch's; -7 read next 50 on the Zen 1 host (mei).
  NOT VERIFIED on the Zen 1 host for this head (role is on shiki).
- lint1 (f5b8f7320): clippy (workspace, all targets, all features) + static + tb_foreign_* green.
- percall (f5b8f7320 vs base d6918f2c3, release, instructions/call): malloc_free 115.2 = 115.2 (glibc 144.1),
  calloc_free 162.9 = 162.9, errno_location 11.0 vs 9.0 (+2, the D-X.60 test from -6, at ruling #4779's limit;
  this run changed no errno code).
- gate (f5b8f7320, direct tier on shiki): 6542 passed, **1 failed: hot_text_products
  init_coreutils_shaped_program_stays_within_its_page_fault_bound (51 > 50)**, the count next itself reads on this
  host (pfbase above). Not caused by this branch; the job is rc 1.
- plants (f5b8f7320): 5 CAUGHT, slow-path-own-copy SURVIVED init_startup_group_ends too (now `equivalent:` with
  the reason), the two cache-word plants caught by a SIGSEGV in the mutant (WARN-NO-MSG; msg widened).
  plants2 (a76a10cd3, same code): the rerun with the fixed file; result in the report json.
- Correction to the comment on `OwnPages` (crates/boundary/src/heap.rs): "a second copy ... put the first
  allocation of a new size class on a text page of its own" held before trace-at-build; -7 and -8 no longer see it
  (slow-path-own-copy passes heap_meta_products and the first-window test). Not edited in this run (a code change
  would have needed the gate again); fix the wording in the next code change there.

## Open
- The start-up group is still ~256 bytes larger than next's (margin 180 vs 436). Remaining growth candidates: the
  inlined `thread::words()` in `node_hint` (KernelPages::shard), `tb::errno_inline` sites in start-up functions.
- Fast coreutils-shaped bound 50 is red on next itself on the Haswell-EP host: a host-class question for the
  layout owners (INFORMATIONAL), not this chain's.

## Next step
1. The gate's one failure is host-bound (next reads the same 51 on Haswell-EP). Coordinator: run the direct-tier
   gate for this head on the Zen 1 host (mei, where -7 read next at 50), or rule on the Haswell-EP bound. If it is
   green there, the branch can land. A FULL tier is the train's.
2. Optional: outline `node_hint`'s words() lookup the same way to win back more of the 256 bytes.

## Not to redo
-1's 0x80-0x9b audit; the registry/FOREIGN/OwnPages/asm design; the heap slow-path KernelPages choice (-5); -7's
SPARE split; this run's base-vs-branch fault comparison on Haswell-EP.
