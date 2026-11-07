# TIMING-IVB-MALLOC-CHAIN: malloc/free pair and realloc-in-place cycles on Ivy Bridge (scheduler-full on a carried chain)

Id: TIMING-IVB-MALLOC-CHAIN
Kind: code
Priority: 75
Size: M
Host: -
Stacks-on: -
Model: opus
Effort: high
BUDGET: 80 calls / 140k

## Rows (TIMING-SWEEP-1105, Ivy Bridge; instr / cycles / time)
malloc_free_16 0.830 / 1.122 / 1.121; _32 0.830 / 1.169 / 1.170; _64 0.830 / 1.183 / 1.183; _128 0.830 / 1.188 / 1.190;
_256 0.830 / 1.183 / 1.184; _1024 0.830 / 1.121 / 1.119; realloc_within_usable 0.980 / 1.187 / 1.186;
realloc_null_24 0.851 / 1.078 / 1.063 (cbench_malloc). N100: all faster than glibc (0.72-0.94).

## Profile evidence
- PERF-MALLOC-IVB-CYCLES (handoff/PERF-MALLOC-IVB-CYCLES.md, same host, isolated pair loop, 50M pairs): per pair
  Liella 51.6 cycles / 123 instr / 132 uops vs glibc 45.9 / 148 / 180; resource_stalls.rs 20.4 vs 7.7 per pair holds
  the whole gap; store-forward blocks, machine clears, branch misses, L1D misses, split loads, LCP, recovery, SB/ROB
  full, DSB-MITE switches all < 0.1 per pair on both sides. Cause recorded there: a dependency chain carried from one
  pair to the next through the bitmap word (malloc: forwarded bitmap load, not+tzcnt, stride imul -> p; free: slab
  hint record load from p, magic imul, shifts, bitmap load, bit clear, store read by the next malloc).
- TIMING-REOPEN-1105 (this round): whole-process counts with CBENCH_ROWS are NOT row-specific for cbench_malloc
  (the mix set-up, mix_step2, is 18-21% of samples and runs regardless of the filter), so they do not add to the
  above; the row profile must use the isolated loop again. free 27-30%, malloc 15-16%, realloc 25% of samples.
- realloc_within_usable: not separately profiled at loop level (NOT VERIFIED that the same chain dominates).

## Hypothesis
Latency-bound carried chain; Ivy Bridge's smaller scheduler fills with the chain's waiting uops (rs-full), so
fewer instructions do not buy time. Shortening the chain is the lever, not the instruction count.

## The change
1. Rebuild the PERF-MALLOC-IVB-CYCLES probe (its sources: ~/w/.scratch/PERF-MALLOC-IVB-CYCLES/) on the
   current next; add a realloc-in-place loop; confirm rs-full is still the gap.
2. Shorten the carried chain. The recorded candidate: a "latest malloc" fast path in free (p equal to the block the
   cache's latest fast malloc returned: take word j and bit b from the cache, skipping the record load and the magic
   multiply, about 12 cycles of the chain by the model; still load the word for the liveness/double-free test).
   It changes the allocator's fast path design: needs a D-12 ruling - recommend measuring it behind a branch first.
3. Do not redo: storing K_HINT only on change (measured negative twice).

## JOBS
gate (clippy, static, affected --base origin/next); plants via `cargo xtask plants` (stale latest-block reuse,
double-free check skipped on the fast path, wrong bit); instruction counts before/after (cbench_malloc and
wl_percall malloc rows) as the regression gate, plus wl_count on heap-heavy workloads; booked timing check of these
rows on the Ivy Bridge timing host (`ops timing-book nico <min> --story <ID>`) and the N100 (no regression).
