# PARITY-MEMSET-SMALL-AVX2: memset 16-256 B rows on the AVX2 class

Id: PARITY-MEMSET-SMALL-AVX2
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
| `memset_128` (cbench_memref) | 1.12 | 0.77 | 27.4 | 24.4 |
| `memset_64` (cbench_memref) | 1.11 | 0.72 | 20.3 | 18.3 |

## Profile (percall, Haswell-EP host with AVX2 and no AVX-512, instructions per call, glibc / Liella (ratio))

| size | memset glibc/Liella (ratio) |
|---:|---|
| 16 | 24 / 27 (1.12) |
| 31 | 24 / 27 (1.12) |
| 32 | 25 / 27 (1.08) |
| 33 | 25 / 27 (1.08) |
| 64 | 25 / 27 (1.08) |
| 65 | 32 / 34 (1.06) |
| 128 | 32 / 34 (1.06) |
| 129 | 39 / 37 (0.95) |
| 256 | 39 / 37 (0.95) |
| 1024 | 84 / 94 (1.12) |
| 4096 | 30 / 33 (1.10) |
| 16384 | 30 / 33 (1.10) |
| 65536 | 30 / 33 (1.10) |

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

1. Ladder (Haswell): memset 16/31 1.12 (Liella 27 against 24), 32-64 1.08, 128 1.06, 65 1.06; 129-256 are 0.95 (Liella wins). The bench rows `memset_64` 1.11 (20.3 vs 18.3) and `memset_128` 1.12
   (27.4 vs 24.4) are 2-3 instructions over; Ivy Bridge is 0.72-0.77 on these rows. A fixed 2-3 instruction head cost: probably the size dispatch (compare chain) before the first store, or the broadcast of the fill byte (imul/movd/vpbroadcastb choice).
2. `memset_4k` / `memset_64k` are PERF-MEMSET-4K-4's: not in this brief. (Haswell memset_4096: 33 against 30, 1.10: say so in the brief's notes for that story's owner; do not touch.)

## The change

Remove the 2-3 instruction head cost for 16-128 B (profile with wl_trace.sh which instructions precede the first store), keeping the Ivy Bridge rows at or below today's count. Small, S-sized; stop at parity (<= 1.05), do not chase below. Bit-identical result (a fill).

Liella already has an AVX2 path for these functions at all of these sizes (crates/boundary/src/cpu.rs selects AVX2 or SSE2); it has no AVX-512 path.

Rules that apply: profile before you design (write the per-phase split in the handoff); a change that measures worse on any row on either class is reverted; no lab host names outside handoff/; plants via `cargo xtask plants` only.

JOBS: parity-memset-small-avx2-gate (direct tier), parity-memset-small-avx2-plants (`cargo xtask plants`, plants file per docs/plants.md), parity-memset-small-avx2-count-avx2 and parity-memset-small-avx2-count-ivy (`cargo xtask bench --compare --counters --runs 3 <bench>` before and after, on BOTH counter classes: a Skylake-SP host and an Ivy Bridge host; rows that rose on either class are reverted), parity-memset-small-avx2-percall (validation/wl_percall.sh with validation/percall/avx2_families.c, before and after).
