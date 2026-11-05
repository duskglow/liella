# ENOMEM-REALPATH-INTERMITTENT(-2): handoff (cause NOT proven; test hardened)

Branch task/enomem-realpath-intermittent-2 (stacks on task/enomem-realpath-intermittent), cut from origin/next dc65f761.
Role mei:~/w/s4 (2 NUMA nodes). Commits of -2: 2c2eb54c (measure guard), 3440d515 (baseline in the summary), this handoff.

## The failure (unchanged facts)
- `enomem_fs_walk.rs` `sweep`: the process-wide `mallinfo2().uordblks` check after a reported failure; per-thread
  `live_bytes` and `caller_blocks` passed. Only in the exports-interposed nextest pass, always at allocation 1 (the first
  measured window after the warm-up), at the START of the pass (keke n3 affected.log line 6446: test 72 of 4909).
- Sightings: realpath 6800 -> 6832 (n3); coordinator: realpath and scandir 5808 -> 6832 (PARITY-MEMSET-SMALL-AVX2-2, keke;
  the grep of ~/logs/parity-memset-small-avx2-2-gate.log found no failure line, the affected.log it used was not found:
  NOT VERIFIED). **The AFTER value is 6832 in every sighting**: the baseline (`before`) may be what read low, not the window
  that grew.

## What -2 did
1. Test-only counter (selftest feature, no product file): `FailAlloc` counts every thread's host-allocator calls when they
   begin and when they return (`failinject::others_begun` / `others_done`). `sweep` reads `others_done` before `before` and
   `others_begun` after `after`; if they differ another thread's allocator call overlapped the window, and the same n is
   measured again instead of being judged (bounded: 1000 windows, then a failure). This closes the "libtest's main
   thread allocates inside the window" mechanism. It is NOT proven to be the cause (below).
2. The sweep summary prints the re-measured windows (and how many had the heap moved) and the C heap in use before the
   first window.

## Evidence (mei, Skylake-SP 48 threads, 2 nodes, host load 17-40; all with the -2 test, which still fails on any
   undisturbed mismatch)
- erpi-kdbg on keke (the previous run's debug loop) was still QUEUED at 18:4x: not read.
- loop1 (sequential, one process per run): realpath 0/2000, scandir 0/1000; 0 runs with an overlapped window.
- burst1 (32 processes started at once, x100, both tests): 0/3200; 0 overlapped windows.
- loop2: realpath 0/600, scandir 0/300; baseline before the first window 5680 (realpath, 600 of 600) and 27552 (scandir,
  300 of 300): deterministic on this build.
- So: the failure did not reproduce on mei in 7100 runs, and another thread's allocator traffic never overlapped a window.
  The main-thread hypothesis is unsupported; the guard costs nothing and stays as a hardening.

## Next step (in order)
1. Reproduce in the real configuration: the exports-interposed nextest pass of `-p liella` (full-pass concurrency) in a
   loop on keke or mei; on a failure compare `before` with the baseline the passing runs print (now in the summary line).
2. If `before` is the low one: read the heap's stats path (crates/rt/src/heap.rs `stats` / `in_use_bits`, shards D-3.10 /
   D-3.12) for a state in which in-use bytes read low (a block between a thread cache and a shard, a remote free being
   collected, a shard not yet visible to the reader). That would be a product defect in mallinfo2, fixed there.
3. Plants for any product fix via `cargo xtask plants` (the plants file of -2 is in mei:~/work/enomem-realpath-intermittent-2/plants.txt).

Not to redo: kanon loops (0/2300, 1 node), mei loops above. Plan rows: D-3.10, D-3.12, D-4.13.
Jobs (mei): enomem-realpath-intermittent-2-plants (3/3 CAUGHT), enomem-realpath-intermittent-2-gate (direct tier);
diagnostic only: -loop1, -burst1, -loop2.
