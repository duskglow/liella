# HEAP-FIRST-USE: the heap's first allocation in one mapping

State: branch `task/heap-first-use` from origin/next b14f639c, role mei:~/w/s1. Commits: 2c8bfdb2 (one
reservation, D-3.19, tests), 7c8ef55f (inline + plants file), 47d83e12 (measured fault bound), 23f04b78 (registry
page layout check), 9b21d1bc (fault bound is a regression guard only), then this handoff.

## Decisions in force
D-3.19 (new, `plan/07-decisions/s03-memory-allocation.md`): the first allocation maps one range: the first segment,
one page with generation 0 of the cache table (64 entries), segment bases and segment hash (128 segments), then
the first cache chunk. Amends D-3.14 ("unlimited keeps the maxima"): generation 0 is capped either way, and later
generations double as under a limit. D-3.12 unchanged (getcpu on each slab refill). Ruling #3383 respected (no
one-shot arena).

## Measured (mei, probe `hf.c` = TIMING-COV-COLD-SETLOCALE's cs.c plus `M` = malloc(16)+free; perf stat -r 200)
Probe, script and outputs: `mei:~/work/HEAP-FIRST-USE/` (base/, after/ = first commit, after2/ = final code).

| variant | base iu / ik / faults | final iu / ik / faults |
|---|---|---|
| M (first malloc only) | 9,303 / 344,018 / 26 | 9,225 / 320,428 / 25 |
| PM | 11,061 / 353,682 / 27 | 11,003 / 323,374 / 25 |
| PS (first setlocale) | 67,887 / 544,494 / 51 | 67,805 / 513,413 / 49 |
| PSG | (not printed) | 102,931 / 681,847 / 62 |
| glibc static PM / PS | 157,277 / 323,731 / 26 ; 177,598 / 411,578 / 34 | (whole process) |

System calls of the first malloc (strace of PM): base prlimit + 5 mmaps (8 MiB, 64 KiB, 8 MiB, 16 MiB, 32 MiB)
+ 2 munmaps; final prlimit + 1 mmap (32 MiB + 68 KiB, NORESERVE) + 2 munmaps (21 -> 17 lines for the whole PM run).
Faults by symbol (perf record, PM): `refill` 4 -> 2 (the segment bases and hash pages are now the cache-table page);
an out-of-line `first_reservation` took a text fault of its own, hence the inline commit.
glibc's first malloc in a static program costs it no extra fault or system call (P -> PM deltas ~0).

What the remaining first-use cost is: 1 registry page, 1 cache page, segment head + bitmap pages, one slab page per
size class used, the vDSO parse for getcpu (~6k user instructions, 1 fault; D-3.12 asks the node on every refill),
the prlimit (D-3.14 sizes the registries from RLIMIT_AS), and the 2 munmaps that align the segment.

## Not done / open
- Timing check (Ivy Bridge, N100): PENDING; the 11-07 booking window had passed when this ran. `cargo xtask bench
  --compare --counters` on the first_* rows was not run (budget); the probe's PS is first_setlocale's path.
- INFORMATIONAL: the product never calls `Heap::set_numa` (D-3.12 says it is called when more than one node is
  allowed); the first malloc also triggers the dispatch guard's lazy read of `/proc/sys/vm/mmap_min_addr`
  (openat/read/close, `boundary/src/dispatch.rs`), not heap code.
- Next candidates (each measured): skip getcpu while one node is known (needs D-3.12 amended: RULING), take the
  alignment slack without the 2 munmaps, place the first cache in the registry page's spare 512 bytes (needs a
  smaller cache or registry).

## Tests and plants
`the_first_allocation_reserves_one_mapping` (rt: one page request, one mapping, registry page layout) and
`the_first_allocation_touches_few_pages` (core oracle `heap_first_faults.c`: 11 faults measured, bound 12). The fault
bound is a regression guard only: plant `first-reservation-off-faults` (registries mapped separately again) SURVIVED
it in hfu-plants-2, so that plant was dropped and the rt test holds the one mapping. `hash-over-bases` survived
hfu-plants-2 and is caught by the layout check added in 23f04b78. Plants file: `handoff/HEAP-FIRST-USE-plants.txt`.

## Jobs (mei)
Final head 9b21d1bc: hfu-gate-3 (clippy + static + affected --base origin/next, rc 0), hfu-plants-4 (4 plants).
Counts: hfu-counts-base (base), hfu-check-2 (after2 counts; code identical to the final head's heap code).
SUPERSEDED: hfu-check-1 (first commit), hfu-gate (rc 0, older head), hfu-plants-1 (killed: old bound), hfu-plants-2
(two survivors, fixed), hfu-gate-2 and hfu-plants-3 (killed: started on an unsynced tree, sync rc 4).
