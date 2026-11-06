# TIMING-NSCACHE-FIRSTCALL: first getpwnam_r with the cache vs without (handoff)

**State.** Branch `task/timing-nscache-firstcall` (from `origin/next` ff125cce). Code commits 707bd576 and
ab77ef60, both `bench/c/cbench_nscache.c` only, with no library change. The report commit is on top.
**Decisions in force.** D-16.10: a process's first cacheable lookup goes to the sources (no attach, no
mapping), and the second lookup attaches and validates. No new D-row: this is a bench-only change.

## Result
The brief's hypothesis (the cached first call does more cold work) is **refuted by counters**. A probe
(scratch `firstcall.c` + `measure.sh` on kanon kn1, a static Liella build) brackets one first `getpwnam_r` per
process with marker syscalls and per-call `getrusage`. On all three paths (region valid, region removed,
`LIELLA_NSCACHE=0`) the first call does the same work:
- syscalls: openat 3, read 4, mmap 5, munmap 2, close 3, newfstatat 1, prlimit64 1 (identical)
- minor faults: 36 / 37 / 37
- whole process at n=1 minus n=0 (perf stat): user instructions 36856 / 36857 / 36840. Kernel instructions,
  icache.misses and itlb walks are the same within run-to-run noise (kernel n=1: 650k / 658k / 655k).

So the cached and create rows can differ from uncached only because of how the bench measured them. The
bench had two biases. The uncached child's environment had one more variable (`LIELLA_NSCACHE=0`), which
lays out its stack differently. And each kind ran as one block of 40, with uncached last. The change: cached and
create children get `LIELLA_NSCACHE=1` (same size, and only "0" turns the cache off), and the three kinds take
turns in each of the 40 rounds (helper `fresh_rounds`). It is a helper rather than an inline loop because
xtask's bench-row mapper reads the statement just before a run of `put()` rows (benchrows
`every_cbench_row_is_mapped` failed with the inline loop).

## Measurements
- Gate on kanon kn1: `cargo xtask clippy` rc 0, `cargo xtask static` rc 0, and
  `cargo xtask affected --base origin/next --run` "affected: ok". The direct tier selects only the xtask unit
  tests, since the bench file is bench data.
- Timing (booked nico, Ivy Bridge, booking 37, cancelled after use): old and new binaries alternately,
  12 pairs, best-of-40 per row per run, medians:
  old cached 291.4 / create 290.6 / uncached 291.2 us;
  **new cached 289.3 / create 288.7 / uncached 291.1 us** (cached and create do not exceed uncached).
  An earlier window of 6 pairs gave old 298.4 / 295.8 / 292.7 and new 290.7 / 290.3 / 292.1 (medians).
- The sweep's 6% gap (290 vs 273) was **not** reproduced with the old bench in this window (1.00-1.02x).
  The 1.25x budget rows (bench/budgets.tsv 110-111) were never exceeded by these numbers.

## INFORMATIONAL
1. **The second lookup costs much more with the cache on: about 197 us vs 30 us uncached on nico.** That
   lookup attaches the region. Probe deltas for the second call: +157k kernel instructions, 15 vs 1 minor
   faults, and newfstatat 6, openat 2, mmap 1, credential calls 4. A process that makes 2 to about 40
   passwd lookups pays more with the cache than without it. Recommendation: a story to make attaching cheaper
   (fewer table pages touched at attach, fewer stats) or to attach later. Measure it with this probe.
2. The nico runs are bimodal: some runs give about 150 us first-call and 16 us second-call (half the usual
   time), in both old and new. Probably a frequency or core-placement state of the host. It does not change
   the medians.
3. The sweep log `TIMING-SWEEP-1105 bench.log` named in the brief was not at `.scratch/TIMING-SWEEP-1105/work`.

## Not run, with reasons
- Plants: no library or cache code changed, and the bench C file is not test-covered, so no plant applies.
  `cargo xtask plants` was not run.
- Percall/wl counts: no library change, so the per-call instruction counts cannot move.

## Next step
Land as is (bench-only). If wanted, open the attach-cost story (INFORMATIONAL 1).
Rows to read: `grep -rn "^| D-16.10 " plan/`.
