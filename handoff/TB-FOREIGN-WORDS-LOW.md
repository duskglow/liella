# TB-FOREIGN-WORDS-LOW: errno, TID, dtor, heap and node words (0x80-0x9b) in a block Liella did not make

State: branch `task/tb-foreign-words-low-10` (from -9's 7936be696, which merged origin/next 630f3afb2). Code head
c0ac5f025; later commits are the plants file and this handoff. Decisions in force: D-X.58, D-X.60, D-X.62, rulings
#4835, #4849, #4851, #4779 (<= +2 per one-access call vs next, judged on Release #4889), #5325, #5378, #4347.
Older reports: handoff/TB-FOREIGN-WORDS-LOW-report.md (-1..-4), git history of this file (-5..-8).

## What -10 found (mei, Skylake-SP, the CI host class; RAM copies of head 7936be696 and next 630f3afb2)
- Jobs `tb-foreign-words-low-10-pfhead` / `-pfbase` (mei): coreutils-shaped **head 51, next 50**; setlocale+printf
  47 vs 46. `perf record -e page-faults -d` of one `tool` run: the same per-section counts except **.text 9 vs 8**.
- Cause: the string kernels (`strlen` first) start on a 32 KiB-aligned boundary. In next the text before them
  ends at 0x477ff8, 8 bytes short of 0x478000. The branch grew that text by about 15.1 KiB, so `strlen` moved to
  0x480000 and every later function moved 32 KiB with it (`bindtextdomain` 0x488 -> 0x490, `main` 0x49b -> 0x4a3).
  The tool's text then spans windows 0x40-0x4a instead of 0x40-0x49. Main contributors (per-symbol size diff below
  `strlen`): `Heap::refill`/`alloc_slow` monomorphised a second time for `OwnPages` through `OWN_CTX` (6.1 KiB), and
  the D-X.60 owned test inlined at about 1200 `errno` sites (a compare, a branch and a cold `foreign_errno` call
  each, 41 of them before `main`), which also changed inlining in nss/nscache (about +4.8 KiB net).
- Fix c0ac5f025: (1) `errno::set_raw` and `errno::get` are `#[inline(never)]` in the products (`set` forwards), so
  a site is a `mov` and a `call` (about 10 bytes, less than next's inline `%fs` load and store). (2) `rt::Pages` gains
  `type General` / `fn general()`: `OwnPages::General = KernelPages`, `Self` elsewhere. `Heap::alloc`/`free`/`resize`
  call the slow paths with `pages.general()`, so each slow path exists once, as the `OwnPages` comment and D-X.60
  already said.
- Job `tb-foreign-words-low-10-pfhead2` (mei, c0ac5f025): **coreutils-shaped 50 (bound 50)**, startup-only 11 (11),
  setlocale+printf 47 (47). `.text` 0x36c6e7 (head before: 0x375867; next: 0x3642e7). `strlen` is still at
  0x480000: the text before it remains above 0x478000. The 50 holds because the earlier windows now pack tighter.
  This is a thin margin.
- Cost (not measured in this run): a failing call that sets `errno` now pays a call and a return, +4 instructions
  against next's inline store, on failure paths only. The rows ruling #4779 bounds (`errno_location` export,
  malloc_free, calloc_free) do not go through these entries. The heap's slow paths reached from the plain `c_*`
  paths now read the cache word through `KernelPages` (one out-of-line `heap_cache_word` call per slow call).

## Jobs (hand-back names the final ones)
lint1 (shiki, c0ac5f025): workspace clippy (all targets, all features) + xtask clippy + static, rc 0.
mei-direct (mei, RAM copy of c0ac5f025): hot_text_products, s02_diagnostics_clauses, tb_foreign_low_words,
s02_diagnostics_symbols, heap_and_sync. gate and plants (shiki, final head): see the hand-back.

## Open / next step
1. Margin: the text before the 32 KiB-aligned string kernels is still about 9 KiB over the boundary next sits
   under. A layout question for LAYOUT-RETHINK (what fills 0x408000-0x478000, and whether the kernels need 32 KiB
   alignment), not this chain's. Next's own 8-byte margin there means unrelated growth on next can flip the tool
   count on the Skylake-SP host too.
2. If the remaining `OwnPages` monomorphisations (malloc_compat memalign family, `first_cache`, `cache_addr`,
   `own_cache`, all after `main`) ever matter, route them the same way.

## Not to redo
-1's audit; the registry/FOREIGN/OwnPages/asm design; -7's SPARE split; -8's Haswell-EP comparison; this run's
mei fault-set comparison (scripts in the scratch dir: pfset.sh, an.py, sz.py; copies in mei
~/work/tb-foreign-words-low-10/).
