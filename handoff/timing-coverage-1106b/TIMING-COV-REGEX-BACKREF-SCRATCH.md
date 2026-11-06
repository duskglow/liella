# TIMING-COV-REGEX-BACKREF-SCRATCH: regexec with a back-reference touches far more memory per call than glibc

Id: TIMING-COV-REGEX-BACKREF-SCRATCH
Kind: code
Priority: 50
Size: M
Host: -
Model: opus
Effort: high
BUDGET: 70 tool calls / 140k

## The change

`cbench_patterns/regexec_backref` (`\(b[a-z]*\)@\1` on the 13-byte subject `xx bob@bob yy`, 2 submatches) is still
slower than glibc in time on Ivy Bridge (1.181; instr 1.059) after REGEX-BACKREF-FIXED-2 (landed 11-04, before the sweep:
ring 16 -> 8). The instruction gap is 4%; the time gap is bigger, and the counters below point at memory traffic, not
instruction count. Cut the per-call memory the back-reference matcher sets up and touches, so that a short subject
touches a few cache lines. Keep matching behaviour unchanged.

## Rows

| row | instr IVB | time IVB | instr N100 | time N100 |
|---|---|---|---|---|
| cbench_patterns/regexec_backref | 1.059 | 1.181 | 1.044 | 1.012 |

## Evidence (TIMING-COVERAGE-1106B, keke = Zen 1, next c101c4c6, job keke:tc1106b-profile; raw: profile-keke-c101c4c6.txt)

`cargo xtask bench --compare --counters --runs 3 cbench_patterns`: glibc 3,509 ns, Liella 3,672 ns (1.05 on Zen 1),
instr/op 22,938 vs 21,960 (1.04). `perf stat` of the staged products, `CBENCH_ROWS=regexec_backref` (140,000 calls),
minus `CBENCH_ROWS=none`, per call: L1-dcache-load-misses Liella about 35, glibc about 1.1; branch-misses Liella about
25, glibc about 8 (the glibc `none` baseline is large because the program's setup runs outside the rows; recheck the
subtraction with a run of a neighbouring row). Liella `perf record -e instructions:u` by symbol: `Matcher::run` closure
33.8%, `insert` 14.4%, `attempt` 12.7%, `walk` 6.2%, `key_hash` 3.8%, `internal_move` 3.7%, `succs` 3.3%, memset 1.9%,
`copy_avx2` 1.9%, `fill_avx2` 1.7%, `Frontier::{first_bucket, prepare, add_thread, index}` 6.2%. cycles:u gives the same
order (`attempt` 14.5%). The memset/fill/copy share (about 6% of instructions) plus 35 L1 misses per call say the
per-call setup (frontier ring of `BR_RING` = 8 frontiers, thread slots, capture blocks, the `key_hash` table) clears or
copies more memory than a 13-byte subject needs.

Decision rows checked: D-14.15 (capture pass without back-references: one bucket per state; this row has a
back-reference, so it takes the other path). PERF-REGEXEC-BACKREF (saturation register section 15, row 6) refuted
"re-match per start"; REGEX-BACKREF-FIXED-2 cut the ring. Neither sized the per-call scratch by the subject length.

## Hypothesis

The back-reference matcher clears and lays out its scratch for the program's worst case on every call (all states x
ring frontiers x capture slots), so a short subject pays for zeroing and touching lines it never uses. Size and clear
lazily: clear a frontier when it is first used, and size the hash table and slot pool from the
program and subject (min of states and subject length + 1). Keep the scratch in the thread cache (`ThreadScratch`)
already used, so warm calls reuse hot lines. First step: count the bytes cleared and the lines touched per call
(instrument a scratch build, not the product) to confirm before changing code.

## Tests / plants

Regex tests stay green (crates/core regex tests, conformance regex programs, the sed corpus). Add tests for
back-references on subjects that grow the frontier past the initial size (lazy growth path) and for repeated calls
with different subject lengths on one compiled pattern (stale scratch). Name the §14.9 clauses. Plants through
`cargo xtask plants` with msg:<regex>: skip the lazy clear of a reused frontier, size the pool one short, keep a stale
hash entry between calls.

## JOBS

gate (direct tier, base origin/next), plants, counts before/after (`cargo xtask bench --compare --counters --runs 3
cbench_patterns`, plus `perf stat -e instructions:u,L1-dcache-load-misses:u,branch-misses:u` with
`CBENCH_ROWS=regexec_backref`; no other cbench_patterns row slower), and a booked timing check on Ivy Bridge:
`nico:<story>-timing-ivb` = `~/bin/timing <label> cargo xtask bench --compare --no-build --runs 5 cbench_patterns`.

First line: COMPLETE:, PENDING JOBS:, INCOMPLETE: or DO NOT LAND:
