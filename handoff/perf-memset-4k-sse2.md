# PERF-MEMSET-4K-SSE2 / -2 (branch task/perf-memset-4k-sse2-2) - COMPLETE

State: `liella_machine_fill_sse2` (scan.S) wired into `hot::fill_large`/`bulk_fill` (the no-AVX2, no-ERMS level). Counters on keke
(before/after, default and -AVX2) and plants are done; results are in docs/saturation-register.md section 14.1a.
Result: -AVX2 memset_4k 542 -> 407 instr/op (glibc 471), memset_64k 7273 -> 5699 (glibc 7203). Default level (AVX2 fill) unchanged:
i 1.13 vs glibc. Shiki (ERMS) takes rep stosb at both levels, unchanged.
Decisions: fill_sse2 is 8 x 16 B aligned stores per step with a 128 B unaligned tail (MEM-FALLBACK).
Open: the AVX2 fill is ~32 instr/4 KiB above glibc; a wider unroll of liella_machine_fill_avx2 is the follow-up. memset_4k_off -AVX2 +0.3 instr.
Do not redo: counts (keke:~/work/perf-memset-4k-sse2-2/*.log), plants (7/7 caught).
Jobs: sumire:perf-memset-4k-sse2-2-gate, sumire:perf-memset-4k-sse2-2-plants, keke:perf-memset-4k-sse2-2-counts.
