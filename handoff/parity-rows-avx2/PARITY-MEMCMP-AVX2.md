# PARITY-MEMCMP-AVX2: memcmp / bcmp / __memcmpeq rows on the AVX2 class

Id: PARITY-MEMCMP-AVX2
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
| `memcmp_64k` (cbench_memref) | 1.13 | 1.0 | 9324.7 | 8240.2 |
| `memcmp_4k` (cbench_memref) | 1.24 | 1.01 | 671.1 | 542.9 |
| `__memcmpeq_tail_32` (cbench_memref) | 1.54 | 1.1 | 31.3 | 20.3 |
| `__memcmpeq_eq_32` (cbench_memref) | 1.54 | 1.1 | 31.3 | 20.4 |
| `__memcmpeq_head_1k` (cbench_memref) | 1.56 | 1.85 | 30.3 | 19.4 |
| `memcmp_diff8_127` (cbench_memref) | 1.43 | 1.75 | 33.4 | 23.4 |
| `memcmp_diff8_96` (cbench_memref) | 1.43 | 1.76 | 33.3 | 23.3 |
| `__memcmpeq_head_128` (cbench_memref) | 1.47 | 1.85 | 28.4 | 19.3 |
| `bcmp_eq_32` (cbench_memref) | 1.40 | 1.07 | 31.4 | 22.4 |
| `bcmp_head_1k` (cbench_memref) | 1.31 | 1.56 | 30.4 | 23.3 |
| `memcmp_diff8_64` (cbench_memref) | 1.31 | 1.31 | 30.4 | 23.3 |
| `__memcmpeq_tail_128` (cbench_memref) | 1.15 | 1.11 | 39.4 | 34.3 |
| `bcmp_tail_32` (cbench_memref) | 1.20 | 0.91 | 31.4 | 26.3 |
| `memcmp_16` (cbench_memref) | 1.23 | 1.08 | 27.3 | 22.3 |
| `__memcmpeq_eq_128` (cbench_memref) | 1.15 | 1.11 | 39.4 | 34.4 |
| `bcmp_head_128` (cbench_memref) | 1.21 | 1.56 | 28.3 | 23.4 |
| `memcmp_16_mix` (cbench_memref) | 1.13 | 1.05 | 42.3 | 37.4 |
| `__memcmpeq_eq_8` (cbench_memref) | 1.15 | 1.0 | 23.3 | 20.3 |
| `__memcmpeq_tail_16` (cbench_memref) | 1.15 | 1.0 | 23.3 | 20.3 |
| `__memcmpeq_eq_16` (cbench_memref) | 1.14 | 1.0 | 23.3 | 20.4 |
| `memcmp_256` (cbench_memref) | 1.05 | 1.1 | 58.4 | 55.5 |

## Profile (percall, Haswell-EP host with AVX2 and no AVX-512, instructions per call, glibc / Liella (ratio))

| size | memcmp_eq glibc/Liella (ratio) |
|---:|---|
| 16 | 31 / 31 (1.00) |
| 31 | 31 / 38 (1.23) |
| 32 | 29 / 38 (1.31) |
| 33 | 29 / 31 (1.07) |
| 64 | 29 / 31 (1.07) |
| 65 | 41 / 40 (0.98) |
| 128 | 41 / 40 (0.98) |
| 129 | 59 / 45 (0.76) |
| 256 | 59 / 56 (0.95) |
| 1024 | 169 / 172 (1.02) |
| 4096 | 577 / 665 (1.15) |
| 16384 | 2209 / 2393 (1.08) |
| 65536 | 8737 / 9305 (1.07) |

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

1. Haswell ladder (equal buffers): sizes 31-32 are 1.23-1.31 (Liella 38 against 29-31), 64 about 1.07, 129 is 0.76 (Liella wins), 4 KiB 1.15 (665 vs 577), 16 KiB/64 KiB 1.07-1.08. The 31-32 B step is a head-shape
   difference: glibc handles 32 B with two overlapping 16 B or one 32 B compare, Liella with a longer path (7-9 extra instructions).
2. 4 KiB-64 KiB: slope (count(64k)-count(16k))/49152*128 is 18.0 instr per 128 B for Liella against 17.0 for glibc on Haswell (6912 and 6528 over 48 KiB); on the Skylake-SP
   host glibc is lower again (memcmp_64k 8240 against 9325). Liella's per-128-byte loop is one to two instructions heavier.
3. The `memcmp-256-avx2` story (handoff/memcmp-256-avx2.md, merged or open: check first) covered the 256 B band; do not redo it.
4. Bench rows with `_head_`/`_diff8_` (1.31-1.56 here) are mismatch-at-offset rows: the early-exit path after the first differing vector
   (find the byte, subtract) is where the surplus is; Ivy Bridge is 1.3-1.85 on those too, so they are not class-specific and the same fix helps both.

## The change

Bring the memcmp / bcmp / __memcmpeq rows to <= 1.05 on both classes: (a) the 17-32 B head, (b) the early-exit after a mismatch in
the first vector (`_head_`, `_diff8_` rows), (c) the long-loop cost per 128 B. `__memcmpeq` and `bcmp` share the equality kernel
(no ordering needed: do not pay for the byte subtract). Results bit-identical (sign and magnitude as today).

Liella already has an AVX2 path for these functions at all of these sizes (crates/boundary/src/cpu.rs selects AVX2 or SSE2); it has no AVX-512 path.

Rules that apply: profile before you design (write the per-phase split in the handoff); a change that measures worse on any row on either class is reverted; no lab host names outside handoff/; plants via `cargo xtask plants` only.

JOBS: parity-memcmp-avx2-gate (direct tier), parity-memcmp-avx2-plants (`cargo xtask plants`, plants file per docs/plants.md), parity-memcmp-avx2-count-avx2 and parity-memcmp-avx2-count-ivy (`cargo xtask bench --compare --counters --runs 3 <bench>` before and after, on BOTH counter classes: a Skylake-SP host and an Ivy Bridge host; rows that rose on either class are reverted), parity-memcmp-avx2-percall (validation/wl_percall.sh with validation/percall/avx2_families.c, before and after).
