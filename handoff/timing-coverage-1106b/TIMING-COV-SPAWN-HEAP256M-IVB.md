# TIMING-COV-SPAWN-HEAP256M-IVB: posix_spawn from a 256 MiB parent, slower in time on Ivy Bridge only

Id: TIMING-COV-SPAWN-HEAP256M-IVB
Kind: code
Priority: 35
Size: S
Host: -
Model: opus
Effort: high
BUDGET: 70 tool calls / 140k

## The change

`cbench_process/posix_spawn_true_heap256m` (posix_spawn of /bin/true + waitpid after the parent allocated and touched
256 MiB) was 1.493 in time on Ivy Bridge in the 1105 sweep with instr:u 1.029 and cycles 1.05, and 0.999 on the N100.
On Zen 1 this story's run finds no gap at all (below). First reproduce it on the Ivy Bridge timing host with kernel
counters; fix only what the reproduction shows. If it does not reproduce (3 booked runs, ratio <= 1.05), hand back
`DO NOT LAND:` with the numbers and close the row as noise, so the next sweep re-times it.

## Rows

| row | instr IVB | time IVB | instr N100 | time N100 |
|---|---|---|---|---|
| cbench_process/posix_spawn_true_heap256m | 1.029 | 1.493 | 1.008 | 0.999 |

## Evidence (TIMING-COVERAGE-1106B, keke = Zen 1, next c101c4c6, job keke:tc1106b-profile; raw: profile-keke-c101c4c6.txt)

`cargo xtask bench --compare --counters --runs 3 cbench_process`: glibc 657,532 ns, Liella 653,722 ns (0.99), instr/op
91,109 vs 86,474 (1.05). `perf stat`, `CBENCH_ROWS=posix_spawn_true_heap256m` (350 spawns + setup), totals: instructions:k
633.3M vs 631.9M, cycles:k 859.6M vs 886.9M, page faults 84,844 vs 84,499, context switches 748 vs 742: the kernel side
is equal on Zen 1. The plain `posix_spawn_true` row is at parity too (0.98 time, 1.03 instr). Liella spawns with
`clone3(CLONE_VM | CLONE_VFORK | CLONE_CLEAR_SIGHAND)` on a fresh mmap'd child stack (crates/boundary/src/spawn.rs
`spawn_shared`, D-18.3; copy-on-write only as the `ENOSYS` fallback), so the parent's heap size should not enter the
cost; the 256 MiB is touched with `memset` before the row, and how it was mapped (Liella's malloc vs the host's: huge
pages or not, alignment) is the one input that differs between the two products in this row.

## Hypothesis

Two candidates, both to be checked on the Ivy Bridge host before code: (1) the per-spawn `mmap`/`munmap` of the
child stack in a 256 MiB address space costs more there (TLB flush on munmap; check with `perf stat -e
dTLB-load-misses,instructions:k` and a run with the stack kept for reuse in a scratch build); (2) the 1.49 is noise
from that sweep (50 iterations x 7 rounds per run; the row is last in the program and runs after 256 MiB of first
touch). Count first; only (1) leads to code (reuse one cached child stack per process, guarded for concurrent spawns
from several threads).

## Tests / plants

If code changes: spawn tests stay green (crates/boundary spawn tests, conformance posix_spawn programs); add a test of
concurrent spawns from two threads (the cached stack must not be shared) naming the §18.4 clause. Plants through
`cargo xtask plants`, msg:<regex>: drop the in-use guard of the cached stack, unmap it while a spawn uses it.

## JOBS

counts on a build host (gate host: `perf stat` as above), a booked reproduction on Ivy Bridge FIRST:
`nico:<story>-repro-ivb` = `~/bin/timing <label> sh -c 'cargo xtask bench --compare --no-build --counters --runs 5
cbench_process'` (3 runs). If code: gate (direct tier, base origin/next), plants, counts before/after, and a second
booked timing check `nico:<story>-timing-ivb`.

First line: COMPLETE:, PENDING JOBS:, INCOMPLETE: or DO NOT LAND:
