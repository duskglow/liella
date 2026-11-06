# PERF-MALLOC-IVB-CYCLES: why malloc_free is 1.11-1.18x in time on Ivy Bridge at 0.83x instructions

State: branch task/perf-malloc-ivb-cycles, base next e8a36c58. Result: **cause recorded, no code change** (the one
change tried measured worse and was reverted). Register row added: docs/saturation/04-allocator.md, row
"`malloc` + `free` pair, cycles on Ivy Bridge (PERF-MALLOC-IVB-CYCLES, 2026-11-05)".

## Measurement (nico = Ivy Bridge host A, E3-1220L v2, booking #27, idle)

Probe: a program holding only the cbench_malloc `pair()` loop (same volatile pointers, `p[0] = 1`), 50M pairs,
`taskset -c 2`, Liella static linked like `xtask bench` (bench build of the role on shiki) against glibc 2.39
dynamic. Per-pair counts, interleaved runs, sd < 0.1 (recorded job `nico:perf-malloc-ivb-cycles-perfab`):

| size | Liella cyc | glibc cyc | ratio | Liella instr / uops | glibc instr / uops | rs-full Liella / glibc |
|---|---|---|---|---|---|---|
| 16 | 51.6 | 45.9 | 1.12 | 123.2 / 132.1 | 148.1 / 180.1 | 20.4 / 7.7 |
| 64 | 51.1 | 44.1 | 1.16 | 123.2 / 132.1 | 148.1 / 180.2 | 19.9 / 6.0 |
| 1024 | 51.1 | 45.9 | 1.11 | 123.2 / 132.1 | 148.1 / 180.1 | 19.9 / 7.7 |

Every other event that can explain a CPI gap is 0 or < 0.1 per pair for both (store-forward blocks, machine clears
incl. memory ordering, branch misses, L1D misses, split loads, LCP, recovery cycles, SB/ROB-full, DSB-MITE switches).
`resource_stalls.rs` alone holds the difference (+13 to +14 cycles of the +6 to +7 cycle gap; the rest overlaps).

## Cause (precise-cycles samples: free 50%, malloc 28%, pair 18%)

The pair is latency-bound on a dependency chain carried from one pair to the next through the bitmap word. Malloc's
bitmap-word load (forwarded from the last free's store) feeds not+tzcnt, then the stride imul, which gives p. Free
then waits for the slab-hint record load, whose address comes from p and whose value is forwarded from malloc's
store. Then come the magic imul, the shifts, the bitmap-word load (address from the multiply), the bit clear and
the store, which the next malloc reads. The scheduler fills with the uops waiting on that chain. The register row
has the full chain and the Skylake-SP reasoning (likely the larger scheduler or the shorter forward latency; which
one was not measured).

Per-phase instruction split: not taken with wl_trace (the gap is in cycles, not instructions; Liella is 25 instr
below glibc). Samples per function are above.

## Tried, measured negative (do not redo)

`freed_into` storing `K_HINT` only when it changes: +2.0 instr, cycles +0.15 to +0.45 in 8 of 9 interleaved
rounds (one round -0.1), rs-full +0.2. The hint's address is not on the critical path; the bitmap word's value is.
Skylake-SP gave the same negative in PERF-HEAP-PAIR-TIME-2.

## Next step (needs a design ruling, D-12)

A "latest malloc" fast path in free: when p equals the block the cache's latest fast malloc returned, take word j
and bit b from the cache. That skips the record load and the magic multiply (about 12 cycles of the chain by the
model). It must still load the word to test liveness (the double-free report). It adds instructions and a branch,
and the shared case gets no faster. Measure on Ivy Bridge and Skylake-SP, plus wl_count on heap-heavy workloads.

## Files outside the repo

Probe sources and logs: /home/claude/w/.scratch/PERF-MALLOC-IVB-CYCLES/ (pair.c, pstat.sh, ab.sh, prec.sh,
annot.sh, annot-liella.txt, ab1.txt, pstat64.txt); copies on nico and shiki in ~/work/PERF-MALLOC-IVB-CYCLES/.
