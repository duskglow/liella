# PARITY-STRCMP-AVX2: strcmp / strncmp / strcasecmp rows on the AVX2 class

Id: PARITY-STRCMP-AVX2
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
| `strcasecmp_c_256` (cbench_locale) | 1.24 | 1.16 | 251.2 | 202.1 |
| `strcasecmp_utf8_256` (cbench_locale) | 1.24 | 1.16 | 251.2 | 202.2 |
| `strcasecmp_en_256` (cbench_locale) | 1.24 | 1.16 | 251.2 | 202.3 |
| `strncasecmp_c_64` (cbench_locale) | 1.27 | 0.93 | 76.0 | 60.0 |
| `strcmp_64_mix` (cbench_memref) | 1.26 | 0.95 | 71.0 | 56.3 |
| `strcmp_16_mix` (cbench_memref) | 1.23 | 0.72 | 52.5 | 42.8 |
| `strcmp_names` (cbench_opt) | 1.17 | 0.94 | 67.0 | 57.3 |
| `strcasecmp_c_16` (cbench_locale) | 1.20 | 0.98 | 54.0 | 45.0 |
| `strcmp_late_256` (cbench_opt) | 1.06 | 1.46 | 108.1 | 102.1 |
| `strcmp_equal_16` (cbench_opt) | 1.15 | 1.06 | 31.0 | 27.0 |
| `strncmp_eq16` (cbench_hot) | 1.09 | 1.01 | 35.1 | 32.1 |
| `strncmp_diff8` (cbench_hot) | 1.06 | 1.44 | 34.1 | 32.1 |

## Profile (percall, Haswell-EP host with AVX2 and no AVX-512, instructions per call, glibc / Liella (ratio))

| size | strcmp_eq glibc/Liella (ratio) |
|---:|---|
| 16 | 31 / 45 (1.45) |
| 31 | 31 / 45 (1.45) |
| 32 | 38 / 53 (1.39) |
| 33 | 38 / 53 (1.39) |
| 64 | 45 / 75 (1.67) |
| 65 | 45 / 75 (1.67) |
| 128 | 89 / 89 (1.00) |
| 129 | 89 / 89 (1.00) |
| 256 | 112 / 113 (1.01) |
| 1024 | 250 / 297 (1.19) |
| 4096 | 804 / 965 (1.20) |
| 16384 | 3012 / 3725 (1.24) |
| 65536 | 11844 / 14765 (1.25) |

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

1. Haswell ladder for equal strings: `strcmp_eq_16/31` 1.45 (Liella 45 against 31), 32-33 1.39 (53 against 38), 64-65 1.67 (75 against 45), 1 KiB 1.19, 4 KiB 1.20, 16-64 KiB 1.24-1.25
   (65536: 14765 against 11844). Liella pays about 1.4 extra instructions per 32 B in the loop ((14765-11844)/2048) and 14-30 more on the 16-64 B head. That is a large, class-independent surplus (the
   Ivy Bridge ratios of the cbench rows are 0.72-1.06, so the bench rows with short, unequal names hide it; the equal-string rows are the exposed ones).
2. Bench rows: `strcmp_64_mix` 1.26 (71 vs 56), `strcmp_16_mix` 1.23, `strcmp_names` 1.17, `strcmp_equal_16` 1.15, `strncmp_eq16` 1.09, `strcasecmp_*_256` 1.24 (251 vs 202), `strcasecmp_c_16` 1.20 (54 vs 45), `strncasecmp_c_64` 1.27 (76 vs 60).
3. Existing story: perf-strncasecmp-64-avx2 (handoff/perf-strncasecmp-64-avx2.md) covered strncasecmp 64; read it, and do not repeat what it found.

## The change

Bring the strcmp / strncmp / strcasecmp / strncasecmp rows to <= 1.05 on both classes: first the two-vector compare loop (equal long strings), then the 16-64 byte
head (one page-cross check, one compare of 32 B), then the case-folding 256 B row (fold two vectors per iteration). Results bit-identical, including the return value's sign for non-ASCII bytes and the
locale rule for the case-insensitive forms (C and en_US.UTF-8 as today).

Liella already has an AVX2 path for these functions at all of these sizes (crates/boundary/src/cpu.rs selects AVX2 or SSE2); it has no AVX-512 path.

Rules that apply: profile before you design (write the per-phase split in the handoff); a change that measures worse on any row on either class is reverted; no lab host names outside handoff/; plants via `cargo xtask plants` only.

JOBS: parity-strcmp-avx2-gate (direct tier), parity-strcmp-avx2-plants (`cargo xtask plants`, plants file per docs/plants.md), parity-strcmp-avx2-count-avx2 and parity-strcmp-avx2-count-ivy (`cargo xtask bench --compare --counters --runs 3 <bench>` before and after, on BOTH counter classes: a Skylake-SP host and an Ivy Bridge host; rows that rose on either class are reverted), parity-strcmp-avx2-percall (validation/wl_percall.sh with validation/percall/avx2_families.c, before and after).
