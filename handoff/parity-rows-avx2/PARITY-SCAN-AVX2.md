# PARITY-SCAN-AVX2: strlen / strnlen / memchr / rawmemchr / memrchr rows on the AVX2 class

Id: PARITY-SCAN-AVX2
Kind: code
Priority: 70
Size: S
Host: -
Stacks-on: -
Model: opus
Effort: high
BUDGET: 70 tool calls / 140k

## Rows (AVX2 class = Skylake-SP host with AVX-512, instruction ratio Liella/glibc, > 1.05 is slower)

| Row | AVX2-class ratio | Ivy Bridge ratio | Liella instr | glibc instr |
|---|---:|---:|---:|---:|
| `strnlen_64k` (cbench_memref) | 1.09 | 0.76 | 6219.4 | 5699.7 |
| `strlen_64k` (cbench_memref) | 1.11 | 0.89 | 5177.3 | 4661.7 |
| `rawmemchr_64k` (cbench_memref) | 1.10 | 0.9 | 5685.0 | 5169.4 |
| `memchr_64k` (cbench_memref) | 1.08 | 0.85 | 6701.0 | 6209.5 |
| `memrchr_4k` (cbench_hot) | 1.11 | 0.86 | 458.5 | 411.6 |
| `strnlen_4k` (cbench_memref) | 1.10 | 0.81 | 452.8 | 412.7 |
| `strlen_4k` (cbench_memref) | 1.11 | 0.92 | 370.7 | 334.6 |
| `rawmemchr_4k` (cbench_memref) | 1.10 | 0.91 | 396.8 | 361.6 |
| `strnlen_16` (cbench_memref) | 1.12 | 0.81 | 28.4 | 25.4 |
| `strnlen_16_mix` (cbench_memref) | 1.08 | 0.83 | 35.3 | 32.5 |
| `memrchr_64_hit` (cbench_hot) | 1.08 | 1.11 | 27.1 | 25.0 |

## Profile (percall, Haswell-EP host with AVX2 and no AVX-512, instructions per call, glibc / Liella (ratio))

| size | strlen glibc/Liella (ratio) | strnlen glibc/Liella (ratio) | memchr glibc/Liella (ratio) | rawmemchr glibc/Liella (ratio) |
|---:|---|---|---|---|
| 16 | 24 / 24 (1.00) | 30 / 31 (1.03) | 32 / 31 (0.97) | 27 / 27 (1.00) |
| 31 | 24 / 24 (1.00) | 30 / 31 (1.03) | 32 / 31 (0.97) | 27 / 27 (1.00) |
| 32 | 32 / 29 (0.91) | 41 / 36 (0.88) | 46 / 36 (0.78) | 33 / 31 (0.94) |
| 33 | 32 / 29 (0.91) | 41 / 36 (0.88) | 46 / 36 (0.78) | 33 / 31 (0.94) |
| 64 | 36 / 36 (1.00) | 45 / 46 (1.02) | 51 / 49 (0.96) | 37 / 37 (1.00) |
| 65 | 36 / 36 (1.00) | 45 / 46 (1.02) | 51 / 49 (0.96) | 37 / 37 (1.00) |
| 128 | 44 / 65 (1.48) | 53 / 53 (1.00) | 62 / 75 (1.21) | 45 / 54 (1.20) |
| 129 | 44 / 65 (1.48) | 53 / 53 (1.00) | 62 / 75 (1.21) | 45 / 54 (1.20) |
| 256 | 71 / 75 (1.06) | 89 / 97 (1.09) | 82 / 88 (1.07) | 73 / 72 (0.99) |
| 1024 | 131 / 135 (1.03) | 161 / 169 (1.05) | 162 / 166 (1.02) | 139 / 138 (0.99) |
| 4096 | 371 / 375 (1.01) | 449 / 457 (1.02) | 474 / 478 (1.01) | 403 / 402 (1.00) |
| 16384 | 1331 / 1335 (1.00) | 1601 / 1609 (1.00) | 1722 / 1726 (1.00) | 1459 / 1458 (1.00) |
| 65536 | 5171 / 5175 (1.00) | 6209 / 6217 (1.00) | 6714 / 6718 (1.00) | 5683 / 5682 (1.00) |

How to repeat: `~/bin/jobrun <label> count -- sh -c 'cd ~/w/<role> && bash validation/wl_percall.sh --base none --spec validation/percall/avx2_families.c .'` (the spec is on this branch; the ladder above is its output on the Haswell-EP host, labelled `parity-rows-avx2-percall`). Repeat on the Skylake-SP host to get the AVX-512 side (the role there is not the story's host: ask the dispatcher for a count job on it).

## What the reference does (black box) and the hypothesis

The reference kernel, black box: on the Haswell-EP host (AVX2, no AVX-512) the ladder above is at parity for the long
loops (`strlen`, `rawmemchr`, `memchr` at 4 KiB and 64 KiB are 1.00-1.01) while on the Skylake-SP host the same rows
are 1.08-1.11 over glibc, and glibc's own count at those sizes is lower there (strlen_4k 334 against 371 on Haswell;
memcmp_64k 8240 against 8737) while Liella's count is the same on both (370.7 and 371; 9325 and 9305). So the
Skylake-SP gap is a glibc path Liella has no counterpart for: Liella has an AVX2 path and an SSE2 path
(`crates/boundary/src/cpu.rs`, "AVX2 or SSE2") and **no AVX-512/EVEX path**; glibc's count drops on the AVX-512 CPU
(the shorter vector loop with mask registers, inferred from the count only; never read or disassembled). On the AVX2-only host the
remaining gaps are at particular sizes (see the ladder), which are Liella's own loop-entry and tail shape.

1. Skylake-SP gap at 4 KiB and 64 KiB (1.08-1.11, 5-10% of the call): glibc's count is lower on the AVX-512 CPU, Liella's identical
   on both hosts. Hypothesis: a 32-byte-per-step loop unrolled to four vectors with the mask reduced once per 4 vectors
   (fewer instructions per 128 B). Check what Liella's AVX2 loop retires per 128 bytes (roughly 4 loads + 3 min/or-reduces +
   compare + test + 2 loop overhead) against glibc's per-128-byte step derived from the ladder slope (count(64k) - count(16k)) / 49152 * 128:
   glibc 5683-1459 over 49152 B -> about 11.0 instr per 128 B for rawmemchr on Haswell, 1.00 vs Liella. Compute the same slope on the Skylake-SP host
   (a percall run there) before choosing the design. An AVX-512 path (dispatch through the existing mechanism, bit-identical results) is
   the candidate if the slope on that host is lower than any AVX2 loop can reach; if it is not, the surplus is in prologue/epilogue.
2. Haswell: `strlen_128/129` 1.48 (Liella 65, glibc 44), `memchr_128/129` 1.21 (75 / 62), `rawmemchr_128/129` 1.20 (54 / 45), `strnlen_256` 1.09:
   the first-loop entry for sizes just over 4 vectors costs Liella about 9-21 instructions more (rawmemchr 9, memchr 13, strlen 21) than glibc's straight-line
   4-vector head. Profile the phases (wl_trace.sh) before changing.
3. Small: `strnlen_16` / `strnlen_16_mix` (Liella 28 / 35 against 25 / 32): 3 instructions of entry cost on the limit check.

## The change

Bring the rows above to <= 1.05 on the Skylake-SP class without making any row slower on Ivy Bridge, Haswell or Zen 1
(WORKLOAD GATE). Order: (a) the 128-256 byte entry shape on the AVX2 path (Haswell ladder rows), (b) the 4 KiB / 64 KiB loop
per-128-byte cost, (c) an AVX-512 variant only if (b) cannot close the gap and the dispatch mechanism supports a third level
without a cost for the other CPUs; results bit-identical to the AVX2/SSE2 paths (test all paths). Read
docs/saturation/03-memory-and-string-kernels.md first: it records what was tried per row, including negatives.

Liella already has an AVX2 path for these functions at all of these sizes (crates/boundary/src/cpu.rs selects AVX2 or SSE2); it has no AVX-512 path.

Rules that apply: profile before you design (write the per-phase split in the handoff); a change that measures worse on any row on either class is reverted; no lab host names outside handoff/; plants via `cargo xtask plants` only.

JOBS: parity-scan-avx2-gate (direct tier), parity-scan-avx2-plants (`cargo xtask plants`, plants file per docs/plants.md), parity-scan-avx2-count-avx2 and parity-scan-avx2-count-ivy (`cargo xtask bench --compare --counters --runs 3 <bench>` before and after, on BOTH counter classes: a Skylake-SP host and an Ivy Bridge host; rows that rose on either class are reverted), parity-scan-avx2-percall (validation/wl_percall.sh with validation/percall/avx2_families.c, before and after).
