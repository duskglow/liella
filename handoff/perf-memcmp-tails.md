# PERF-MEMCMP-TAILS (handoff)

State: branch `task/perf-memcmp-tails` (from origin/next 361836f7), code commit 97494cbc, plants-anchor commit on top;
HEAD in `handoff/reports/PERF-MEMCMP-TAILS.json`. Role keke:~/w/n3 (Zen 1 host).

## Change
csys/src/machine/fold.S: the AVX2 65-128 and 129-2048 classes of `liella_machine_compare` (memcmp) and
`liella_machine_equal` (bcmp, __memcmpeq) lose their four explicit NULL tests (ruling #2517 (coordinator, 2026-11-04) as recorded in
handoff/memcmpeq-early.md: NULL folding to 2048) and their second class word: one compare against
`liella_machine_cmp_long` gates 65-2048, the 32-byte head block runs next, then `n > 128` picks the long class.
crates/boundary/src/dispatch.rs: `cmp_long` is now 2048 only while folding AND AVX2 are on (`store_cmp_gates`,
dropped first, raised last); with folding off those sizes take the SSE2 class / checked entries (S6 abort as before).
`cmp_mid` stays the AVX2 flag word of `.Lcmp_sse_long`. Test: small_compare NULL test expects SIGSEGV for 65-2048 in
the folded AVX2 build only (2048, 2049 added). Decision row `D-4.52` in plan/07-decisions/s04.

## Per-phase split (static count from the listing, instructions from entry to ret; bench overhead about 8.4)
| path | dispatch to class | class gate | NULL | head block | size test | locate + order | total before -> after |
|---|---:|---:|---:|---:|---:|---:|---:|
| memcmp diff at byte 8, 65-128 | 6 | 2 | 4 -> 0 | 5 | - | 8 | 25 -> 21 |
| __memcmpeq head diff, 65-128 / 129-2048 | 6 | 2 / 4 -> 2 | 4 -> 0 | 5 | - | 3 | 20 / 22 -> 16 |
| memcmp / bcmp equal 65-128 | 6 | 2 | 4 -> 0 | 5 | 0 -> 2 | blocks | -2 |
| 129-2048 any shape | 6 | 4 -> 2 | 4 -> 0 | 5 | 2 | ... | -6 |
glibc's totals from the counters (row minus overhead): diff8 about 16, head rows about 12-16. The surplus left after
this change is the shared entry dispatch (6 instructions to reach the >64 classes, a structure that favours 8-64)
and the -1/1 ordering with the D-4.21 re-read check (6): neither can shrink without slowing 8-64 rows or changing
D-4.21/D-4.23.

## Counts (keke, `cargo xtask bench --compare --counters --runs 3 cbench_memref`, instructions per call, glibc)
Before (perf-memcmp-tails-count-before, rc 0): diff8_64/96/127 30.4/33.4/33.4 (24.4); __memcmpeq_head_128 28.5 (20.4),
head_1k 30.4 (20.4); bcmp_head_128 28.4 (24.5), bcmp_head_1k 30.4 (24.4); memcmp_128 38.4 (39.4), _144 47.5 (57.5),
_256 58.4 (57.5), _4k 671.2 (576.3), _64k 9323.7 (8750.1); __memcmpeq_tail_32 31.3 (20.4), bcmp_tail_32 31.4 (24.4).
After: job perf-memcmp-tails-count-after (keke) waits for the gate, then counts; NOT READ at hand-back. Expected from the
static count: diff8_96/127 29.4, head_128 24.5, head_1k 24.4, memcmp_128 36.4, _144 41.5, _256 52.4, _4k 669.2.
Plants: perf-memcmp-tails-plants2, 5 CAUGHT (rc 0). Gate perf-memcmp-tails-gate running at hand-back. clippy rc 0 and
static (no failure lines) on the role before the gate. Superseded: perf-memcmp-tails-plants (tool refused unanchored .S plants).

## Not done / next step
* tail_32 rows (17-32 SSE2 class, 1.29-1.54): the head probe + two size tests cost ~7 over glibc; reordering the
  16/32 tests before the head probe slows every 17-32 head-difference row (+10). Not changed.
* memcmp_4k/64k (Rust `liella_hot_compare_big` above 2048): loop cost, -2 only from the merged gate here.
* percall (validation/wl_percall.sh) was NOT run in this story (budget); the per-phase split above is static.
* brief targets not met: diff8_96/127 and head rows stay above 1.05x (see the split for why).
