# TIMING-COV-SPAWN-HEAP256M-IVB (PENDING JOBS; no code change)

State: branch task/timing-cov-spawn-heap256m-ivb from origin/next cc39389c; this handoff and the report json are the
only commits. Build role keke:~/w/n1 (synced, tree 72fc81a7-clean).

## What was done
- Stage built on keke (job `keke:tcsh-ivb-stage`, rc 0): `cargo xtask bench --stage ~/work/TCSH/stage.tar --only
  cbench_process` (commit cc39389c, profile release, 61 products in bench-stage.manifest). Copied to
  nico:~/work/TIMING-COV-SPAWN-HEAP256M-IVB/stage.tar.
- Booked nico (Ivy Bridge E3-1220L v2): ops booking 74, 45 min (was "draining" when taken).
- Queued `nico:timing-cov-spawn-heap256m-ivb-repro-ivb` = `~/bin/timing <label> bash
  ~/work/TIMING-COV-SPAWN-HEAP256M-IVB/time.sh` (script copy: .scratch/TIMING-COV-SPAWN-HEAP256M-IVB/time.sh). At
  hand-back it was queue entry #162, position 2, behind timing:cfcr2-ivb (running 58 min) and
  timing:timing-cov-strstr-nonavx2-time.
- time.sh: unpacks the stage, runs `./xtask-bin bench --no-build --staged --counters --runs 5 --only cbench_process`
  3 times (tsv copies in out/run{1,2,3}/), then 3 x `perf stat` per side of the row alone
  (`CBENCH_ROWS=posix_spawn_true_heap256m`; Liella `cbench_process`, glibc `cbench_process.host-dynamic`), set A
  instructions:k,cycles:k,instructions:u,cycles:u,page-faults,context-switches; set B
  cycles,dTLB-load-misses,dTLB-store-misses,iTLB-load-misses (out/perf{A,B}-<side>-<n>.csv).

## Collect
`ssh claude@nico 'tail -40 ~/logs/timing-cov-spawn-heap256m-ivb-repro-ivb.log; ls ~/work/TIMING-COV-SPAWN-HEAP256M-IVB/out'`
then cancel booking 74 (`ops timing-cancel 74`) if it is still open.

## Decision rule (from the brief)
- All 3 runs posix_spawn_true_heap256m time ratio <= 1.05: `DO NOT LAND:` close the row as noise of the 1105 sweep;
  the next sweep re-times it.
- Otherwise: compare the perf sets. Only a kernel-side gap (instructions:k / cycles:k / dTLB misses higher on Liella)
  supports hypothesis (1) and leads to code: one cached child stack per process with an in-use guard for concurrent
  spawns, plus a two-thread concurrent-spawn test (§18.4 clause) and the two plants in the brief.

## Informational (code read, not measured)
- `spawn_shared` (crates/boundary/src/spawn.rs:272-292) maps and unmaps TWO regions per spawn: the 128 KiB child stack
  (`CHILD_STACK`) and a one-page `MAP_SHARED` region. If (1) holds, both are candidates for caching; the shared page
  cannot be reused across concurrent spawns either.
- The host has THP `madvise`; how each malloc maps the 256 MiB (huge pages or not) is the one input that differs.

## Not to redo
The Zen 1 counts in the brief (kernel side equal on keke). The stage (reuse nico's stage.tar for any rerun; new label).
