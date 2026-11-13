# TB-FOREIGN-WORDS-LOW: errno, TID, dtor, heap and node words (0x80-0x9b) in a block Liella did not make

State: branch `task/tb-foreign-words-low-4` (stacks on -3 / TB-FOREIGN-SAFETY-6). HEAD: this handoff commit.
Decisions in force: D-X.58, D-X.60 (records/decisions/D-X/D-X.60.toml), rulings #4835 (FOREIGN bit, OwnPages,
errno in tp.S), #4849 (hand rename of the errno hot-text key), #4851 (selftest hook for SPARE), #4779 (<= +2 per
access vs next), #4889 (judge on Release). Full report: handoff/TB-FOREIGN-WORDS-LOW-report.md.

## What -4 did
- **The five fast-profile hot-text keys (cause found).** They are not renames: no symbol of those functions is
  left in the fast archive (nm/objdump of target/product/fast/libc.a), and none of their modules had an impl
  inserted (#4893 does not apply). They are inlined into their callers, which this chain's owned() gating
  (D-X.58/D-X.60) reshaped in the fast profile (thin LTO, 16 CGUs). Example: next's fast build keeps
  `Sys::wait4` for CSysImpl<true> as its own section and also inlines it into `spawn::wait4`. On this branch
  `spawn::wait4` has the `owned()` test before the 0xa0 descriptor load, and the generic has no copy left.
  The callers (time_compat::select, spawn::wait4, catalog::textdomain, NsCache::attach) are hot-text keys, so
  the code stays in the hot group. Fixed with the repo's tool, `cargo xtask hottext-repair --at 2ca200b6c`
  (07c81c2b3): intl::bind, intl::textdomain and TscGate::fresh match nothing in either profile (dropped; they were
  traced only in fast). Sys::wait4 (CSysImpl<true>) and the NsCache::attach closure still match in release, so
  they are recorded in hot-text.absent for fast. errno_inline (34ec7567d) stays: it is not the cause.
- **`c_malloc` / `hyp_quick` dead-code warnings:** they come from the product build (`so: profile fast`, no
  selftest feature). The same code is on next, and the warnings appear in other stories' logs on keke
  (argp-help-filter-keys-2). They are not this branch's; nothing changed.
- **percall spec fix (fbf21301f):** the errno_location case took `&errno`, which the const declaration lets the
  compiler hoist, so every variant counted 6.0 (loop only). It now calls through a volatile function pointer.

## Results (keke:~/w/n1)
- tb-foreign-words-low-4-lint0 (2ca200b6c): clippy, static green; fast `so` red on the 5 keys.
- tb-foreign-words-low-4-repair: hottext-repair, both profiles built; 3 dropped, 2 recorded.
- tb-foreign-words-low-4-lint1 (07c81c2b3): clippy, static, fast `so` GREEN.
- percall2 / percall2-rel / percall2-relbase (instructions:u per call; the Release base is the merge-base
  ca8aa835a exported with git archive and built with LIELLA_FULL_PROFILE=1 under its own --rev key):

  | case | glibc | next Release | new Release | next Fast | new Fast |
  |---|---|---|---|---|---|
  | errno_location | 10.0 | 9.0 | 11.0 (+2) | 9.0 | 11.0 (+2) |
  | malloc_free | 144.1 | 115.2 | 115.2 | 116.2 | 116.2 |
  | calloc_free | 281.4 | 162.9 | 162.9 | 163.9 | 164.0 |

  #4779 is met on Release (+2 on errno, +0 on malloc/calloc).
- Plants: tb-foreign-words-low-4-plants 6 CAUGHT + tb-foreign-words-low-4-plants2 1 CAUGHT (errno-inline-registry,
  whose pattern in -3's file did not compile; fixed) = 7/7 CAUGHT.
- Gate: tb-foreign-words-low-4-gate (`affected --run --base origin/next --tier full`, head 07c81c2b3) was still
  running at hand-back (153/6426 after 55 min): the dispatcher reads it.

## Not done / next step
1. If the gate or plants are red, fix the cause and rerun only the failing suite.
2. The foreign-loader probe (D-22.10 harness) for errno: not started (an optional follow-up).

## Not to redo
The 0x80-0x9b audit table of -1, the registry words, the FOREIGN/OwnPages/asm design, the next/merge-base
hot-text check, and the five-key analysis above (they are inlined, not renamed: the hot-text.absent lines are correct).
