# ENOMEM-REALPATH-INTERMITTENT-3: handoff (heap statistics ruled out as the low reading; stress test added)

Branch task/enomem-realpath-intermittent-3, cut from origin/next d1a7f653 (not stacked on -2: -2's guard lands on its
own, ruling #3147). Role mei:~/w/s1 (Skylake-SP, 48 threads, 2 NUMA nodes). Kept in its own file so it does not
conflict with -2's handoff/ENOMEM-REALPATH-INTERMITTENT.md when both land; this one supersedes it for the next step.

## Question (coordinator 11-05)
Every sighting ends at 6832 (6800->6832, 5808->6832): can `mallinfo2().uordblks` read LOW (a block live for the whole
call counted as free) while other threads allocate and free across 2 NUMA nodes?

## Code read (crates/rt/src/heap.rs, by range)
- `stats` (about line 2152) scans every slab record of every segment; `in_use_bits` counts `local & !remote`, reading
  `any`, then each remote word, then its local word. `collect` clears local bits before remote bits; `free_remote`
  sets `any`, pins, then the remote bit after checking the local bit; `release_slab` clears tail bits only with no
  live block (and the per-slab `saturating_sub` keeps that at 0). Large blocks: `large_mapped - large_kept` under the
  large lock (and >= 64 KiB, so not the 32/1024-byte deltas).
- The only transient low reading the code allows: a block remotely freed, collected and REALLOCATED by its owner
  between the reader's remote and local loads reads free. That block was allocated during the call, which `stats`'
  contract (D-3.4) lets count either way. No path found where a block live for the whole call reads free.

## Test added (crates/boundary/tests/heap_threads.rs)
`statistics_never_read_below_the_blocks_live_throughout`: 16 workers pinned across all CPUs hold blocks in the slabs
where they churn (local frees, remote frees through inboxes, collection, reuse; sizes 24..4000). The sampler reads
ALLOCATED (bumped after malloc returns), then uordblks, then FREEING (bumped before free starts): base + ALLOCATED -
FREEING is a lower bound on the bytes live throughout the call. 100000 samples per run; the sweep line reports how
many samples came within one 32-byte block of the bound (the test's power). Under `export` the bound allows 64 KiB of
harness residue (as the file's first test does), so the power is in the non-export pass.

## Evidence (mei, load 34-39 during the loops; diagnostic, not on JOBS)
- statloop1 (loose floor, superseded): 0/100 non-export.
- statloop2 (tight bound, final test): non-export 0/100 runs = 10M samples below the bound; per run 784..3562 samples
  within one block of the bound, least slack exactly 0 (the bound is reached, so a single 16- or 32-byte undercount
  would fail). Export pass: no failure before it was stopped at hand-back (run count not logged; no power anyway: least slack ~102000).
- Plants (cargo xtask plants): one block low per slab; a live neighbour of an uncollected remote free read as free
  (only while remote frees are pending): 2 CAUGHT, 0 SURVIVED, 0 WARN (job enomem-realpath-intermittent-3-plants2).
- Gate: enomem-realpath-intermittent-3-gate (direct tier, --base origin/next) rc 0. Superseded: -plants (rc 2: a `|` in
  plant 2 split its fields; baseline refused, no plant applied). clippy --all-features -D warnings and xtask static clean.
- Conclusion: the statistics do not read low under concurrent churn on 2 nodes; mallinfo2 callers outside tests see
  no undercount of blocks live through their call. No product change.

## New sighting
search_nomem `hcreate_and_hsearch_fail_with_the_table_intact` (CI 11-05 19:23, kanon, log overwritten) does NOT read
uordblks: it runs in a forked child under RLIMIT_AS with ballast (`limit_now`). A different mechanism (address-space
headroom); nothing to compare without its failure message.

## Next step
1. Land -2's guard and this test. On the next enomem_fs_walk sighting, read the summary's baseline (5680 realpath /
   27552 scandir on -2's build): if `before` equals the baseline, the AFTER is high, i.e. the failure path left one
   block on a rare path (the failed call's own nondeterministic inputs: the scratch tree, getcwd/readlink results),
   not the counter.
2. search_nomem: capture its failure text on the next sighting (keep that CI log) before diagnosing.

Not to redo: the heap statistics question (above), -2's loops (mei 7100 runs, kanon 2300). Plan rows: D-3.4, D-3.10,
D-3.11, D-3.15, D-14.3.
