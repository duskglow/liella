# TB-FOREIGN-WORDS-LOW: errno, TID, dtor, heap and node words (0x80-0x9b) in a block Liella did not make

State: branch `task/tb-foreign-words-low-5` (stacks on -4, next merged in at 3dc8d485a). HEAD: this handoff commit
(code head dedbe2223). Decisions in force: D-X.58, D-X.60 (records/decisions/D-X/D-X.60.toml), rulings #4835 (FOREIGN
bit, OwnPages, errno in tp.S), #4849, #4851, #4779 (<= +2 per access vs next), #4889 (judge on Release), #5325
(a branch that moves hot functions regenerates the start-up lists). Older report: handoff/TB-FOREIGN-WORDS-LOW-report.md.

## What -5 did (the three FULL-gate reds of -4)
- **Merge of origin/next** (3dc8d485a): ld.rs kept both STARTED_HERE (D-X.58) and next's rseq exports; the
  generated lists took next's copy and were then regenerated (below).
- **Start-up state list** (75181c245): next's hand-written startup-state.list (D-X.64) named `thread::OWNED`, now an
  alias of the assembly byte, so `cargo xtask so` stopped. It now names `thread::OWNED_HERE` and
  `symbol liella_machine_owned`; that byte got a section of its own in machine/points.S (it shared the aio flag's).
  The stale fast `hot-text.absent` line for `Sys::wait4` was removed.
- **errno tests** (875c56a37): s2_1_c_0 and sym___errno_location assert the case they run in (test process: not
  started by the runtime; `__errno_location` = the registry word, outside the block, stable). They use new selftest
  hooks `runtime_started()` and `foreign_errno_word()`. s2_1_c_0 also links oracle/errno_owned.c statically against the
  products and checks the started case: tp + 0x80 in the initial thread and in a created one. Conformance
  s01_startup also runs on the host library, so it cannot check the offset.
- **Heap fault** (646371012): a perf page-fault map of
  heap_meta_faults (tools/hottext/faultmap.py) put the 10th fault on the text page of
  `Heap::alloc_slow::<CSysImpl<false>, OwnPages>`. That second copy sat beside the KernelPages copy start-up runs,
  and a new class's first allocation touched its page. The slow paths (alloc_slow, free_slow, resize_held) take
  KernelPages again, and the fast paths keep OwnPages. 10 -> 9 (bound 9), with next's lists and with regenerated ones.
- **Lists** (dedbe2223): `tools/hottext/regen.sh . --both --allow-drop`. The first regen (before the heap fix)
  dropped iconv::codec_named (fast) plus locale::publish_global, regex ThreadScratch::table, `__errno_location` (now
  the label of liella_machine_errno_location, which is listed) and nscache::passwd_from_files (release). The final
  regen dropped nothing.

## Results (kanon:~/w/kn1)
- lint0 (inherited 483a29c2a): clippy + static green.
- m3/m4 (probe jobs): s2_1_c_0 and sym___errno_location pass. heap_meta_products is 10 without the fix and 9 with
  it (both lists).
- lint1 (dedbe2223) and lint2 (cebb4c756, code head): clippy (workspace, all targets, all features) GREEN; `cargo
  xtask static` RED in `hottext` check_pinned. The regenerated lists add locale-tier keys on functions that are not
  #[inline(never)], and the check names one per run: lint1 `locale::name::candidates` (pinned in cebb4c756), lint2
  `text::builtin_charset` (crates/core/src/text.rs). hot-text.unpinned only shrinks (rulings #4997, #5024).
- NOT RUN (budget): the FULL gate, plants (handoff/TB-FOREIGN-WORDS-LOW-5-plants.txt, 4 plants, written, never run),
  and per-call counts (#4779). The fast paths did not change; the slow paths now carry the owned test (a few
  instructions per slab refill, not counted).

## Next step
1. List EVERY key check_pinned rejects in one pass (the new keys of dedbe2223's hot-text.list against next's: `git
   diff 3dc8d485a dedbe2223 -- crates/libc-so/hot-text.list`, each Rust key's function checked for
   #[inline(never)]), then pin them all in one commit (#[inline(never)] where a call is free, e.g. builtin_charset,
   or #[inline(always)] into a keyed caller and drop the key). If the regen keeps adding unpinnable keys, that is
   a RULING NEEDED for regen.sh (it should not emit keys static refuses).
2. Then on kanon:~/w/kn1: ~/work/TB-FOREIGN-WORDS-LOW-5/lint.sh (clippy, static, release `so`, fast `so`), the
   plants file through `cargo xtask plants` (RAM dir under /mnt/zram/claude/), and the FULL tier
   (`cargo xtask affected --run --base origin/next --tier full`) as the brief asks.
3. Optional: the foreign-loader probe (D-22.10 harness) for errno; not started.

## Not to redo
-1's 0x80-0x9b audit table; the registry/FOREIGN/OwnPages/asm design; -4's five-key analysis; the fault map above
(the slow-path copy, not metadata, held the extra fault).
