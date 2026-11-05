# ENOMEM-REALPATH-INTERMITTENT: handoff (WIP, cause not yet proven)

Branch task/enomem-realpath-intermittent, cut from origin/next dc65f761. No code change yet: this commit is the handoff only.
Role: kanon:~/w/kn1 (dispatcher). A repro loop also runs on keke (2 NUMA nodes) in ~/work/ENOMEM-REALPATH-INTERMITTENT/tree
(my scratch copy, not a role dir), because kanon has 1 NUMA node and the failures were all seen on keke.

## Established (read from code and the keke log)
- The failing run is the gate's SECOND workspace pass: `cargo xtask test` runs nextest twice; the 2nd (keke n3 affected.log
  line 6373 "Starting 4909 tests", fail at 6446) has features `liella-boundary/export,liella-boundary/selftest`, so the
  exports are interposed (xtask/src/main.rs ~233). The pass at line 596 is the 1st pass (no exports).
- nextest runs one test per process ("running 1 test ... 7 filtered out"), so no other test of the file shares the process;
  the process has libtest's main thread plus the test thread (thread 'enomem_realpath_null').
- The check that fires is the `mallinfo2().uordblks` one in `sweep` (crates/core/tests/enomem_fs_walk.rs ~107). The
  `failinject::live_bytes` check before it passed. `live_bytes` is PER THREAD (thread_local LIVE in
  crates/boundary/src/selftest/failinject.rs). `uordblks` is PROCESS-WIDE: in the exports pass `System` = Liella's malloc,
  so EVERY thread's Rust allocations count in it. That is the only check that another thread's allocation (or the heap's
  own bookkeeping) can move. That makes a measurement problem likely, but it is NOT proven.
- Heap stats (crates/rt/src/heap.rs `stats`, `in_use_bits`) already count an uncollected remote free as free, so
  "a block freed from another NUMA node and not yet collected" is not by itself a +32.
- The failure is at allocation 1, the first measured window after the warm-up run.

## Repetition results (exports-interposed binary, `--exact enomem_realpath_null --nocapture`, one process per run)
- kanon (Xeon E5-2699 v4, 1 NUMA node), unloaded: 0 of 300 (job erpi-loop0).
- kanon, 70 busy-loop burners on 35 CPUs: 0 of 2000 (job erpi-loop1).
  Either load alone does not trigger it, or it needs 2 NUMA nodes (keke, mei). The sibling story REGEX-ENOMEM-INTERMITTENT
  found the heap's per-node shard choice (D-3.10/D-3.12) behind a keke/mei-only flake. That is a candidate here, not proven.
- keke: job erpi-kdbg QUEUED (detached): debug variant of the test, 2000 runs unloaded (d0) then 3000 with 90 burners (d1).
  Re-attach: `ssh claude@10.0.100.53 'tail -4 ~/logs/erpi-kdbg.log; cat ~/work/ENOMEM-REALPATH-INTERMITTENT/d?/fails.txt'`.
  The debug variant only adds output on a failure: CPU before/after the call (/proc/thread-self/stat field 39), thread
  count, `uordblks` again after 100 ms, and a retry of the same n with its before/after. How to read it:
  - retry shows the +32 again -> a real deterministic leak on that path (fix in realpath);
  - CPU on another node across the window and no recurrence -> heap shard effect (product question, see below);
  - same CPU, no recurrence, value stays +32 after 100 ms -> another thread (libtest main) allocated in the window.
  Sources: loop.sh and dbg_enomem_fs_walk.rs in my scratch and on keke under ~/work/ENOMEM-REALPATH-INTERMITTENT/.

## Not done (next agent)
1. Read erpi-kdbg's result. If it is 0 failures, add a forced migration across nodes (taskset per node inside the
   window) to test the shard hypothesis directly.
2. Fix the proven cause. If it is a measurement problem: the C-heap check must see only this thread's effect. Options are a
   per-thread measure, or re-checking the same n and requiring a recurrence. Either way, keep the check able to catch a
   deterministic partial-result leak. Add a comment saying why.
3. Run the loop many times on keke under load (state N), then the direct-tier gate, then `cargo xtask plants` with a
   planted partial-result leak in realpath (msg:`a partial result stays allocated`).

Plan rows: D-3.10, D-3.12 (heap shards), D-4.13 (exports-interposed pass).
