# TIMING-N100-MUTEX-MT: uncontended normal mutex lock/unlock after the first thread is slower in time on the N100

Kind: code
Model: opus
BUDGET: 80 calls / 140k

## Rows (sweep N100: instr / cycles / time, Liella / glibc)
cbench_hot/mutex_lock_unlock_normal_mt: 0.852 / 1.229 / 1.199 (67.8 vs 55.1 cycles per pair). Control: mutex_lock_unlock_normal (before any thread) 0.889 / 0.905 / 0.904, so only the multi-thread path is slow. Ivy Bridge time 1.000.

## Profile evidence (ratios, same run)
- cycles 1.222, instructions 0.861 (reproduces the sweep).
- Reorder-buffer-full slots: 73% of Liella slots vs 63% of glibc slots (1.41x in count).
- ld_head.l1_bound_at_ret (cycles a load sits at the retirement head, L1-bound): 1.80x (45% of Liella cycles vs 31%).
- topdown_be_bound.non_mem_scheduler: 4216 vs 54 units; topdown_fe_bound.decode 4.1x. Mispredicts: Liella 0.03x of glibc (not the cause).
- Liella samples: 61% pthread_mutex_unlock, 33% pthread_mutex_lock, 5% the bench loop.

## Hypothesis
On this core the cost is retirement blocked behind locked (serialising) read-modify-write operations: the ROB fills while a locked op waits at the head. Liella pays about 1.4x the ROB-full stall with fewer instructions, concentrated in unlock, which points at more locked or SeqCst operations per pair in the multi-thread path than the minimum (one RMW to take the lock, one RMW to release it and learn whether to wake), or a fence / SeqCst store in owner or count bookkeeping. Not verified at instruction level (no annotate on the timing host).

## The change
1. Annotate the Liella samples (method below) and list every locked instruction, xchg and fence on the lock+unlock pair of a PTHREAD_MUTEX_NORMAL mutex after pthread_create (crates/boundary/src/posix/s22_threads.rs:496 / :532 and what they call).
2. Reduce the pair to the minimum the futex protocol needs (one atomic RMW in lock, one in unlock); bookkeeping that is not part of the handoff gets Relaxed/Release ordering with a written argument why it stays correct. No change to the protocol's waiter detection: a missed wake is a hang, prove it with the existing contended mutex tests plus a plant that drops the wake.
3. A decision row in plan/07-decisions/ for any ordering change (the memory-ordering argument is the reviewable part).

## Profile method (TIMING-REOPEN-N100)
Sweep stage of next f2157504 (the TIMING-SWEEP-1105 products) on the booked N100 timing host (Alder Lake-N, Gracemont E-cores), perf 6.8, user mode, pinned to one CPU. perf stat -r 5, seven named-event sets, Liella static bench vs glibc host-dynamic bench (the sweep pairing). Per call = (row run minus a setup-only run with a non-matching CBENCH_ROWS) / 84000 calls; this reproduces the sweep per-call cycles within 1% (memcpy_144 14.18/13.12 vs sweep 14.3/13.3; memchr_64k 3565/3137 vs 3567/3160). Topdown events count slots (5 per cycle). glibc: aggregate counters only. Liella: cycle samples by symbol; perf annotate produced nothing (the timing host has no objdump), so instruction-level attribution is step 1 of each brief: copy rec.<row>.data and the staged bench binary from ~/work/TIMING-REOPEN-N100/out on the N100 timing host to a build host and annotate there (Liella only). Full per-call table: handoff/timing-reopen-n100/profile-n100-percall.txt.

## JOBS
- gate: `cargo xtask clippy`, `cargo xtask static`, then the direct-tier affected gate with `--base origin/next` (label TIMING-N100-MUTEX-MT-gate).
- plants: `cargo xtask plants` on the changed kernel, msg: regexes naming the expected failing test (label TIMING-N100-MUTEX-MT-plants).
- counts: `cargo xtask bench --compare --counters cbench_hot` before and after on an AVX2 build host and the Ivy Bridge-class path (labels TIMING-N100-MUTEX-MT-count-before / TIMING-N100-MUTEX-MT-count-after); regression gate: no row of the bench gains instructions.
- timing: booked N100 check of the rows above, Liella vs glibc, before and after (`<N100 host>:TIMING-N100-MUTEX-MT-n100`), and booked Ivy Bridge check of the same rows (`<IVB host>:TIMING-N100-MUTEX-MT-ivb`): an AVX2-path change must not slow the SSE2 class. Book with `ops timing-book <host> <minutes> --story TIMING-N100-MUTEX-MT`; cancel when done.
