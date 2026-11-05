# PARITY-ROWS-AVX2

State: branch task/parity-rows-avx2. Planning story: five code briefs and a summary table in handoff/parity-rows-avx2/ (summary.md, PARITY-SCAN-AVX2, PARITY-MEMCMP-AVX2, PARITY-STRCMP-AVX2, PARITY-MEMSET-SMALL-AVX2, PARITY-COPY-AVX2). New spec validation/percall/avx2_families.c (size ladder, 9 functions x 13 sizes).

Method: the row list is the earlier AVX2 run's measure.tsv (Skylake-SP host, a6fc34c2; scratch of PRE14-BENCH-AVX2-COUNTS), not recounted: 81 non-libm rows above 1.05 (15 covered by open stories). One percall job (label parity-rows-avx2-percall) on the Haswell-EP role host (AVX2, no AVX-512), rc 0, 107 s. The Skylake-SP host was not used for percall (no role there).

Findings:
- Liella has AVX2 and SSE2 paths and NO AVX-512 path. On Haswell the long-loop rows (strlen/rawmemchr/memchr 4k-64k) are at parity (1.00-1.01); on Skylake-SP they are 1.08-1.11 because glibc's own count falls there while Liella's is identical on both hosts. So the long-loop gap is a missing AVX-512 level (inferred from counts only).
- Real AVX2-class gaps by size on Haswell: strlen/memchr/rawmemchr at 128-129 B (1.2-1.48), memcmp 31-32 B (1.23-1.31) and 4 KiB (1.15), strcmp equal strings at every size (1.2-1.67), memset 16-128 B (1.06-1.12), memcpy/memmove 1 KiB (1.24).
- memcpy_4k: on Haswell and Skylake-SP a 16-64 KiB glibc copy counts 44 instructions: rep movsb retires as ONE instruction there. Liella 73 at 4 KiB on Skylake-SP vs 434 on Haswell means a different path per host. The Ivy Bridge 26.3 is NOT verified (probably per-iteration counting of rep movsb); the COPY brief starts with that probe.
- Not briefed (not AVX2-specific, reasons in summary.md): re_mt4_*, wide-ctype/locale rows (extend PERF-WCTYPE-ENUS), sprintf_chk_ls_nlink, getline_long, env cycles, sys_close_ebadf.

Not to redo: the 256 B memcmp and 64 B strncasecmp stories (handoff/memcmp-256-avx2.md, perf-strncasecmp-64-avx2.md).
Next step: dispatch the five briefs; PARITY-COPY-AVX2 first (the probe decides whether copy rows are comparable by instructions). A Skylake-SP percall run of avx2_families.c is the first job of PARITY-SCAN-AVX2.
