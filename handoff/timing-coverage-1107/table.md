# TIMING-COVERAGE-1107: coverage of the 1106E slower-in-time rows

Source: results/timing-sweep-1106e-table.md (build 34c98f61), every row with verdict slower on a class (62 rows,
76 row-class entries: Ivy Bridge 56, N100 20). wordexp rows are OWNER-DEFERRED and not listed; the table has no
libm or nscache rows in its slower list. Open stories: `ops status` and the ops store, 2026-11-07 02:35Z; landed
work: origin/next 34c98f61..66b8d3ba. Status: covered by <story> (open), re-time (landed after the build), NONE
(no open story; the group brief named, or unbriefed with its rank in README.md).

| row | class | instr | time | spread | status | story / note |
|---|---|---|---|---|---|---|
| cbench_hot/getline_long | Ivy Bridge | 1.194 | 1.078 | 0.046 | NONE | (unbriefed: rank 15; TIMING-COV-GETLINE-LONG-4 is in the build, 1.121 -> 1.078) |
| cbench_hot/memrchr_64_hit | Ivy Bridge | 1.107 | 1.277 | 0.003 | NONE | TIMING-COV-STRSCAN-SHORT-IVB |
| cbench_hot/strchr_16_hit | Ivy Bridge | 1.000 | 1.116 | 0.002 | NONE | TIMING-COV-STRSCAN-SHORT-IVB |
| cbench_hot/strchr_64_hit | Ivy Bridge | 1.000 | 1.135 | 0.000 | NONE | TIMING-COV-STRSCAN-SHORT-IVB |
| cbench_hot/strchr_64_miss | Ivy Bridge | 1.000 | 1.289 | 0.005 | NONE | TIMING-COV-STRSCAN-SHORT-IVB |
| cbench_hot/strchrnul_16_hit | Ivy Bridge | 1.000 | 1.110 | 0.000 | NONE | TIMING-COV-STRSCAN-SHORT-IVB |
| cbench_hot/strchrnul_64_hit | Ivy Bridge | 1.000 | 1.269 | 0.005 | NONE | TIMING-COV-STRSCAN-SHORT-IVB |
| cbench_hot/strchrnul_64_miss | Ivy Bridge | 1.000 | 1.194 | 0.005 | NONE | TIMING-COV-STRSCAN-SHORT-IVB |
| cbench_hot/strncmp_diff8 | Ivy Bridge | 1.436 | 1.417 | 0.004 | NONE | TIMING-COV-COMPARE-HEAD-SSE2 |
| cbench_locale/iswalpha_l_en_us | Ivy Bridge | 1.489 | 1.330 | 0.000 | NONE | TIMING-COV-WCTYPE-L |
| cbench_locale/iswalpha_l_en_us | N100 | 1.489 | 1.448 | 0.000 | NONE | TIMING-COV-WCTYPE-L |
| cbench_locale/iswalpha_utf8 | N100 | 1.347 | 1.061 | 0.002 | NONE | TIMING-COV-WCTYPE-L |
| cbench_locale/iswctype_en_us | Ivy Bridge | 1.768 | 2.090 | 0.000 | NONE | TIMING-COV-WCTYPE-L |
| cbench_locale/iswctype_en_us | N100 | 1.767 | 1.840 | 0.000 | NONE | TIMING-COV-WCTYPE-L |
| cbench_locale/nl_langinfo_en_us | Ivy Bridge | 0.839 | 1.100 | 0.000 | NONE | TIMING-COV-WCTYPE-L |
| cbench_locale/nl_langinfo_l_en_us | Ivy Bridge | 1.250 | 1.221 | 0.005 | NONE | TIMING-COV-WCTYPE-L |
| cbench_locale/strcasecmp_c_16 | N100 | 1.060 | 1.066 | 0.000 | NONE | (unbriefed: casecmp N100, rank 9; strncasecmp_c_64 is 1.0502, rounding) |
| cbench_locale/strncasecmp_c_64 | N100 | 1.029 | 1.050 | 0.000 | NONE | (unbriefed: casecmp N100, rank 9; strncasecmp_c_64 is 1.0502, rounding) |
| cbench_locale/towupper_l_en_us | Ivy Bridge | 1.000 | 1.067 | 0.000 | NONE | TIMING-COV-WCTYPE-L |
| cbench_malloc/malloc_free_1024 | Ivy Bridge | 0.830 | 1.107 | 0.091 | NONE | (not briefed: saturated, docs/saturation/04-allocator.md rows PERF-MALLOC-IVB-CYCLES, latest-free (owner #2826, #3210), PAIR-CHAIN NEGATIVE; parity on Skylake-SP, see README) |
| cbench_malloc/malloc_free_128 | Ivy Bridge | 0.830 | 1.178 | 0.000 | NONE | (not briefed: saturated, docs/saturation/04-allocator.md rows PERF-MALLOC-IVB-CYCLES, latest-free (owner #2826, #3210), PAIR-CHAIN NEGATIVE; parity on Skylake-SP, see README) |
| cbench_malloc/malloc_free_16 | Ivy Bridge | 0.830 | 1.114 | 0.001 | NONE | (not briefed: saturated, docs/saturation/04-allocator.md rows PERF-MALLOC-IVB-CYCLES, latest-free (owner #2826, #3210), PAIR-CHAIN NEGATIVE; parity on Skylake-SP, see README) |
| cbench_malloc/malloc_free_256 | Ivy Bridge | 0.830 | 1.179 | 0.044 | NONE | (not briefed: saturated, docs/saturation/04-allocator.md rows PERF-MALLOC-IVB-CYCLES, latest-free (owner #2826, #3210), PAIR-CHAIN NEGATIVE; parity on Skylake-SP, see README) |
| cbench_malloc/malloc_free_32 | Ivy Bridge | 0.830 | 1.284 | 0.003 | NONE | (not briefed: saturated, docs/saturation/04-allocator.md rows PERF-MALLOC-IVB-CYCLES, latest-free (owner #2826, #3210), PAIR-CHAIN NEGATIVE; parity on Skylake-SP, see README) |
| cbench_malloc/malloc_free_64 | Ivy Bridge | 0.830 | 1.170 | 0.004 | NONE | (not briefed: saturated, docs/saturation/04-allocator.md rows PERF-MALLOC-IVB-CYCLES, latest-free (owner #2826, #3210), PAIR-CHAIN NEGATIVE; parity on Skylake-SP, see README) |
| cbench_malloc/realloc_null_24 | Ivy Bridge | 0.857 | 1.168 | 0.002 | NONE | (not briefed: saturated, docs/saturation/04-allocator.md rows PERF-MALLOC-IVB-CYCLES, latest-free (owner #2826, #3210), PAIR-CHAIN NEGATIVE; parity on Skylake-SP, see README) |
| cbench_malloc/realloc_within_usable | Ivy Bridge | 0.995 | 1.299 | 0.000 | NONE | (not briefed: saturated, docs/saturation/04-allocator.md rows PERF-MALLOC-IVB-CYCLES, latest-free (owner #2826, #3210), PAIR-CHAIN NEGATIVE; parity on Skylake-SP, see README) |
| cbench_memref/__memcmpeq_eq_128 | Ivy Bridge | 0.955 | 1.263 | 0.000 | re-time | TIMING-COV-MEMCMP-NONAVX2-4 (landed after 34c98f61; its own counts leave head/diff8 above 1.10) -> residual in TIMING-COV-COMPARE-HEAD-SSE2 |
| cbench_memref/__memcmpeq_eq_1k | Ivy Bridge | 0.963 | 1.077 | 0.394 | re-time | TIMING-COV-MEMCMP-NONAVX2-4 (landed after 34c98f61); spread 0.30-0.39 |
| cbench_memref/__memcmpeq_head_128 | Ivy Bridge | 1.282 | 1.538 | 0.001 | re-time | TIMING-COV-MEMCMP-NONAVX2-4 (landed after 34c98f61; its own counts leave head/diff8 above 1.10) -> residual in TIMING-COV-COMPARE-HEAD-SSE2 |
| cbench_memref/__memcmpeq_head_1k | Ivy Bridge | 1.288 | 1.538 | 0.001 | re-time | TIMING-COV-MEMCMP-NONAVX2-4 (landed after 34c98f61; its own counts leave head/diff8 above 1.10) -> residual in TIMING-COV-COMPARE-HEAD-SSE2 |
| cbench_memref/bcmp_eq_128 | Ivy Bridge | 0.926 | 1.260 | 0.050 | re-time | TIMING-COV-MEMCMP-NONAVX2-4 (landed after 34c98f61; its own counts leave head/diff8 above 1.10) -> residual in TIMING-COV-COMPARE-HEAD-SSE2 |
| cbench_memref/bcmp_eq_1k | Ivy Bridge | 0.960 | 1.065 | 0.297 | re-time | TIMING-COV-MEMCMP-NONAVX2-4 (landed after 34c98f61); spread 0.30-0.39 |
| cbench_memref/bcmp_head_128 | Ivy Bridge | 1.080 | 1.462 | 0.000 | re-time | TIMING-COV-MEMCMP-NONAVX2-4 (landed after 34c98f61; its own counts leave head/diff8 above 1.10) -> residual in TIMING-COV-COMPARE-HEAD-SSE2 |
| cbench_memref/bcmp_head_1k | Ivy Bridge | 1.084 | 1.462 | 0.000 | re-time | TIMING-COV-MEMCMP-NONAVX2-4 (landed after 34c98f61; its own counts leave head/diff8 above 1.10) -> residual in TIMING-COV-COMPARE-HEAD-SSE2 |
| cbench_memref/bcmp_tail_128 | Ivy Bridge | 0.826 | 1.101 | 0.002 | re-time | TIMING-COV-MEMCMP-NONAVX2-4 (landed after 34c98f61; its own counts leave head/diff8 above 1.10) -> residual in TIMING-COV-COMPARE-HEAD-SSE2 |
| cbench_memref/memchr_16 | Ivy Bridge | 1.001 | 1.251 | 0.000 | NONE | TIMING-COV-STRSCAN-SHORT-IVB |
| cbench_memref/memchr_64 | Ivy Bridge | 0.780 | 1.117 | 0.000 | NONE | TIMING-COV-STRSCAN-SHORT-IVB |
| cbench_memref/memchr_64k | N100 | 0.770 | 1.066 | 0.028 | NONE | (unbriefed: 64k scans N100, rank 11; 1106 1.163/1.064) |
| cbench_memref/memcmp_128 | Ivy Bridge | 0.895 | 1.052 | 0.052 | re-time | TIMING-COV-MEMCMP-NONAVX2-4 (landed after 34c98f61; its own counts leave head/diff8 above 1.10) -> residual in TIMING-COV-COMPARE-HEAD-SSE2 |
| cbench_memref/memcmp_256 | Ivy Bridge | 1.010 | 1.162 | 0.028 | re-time | TIMING-COV-MEMCMP-NONAVX2-4 (landed after 34c98f61; its own counts leave head/diff8 above 1.10) -> residual in TIMING-COV-COMPARE-HEAD-SSE2 |
| cbench_memref/memcmp_256 | N100 | 0.913 | 1.061 | 0.000 | NONE | (unbriefed: memcmp_256 N100 AVX2, rank 10) |
| cbench_memref/memcmp_diff8_127 | Ivy Bridge | 1.396 | 1.334 | 0.000 | re-time | TIMING-COV-MEMCMP-NONAVX2-4 (landed after 34c98f61; its own counts leave head/diff8 above 1.10) -> residual in TIMING-COV-COMPARE-HEAD-SSE2 |
| cbench_memref/memcmp_diff8_64 | Ivy Bridge | 1.317 | 1.251 | 0.000 | re-time | TIMING-COV-MEMCMP-NONAVX2-4 (landed after 34c98f61; its own counts leave head/diff8 above 1.10) -> residual in TIMING-COV-COMPARE-HEAD-SSE2 |
| cbench_memref/memcmp_diff8_96 | Ivy Bridge | 1.400 | 1.334 | 0.000 | re-time | TIMING-COV-MEMCMP-NONAVX2-4 (landed after 34c98f61; its own counts leave head/diff8 above 1.10) -> residual in TIMING-COV-COMPARE-HEAD-SSE2 |
| cbench_memref/memmove_144 | Ivy Bridge | 0.798 | 1.167 | 0.000 | NONE | (unbriefed: copy/fill 144 Ivy Bridge, rank 7) |
| cbench_memref/memset_144 | Ivy Bridge | 0.824 | 1.215 | 0.000 | NONE | (unbriefed: copy/fill 144 Ivy Bridge, rank 7) |
| cbench_memref/rawmemchr_4k | Ivy Bridge | 0.908 | 1.078 | 0.017 | NONE | TIMING-COV-STRSCAN-SHORT-IVB |
| cbench_memref/rawmemchr_64k | N100 | 0.822 | 1.064 | 0.012 | NONE | (unbriefed: 64k scans N100, rank 11; 1106 1.163/1.064) |
| cbench_memref/strstr_hit_4k | Ivy Bridge | 0.989 | 1.104 | 0.929 | NONE | TIMING-COV-STRSTR-PAIR (spread 0.929: re-time inside that story) |
| cbench_memref/strstr_hit_64 | Ivy Bridge | 1.555 | 1.675 | 0.007 | NONE | TIMING-COV-STRSTR-PAIR |
| cbench_memref/strstr_hit_64 | N100 | 1.262 | 1.350 | 0.009 | NONE | TIMING-COV-STRSTR-PAIR |
| cbench_memref/strstr_miss_64 | Ivy Bridge | 0.908 | 1.087 | 0.003 | NONE | TIMING-COV-STRSTR-PAIR |
| cbench_opt/first_localtime_r_new_york | Ivy Bridge | - | 1.811 | 0.012 | covered | PERF-ZONE-LOAD-FAULTS-3 (verifying) |
| cbench_opt/first_localtime_r_new_york | N100 | - | 1.855 | 0.067 | covered | PERF-ZONE-LOAD-FAULTS-3 (verifying) |
| cbench_opt/first_mktime_new_york | Ivy Bridge | - | 4.885 | 0.025 | re-time | PERF-FIRSTCALL-MKTIME-SNPRINTF-2 (landed after 34c98f61) |
| cbench_opt/first_mktime_new_york | N100 | - | 4.800 | 0.060 | re-time | PERF-FIRSTCALL-MKTIME-SNPRINTF-2 (landed after 34c98f61) |
| cbench_opt/first_setlocale_en_US | Ivy Bridge | - | 2.174 | 0.021 | NONE | TIMING-COV-COLD-SETLOCALE |
| cbench_opt/first_setlocale_en_US | N100 | - | 2.171 | 0.066 | NONE | TIMING-COV-COLD-SETLOCALE |
| cbench_opt/first_snprintf | Ivy Bridge | - | 1.449 | 0.017 | re-time | PERF-FIRSTCALL-MKTIME-SNPRINTF-2 (landed after 34c98f61) |
| cbench_opt/first_snprintf | N100 | - | 1.263 | 0.179 | re-time | PERF-FIRSTCALL-MKTIME-SNPRINTF-2 (landed after 34c98f61) |
| cbench_opt/first_strcmp | Ivy Bridge | - | 1.292 | 0.030 | NONE | (unbriefed: cold strcmp, rank 12: 30-45 ns once per process on the build host) |
| cbench_opt/first_strcmp | N100 | - | 1.444 | 0.104 | NONE | (unbriefed: cold strcmp, rank 12: 30-45 ns once per process on the build host) |
| cbench_opt/first_strcoll | Ivy Bridge | - | 23.930 | 0.027 | NONE | TIMING-COV-COLD-STRCOLL |
| cbench_opt/first_strcoll | N100 | - | 20.146 | 0.213 | NONE | TIMING-COV-COLD-STRCOLL |
| cbench_opt/gettext_language_steady | Ivy Bridge | 1.587 | 1.051 | 0.005 | NONE | (unbriefed: marginal 1.051, instructions unchanged since 1106) |
| cbench_opt/second_bindtextdomain_gettext | Ivy Bridge | - | 1.567 | 0.029 | NONE | TIMING-COV-COLD-SETLOCALE |
| cbench_opt/second_bindtextdomain_gettext | N100 | - | 1.626 | 0.122 | NONE | TIMING-COV-COLD-SETLOCALE |
| cbench_opt/second_mktime_new_york | Ivy Bridge | - | 1.193 | 0.046 | NONE | (unbriefed: rank 8; PERF-LOCALTIME-FIRST-SECOND-MKTIME (landed, analysis only) names the next step: attribute under the bench sequence, then cluster the mktime path) |
| cbench_opt/second_mktime_new_york | N100 | - | 1.217 | 0.119 | NONE | (unbriefed: rank 8; PERF-LOCALTIME-FIRST-SECOND-MKTIME (landed, analysis only) names the next step: attribute under the bench sequence, then cluster the mktime path) |
| cbench_opt/second_setlocale_en_US | Ivy Bridge | - | 1.717 | 0.043 | NONE | TIMING-COV-COLD-SETLOCALE |
| cbench_opt/second_setlocale_en_US | N100 | - | 1.545 | 0.215 | NONE | TIMING-COV-COLD-SETLOCALE |
| cbench_opt/second_strcmp | Ivy Bridge | - | 1.371 | 0.111 | NONE | (unbriefed: cold strcmp, rank 12: 30-45 ns once per process on the build host) |
| cbench_opt/second_strcmp | N100 | - | 1.455 | 0.062 | NONE | (unbriefed: cold strcmp, rank 12: 30-45 ns once per process on the build host) |
| cbench_opt/strcmp_names | N100 | 1.022 | 1.111 | 0.006 | NONE | (unbriefed: rank 13; TIMING-COV-STRCMP-NAMES-AVX2-2 is in the build) |
| cbench_patterns/regexec_backref | Ivy Bridge | 1.014 | 1.146 | 0.028 | NONE | (unbriefed: rank 14; TIMING-COV-REGEX-BACKREF-SCRATCH is in the build) |

Counts: NONE 56, covered 2, re-time 18
