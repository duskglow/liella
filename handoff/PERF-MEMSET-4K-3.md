# PERF-MEMSET-4K-3 (branch task/perf-memset-4k-3)

State: one code commit on origin/next 361836f7 (see `git log origin/next..`). The `memset` entry (csys/src/machine/fold.S)
now chooses the kernel above 256 bytes itself: `rep stosb` from the word `liella_machine_fill_erms_from` (threshold, at
least 257, while ERMS is decided; usize::MAX otherwise and before startup), with the 64-byte unaligned head and `rep stosb`
from the next 64-byte boundary; else, with the AVX2 flag, a tail jump to the AVX2 fill through the hidden label
`liella_machine_fill_avx2_entry` (scan.S). NULL and the SSE2 level still go through `hot::fill_large`. `crate::cpu`
publishes the word from `init_with` and `force_erms`. The choice matches `hot::bulk_fill`. Behaviour unchanged.

## Profile (wl_percall, instructions:u per call incl. harness loop, Haswell-EP host shiki, ERMS + AVX2, no FSRM)

Spec validation/percall/memset_sizes.c. Before = job shiki:perf-memset-4k-3-percall-before (origin/next code), after =
shiki:perf-memset-4k-3-percall2 (head 9cfdddee). glibc / before / after:

| size | glibc | before | after | after/glibc |
|---|---:|---:|---:|---:|
| 257 | 49 | 72 | 58 | 1.18 |
| 1024 (a / +1) | 84 / 91 | 108 | 94 | 1.12 / 1.03 |
| 1279 | 98 | 120 | 106 | 1.08 |
| 1280 | 98 / 105 | 65 | 41 | 0.42 / 0.39 |
| 2048 | 140 / 147 | 65 | 41 | 0.29 |
| 4096, 8192, 16384, 65536 | 30 | 65 | 41 | 1.37 |

Where the 4 KiB instructions went before: `rep stosb` counts as one instruction, so the count is size-independent from the
threshold on; 35 instructions over glibc were dispatch: entry chain (11), `fill_large` (3 flag loads, compares, ~14),
`fill_rep` (frame, head broadcast + 4 stores, alignment, ~19), `fill_erms` call (4), return (3). After: entry chain to
`.Lset_out` (8), word compare (2), NULL test (2), broadcast (4), head stores (4), alignment and count (5), rep, mov, ret.

Remaining gap at 4 KiB-64 KiB: 11 instructions per call (41 vs 30). NOT closed to 1.05. Candidates (not done, budget):
(a) skip the 64-byte head for a 64-byte-aligned d (`rep stosb` from d; -9 instr for aligned d, +2 for unaligned):
needs a timing booking to show it does not lose time; (b) test the `rep stosb` word before the 65-256 class compares
(-4 on the large path) costs +2 on memset_128/256, so it would make a measured row slower: rejected.
Zen 1 (no ERMS, AVX2 fill at 4 KiB): the same AVX2 direct jump applies (-14 instr expected, as at 1024/1279 here); NOT
MEASURED on a Zen host in this run.

## Jobs (shiki)
- gate: perf-memset-4k-3-gate (affected --base origin/next). plants: perf-memset-4k-3-plants (handoff/PERF-MEMSET-4K-3-plants.txt,
  3 plants on the entry's rep path, all via `cargo xtask plants`). counts: perf-memset-4k-3-counts
  (`cargo xtask bench --compare --counters --runs 3 cbench_memref`, after only; starts when the gate ends).
  percall: perf-memset-4k-3-percall-before, perf-memset-4k-3-percall2.
- Superseded: perf-memset-4k-3-percall (rc 2: base not primed on this host; rerun with --base none).
- plants: 3 CAUGHT, 0 SURVIVED, 0 WARN (rc 0). clippy and static clean on the role (head 9cfdddee).
- gate and counts were still running at hand-back (gate started 07:14Z; counts waits for the gate in-job).
- The first gate+plants ssh call blocked on the gate's jobrun and was killed by the 10-minute tool limit; the gate kept
  running, plants had not started and were started separately.

## Not done / next step
- Time ratio (brief target <= 1.0 on Broadwell and Zen) NOT measured: needs a timing booking. Next: count and time on the
  Broadwell-EP host (kanon) and a Zen 1 host (keke), then try (a) above with a booking.
- Saturation register row not written (the gap is 11 instructions, not "below one cache line of setup"; no ruling to accept).
- Plants on the AVX2 direct jump and the published word are equivalent by test outcome (a wrong choice still fills
  correctly); no selftest reads the word. A selftest accessor for the word would make them plantable.
