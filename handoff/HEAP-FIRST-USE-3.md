# HEAP-FIRST-USE-3: continue HEAP-FIRST-USE-2 (D-3.20, #4170, the first-use cuts)

State: branch `task/heap-first-use-3` (= HEAP-FIRST-USE-2's commits on top of landed HEAP-FIRST-USE + this run),
role mei:~/w/s3. Code commit b22a125c9, plants file 0f996e342, then this handoff/report commit.
Status: INCOMPLETE. One direct test is red (below); do not land as it stands.

## Decisions in force
D-3.20 (`records/decisions/D-3/D-3.20.toml`, amends D-3.12; this chain's own record, not landed): allowed-node set
asked once on the first slow path; one node never reads the node; several nodes turn NUMA on. This run added the
#4170 paragraph and `crates/core/tests/heap_meta_products.rs` to its `tests`. D-3.19 unchanged. Never commit the
generated views (`plan/07-decisions/*.md`).

## What this run found and did
1. HEAP-FIRST-USE-2's gate `hfu2-gate` (FULL) was red on one test, deterministic, reproduced here (`hfu3-step0`):
   `hot_text_products init_first_mktime_after_localtime_r_takes_no_fault` (1 fault). Cause: D-3.20's one-time
   question was inlined into `Heap::first_cache` (+867 bytes) and `Heap::refill` (+896 bytes) of hot text, which,
   with inlining shifts elsewhere, moved `days_from_civil64` from 0x44f600 (origin/next build, test green there:
   `hfu3-nextcheck`) to 0x4508c0, past a 64 KiB fault-around window. Fix: `Heap::decide_topology`, `#[cold]
   #[inline(never)]`; sizes now 0x788 / 0x102c (next 0x758 / 0xfec) and `days_from_civil64` at 0x44f8c0; test green.
2. #4170 (ruling #4197): the trigger is `Vdso::lookup_many`'s name compare (slice `==` lowers to `bcmp`, i.e. the
   exported compare entry `hot_equal_rest`, which takes the folding decision). Stack from gdb on the static probe:
   resolve_fold_slow <- hot_equal_rest <- Vdso::lookup_many <- sys::vdso_fn <- Heap::first_cache. Fix:
   `vdso::same_bytes` (byte loop) for the symbol and version names; the guard is unchanged. Verified on mei (2 allowed
   nodes): the first malloc no longer opens mmap_min_addr (strace), `lookup_many` calls no compare entry (objdump).
   Test: `heap_meta_products the_vdso_lookup_takes_no_folding_decision` (oracle `tests/oracle/vdso_lookup_reads.c`:
   the process's own syscr from /proc/self/io around the first clock_gettime; skips without task I/O accounting).

## RED, open: `hot_text_products init_first_string_calls_after_printf_take_no_text_fault` (1 fault)
With the decision no longer taken by the heap, the strings oracle's first `memchr` (after printf) runs before the
decision and takes the checked path `__liella_libc_liella_hot_memchr_rest` (0x675f80, outside the hot group): one
text fault (perf, `hfu3-verify2`). HEAP-FIRST-USE-2 alone has the same effect on one-node hosts (no vDSO lookup
on the first malloc there), so it was a latent red. `tools/hottext/regen.sh . --build` in both profiles
(`hfu3-regen`) did NOT add the key (list 665 -> 671 keys, test still red); that list change was discarded.
Next step, in order:
1. Find why regen's probes miss `hot_memchr_rest` (its p_str probe likely takes the decision earlier); try
   `regen.sh . --build './target/test-oracles/page_faults_liella_strings strings'` (extra-program form), both
   profiles, then commit the regenerated list (it is the tool's output, not a hand edit).
2. If the key cannot be grouped, RULING NEEDED: where the folding decision should be taken now that malloc no longer
   takes it (recommendation: keep it lazy; group the checked rest paths of memchr/compare/copy with the hot text).
   Never loosen the 0-fault bound (ALWAYS FORWARD).
3. Then: clippy + static on the head, `affected --run --base origin/next` (it escalates to FULL on `records/`
   paths), counts (first malloc / first_setlocale_en_US: HEAP-FIRST-USE-2's `~/work/HEAP-FIRST-USE-2/counts.sh`
   pattern; base `~/work/HEAP-FIRST-USE/after2/counts.txt`), booked Ivy Bridge + N100 timing of the first_* rows.

## Results (mei)
- `hfu3-step0` (inherited head): clippy rc 0, static rc 0, so ok, hot_text 11/12 (mktime red), rt 27 ok (job rc 101).
- `hfu3-verify1` (vDSO fix): clippy, static, records check ok; kernel_types 10 ok; heap_meta_products 3 ok
  (`reads 0`); hot_text 10/12 (mktime + strings red).
- `hfu3-verify2` (+ cold decision, head b22a125c9): rt heap_and_sync 27 ok, heap_meta_products 3 ok, hot_text 11/12
  (strings red). No clippy/static run after the `decide_topology` edit (it adds no unsafe; NOT VERIFIED).
- `hfu3-plants` (`handoff/HEAP-FIRST-USE-3-plants.txt`, 6 plants incl. the 3 of HEAP-FIRST-USE-2): running at
  hand-back (`~/bin/jobwait hfu3-plants` on mei).
- Counts and timing: not run this time (head red).

## Not to redo
The origin/next comparison (`hfu3-nextcheck`: 12/12 green, symbol map above); the gdb stack of #4170.
#4198 (rseq node_id) is HEAP-RSEQ-NODE, not this chain.
