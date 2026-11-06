# TIMING-N100-MUTEX-MT handoff (continued by TIMING-N100-MUTEX-MT-2)

State: branch task/timing-n100-mutex-mt-2 (stacked on task/timing-n100-mutex-mt), code 369c1872, HEAD = the commit
adding this file. Base origin/next 730f416b. Role shiki:~/w/sh1 (the -1 run used keke:~/w/n3).

## Run -2 (2026-11-06)
1. The -1 plants job (keke TIMING-N100-MUTEX-MT-plants, rc 1): `boundary-wake-dropped` SURVIVED. Cause: no test
   reached the export's deferred wake (`mutex_unlock_slow(.., wake=true)` in crates/boundary/src/sync.rs): s22_sync
   never had a waiter asleep in `pthread_mutex_lock` at the unlock. Fix: s22_sync gains `wakes_sleeper` (normal,
   error-checking, recursive; returns 31/32/33): the waiter publishes its TID and blocks; main waits until
   /proc/self/task/<tid>/stat shows state S (asleep in the kernel), then unlocks; join with a 60 s hang guard. A dropped
   wake now fails every run (plant CAUGHT, rc 1).
2. Lock fast path split by type BEFORE the acquire (`Mutex::lock_fast`, helper `acquire_free`): normal/error-checking/
   adaptive path stores only the owner; recursive path stores owner and `__count` = 1 and keeps the relock. Nothing is
   tested after the CAS. Unlock (`unlock_fast_deferred`) tests the TID before comparing the owner (the Option compare
   had become sete + re-test).
   Disassembly (shiki, `__liella_libc___pthread_mutex_lock`): entry `cmp $1; je rec; test; je plain; cmp $4; jae slow`.
   The normal path still pays 4 type-test instructions (LLVM put the recursive compare first); recursive pays 2.
3. Decision row D-22.14 (placeholder, no id reserved) in plan/07-decisions/s22-threads.md: `__count` kept by recursive
   mutexes only (probe-settled, matches glibc).

## Results (final code 369c1872)
- Counts (Skylake-SP host, `cargo xtask bench --compare --counters cbench_hot`, job timing-n100-mutex-mt-2-counts2,
  same host and run; instructions per call Liella, glibc in brackets):
  normal 64.0 -> 61.0 (72.0); recursive 83.0 -> 79.0 (90.1); normal_mt 63.1 -> 61.1 (74.2); recursive_mt 82.1 -> 79.1 (93.2).
  Every row lost instructions (the -1 head had recursive 86.0 / 85.2).
- Plants (shiki timing-n100-mutex-mt-2-plants, `cargo xtask plants`, 7 plants): 7 CAUGHT, 0 SURVIVED, 0 WARN.
  rt-wake-dropped, rt-waiter-mark-missed and rt-general-count-always caught by the 600 s timeout (the last one was
  rc 101 with its message in the -1 run: a failing assertion inside a contended thread can leave the test waiting).
- Lint (timing-n100-mutex-mt-2-lint): cargo xtask clippy + static clean on 369c1872.
- Gate: timing-n100-mutex-mt-2-gate (shiki, direct tier, --base origin/next): rc 0; 6 suites, 852 passed / 0 failed /
  92 ignored; conformance 16 passed; peak 2.6G. Final clippy (--all-features) + static: timing-n100-mutex-mt-2-lint2 rc 0.
- Timing: staged trees before.tar (origin/next's pthread_sync.rs and sync.rs over the branch tree; manifest commit
  reads 369c1872 because the role's .synced head names the branch) and after.tar; 3 interleaved rounds of the 4 rows
  under `~/bin/timing`: nozomi (N100) timing-n100-mutex-mt-2-n100, nico (Ivy Bridge) timing-n100-mutex-mt-2-ivb;
  bookings 62 (nozomi) and 63 (nico). First attempts (-n100, -ivb) rc 1 in round 2: a bench run rewrites
  target/liella/bench/bench_times.tsv in the staged tree and the manifest check then refuses it; run.sh now
  re-extracts the tree before every round (labels -n100b, -ivbb, rc 0).
  Liella/glibc time, median of the 3 rounds, before -> after:
  N100:       normal 0.904 -> 0.667; recursive 0.714 -> 0.616; normal_mt 1.215 -> 0.985; recursive_mt 0.847 -> 0.707.
  Ivy Bridge: normal 0.953 -> 0.951; recursive 0.813 -> 0.782; normal_mt 0.999 -> 0.820; recursive_mt 0.959 -> 0.960
  (rounds 0.958-0.960 before, 0.957-0.962 after: parity). No row slower on either class.

## Open / informational
- `__nusers` (out of scope): glibc reports 1 while a mutex is held, Liella 0. A program sees it only by reading the
  private `__data.__nusers` field: no public interface reports it (pthread_mutex_* return values, trylock, timedlock,
  the cond wait paths do not expose it). Not changed.

## Exact next step
None required: the branch lands (COMPLETE). Optional: the normal lock path's 2 extra type-test instructions (cmp $1/je before the plain test) are the next candidate if the
N100 normal rows need more.

## Do not redo
The -1 annotation and probe (hash b2d4f23094db); the before/after staged tars in ~/work/TIMING-N100-MUTEX-MT-2/ on
shiki, nozomi and nico.
