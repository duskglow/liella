# TIMING-COVERAGE-1107: the 1106E slower rows, their stories, and six briefs

Input: results/timing-sweep-1106e-table.md (build origin/next 34c98f61; Ivy Bridge 56 and N100 20 slower entries,
62 rows). Coverage per row: table.md. Open stories from the ops store (2026-11-07 02:35Z): the only open timing story
is PERF-ZONE-LOAD-FAULTS-3 (verifying). Landed since 34c98f61 with timing effect: TIMING-COV-MEMCMP-NONAVX2-4
(SSE2 memcmp NULL fold, never timed), PERF-FIRSTCALL-MKTIME-SNPRINTF-2 (first_mktime, first_snprintf),
PERF-LOCALTIME-FIRST-SECOND-MKTIME (analysis only). Result: 76 entries = covered 2, re-time 18, NONE 56.

## Ranking of the NONE groups (gap x how common the call is)
Commonness from validation/profiles.md (the workload profiles: sort, bash, coreutils, grep/sed-type text work)
and the cold rows' once-per-process weight. Build-host figures: counters-mei.txt (Skylake-SP class, not a timing
host; cycles there are context, the 1106E times are the verdicts).

| rank | group (brief) | rows, 1106E time IVB / N100 | why this rank |
|---|---|---|---|
| 1 | TIMING-COV-COLD-SETLOCALE | first_setlocale_en_US 2.17/2.17, second_setlocale 1.72/1.55, second_bindtextdomain_gettext 1.57/1.63 | every localised program; 110.0 us vs 45.8 us on the build host (+64 us per process) |
| 2 | TIMING-COV-COLD-STRCOLL | first_strcoll 23.9/20.1 | ls/sort in a UTF-8 locale; 11.7 us vs 0.28 us per process |
| 3 | TIMING-COV-WCTYPE-L | iswctype_en_us 2.09/1.84, iswalpha_l_en_us 1.33/1.45, nl_langinfo_l 1.22 (IVB), nl_langinfo 1.10 (IVB), towupper_l 1.07 (IVB), iswalpha_utf8 1.06 (N100) | both classes, per call; regex classes and C++ locales call the _l / iswctype forms |
| 4 | TIMING-COV-COMPARE-HEAD-SSE2 | memcmp/bcmp/__memcmpeq head and diff8 1.25-1.54 (IVB, re-time first), strncmp_diff8 1.42 (IVB) | sort's comparison loop is memcmp (57% in sort-C) |
| 5 | TIMING-COV-STRSCAN-SHORT-IVB | strchr/strchrnul 16/64 1.11-1.29, memrchr_64_hit 1.28, memchr_16/64 1.25/1.12, rawmemchr_4k 1.08 (IVB) | very common, but Ivy Bridge only and same instruction count |
| 6 | TIMING-COV-STRSTR-PAIR | strstr_hit_64 1.68/1.35, strstr_miss_64 1.09 (IVB), strstr_hit_4k 1.10 (IVB, spread 0.93) | both classes, instruction surplus 1.26-1.55 |
| 7 | (unbriefed) copy/fill 144 | memmove_144 1.17, memset_144 1.22 (IVB) | not profiled here; next round |
| 8 | (unbriefed) second_mktime | 1.19/1.22; build host 391 vs ~333 ns | PERF-LOCALTIME-FIRST-SECOND-MKTIME names the next step |
| 9-15 | (unbriefed) casecmp N100, memcmp_256 N100, 64k scans N100, cold strcmp, strcmp_names N100, regexec_backref IVB, getline_long IVB | 1.05-1.46 | small, single-class or already worked in the build |
| - | malloc_free_*, realloc_null_24, realloc_within_usable (IVB 1.11-1.30) | not briefed | saturated: docs/saturation/04-allocator.md (PERF-MALLOC-IVB-CYCLES latency chain; latest-free path refused, owner #2826 and #3210; PAIR-CHAIN NEGATIVE). Build host: 46.3 vs 45.6 cycles at 122 vs 147 instructions, parity. A new lever needs an owner ruling, not a story |

## Evidence collected (mei, role s2, tree 66b8d3ba)
- Jobs: timing-coverage-1107-prof (prof.sh: bench build + region counters + Liella perf record, rc 0) and
  timing-coverage-1107-prof2 (prof2.sh: strstr rows, rc 0). Summary: counters-mei.txt. glibc side: region and
  aggregate counters and the bench's own times only.
- The cold group's Liella samples are kernel-dominated (kernel symbols unresolved for the claude user); the
  aggregates for 62 children: Liella 6337 page faults, 98.3M kernel instructions; glibc 7103 and 126.6M
  (glibc dynamic includes ld.so). Per-row attribution of cold rows needs the probe approach (see the briefs).
- Ivy Bridge-only groups (5, part of 4) were measured with GLIBC_TUNABLES=glibc.cpu.hwcaps=-AVX2 on Skylake-SP:
  instruction ratios match the 1106E table; cycle gaps reproduce partly (memrchr 1.32, strchr_64 1.11-1.13,
  __memcmpeq_head 1.66, bcmp_head 1.45), not for memchr_64 (0.79) or rawmemchr_4k (0.99).
