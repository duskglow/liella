# PARITY-COPY-AVX2: memcpy / memmove rows and the memcpy_4k counter question

Id: PARITY-COPY-AVX2
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
| `memcpy_64k` (cbench_memref) | 1.49 | 1.85 | 85.5 | 57.5 |
| `memmove_4k` (cbench_memref) | 1.06 | 0.89 | 436.8 | 413.0 |
| `memcpy_64` (cbench_memref) | 1.05 | 0.7 | 20.4 | 19.4 |
| `memmove_64` (cbench_memref) | 1.05 | 0.69 | 20.3 | 19.3 |

## Profile (percall, Haswell-EP host with AVX2 and no AVX-512, instructions per call, glibc / Liella (ratio))

| size | memcpy glibc/Liella (ratio) | memmove glibc/Liella (ratio) |
|---:|---|---|
| 16 | 23 / 24 (1.04) | 23 / 24 (1.04) |
| 31 | 23 / 24 (1.04) | 23 / 24 (1.04) |
| 32 | 24 / 24 (1.00) | 24 / 24 (1.00) |
| 33 | 24 / 26 (1.08) | 24 / 26 (1.08) |
| 64 | 24 / 26 (1.08) | 24 / 26 (1.08) |
| 65 | 34 / 34 (1.00) | 34 / 34 (1.00) |
| 128 | 34 / 34 (1.00) | 34 / 34 (1.00) |
| 129 | 42 / 44 (1.05) | 42 / 44 (1.05) |
| 256 | 42 / 44 (1.05) | 42 / 44 (1.05) |
| 1024 | 137 / 170 (1.24) | 137 / 170 (1.24) |
| 4096 | 425 / 434 (1.02) | 425 / 434 (1.02) |
| 16384 | 44 / 80 (1.82) | 44 / 80 (1.82) |
| 65536 | 44 / 5718 (129.93) | 44 / 5718 (129.93) |

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

1. **The memcpy_4k counter (do this first, a short probe, before any change).** The rows: memcpy_4k is 0.17 on the AVX2 class (Liella 73, glibc 419) and 26.3 on Ivy Bridge. On the Haswell host the ladder shows
   Liella 434 and glibc 425 for 4 KiB (parity, both vector loops), and at 16 KiB Liella 80 against glibc 44, at 64 KiB Liella 5718 against glibc 44: a count of 44 for a 16-64 KiB copy can only be a `rep movsb`
   retired as ONE instruction (plus about 40 of setup) on Haswell and Skylake-SP. So on these two classes the counter counts `rep movsb` as one instruction and the
   row's count is the setup, not the work; "Liella 73 at 4 KiB on Skylake-SP" is consistent with that run's Liella taking the `rep movsb` path at 4 KiB while Liella on Haswell took the vector loop (434): the same product dispatches differently on the two hosts (cpu.rs: ERMS/FSRM/AVX2 thresholds and the
   head-aligned placement check). On Ivy Bridge the 26.3 is the opposite way round (Liella 26 times MORE): consistent with the counter counting each `rep movsb` iteration (microcoded string op) as an instruction there, but that is **not verified** (no Ivy Bridge host in this run). The probe: `perf stat -e instructions:u` of a loop of `rep movsb` of 4096 B on each class, and a
   percall run of memcpy_4k on a Skylake-SP and an Ivy Bridge host. Outcome decides whether instruction counts of copy rows are comparable at all; if not, the memcpy / memmove rows move to cycle counts (booked host) and this story shrinks to the probe plus a documentation row in docs/benchmarks.md.
2. Haswell (real instruction counts, vector loops on both sides): memcpy/memmove 1 KiB 1.24 (170 vs 137), 33/64 B 1.08, 16 KiB 1.82 and 64 KiB 130x (rep movsb on glibc side, vector loop on Liella's). The 16-64 KiB gap on Haswell is a CHOICE (D-4.14 chose the vector loop up to 64 KiB by cycles: AVX2_COPY_END), so an instruction ratio there is not a defect; do not change without cycle evidence.
3. Bench rows: `memcpy_64k` 1.49 (85.5 vs 57.5), `memmove_4k` 1.06, `memcpy_64`/`memmove_64` 1.05 (20.4 vs 19.4): 1 instruction over, parity.

## The change

(1) Run the probe in point 1 and write the result into docs/benchmarks.md section 2a (what the counter counts per class) and the saturation register. (2) Only if the probe shows the
copy rows are comparable by instructions: reduce the 1 KiB (137 vs 170) head/tail shape on the AVX2 copy and the 64-byte entry (one instruction). Do not change AVX2_COPY_END or the rep movsb thresholds
without cycle timing from a booked host (D-4.14 is by cycles).

Liella already has an AVX2 path for these functions at all of these sizes (crates/boundary/src/cpu.rs selects AVX2 or SSE2); it has no AVX-512 path.

Rules that apply: profile before you design (write the per-phase split in the handoff); a change that measures worse on any row on either class is reverted; no lab host names outside handoff/; plants via `cargo xtask plants` only.

JOBS: parity-copy-avx2-gate (direct tier), parity-copy-avx2-plants (`cargo xtask plants`, plants file per docs/plants.md), parity-copy-avx2-count-avx2 and parity-copy-avx2-count-ivy (`cargo xtask bench --compare --counters --runs 3 <bench>` before and after, on BOTH counter classes: a Skylake-SP host and an Ivy Bridge host; rows that rose on either class are reverted), parity-copy-avx2-percall (validation/wl_percall.sh with validation/percall/avx2_families.c, before and after).
