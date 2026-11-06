# TIMING-COV-STRSTR-NONAVX2: strstr on 64-byte and 4 KiB haystacks on the non-AVX2 class

Id: TIMING-COV-STRSTR-NONAVX2
Kind: code
Priority: 45
Size: M
Host: -
Stacks-on: TIMING-N100-STRSTR-64
Model: opus
Effort: high
BUDGET: 80 calls / 160k

## The change
Cut strstr's non-AVX2 path (strstr entry and liella_machine_strpair, csys/src/machine/scan.S) for short haystacks and for
a late hit in 4 KiB. Stack on TIMING-N100-STRSTR-64 (running, same entry and kernel on the AVX2 class). PERF-STRSTR-HIT-4K-2
(red) recorded ruling #2521 and no code change: read it first.

## Rows (TIMING-SWEEP-1105, Ivy Bridge, instr / time)
strstr_hit_64 2.169 / 2.398, strstr_miss_64 1.387 / 1.411, strstr_hit_4k 1.023 / 1.277. N100 rows of 64 B: TIMING-N100-STRSTR-64.

## Evidence (keke, next 730f416b, job timing-coverage-1106-percall; spec handoff/timing-coverage-1106/tc1106.c)
Non-AVX2 path, Liella / glibc instructions per call: hit_64 220 / 204, miss_64 245 / 409, hit_4k 2299 / 1506.
AVX2 path: 139 / 166, 144 / 339, 667 / 827. One traced hit_64 call (non-AVX2): 210 instructions, strpair 133, strstr entry 77.
NOT VERIFIED: this probe does not reproduce the sweep's IVB instr ratios (hit_64 1.08 vs 2.17, hit_4k 1.53 vs 1.02).
Either the bench row's shape differs (bench/c/cbench_memref.c find_rows: haystack 16 bytes past a 4096-aligned block,
the same placement as the probe) or Ivy Bridge selects other Liella builds than `-AVX2` on Zen 1 (check every feature
the dispatch reads: grep -n cpuid crates/boundary/src/dispatch.rs). Step 1 settles which before any change.
Workloads: strstr 1,963 calls in 3 of 30 workloads (rare; ranked last of the six for that reason, despite the largest gap).

## Hypothesis
77 instructions in the strstr entry (needle length, first-pair setup) is fixed cost a 64-byte haystack cannot repay;
the 4 KiB hit row's 1.53x on the non-AVX2 path is the strpair loop's per-16-byte cost.

## Steps
1. Run the row's own shape on the Ivy Bridge timing host with counters only (booked), or on a build host with the exact
   feature mask Ivy Bridge reports, and reconcile with the sweep.
2. Shorten the entry for needles <= 8 bytes and haystacks < 128 bytes; then the loop if the 4k row is still above 1.05x.
3. Tests: strstr every needle length 0-40 x haystack length 0-300 x match position x page-end placement, both paths.
4. Plants: skip the last candidate position, compare one byte short, page-cross test removed.

## JOBS
gate, plants, counts before / after (tc1106.c --cases strstr_hit_64,strstr_miss_64,strstr_hit_4k, both dispatch settings),
timing nico:timing-cov-strstr-nonavx2-time (`ops timing-book nico <min> --story TIMING-COV-STRSTR-NONAVX2`).

First line: COMPLETE:, PENDING JOBS:, INCOMPLETE: or DO NOT LAND:
