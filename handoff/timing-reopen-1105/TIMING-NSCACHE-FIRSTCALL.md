# TIMING-NSCACHE-FIRSTCALL: the cache's first getpwnam_r costs more than no cache on Ivy Bridge

Id: TIMING-NSCACHE-FIRSTCALL
Kind: code
Priority: 65
Size: M
Host: -
Stacks-on: -
Model: opus
Effort: high
BUDGET: 70 calls / 140k

## Rows (Ivy Bridge bench budget failure, TIMING-SWEEP-1105 bench.log)
cbench_nscache first_getpwnam_r_cached 290 us and first_getpwnam_r_create 286 us vs first_getpwnam_r_uncached 273 us.
Liella-only bench (no glibc build), so no glibc ratio.

## Profile evidence (TIMING-REOPEN-1105, booked Ivy Bridge host)
- CBENCH_ROWS does not separate these rows: each filtered run printed first_getpwnam_r_cached and
  second_getpwnam_r_cached (295.4 / 290.4 / 299.9 us first, 184.9-188.2 us second), so per-row stall attribution is
  NOT VERIFIED. The first calls run in forked children (fork::child_reset in the top samples).
- Whole process (user mode): 24.7M cycles for 9.7M instructions (IPC 0.39); undelivered front-end slots 35M
  (~8.8M cycles, about 35% of all cycles); MITE uops 5.9M; load-pending stalls 4.0M cycles. Kernel samples 22-28%.
  The first-call cost is cold-start work: instruction-cache/iTLB misses on first-touch code and page faults.

## Hypothesis
The cached path's first call touches more cold code and pages (opening/mapping the cache, validation, then the
lookup) than the uncached parse; on this host that start-up cost exceeds the parse it saves. Unmeasured: page-fault
and syscall counts per path.

## The change
1. Measure per path: page-faults, syscalls (strace -c on the child, or perf trace -s), iTLB/i-cache misses
   (icache.misses, itlb_misses.walk_completed), in a probe that runs exactly one first call per process per path.
2. Cut the first-call path's extra faults/syscalls (e.g. read instead of map for one lookup, fewer distinct code
   pages on the first call, no validation work that the uncached path skips). Keep the steady (second-call) gain.

## JOBS
gate (clippy, static, affected --base origin/next); plants via `cargo xtask plants` on the changed cache code
(stale cache accepted, wrong record returned); syscall/fault counts before/after as the regression gate (deterministic
counters); booked timing check of the three first_* rows on the Ivy Bridge timing host
(`ops timing-book nico <min> --story <ID>`): cached and create must not exceed uncached.
