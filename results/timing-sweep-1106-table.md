# Timing sweep 1106: non-libm rows, Ivy Bridge and N100 (Alder Lake-N)

Build: origin/next 4d454f06, staged once with `cargo xtask bench --stage --only <17 groups>` and run unchanged on
both timing hosts with `./xtask-bin bench --no-build --counters --only <17 groups>` (Liella and glibc on the same
host, rounds interleaved, V-4 procedure, the same 17 groups as the 1105 sweep: aio, fstree, hot, locale, malloc,
memref, netdb, nscache, opt, patterns, process, services, startup, stdio, thread, time, unmeasured). The previous
sweep (results/timing-sweep-1105-table.md) was built from next f2157504.

Hosts (CPU classes):
- Ivy Bridge (Xeon E3-1220L v2 class, 4 threads, no AVX2). 1105 ran on another host of the same class.
- N100 (Intel Alder Lake-N, 4 E-cores, AVX2). Its verdicts are for that core type, not for big cores.

Columns: `instr` = Liella/glibc instructions per call on that class (latest counts, this run); `time` = median
Liella/glibc time ratio over the interleaved rounds; `spread` = run-to-run spread of the round ratios for the row;
verdict faster (< 0.95), parity, slower (> 1.05); `1105 time` and `now/1105` = the same row in the 1105 sweep and
the change; `1106B re-time` = the new verdict of each of the 38 rows TIMING-COVERAGE-1106B marked "re-time".

## Summary

| | faster | parity | slower | spread median | spread p90 | spread max |
|---|---|---|---|---|---|---|
| Ivy Bridge now (404 judged rows) | 238 | 97 | 69 | 0.010 | 0.101 | 0.930 |
| Ivy Bridge 1105 | 230 | 95 | 79 | | | |
| N100 now (404 judged rows) | 307 | 73 | 24 | 0.015 | 0.105 | 0.300 |
| N100 1105 | 297 | 78 | 29 | | | |

Sweep-to-sweep drift (now/1105 over all rows): Ivy Bridge median 1.000 (p10 0.924, p90 1.027); N100 median 1.000
(p10 0.934, p90 1.010). A change inside the row's spread is not a finding. Ten wordexp rows are OWNER-DEFERRED:
timed and listed apart at the end of the table, not counted above.

The Ivy Bridge run's bench exited 1. The run finished and wrote every table; the exit came from the bench's own
per-host budget check (calibration main 5236e77), which flagged 11 rows: cbench_opt/iconv_utf8_utf16_mixed_4k,
cbench_memref/__memcmpeq_eq_64, _eq_128, _tail_64, _tail_128, _tail_1k, bcmp_eq_64, bcmp_eq_128, memchr_16_mix,
cbench_malloc/xthread_free_64 and cbench_hot/getenv_hit_mid. Each budget is 1.25 x a ratio calibrated on main
5236e77, so the check measures drift from that calibration, not slowness against glibc. Four of the 11 are still
faster than glibc: iconv_utf8_utf16_mixed_4k, __memcmpeq_tail_1k, xthread_free_64 (spread 0.283) and
getenv_hit_mid. Seven are slower than glibc in time on Ivy Bridge (__memcmpeq_eq_64/_eq_128/_tail_64/_tail_128,
bcmp_eq_64/_eq_128, memchr_16_mix; ratios 1.17 to 1.46) and were already slower in 1105, so they are not in list (a).
They are not in list (b) either, because their instruction ratio is above 1.05. The 1105 Ivy Bridge run also exited 1 on that check
(nscache). The N100 run exited 0 (all 431 budgets met).

## (a) Regressions: slower now, parity or faster in 1105 (4)

| row | class | instr now (1105) | time now (1105) | spread | likely cause |
|---|---|---|---|---|---|
| cbench_memref/memmove_4k | N100 | 1.000 (1.034) | 1.080 (0.965) | 0.008 | **parity-copy-avx2-3** (ff125cce): the only landing between the builds that changed the AVX2 copy entry (crates/boundary/src/hot.rs and csys/src/machine). It brought the row's instructions to parity, and its time ratio rose by 0.115. Real: spread 0.008 and cycles 1.080. Reopen on N100. |
| cbench_opt/first_bindtextdomain_gettext | Ivy Bridge | - | 1.053 (1.010) | 0.018 | Marginal (0.003 over the line). Landings on the gettext/locale path: perf-gettext-language-steady-5 (d2fe1b9d, the gettext lookup), parity-small-entry-1105n-2 (c9d7ca61), coldstart-tz-first (80d5bdfb). The most likely is perf-gettext-language-steady-5. Re-time before opening a story. |
| cbench_process/openpty_close | Ivy Bridge | 0.992 (0.992) | 1.073 (0.949) | 0.131 | No landing touched its path (crates/boundary/src/posix/s19_terminals.rs) between the builds. Instructions did not change, cycles 0.982, spread 0.131: noise, not a regression. |
| cbench_fstree/mkstemp_unlink_close | N100 | 1.029 (1.030) | 1.078 (1.009) | 0.233 | No landing touched its path (crates/boundary/src/posix/s13_filesystem.rs). Instructions did not change, cycles 1.025, spread 0.233 (it was 0.224 in 1105): noise, not a regression. |

## (b) Reopen, (c) improved, re-time verdicts

The lists below are the analyzer's output (handoff/timing-sweep-1106/analyze.py). List (b) has 39 entries. Most of
them were already on the 1105 reopen list with the same time ratio: Ivy Bridge malloc_free_* (TIMING-IVB-MALLOC-CHAIN-2
landed after the 1106 table, but Ivy Bridge times did not change), and the memcmp/memmove/memset/rawmemchr mid
sizes on Ivy Bridge. The strchr/strchrnul rows on Ivy Bridge improved 0.17 to 0.33 since 1105 but are still slower
(TIMING-IVB-STRSCAN-DISPATCH-2). List (c) has 19 rows that went from slower to parity or better, each with the
story behind it from the 1106B table. Three rows not in 1106B: cbench_hot/mutex_lock_unlock_normal_mt N100
(TIMING-N100-MUTEX-MT-2, 4d454f06), cbench_memref/strstr_miss_64 N100 (TIMING-N100-STRSTR-64, c101c4c6), and
cbench_process/posix_spawn_true_heap256m Ivy Bridge (spread 0.699: noise, no story). Their open stories may close
once the coordinator checks them.

## Lists

## REGRESSIONS (slower now, parity/faster in 1105): 4
- cbench_fstree/mkstemp_unlink_close | N100 | instr 1.029 (1105 1.030) | time 1.078 (1105 1.009) | spread 0.233 | cycles 1.025
- cbench_memref/memmove_4k | N100 | instr 1.000 (1105 1.034) | time 1.080 (1105 0.965) | spread 0.008 | cycles 1.080
- cbench_opt/first_bindtextdomain_gettext | Ivy Bridge | instr - (1105 -) | time 1.053 (1105 1.010) | spread 0.018 | cycles -
- cbench_process/openpty_close | Ivy Bridge | instr 0.992 (1105 0.992) | time 1.073 (1105 0.949) | spread 0.131 | cycles 0.982
## REOPEN (instr <= 1.05, time > 1.05): 39
- cbench_fstree/mkstemp_unlink_close | N100 | instr 1.029 (1105 1.030) | time 1.078 (1105 1.009) | spread 0.233 | cycles 1.025
- cbench_hot/strchr_16_hit | Ivy Bridge | instr 1.000 (1105 1.063) | time 1.116 (1105 1.318) | spread 0.000 | cycles 1.121
- cbench_hot/strchr_64_hit | Ivy Bridge | instr 1.000 (1105 1.036) | time 1.135 (1105 1.373) | spread 0.001 | cycles 1.142
- cbench_hot/strchr_64_miss | Ivy Bridge | instr 1.000 (1105 1.036) | time 1.288 (1105 1.472) | spread 0.003 | cycles 1.291
- cbench_hot/strchrnul_16_hit | Ivy Bridge | instr 1.000 (1105 1.069) | time 1.221 (1105 1.557) | spread 0.000 | cycles 1.227
- cbench_hot/strchrnul_64_hit | Ivy Bridge | instr 1.000 (1105 1.038) | time 1.267 (1105 1.534) | spread 0.000 | cycles 1.273
- cbench_hot/strchrnul_64_miss | Ivy Bridge | instr 1.000 (1105 1.038) | time 1.188 (1105 1.439) | spread 0.000 | cycles 1.187
- cbench_hot/strncmp_eq16 | Ivy Bridge | instr 1.015 (1105 1.015) | time 1.158 (1105 1.265) | spread 0.002 | cycles 1.165
- cbench_locale/nl_langinfo_en_us | Ivy Bridge | instr 0.839 (1105 1.129) | time 1.100 (1105 1.199) | spread 0.043 | cycles 1.100
- cbench_locale/strncasecmp_c_64 | N100 | instr 1.029 (1105 1.118) | time 1.050 (1105 1.099) | spread 0.000 | cycles 1.050
- cbench_locale/towupper_l_en_us | Ivy Bridge | instr 1.000 (1105 1.743) | time 1.067 (1105 1.573) | spread 0.003 | cycles 1.069
- cbench_malloc/malloc_free_1024 | Ivy Bridge | instr 0.830 (1105 0.830) | time 1.112 (1105 1.119) | spread 0.125 | cycles 1.115
- cbench_malloc/malloc_free_128 | Ivy Bridge | instr 0.830 (1105 0.830) | time 1.179 (1105 1.190) | spread 0.135 | cycles 1.177
- cbench_malloc/malloc_free_16 | Ivy Bridge | instr 0.830 (1105 0.830) | time 1.118 (1105 1.121) | spread 0.125 | cycles 1.118
- cbench_malloc/malloc_free_256 | Ivy Bridge | instr 0.830 (1105 0.830) | time 1.179 (1105 1.184) | spread 0.140 | cycles 1.175
- cbench_malloc/malloc_free_32 | Ivy Bridge | instr 0.830 (1105 0.830) | time 1.170 (1105 1.170) | spread 0.133 | cycles 1.168
- cbench_malloc/malloc_free_64 | Ivy Bridge | instr 0.830 (1105 0.830) | time 1.181 (1105 1.183) | spread 0.133 | cycles 1.177
- cbench_malloc/realloc_null_24 | Ivy Bridge | instr 0.857 (1105 0.851) | time 1.075 (1105 1.063) | spread 0.165 | cycles 1.076
- cbench_malloc/realloc_within_usable | Ivy Bridge | instr 0.995 (1105 0.980) | time 1.183 (1105 1.186) | spread 0.095 | cycles 1.178
- cbench_memref/__memcmpeq_head_8 | Ivy Bridge | instr 0.786 (1105 0.790) | time 1.100 (1105 1.101) | spread 0.002 | cycles 1.096
- cbench_memref/__memcmpeq_tail_8 | Ivy Bridge | instr 0.790 (1105 0.783) | time 1.221 (1105 1.221) | spread 0.000 | cycles 1.233
- cbench_memref/bcmp_tail_128 | Ivy Bridge | instr 0.960 (1105 0.960) | time 1.174 (1105 1.173) | spread 0.001 | cycles 1.210
- cbench_memref/memchr_64 | Ivy Bridge | instr 0.744 (1105 0.780) | time 1.059 (1105 1.177) | spread 0.000 | cycles 1.066
- cbench_memref/memchr_64k | N100 | instr 0.922 (1105 0.922) | time 1.163 (1105 1.169) | spread 0.038 | cycles 1.130
- cbench_memref/memcmp_128 | Ivy Bridge | instr 0.909 (1105 0.910) | time 1.105 (1105 1.103) | spread 0.051 | cycles 1.128
- cbench_memref/memcmp_144 | Ivy Bridge | instr 0.988 (1105 0.988) | time 1.173 (1105 1.172) | spread 0.001 | cycles 1.134
- cbench_memref/memcmp_256 | N100 | instr 0.913 (1105 0.913) | time 1.061 (1105 1.061) | spread 0.001 | cycles 1.059
- cbench_memref/memcmp_4k | N100 | instr 0.962 (1105 0.962) | time 1.084 (1105 1.086) | spread 0.064 | cycles 1.087
- cbench_memref/memcpy_144 | N100 | instr 1.001 (1105 1.000) | time 1.077 (1105 1.077) | spread 0.000 | cycles 1.077
- cbench_memref/memcpy_256 | N100 | instr 1.001 (1105 1.001) | time 1.077 (1105 1.077) | spread 0.000 | cycles 1.077
- cbench_memref/memmove_144 | Ivy Bridge | instr 0.830 (1105 0.831) | time 1.167 (1105 1.167) | spread 0.000 | cycles 1.165
- cbench_memref/memmove_4k | N100 | instr 1.000 (1105 1.034) | time 1.080 (1105 0.965) | spread 0.008 | cycles 1.080
- cbench_memref/memset_144 | Ivy Bridge | instr 0.870 (1105 0.866) | time 1.215 (1105 1.214) | spread 0.000 | cycles 1.221
- cbench_memref/rawmemchr_4k | Ivy Bridge | instr 0.908 (1105 0.908) | time 1.090 (1105 1.076) | spread 0.000 | cycles 1.087
- cbench_memref/rawmemchr_64k | N100 | instr 0.909 (1105 0.909) | time 1.064 (1105 1.060) | spread 0.014 | cycles 1.062
- cbench_memref/strstr_hit_4k | Ivy Bridge | instr 1.022 (1105 1.023) | time 1.106 (1105 1.277) | spread 0.840 | cycles 1.102
- cbench_opt/strcmp_names | N100 | instr 1.022 (1105 1.062) | time 1.110 (1105 1.142) | spread 0.007 | cycles 1.119
- cbench_patterns/regexec_class_nomatch_1k | Ivy Bridge | instr 0.946 (1105 0.946) | time 1.145 (1105 1.145) | spread 0.004 | cycles 1.063
- cbench_process/openpty_close | Ivy Bridge | instr 0.992 (1105 0.992) | time 1.073 (1105 0.949) | spread 0.131 | cycles 0.982
## IMPROVED (slower in 1105, parity/faster now): 19
- cbench_fstree/fts_physical_1000 | Ivy Bridge | instr 1.055 (1105 1.054) | time 1.030 (1105 1.052) | spread 0.003 | cycles 1.040 | 1106B: PERF-NFTW-ENTRY-12 landed after the sweep (re-timed 1.04 IVB there)
- cbench_hot/mutex_lock_unlock_normal_mt | N100 | instr 0.825 (1105 0.852) | time 0.985 (1105 1.199) | spread 0.002 | cycles 0.984
- cbench_hot/strchr_16_miss | Ivy Bridge | instr 0.714 (1105 0.750) | time 0.918 (1105 1.162) | spread 0.000 | cycles 0.921 | 1106B: TIMING-IVB-STRSCAN-DISPATCH-2 landed after the 1106 table
- cbench_locale/iconv_utf8_ascii_translit_4k | Ivy Bridge | instr 0.410 (1105 0.864) | time 0.492 (1105 1.299) | spread 0.037 | cycles 0.495 | 1106B: TIMING-ICONV-TRANSLIT-CPI-4 landed after the 1106 table
- cbench_locale/iconv_utf8_ascii_translit_4k | N100 | instr 0.410 (1105 0.864) | time 0.565 (1105 1.454) | spread 0.002 | cycles 0.552 | 1106B: TIMING-ICONV-TRANSLIT-CPI-4 landed after the 1106 table
- cbench_locale/iswalpha_en_us | Ivy Bridge | instr 1.244 (1105 1.856) | time 0.884 (1105 1.348) | spread 0.221 | cycles 0.888 | 1106B: PERF-WCTYPE-ENUS-2 landed after the sweep (-19..-30 instr on en_US rows)
- cbench_locale/iswalpha_en_us | N100 | instr 1.245 (1105 1.857) | time 0.988 (1105 1.485) | spread 0.003 | cycles 0.992 | 1106B: PERF-WCTYPE-ENUS-2 landed after the sweep (-19..-30 instr on en_US rows)
- cbench_locale/nl_langinfo_l_en_us | N100 | instr 1.250 (1105 1.571) | time 0.801 (1105 1.100) | spread 0.000 | cycles 0.795 | 1106B: PARITY-SMALL-ENTRY-1105N-2 landed after the 1106 table
- cbench_locale/towupper_l_en_us | N100 | instr 1.000 (1105 1.743) | time 0.909 (1105 1.454) | spread 0.000 | cycles 0.982 | 1106B: PERF-WCTYPE-ENUS-2 landed after the sweep (-19..-30 instr on en_US rows)
- cbench_malloc/realloc_grow_x2 | Ivy Bridge | instr 0.962 (1105 1.107) | time 0.888 (1105 1.093) | spread 0.042 | cycles 0.881 | 1106B: TIMING-COV-REALLOC-GROW-NONAVX2 landed after the 1106 table
- cbench_memref/strstr_miss_64 | N100 | instr 0.740 (1105 0.804) | time 0.987 (1105 1.102) | spread 0.026 | cycles 0.995
- cbench_opt/gettext_language_alternating | Ivy Bridge | instr 1.051 (1105 1.154) | time 0.988 (1105 1.065) | spread 0.028 | cycles 0.982 | 1106B: PERF-GETTEXT-LANGUAGE-STEADY-5 landed after the sweep (-32 instr/call)
- cbench_opt/gettext_language_steady | Ivy Bridge | instr 1.588 (1105 1.832) | time 1.045 (1105 1.209) | spread 0.006 | cycles 1.208 | 1106B: PERF-GETTEXT-LANGUAGE-STEADY-5 landed after the sweep (-32 instr/call)
- cbench_opt/gettext_language_steady | N100 | instr 1.400 (1105 1.575) | time 0.976 (1105 1.094) | spread 0.018 | cycles 0.985 | 1106B: PERF-GETTEXT-LANGUAGE-STEADY-5 landed after the sweep (-32 instr/call)
- cbench_opt/sprintf_chk_ls_nlink | Ivy Bridge | instr 1.035 (1105 1.171) | time 0.955 (1105 1.141) | spread 0.001 | cycles 0.956 | 1106B: PARITY-SPRINTF-CHK-LS-2 landed after the 1106 table
- cbench_process/posix_spawn_true_heap256m | Ivy Bridge | instr 1.017 (1105 1.029) | time 0.971 (1105 1.493) | spread 0.699 | cycles 0.981
- cbench_unmeasured/qsort_c_cmp_1024 | Ivy Bridge | instr 1.318 (1105 1.517) | time 0.998 (1105 1.083) | spread 0.410 | cycles 1.000 | 1106B: PERF-QSORT-OFFSET-EXP-2 landed after the sweep (-8..-10% instr); residual comparison surplus is by design (PERF-QSORT-COMPARISONS)
- cbench_unmeasured/qsort_c_cmp_1024_e4 | Ivy Bridge | instr 1.313 (1105 1.511) | time 0.713 (1105 1.082) | spread 0.930 | cycles 0.982 | 1106B: PERF-QSORT-OFFSET-EXP-2 landed after the sweep (-8..-10% instr); residual comparison surplus is by design (PERF-QSORT-COMPARISONS)
- cbench_unmeasured/qsort_c_cmp_1024_e8 | Ivy Bridge | instr 1.272 (1105 1.465) | time 1.010 (1105 1.103) | spread 0.294 | cycles 1.016 | 1106B: PERF-QSORT-OFFSET-EXP-2 landed after the sweep (-8..-10% instr); residual comparison surplus is by design (PERF-QSORT-COMPARISONS)
## 1106B re-time rows: 38
- cbench_fstree/fts_physical_1000 | IVB parity (1.030, 1105 1.052) | N100 parity (1.038, 1105 1.026) | PERF-NFTW-ENTRY-12 landed after the sweep (re-timed 1.04 IVB there)
- cbench_hot/strchr_16_hit | IVB slower (1.116, 1105 1.318) | N100 faster (0.777, 1105 0.777) | TIMING-IVB-STRSCAN-DISPATCH-2 landed after the 1106 table
- cbench_hot/strchr_16_miss | IVB faster (0.918, 1105 1.162) | N100 faster (0.728, 1105 0.728) | TIMING-IVB-STRSCAN-DISPATCH-2 landed after the 1106 table
- cbench_hot/strchr_64_hit | IVB slower (1.135, 1105 1.373) | N100 faster (0.909, 1105 0.909) | TIMING-IVB-STRSCAN-DISPATCH-2 landed after the 1106 table
- cbench_hot/strchr_64_miss | IVB slower (1.288, 1105 1.472) | N100 faster (0.909, 1105 0.909) | TIMING-IVB-STRSCAN-DISPATCH-2 landed after the 1106 table
- cbench_hot/strchrnul_16_hit | IVB slower (1.221, 1105 1.557) | N100 faster (0.701, 1105 0.701) | TIMING-IVB-STRSCAN-DISPATCH-2 landed after the 1106 table
- cbench_hot/strchrnul_64_hit | IVB slower (1.267, 1105 1.534) | N100 faster (0.820, 1105 0.820) | TIMING-IVB-STRSCAN-DISPATCH-2 landed after the 1106 table
- cbench_hot/strchrnul_64_miss | IVB slower (1.188, 1105 1.439) | N100 faster (0.751, 1105 0.751) | TIMING-IVB-STRSCAN-DISPATCH-2 landed after the 1106 table
- cbench_hot/strncmp_diff8 | IVB slower (1.585, 1105 1.752) | N100 parity (1.000, 1105 1.000) | TIMING-IVB-STRSCAN-DISPATCH-2 landed after the 1106 table
- cbench_hot/strncmp_eq16 | IVB slower (1.158, 1105 1.265) | N100 parity (1.000, 1105 1.000) | TIMING-IVB-STRSCAN-DISPATCH-2 landed after the 1106 table
- cbench_locale/iconv_utf8_ascii_translit_4k | IVB faster (0.492, 1105 1.299) | N100 faster (0.565, 1105 1.454) | TIMING-ICONV-TRANSLIT-CPI-4 landed after the 1106 table
- cbench_locale/iswalpha_en_us | IVB faster (0.884, 1105 1.348) | N100 parity (0.988, 1105 1.485) | PERF-WCTYPE-ENUS-2 landed after the sweep (-19..-30 instr on en_US rows)
- cbench_locale/iswalpha_l_en_us | IVB slower (1.330, 1105 1.791) | N100 slower (1.448, 1105 1.851) | PERF-WCTYPE-ENUS-2 landed after the sweep (-19..-30 instr on en_US rows)
- cbench_locale/iswalpha_utf8 | IVB parity (0.954, 1105 0.956) | N100 slower (1.063, 1105 1.132) | PERF-WCTYPE-ENUS-2 landed after the sweep (-19..-30 instr on en_US rows)
- cbench_locale/iswctype_en_us | IVB slower (2.090, 1105 2.600) | N100 slower (1.840, 1105 2.108) | PERF-WCTYPE-ENUS-2 landed after the sweep (-19..-30 instr on en_US rows)
- cbench_locale/nl_langinfo_en_us | IVB slower (1.100, 1105 1.199) | N100 faster (0.701, 1105 1.000) | PARITY-SMALL-ENTRY-1105N-2 landed after the 1106 table
- cbench_locale/nl_langinfo_l_en_us | IVB slower (1.221, 1105 1.557) | N100 faster (0.801, 1105 1.100) | PARITY-SMALL-ENTRY-1105N-2 landed after the 1106 table
- cbench_locale/strcasecmp_c_16 | IVB parity (1.022, 1105 1.024) | N100 slower (1.066, 1105 1.136) | TIMING-COV-STRCMP-SHORT-AVX2-2 landed after the 1106 table
- cbench_locale/strncasecmp_c_64 | IVB faster (0.947, 1105 0.950) | N100 slower (1.050, 1105 1.099) | TIMING-COV-STRCMP-SHORT-AVX2-2 landed after the 1106 table
- cbench_locale/towupper_l_en_us | IVB slower (1.067, 1105 1.573) | N100 faster (0.909, 1105 1.454) | PERF-WCTYPE-ENUS-2 landed after the sweep (-19..-30 instr on en_US rows)
- cbench_malloc/malloc_free_1024 | IVB slower (1.112, 1105 1.119) | N100 faster (0.739, 1105 0.738) | TIMING-IVB-MALLOC-CHAIN-2 landed after the 1106 table
- cbench_malloc/malloc_free_128 | IVB slower (1.179, 1105 1.190) | N100 faster (0.740, 1105 0.738) | TIMING-IVB-MALLOC-CHAIN-2 landed after the 1106 table
- cbench_malloc/malloc_free_16 | IVB slower (1.118, 1105 1.121) | N100 faster (0.739, 1105 0.738) | TIMING-IVB-MALLOC-CHAIN-2 landed after the 1106 table
- cbench_malloc/malloc_free_256 | IVB slower (1.179, 1105 1.184) | N100 faster (0.719, 1105 0.719) | TIMING-IVB-MALLOC-CHAIN-2 landed after the 1106 table
- cbench_malloc/malloc_free_32 | IVB slower (1.170, 1105 1.170) | N100 faster (0.742, 1105 0.738) | TIMING-IVB-MALLOC-CHAIN-2 landed after the 1106 table
- cbench_malloc/malloc_free_64 | IVB slower (1.181, 1105 1.183) | N100 faster (0.722, 1105 0.719) | TIMING-IVB-MALLOC-CHAIN-2 landed after the 1106 table
- cbench_malloc/realloc_grow_x2 | IVB faster (0.888, 1105 1.093) | N100 faster (0.788, 1105 0.962) | TIMING-COV-REALLOC-GROW-NONAVX2 landed after the 1106 table
- cbench_malloc/realloc_null_24 | IVB slower (1.075, 1105 1.063) | N100 faster (0.919, 1105 0.912) | TIMING-IVB-MALLOC-CHAIN-2 landed after the 1106 table
- cbench_malloc/realloc_within_usable | IVB slower (1.183, 1105 1.186) | N100 faster (0.905, 1105 0.944) | TIMING-IVB-MALLOC-CHAIN-2 landed after the 1106 table
- cbench_memref/memcpy_144 | IVB parity (1.011, 1105 1.010) | N100 slower (1.077, 1105 1.077) | PARITY-COPY-AVX2-3 landed after the sweep; TIMING-N100-COPY-MID (running)
- cbench_memref/memcpy_256 | IVB faster (0.740, 1105 0.730) | N100 slower (1.077, 1105 1.077) | PARITY-COPY-AVX2-3 landed after the sweep; TIMING-N100-COPY-MID (running)
- cbench_opt/gettext_language_alternating | IVB parity (0.988, 1105 1.065) | N100 faster (0.927, 1105 1.028) | PERF-GETTEXT-LANGUAGE-STEADY-5 landed after the sweep (-32 instr/call)
- cbench_opt/gettext_language_steady | IVB parity (1.045, 1105 1.209) | N100 parity (0.976, 1105 1.094) | PERF-GETTEXT-LANGUAGE-STEADY-5 landed after the sweep (-32 instr/call)
- cbench_opt/sprintf_chk_ls_nlink | IVB parity (0.955, 1105 1.141) | N100 faster (0.896, 1105 1.047) | PARITY-SPRINTF-CHK-LS-2 landed after the 1106 table
- cbench_opt/strcmp_names | IVB faster (0.773, 1105 0.759) | N100 slower (1.110, 1105 1.142) | TIMING-COV-STRCMP-SHORT-AVX2-2, PARITY-STRCMP-AVX2 landed after the 1106 table
- cbench_unmeasured/qsort_c_cmp_1024 | IVB parity (0.998, 1105 1.083) | N100 faster (0.822, 1105 0.937) | PERF-QSORT-OFFSET-EXP-2 landed after the sweep (-8..-10% instr); residual comparison surplus is by design (PERF-QSORT-COMPARISONS)
- cbench_unmeasured/qsort_c_cmp_1024_e4 | IVB faster (0.713, 1105 1.082) | N100 faster (0.801, 1105 0.913) | PERF-QSORT-OFFSET-EXP-2 landed after the sweep (-8..-10% instr); residual comparison surplus is by design (PERF-QSORT-COMPARISONS)
- cbench_unmeasured/qsort_c_cmp_1024_e8 | IVB parity (1.010, 1105 1.103) | N100 faster (0.800, 1105 0.930) | PERF-QSORT-OFFSET-EXP-2 landed after the sweep (-8..-10% instr); residual comparison surplus is by design (PERF-QSORT-COMPARISONS)
## rows not in the 1105 table: 0
## re-time rows not measured: []
spread ivb: n=414 median 0.010 p90 0.101 max 0.930
ivb now: faster 238, parity 97, slower 69 of 404; 1105: faster 230, parity 95, slower 79 of 404
spread n100: n=414 median 0.015 p90 0.105 max 0.300
n100 now: faster 307, parity 73, slower 24 of 404; 1105: faster 297, parity 78, slower 29 of 404

## Table

| bench/row | instr IVB | time IVB | spread IVB | verdict IVB | 1105 time IVB | now/1105 IVB | instr N100 | time N100 | spread N100 | verdict N100 | 1105 time N100 | now/1105 N100 | 1106B re-time |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| cbench_fstree/fdopendir_readdir_closedir_99 | 1.000 | 0.945 | 0.003 | faster | 0.949 | 0.995 | 1.000 | 0.941 | 0.012 | faster | 0.954 | 0.987 |  |
| cbench_fstree/fts_nochdir_flat_1000 | 0.846 | 0.891 | 0.007 | faster | 0.906 | 0.983 | 0.829 | 0.920 | 0.015 | faster | 0.922 | 0.997 |  |
| cbench_fstree/fts_physical_1000 | 1.055 | 1.030 | 0.003 | parity | 1.052 | 0.979 | 1.063 | 1.038 | 0.006 | parity | 1.026 | 1.012 | re-time: IVB parity, N100 parity |
| cbench_fstree/getcwd | 1.016 | 1.005 | 0.024 | parity | 0.999 | 1.006 | 1.023 | 1.007 | 0.008 | parity | 1.006 | 1.001 |  |
| cbench_fstree/getmntent_50_lines | 1.036 | 0.974 | 0.007 | parity | 0.976 | 0.998 | 1.041 | 0.863 | 0.011 | faster | 0.860 | 1.003 |  |
| cbench_fstree/mkstemp_unlink_close | 1.029 | 1.035 | 0.006 | parity | 1.035 | 1.000 | 1.029 | 1.078 | 0.233 | slower | 1.009 | 1.068 |  |
| cbench_fstree/nftw_phys_1000 | 1.024 | 0.992 | 0.016 | parity | 1.010 | 0.982 | 1.027 | 0.996 | 0.030 | parity | 0.997 | 0.999 |  |
| cbench_fstree/opendir_readdir_closedir_1000 | 1.000 | 0.939 | 0.003 | faster | 0.941 | 0.998 | 0.997 | 0.942 | 0.023 | faster | 0.945 | 0.997 |  |
| cbench_fstree/readdir_entry | 1.001 | 0.941 | 0.002 | faster | 0.943 | 0.998 | 0.998 | 0.944 | 0.020 | faster | 0.946 | 0.998 |  |
| cbench_fstree/realpath_5 | 1.038 | 1.024 | 0.144 | parity | 1.011 | 1.013 | 1.046 | 1.030 | 0.012 | parity | 1.030 | 1.000 |  |
| cbench_fstree/scandir_alphasort_1000 | 0.970 | 0.928 | 0.003 | faster | 0.931 | 0.997 | 0.974 | 0.930 | 0.014 | faster | 0.940 | 0.989 |  |
| cbench_hot/ctype_b_loc | 1.199 | 0.858 | 0.000 | faster | 0.856 | 1.002 | 1.198 | 0.376 | 0.000 | faster | 0.376 | 1.000 |  |
| cbench_hot/ctype_get_mb_cur_max | 0.909 | 1.000 | 0.000 | parity | 1.000 | 1.000 | 0.908 | 0.499 | 0.000 | faster | 0.499 | 0.999 |  |
| cbench_hot/errno_location | 0.900 | 1.000 | 0.000 | parity | 1.000 | 1.000 | 0.899 | 0.499 | 0.003 | faster | 0.499 | 0.999 |  |
| cbench_hot/fwrite_unlocked_16 | 0.729 | 0.701 | 0.027 | faster | 0.710 | 0.987 | 0.729 | 0.602 | 0.001 | faster | 0.602 | 1.000 |  |
| cbench_hot/fwrite_unlocked_64 | 0.704 | 0.714 | 0.012 | faster | 0.723 | 0.988 | 0.757 | 0.637 | 0.005 | faster | 0.637 | 1.000 |  |
| cbench_hot/getc | 0.817 | 1.000 | 0.004 | parity | 1.000 | 1.000 | 0.815 | 0.643 | 0.000 | faster | 0.643 | 1.000 |  |
| cbench_hot/getc_unlocked | 1.000 | 0.882 | 0.011 | faster | 0.888 | 0.993 | 0.999 | 0.423 | 0.000 | faster | 0.423 | 0.999 |  |
| cbench_hot/getenv_hit_last | 0.464 | 0.576 | 0.013 | faster | 0.574 | 1.003 | 0.805 | 0.677 | 0.005 | faster | 0.676 | 1.002 |  |
| cbench_hot/getenv_hit_last_2000 | 0.914 | 0.919 | 0.101 | faster | 0.956 | 0.961 | 0.921 | 0.886 | 0.000 | faster | 0.885 | 1.001 |  |
| cbench_hot/getenv_hit_last_300 | 0.899 | 1.012 | 0.007 | parity | 1.012 | 1.000 | 0.943 | 0.500 | 0.003 | faster | 0.499 | 1.001 |  |
| cbench_hot/getenv_hit_mid | 0.754 | 0.941 | 0.089 | faster | 0.937 | 1.004 | 0.844 | 0.773 | 0.001 | faster | 0.771 | 1.003 |  |
| cbench_hot/getenv_hit_mid_2000 | 0.918 | 0.956 | 0.039 | parity | 0.968 | 0.987 | 0.923 | 0.905 | 0.003 | faster | 0.905 | 1.000 |  |
| cbench_hot/getenv_hit_mid_300 | 0.928 | 1.033 | 0.001 | parity | 1.033 | 1.000 | 0.958 | 0.588 | 0.005 | faster | 0.587 | 1.002 |  |
| cbench_hot/getenv_miss | 0.417 | 0.513 | 0.001 | faster | 0.513 | 1.000 | 0.460 | 0.374 | 0.001 | faster | 0.371 | 1.008 |  |
| cbench_hot/getenv_miss_2000 | 0.914 | 0.923 | 0.099 | faster | 0.952 | 0.969 | 0.915 | 0.887 | 0.001 | faster | 0.887 | 1.000 |  |
| cbench_hot/getenv_miss_300 | 0.900 | 1.001 | 0.002 | parity | 1.001 | 1.000 | 0.904 | 0.494 | 0.006 | faster | 0.494 | 0.999 |  |
| cbench_hot/getline_80 | 0.995 | 1.010 | 0.043 | parity | 0.990 | 1.021 | 1.021 | 0.986 | 0.005 | parity | 0.989 | 0.997 |  |
| cbench_hot/getline_long | 1.196 | 1.121 | 0.024 | slower | 1.080 | 1.038 | 1.166 | 1.011 | 0.019 | parity | 1.009 | 1.002 |  |
| cbench_hot/memcpy_chk_64 | 0.692 | 0.799 | 0.002 | faster | 0.800 | 0.999 | 0.931 | 0.637 | 0.000 | faster | 0.637 | 1.000 |  |
| cbench_hot/mempcpy_16 | 0.955 | 1.000 | 0.000 | parity | 1.000 | 1.000 | 0.954 | 0.546 | 0.000 | faster | 0.546 | 0.999 |  |
| cbench_hot/mempcpy_64 | 0.719 | 0.799 | 0.000 | faster | 0.800 | 0.999 | 1.000 | 0.701 | 0.000 | faster | 0.701 | 1.000 |  |
| cbench_hot/memrchr_4k | 0.860 | 0.885 | 0.001 | faster | 0.885 | 1.000 | 0.897 | 1.037 | 0.000 | parity | 1.037 | 1.000 |  |
| cbench_hot/memrchr_64 | 0.906 | 1.000 | 0.000 | parity | 1.000 | 1.000 | 0.846 | 0.667 | 0.000 | faster | 0.667 | 1.000 |  |
| cbench_hot/memrchr_64_hit | 1.107 | 1.277 | 0.000 | slower | 1.276 | 1.001 | 1.000 | 0.777 | 0.000 | faster | 0.777 | 1.000 |  |
| cbench_hot/mutex_lock_unlock_normal | 0.847 | 0.951 | 0.004 | parity | 0.952 | 0.999 | 0.847 | 0.667 | 0.000 | faster | 0.904 | 0.737 |  |
| cbench_hot/mutex_lock_unlock_normal_mt | 0.824 | 0.820 | 0.006 | faster | 1.000 | 0.820 | 0.825 | 0.985 | 0.002 | parity | 1.199 | 0.821 |  |
| cbench_hot/mutex_lock_unlock_recursive | 0.878 | 0.780 | 0.002 | faster | 0.813 | 0.959 | 0.878 | 0.616 | 0.002 | faster | 0.715 | 0.862 |  |
| cbench_hot/mutex_lock_unlock_recursive_mt | 0.850 | 0.961 | 0.001 | parity | 0.962 | 0.999 | 0.849 | 0.701 | 0.024 | faster | 0.847 | 0.827 |  |
| cbench_hot/putc_unlocked | 0.897 | 0.863 | 0.004 | faster | 0.863 | 0.999 | 0.897 | 0.466 | 0.000 | faster | 0.466 | 1.000 |  |
| cbench_hot/putenv_2000 | 0.130 | 0.185 | 0.005 | faster | 0.185 | 0.999 | 0.144 | 0.199 | 0.260 | faster | 0.197 | 1.010 |  |
| cbench_hot/putenv_30 | 0.791 | 0.845 | 0.004 | faster | 0.842 | 1.004 | 0.888 | 0.679 | 0.218 | faster | 0.706 | 0.962 |  |
| cbench_hot/putenv_remove_cycle_2000 | 0.134 | 0.192 | 0.004 | faster | 0.192 | 1.001 | 0.150 | 0.228 | 0.131 | faster | 0.226 | 1.007 |  |
| cbench_hot/putenv_remove_cycle_30 | 0.930 | 0.988 | 0.007 | parity | 0.967 | 1.021 | 1.040 | 0.840 | 0.162 | faster | 0.891 | 0.942 |  |
| cbench_hot/setenv_2000 | 0.091 | 0.127 | 0.009 | faster | 0.127 | 0.998 | 0.146 | 0.219 | 0.107 | faster | 0.220 | 0.996 |  |
| cbench_hot/setenv_30 | 0.603 | 0.566 | 0.006 | faster | 0.564 | 1.003 | 0.851 | 0.575 | 0.062 | faster | 0.581 | 0.989 |  |
| cbench_hot/setenv_300 | 0.119 | 0.127 | 0.004 | faster | 0.125 | 1.012 | 0.190 | 0.133 | 0.008 | faster | 0.132 | 1.004 |  |
| cbench_hot/setenv_getenv_2000 | 0.129 | 0.172 | 0.012 | faster | 0.171 | 1.004 | 0.201 | 0.292 | 0.098 | faster | 0.294 | 0.993 |  |
| cbench_hot/setenv_getenv_300 | 0.159 | 0.168 | 0.003 | faster | 0.168 | 1.001 | 0.246 | 0.174 | 0.117 | faster | 0.174 | 0.998 |  |
| cbench_hot/strchr_16_hit | 1.000 | 1.116 | 0.000 | slower | 1.318 | 0.847 | 0.965 | 0.777 | 0.000 | faster | 0.777 | 1.000 | re-time: IVB slower, N100 faster |
| cbench_hot/strchr_16_miss | 0.714 | 0.918 | 0.000 | faster | 1.162 | 0.790 | 0.965 | 0.728 | 0.000 | faster | 0.728 | 1.000 | re-time: IVB faster, N100 faster |
| cbench_hot/strchr_64_hit | 1.000 | 1.135 | 0.001 | slower | 1.373 | 0.827 | 0.921 | 0.909 | 0.000 | faster | 0.909 | 1.000 | re-time: IVB slower, N100 faster |
| cbench_hot/strchr_64_miss | 1.000 | 1.288 | 0.003 | slower | 1.472 | 0.875 | 0.921 | 0.909 | 0.000 | faster | 0.909 | 1.000 | re-time: IVB slower, N100 faster |
| cbench_hot/strchrnul_16_hit | 1.000 | 1.221 | 0.000 | slower | 1.557 | 0.784 | 0.926 | 0.701 | 0.000 | faster | 0.701 | 1.000 | re-time: IVB slower, N100 faster |
| cbench_hot/strchrnul_16_miss | 0.698 | 0.750 | 0.000 | faster | 1.000 | 0.750 | 0.926 | 0.701 | 0.000 | faster | 0.701 | 1.000 |  |
| cbench_hot/strchrnul_64_hit | 1.000 | 1.267 | 0.000 | slower | 1.534 | 0.826 | 0.889 | 0.820 | 0.000 | faster | 0.820 | 0.999 | re-time: IVB slower, N100 faster |
| cbench_hot/strchrnul_64_miss | 1.000 | 1.188 | 0.000 | slower | 1.439 | 0.826 | 0.889 | 0.751 | 0.000 | faster | 0.751 | 1.000 | re-time: IVB slower, N100 faster |
| cbench_hot/strcpy_chk_16 | 0.488 | 0.600 | 0.001 | faster | 0.599 | 1.002 | 0.590 | 0.442 | 0.106 | faster | 0.442 | 0.999 |  |
| cbench_hot/strcpy_chk_64 | 0.587 | 0.599 | 0.004 | faster | 0.599 | 1.001 | 0.586 | 0.499 | 0.201 | faster | 0.555 | 0.900 |  |
| cbench_hot/strncmp_diff8 | 1.436 | 1.585 | 0.000 | slower | 1.752 | 0.905 | 1.000 | 1.000 | 0.000 | parity | 1.000 | 1.000 | re-time: IVB slower, N100 parity |
| cbench_hot/strncmp_eq16 | 1.015 | 1.158 | 0.002 | slower | 1.265 | 0.916 | 1.029 | 1.000 | 0.000 | parity | 1.000 | 1.000 | re-time: IVB slower, N100 parity |
| cbench_hot/strncmp_names64 | 1.072 | 0.971 | 0.068 | parity | 1.000 | 0.971 | 0.977 | 0.846 | 0.000 | faster | 0.846 | 1.000 |  |
| cbench_hot/sys_close_ebadf | 1.062 | 1.002 | 0.048 | parity | 1.026 | 0.976 | 1.066 | 1.004 | 0.000 | parity | 1.003 | 1.001 |  |
| cbench_hot/sys_fstat | 1.008 | 1.014 | 0.028 | parity | 1.006 | 1.008 | 1.008 | 1.002 | 0.022 | parity | 1.001 | 1.001 |  |
| cbench_hot/sys_read_1 | 1.012 | 1.021 | 0.075 | parity | 0.981 | 1.041 | 1.012 | 1.003 | 0.000 | parity | 1.003 | 1.000 |  |
| cbench_hot/unsetenv_cycle_2000 | 0.109 | 0.154 | 0.005 | faster | 0.153 | 1.008 | 0.149 | 0.219 | 0.124 | faster | 0.216 | 1.012 |  |
| cbench_hot/unsetenv_cycle_30 | 0.750 | 0.827 | 0.008 | faster | 0.827 | 1.000 | 1.005 | 0.836 | 0.161 | faster | 0.875 | 0.955 |  |
| cbench_hot/unsetenv_miss_2000 | 0.135 | 0.196 | 0.015 | faster | 0.196 | 1.001 | 0.150 | 0.237 | 0.003 | faster | 0.238 | 0.995 |  |
| cbench_hot/unsetenv_miss_30 | 0.592 | 0.599 | 0.003 | faster | 0.613 | 0.977 | 0.657 | 0.546 | 0.150 | faster | 0.546 | 1.000 |  |
| cbench_locale/fnmatch_casefold_en_32 | 0.967 | 0.837 | 0.011 | faster | 0.809 | 1.035 | 0.943 | 0.988 | 0.007 | parity | 0.985 | 1.003 |  |
| cbench_locale/fnmatch_en_us | 0.282 | 0.312 | 0.019 | faster | 0.312 | 1.000 | 0.299 | 0.323 | 0.007 | faster | 0.322 | 1.002 |  |
| cbench_locale/iconv_utf8_8859_15_4k | 0.321 | 0.277 | 0.003 | faster | 0.278 | 0.998 | 0.321 | 0.351 | 0.011 | faster | 0.351 | 1.000 |  |
| cbench_locale/iconv_utf8_ascii_translit_4k | 0.410 | 0.492 | 0.037 | faster | 1.299 | 0.379 | 0.410 | 0.565 | 0.002 | faster | 1.454 | 0.389 | re-time: IVB faster, N100 faster |
| cbench_locale/iconv_utf8_utf16_4k | 0.485 | 0.568 | 0.001 | faster | 0.641 | 0.886 | 0.485 | 0.737 | 0.007 | faster | 0.651 | 1.132 |  |
| cbench_locale/isalpha_c | 1.437 | 1.000 | 0.006 | parity | 1.000 | 1.000 | 1.436 | 0.554 | 0.000 | faster | 0.554 | 1.001 |  |
| cbench_locale/iswalpha_en_us | 1.244 | 0.884 | 0.221 | faster | 1.348 | 0.656 | 1.245 | 0.988 | 0.003 | parity | 1.485 | 0.665 | re-time: IVB faster, N100 parity |
| cbench_locale/iswalpha_l_en_us | 1.489 | 1.330 | 0.004 | slower | 1.791 | 0.743 | 1.489 | 1.448 | 0.000 | slower | 1.851 | 0.782 | re-time: IVB slower, N100 slower |
| cbench_locale/iswalpha_utf8 | 1.346 | 0.954 | 0.224 | parity | 0.956 | 0.998 | 1.347 | 1.063 | 0.003 | slower | 1.132 | 0.939 | re-time: IVB parity, N100 slower |
| cbench_locale/iswctype_en_us | 1.768 | 2.090 | 0.003 | slower | 2.600 | 0.804 | 1.767 | 1.840 | 0.000 | slower | 2.108 | 0.873 | re-time: IVB slower, N100 slower |
| cbench_locale/localeconv_en_us | 0.491 | 0.532 | 0.123 | faster | 0.532 | 1.000 | 0.491 | 0.520 | 0.001 | faster | 0.520 | 1.000 |  |
| cbench_locale/mbrtowc_en_us_3 | 0.889 | 0.885 | 0.001 | faster | 0.884 | 1.001 | 0.889 | 0.840 | 0.002 | faster | 0.842 | 0.997 |  |
| cbench_locale/mbrtowc_utf8_1 | 0.202 | 0.277 | 0.007 | faster | 0.277 | 0.998 | 0.202 | 0.229 | 0.036 | faster | 0.229 | 0.998 |  |
| cbench_locale/mbrtowc_utf8_2 | 0.241 | 0.299 | 0.020 | faster | 0.300 | 0.998 | 0.241 | 0.268 | 0.038 | faster | 0.269 | 0.997 |  |
| cbench_locale/mbrtowc_utf8_3 | 0.889 | 0.910 | 0.005 | faster | 0.910 | 1.000 | 0.889 | 0.858 | 0.000 | faster | 0.860 | 0.997 |  |
| cbench_locale/newlocale_free | 0.348 | 0.341 | 0.069 | faster | 0.339 | 1.005 | 0.282 | 0.309 | 0.115 | faster | 0.330 | 0.937 |  |
| cbench_locale/nl_langinfo_en_us | 0.839 | 1.100 | 0.043 | slower | 1.199 | 0.917 | 0.839 | 0.701 | 0.000 | faster | 1.000 | 0.701 | re-time: IVB slower, N100 faster |
| cbench_locale/nl_langinfo_l_en_us | 1.250 | 1.221 | 0.000 | slower | 1.557 | 0.784 | 1.250 | 0.801 | 0.000 | faster | 1.100 | 0.729 | re-time: IVB slower, N100 faster |
| cbench_locale/snprintf_f_en_us | 0.442 | 0.299 | 0.001 | faster | 0.307 | 0.974 | 0.442 | 0.410 | 0.016 | faster | 0.416 | 0.986 |  |
| cbench_locale/strcasecmp_c_16 | 0.979 | 1.022 | 0.119 | parity | 1.024 | 0.998 | 1.060 | 1.066 | 0.000 | slower | 1.136 | 0.939 | re-time: IVB parity, N100 slower |
| cbench_locale/strcasecmp_c_256 | 1.161 | 0.971 | 0.093 | parity | 0.971 | 1.000 | 1.068 | 0.803 | 0.002 | faster | 0.804 | 0.999 |  |
| cbench_locale/strcasecmp_en_256 | 1.161 | 0.976 | 0.071 | parity | 0.976 | 1.000 | 1.068 | 0.803 | 0.001 | faster | 0.804 | 0.999 |  |
| cbench_locale/strcasecmp_utf8_256 | 1.161 | 0.976 | 0.071 | parity | 0.976 | 1.000 | 1.068 | 0.803 | 0.001 | faster | 0.804 | 0.999 |  |
| cbench_locale/strcoll_utf8_16 | 0.576 | 0.731 | 0.020 | faster | 0.731 | 1.001 | 0.897 | 0.866 | 0.000 | faster | 0.866 | 1.000 |  |
| cbench_locale/strftime_names_en_us | 1.011 | 0.958 | 0.005 | parity | 0.956 | 1.002 | 1.011 | 0.937 | 0.002 | faster | 0.971 | 0.965 |  |
| cbench_locale/strncasecmp_c_64 | 0.932 | 0.947 | 0.357 | faster | 0.950 | 0.997 | 1.029 | 1.050 | 0.000 | slower | 1.099 | 0.956 | re-time: IVB faster, N100 slower |
| cbench_locale/strtod_c | 0.318 | 0.223 | 0.015 | faster | 0.223 | 1.001 | 0.316 | 0.244 | 0.022 | faster | 0.246 | 0.994 |  |
| cbench_locale/strtod_en_us | 0.318 | 0.226 | 0.004 | faster | 0.226 | 0.998 | 0.316 | 0.244 | 0.019 | faster | 0.246 | 0.992 |  |
| cbench_locale/strtod_utf8 | 0.318 | 0.225 | 0.007 | faster | 0.225 | 1.000 | 0.316 | 0.245 | 0.025 | faster | 0.246 | 0.997 |  |
| cbench_locale/toupper_c | 1.050 | 1.000 | 0.237 | parity | 1.000 | 1.000 | 1.049 | 0.554 | 0.000 | faster | 0.554 | 1.001 |  |
| cbench_locale/towupper_l_en_us | 1.000 | 1.067 | 0.003 | slower | 1.573 | 0.678 | 1.000 | 0.909 | 0.000 | faster | 1.454 | 0.625 | re-time: IVB slower, N100 faster |
| cbench_locale/towupper_utf8 | 1.324 | 0.845 | 0.237 | faster | 0.798 | 1.059 | 1.325 | 1.026 | 0.004 | parity | 0.944 | 1.087 |  |
| cbench_locale/wcwidth_utf8 | 1.162 | 0.790 | 0.265 | faster | 0.791 | 0.999 | 1.163 | 1.000 | 0.000 | parity | 1.000 | 1.000 |  |
| cbench_malloc/calloc_free_4096 | 0.440 | 0.831 | 0.012 | faster | 0.829 | 1.003 | 0.442 | 0.870 | 0.018 | faster | 0.869 | 1.001 |  |
| cbench_malloc/calloc_free_64 | 0.608 | 0.698 | 0.013 | faster | 0.696 | 1.003 | 0.608 | 0.599 | 0.124 | faster | 0.599 | 1.000 |  |
| cbench_malloc/heap_mix_real | 0.735 | 0.670 | 0.012 | faster | 0.678 | 0.989 | 0.737 | 0.697 | 0.006 | faster | 0.701 | 0.994 |  |
| cbench_malloc/heap_mix_real_2t | - | 0.448 | 0.055 | faster | 0.426 | 1.052 | - | 0.562 | 0.002 | faster | 0.565 | 0.995 |  |
| cbench_malloc/lifo_batch64 | 0.735 | 0.960 | 0.049 | parity | 0.950 | 1.010 | 0.735 | 0.698 | 0.001 | faster | 0.714 | 0.977 |  |
| cbench_malloc/malloc_free_1024 | 0.830 | 1.112 | 0.125 | slower | 1.119 | 0.994 | 0.830 | 0.739 | 0.004 | faster | 0.738 | 1.001 | re-time: IVB slower, N100 faster |
| cbench_malloc/malloc_free_128 | 0.830 | 1.179 | 0.135 | slower | 1.190 | 0.991 | 0.830 | 0.740 | 0.004 | faster | 0.738 | 1.003 | re-time: IVB slower, N100 faster |
| cbench_malloc/malloc_free_16 | 0.830 | 1.118 | 0.125 | slower | 1.121 | 0.997 | 0.830 | 0.739 | 0.003 | faster | 0.738 | 1.001 | re-time: IVB slower, N100 faster |
| cbench_malloc/malloc_free_256 | 0.830 | 1.179 | 0.140 | slower | 1.184 | 0.995 | 0.830 | 0.719 | 0.002 | faster | 0.719 | 1.001 | re-time: IVB slower, N100 faster |
| cbench_malloc/malloc_free_32 | 0.830 | 1.170 | 0.133 | slower | 1.170 | 1.000 | 0.830 | 0.742 | 0.004 | faster | 0.738 | 1.005 | re-time: IVB slower, N100 faster |
| cbench_malloc/malloc_free_64 | 0.830 | 1.181 | 0.133 | slower | 1.183 | 0.998 | 0.830 | 0.722 | 0.004 | faster | 0.719 | 1.004 | re-time: IVB slower, N100 faster |
| cbench_malloc/realloc_grow_16step | 0.539 | 0.542 | 0.013 | faster | 0.531 | 1.020 | 0.524 | 0.503 | 0.008 | faster | 0.496 | 1.015 |  |
| cbench_malloc/realloc_grow_x2 | 0.962 | 0.888 | 0.042 | faster | 1.093 | 0.813 | 0.829 | 0.788 | 0.031 | faster | 0.962 | 0.819 | re-time: IVB faster, N100 faster |
| cbench_malloc/realloc_moving | 0.564 | 0.570 | 0.005 | faster | 0.596 | 0.956 | 0.564 | 0.543 | 0.017 | faster | 0.580 | 0.936 |  |
| cbench_malloc/realloc_null_24 | 0.857 | 1.075 | 0.165 | slower | 1.063 | 1.012 | 0.856 | 0.919 | 0.136 | faster | 0.912 | 1.007 | re-time: IVB slower, N100 faster |
| cbench_malloc/realloc_within_usable | 0.995 | 1.183 | 0.095 | slower | 1.186 | 0.997 | 0.995 | 0.905 | 0.056 | faster | 0.944 | 0.959 | re-time: IVB slower, N100 faster |
| cbench_malloc/xthread_free_64 | - | 0.654 | 0.283 | faster | 0.684 | 0.956 | - | 0.468 | 0.007 | faster | 0.469 | 0.997 |  |
| cbench_memref/__memcmpeq_eq_128 | 1.109 | 1.350 | 0.000 | slower | 1.349 | 1.001 | 1.028 | 0.849 | 0.009 | faster | 0.847 | 1.002 |  |
| cbench_memref/__memcmpeq_eq_16 | 1.001 | 0.729 | 0.000 | faster | 0.728 | 1.001 | 0.769 | 0.419 | 0.013 | faster | 0.419 | 1.000 |  |
| cbench_memref/__memcmpeq_eq_1k | 1.212 | 1.247 | 0.319 | slower | 1.239 | 1.006 | 0.918 | 1.015 | 0.000 | parity | 1.015 | 1.000 |  |
| cbench_memref/__memcmpeq_eq_32 | 1.103 | 1.002 | 0.000 | parity | 1.002 | 1.000 | 1.188 | 0.730 | 0.017 | faster | 0.730 | 1.000 |  |
| cbench_memref/__memcmpeq_eq_64 | 1.373 | 1.188 | 0.001 | slower | 1.187 | 1.001 | 0.963 | 0.641 | 0.013 | faster | 0.641 | 0.999 |  |
| cbench_memref/__memcmpeq_eq_8 | 1.001 | 0.801 | 0.000 | faster | 0.801 | 1.000 | 0.851 | 0.457 | 0.023 | faster | 0.457 | 0.999 |  |
| cbench_memref/__memcmpeq_head_128 | 1.852 | 1.768 | 0.000 | slower | 1.768 | 1.000 | 1.195 | 0.732 | 0.020 | faster | 0.730 | 1.003 |  |
| cbench_memref/__memcmpeq_head_16 | 0.790 | 1.000 | 0.000 | parity | 1.000 | 1.000 | 0.669 | 0.638 | 0.037 | faster | 0.638 | 1.001 |  |
| cbench_memref/__memcmpeq_head_1k | 1.844 | 1.768 | 0.000 | slower | 1.768 | 1.000 | 1.199 | 0.730 | 0.018 | faster | 0.730 | 1.000 |  |
| cbench_memref/__memcmpeq_head_32 | 0.864 | 0.847 | 0.000 | faster | 0.847 | 1.000 | 0.902 | 0.638 | 0.020 | faster | 0.638 | 1.001 |  |
| cbench_memref/__memcmpeq_head_64 | 0.865 | 0.847 | 0.000 | faster | 0.847 | 1.000 | 0.804 | 0.638 | 0.021 | faster | 0.638 | 1.001 |  |
| cbench_memref/__memcmpeq_head_8 | 0.786 | 1.100 | 0.002 | slower | 1.101 | 0.999 | 0.670 | 0.638 | 0.014 | faster | 0.638 | 1.001 |  |
| cbench_memref/__memcmpeq_tail_128 | 1.109 | 1.421 | 0.001 | slower | 1.419 | 1.001 | 1.028 | 0.918 | 0.012 | faster | 0.920 | 0.998 |  |
| cbench_memref/__memcmpeq_tail_16 | 0.997 | 0.801 | 0.002 | faster | 0.801 | 1.000 | 0.854 | 0.502 | 0.021 | faster | 0.502 | 1.001 |  |
| cbench_memref/__memcmpeq_tail_1k | 0.178 | 0.264 | 0.158 | faster | 0.264 | 1.000 | 0.246 | 0.194 | 0.000 | faster | 0.194 | 1.002 |  |
| cbench_memref/__memcmpeq_tail_32 | 1.107 | 1.093 | 0.002 | slower | 1.091 | 1.002 | 1.545 | 0.801 | 0.009 | faster | 0.801 | 1.001 |  |
| cbench_memref/__memcmpeq_tail_64 | 1.370 | 1.265 | 0.001 | slower | 1.267 | 0.999 | 0.960 | 0.703 | 0.007 | faster | 0.703 | 1.000 |  |
| cbench_memref/__memcmpeq_tail_8 | 0.790 | 1.221 | 0.000 | slower | 1.221 | 1.000 | 0.670 | 0.701 | 0.021 | faster | 0.701 | 1.000 |  |
| cbench_memref/bcmp_eq_128 | 1.076 | 1.444 | 0.004 | slower | 1.444 | 1.000 | 0.950 | 0.918 | 0.010 | faster | 0.920 | 0.998 |  |
| cbench_memref/bcmp_eq_16 | 0.922 | 0.704 | 0.000 | faster | 0.703 | 1.001 | 0.660 | 0.419 | 0.017 | faster | 0.419 | 1.000 |  |
| cbench_memref/bcmp_eq_1k | 1.208 | 1.231 | 0.021 | slower | 1.249 | 0.985 | 0.913 | 1.015 | 0.000 | parity | 1.015 | 1.000 |  |
| cbench_memref/bcmp_eq_32 | 1.069 | 1.002 | 0.000 | parity | 1.000 | 1.002 | 1.145 | 0.703 | 0.020 | faster | 0.703 | 1.000 |  |
| cbench_memref/bcmp_eq_64 | 1.342 | 1.199 | 0.002 | slower | 1.200 | 0.999 | 0.928 | 0.703 | 0.007 | faster | 0.703 | 1.000 |  |
| cbench_memref/bcmp_eq_8 | 0.793 | 0.782 | 0.003 | faster | 0.779 | 1.004 | 0.794 | 0.502 | 0.007 | faster | 0.502 | 1.001 |  |
| cbench_memref/bcmp_head_128 | 1.559 | 1.915 | 0.001 | slower | 1.916 | 0.999 | 1.001 | 0.801 | 0.020 | faster | 0.801 | 1.001 |  |
| cbench_memref/bcmp_head_16 | 0.725 | 0.917 | 0.000 | faster | 0.918 | 0.999 | 0.565 | 0.638 | 0.018 | faster | 0.638 | 1.001 |  |
| cbench_memref/bcmp_head_1k | 1.559 | 1.915 | 0.001 | slower | 1.916 | 0.999 | 1.001 | 0.801 | 0.024 | faster | 0.801 | 1.001 |  |
| cbench_memref/bcmp_head_32 | 0.724 | 0.917 | 0.000 | faster | 0.918 | 0.999 | 0.753 | 0.701 | 0.016 | faster | 0.701 | 1.000 |  |
| cbench_memref/bcmp_head_64 | 0.721 | 0.917 | 0.000 | faster | 0.918 | 0.999 | 0.671 | 0.701 | 0.017 | faster | 0.701 | 1.000 |  |
| cbench_memref/bcmp_head_8 | 0.550 | 1.000 | 0.000 | parity | 1.000 | 1.000 | 0.566 | 0.638 | 0.024 | faster | 0.638 | 1.001 |  |
| cbench_memref/bcmp_tail_128 | 0.960 | 1.174 | 0.001 | slower | 1.173 | 1.001 | 0.841 | 0.786 | 0.012 | faster | 0.788 | 0.998 |  |
| cbench_memref/bcmp_tail_16 | 0.796 | 0.617 | 0.000 | faster | 0.616 | 1.001 | 0.722 | 0.419 | 0.020 | faster | 0.419 | 1.000 |  |
| cbench_memref/bcmp_tail_1k | 0.174 | 0.255 | 0.043 | faster | 0.254 | 1.002 | 0.238 | 0.191 | 0.000 | faster | 0.191 | 1.003 |  |
| cbench_memref/bcmp_tail_32 | 0.913 | 0.801 | 0.002 | faster | 0.802 | 0.999 | 1.290 | 0.730 | 0.020 | faster | 0.730 | 1.000 |  |
| cbench_memref/bcmp_tail_64 | 1.124 | 1.055 | 0.000 | slower | 1.056 | 0.999 | 0.784 | 0.587 | 0.010 | faster | 0.587 | 1.000 |  |
| cbench_memref/bcmp_tail_8 | 0.551 | 0.917 | 0.002 | faster | 0.918 | 0.999 | 0.565 | 0.585 | 0.025 | faster | 0.585 | 1.000 |  |
| cbench_memref/memchr_16 | 1.517 | 1.583 | 0.000 | slower | 1.584 | 0.999 | 0.895 | 0.637 | 0.002 | faster | 0.637 | 1.000 |  |
| cbench_memref/memchr_16_mix | 1.285 | 1.174 | 0.054 | slower | 1.171 | 1.003 | 0.916 | 0.624 | 0.018 | faster | 0.617 | 1.012 |  |
| cbench_memref/memchr_4k | 0.834 | 0.837 | 0.018 | faster | 0.883 | 0.948 | 0.906 | 1.036 | 0.001 | parity | 1.036 | 1.000 |  |
| cbench_memref/memchr_64 | 0.744 | 1.059 | 0.000 | slower | 1.177 | 0.899 | 0.727 | 0.541 | 0.000 | faster | 0.541 | 1.001 |  |
| cbench_memref/memchr_64_mix | 0.714 | 0.858 | 0.020 | faster | 0.931 | 0.921 | 0.694 | 0.464 | 0.025 | faster | 0.458 | 1.012 |  |
| cbench_memref/memchr_64k | 0.852 | 1.006 | 0.007 | parity | 1.006 | 1.000 | 0.922 | 1.163 | 0.038 | slower | 1.169 | 0.995 |  |
| cbench_memref/memcmp_128 | 0.909 | 1.105 | 0.051 | slower | 1.103 | 1.002 | 0.923 | 0.772 | 0.000 | faster | 0.772 | 1.000 |  |
| cbench_memref/memcmp_144 | 0.988 | 1.173 | 0.001 | slower | 1.172 | 1.001 | 0.721 | 0.707 | 0.000 | faster | 0.707 | 1.000 |  |
| cbench_memref/memcmp_16 | 1.080 | 0.817 | 0.005 | faster | 0.817 | 1.000 | 0.930 | 0.549 | 0.011 | faster | 0.549 | 1.000 |  |
| cbench_memref/memcmp_16_mix | 1.050 | 0.924 | 0.000 | faster | 0.924 | 0.999 | 0.938 | 0.553 | 0.029 | faster | 0.556 | 0.995 |  |
| cbench_memref/memcmp_256 | 1.086 | 1.257 | 0.026 | slower | 1.226 | 1.025 | 0.913 | 1.061 | 0.001 | slower | 1.061 | 1.000 |  |
| cbench_memref/memcmp_4k | 1.008 | 0.976 | 0.000 | parity | 0.976 | 1.000 | 0.962 | 1.084 | 0.064 | slower | 1.086 | 0.998 |  |
| cbench_memref/memcmp_64 | 1.023 | 0.936 | 0.002 | faster | 0.936 | 1.000 | 1.001 | 0.730 | 0.000 | faster | 0.730 | 1.000 |  |
| cbench_memref/memcmp_64_mix | 1.020 | 0.960 | 0.050 | parity | 0.959 | 1.001 | 1.002 | 0.998 | 0.002 | parity | 0.998 | 1.000 |  |
| cbench_memref/memcmp_64k | 1.000 | 0.972 | 0.004 | parity | 0.969 | 1.003 | 0.942 | 0.831 | 0.010 | faster | 0.830 | 1.001 |  |
| cbench_memref/memcmp_diff8_127 | 1.677 | 1.583 | 0.000 | slower | 1.584 | 0.999 | 1.207 | 0.801 | 0.018 | faster | 0.801 | 1.001 |  |
| cbench_memref/memcmp_diff8_64 | 1.321 | 1.251 | 0.000 | slower | 1.250 | 1.001 | 1.249 | 0.801 | 0.008 | faster | 0.801 | 1.001 |  |
| cbench_memref/memcmp_diff8_96 | 1.672 | 1.583 | 0.000 | slower | 1.584 | 0.999 | 1.204 | 0.801 | 0.002 | faster | 0.801 | 1.001 |  |
| cbench_memref/memcpy_128 | 0.945 | 1.000 | 0.000 | parity | 1.000 | 1.000 | 0.935 | 0.752 | 0.000 | faster | 0.752 | 1.001 |  |
| cbench_memref/memcpy_144 | 0.884 | 1.011 | 0.031 | parity | 1.010 | 1.001 | 1.001 | 1.077 | 0.000 | slower | 1.077 | 1.000 | re-time: IVB parity, N100 slower |
| cbench_memref/memcpy_16 | 0.954 | 0.779 | 0.003 | faster | 0.779 | 1.000 | 0.949 | 0.404 | 0.005 | faster | 0.404 | 1.001 |  |
| cbench_memref/memcpy_16_mix | 0.998 | 1.001 | 0.000 | parity | 1.001 | 1.000 | 0.998 | 1.016 | 0.037 | parity | 1.023 | 0.993 |  |
| cbench_memref/memcpy_256 | 0.633 | 0.740 | 0.041 | faster | 0.730 | 1.014 | 1.001 | 1.077 | 0.000 | slower | 1.077 | 1.000 | re-time: IVB faster, N100 slower |
| cbench_memref/memcpy_4k | 26.375 | 0.867 | 0.004 | faster | 0.866 | 1.001 | 10.296 | 0.476 | 0.052 | faster | 0.479 | 0.993 |  |
| cbench_memref/memcpy_4k_off | 0.996 | 0.889 | 0.000 | faster | 0.889 | 1.000 | 1.006 | 1.048 | 0.001 | parity | 1.048 | 1.000 |  |
| cbench_memref/memcpy_64 | 0.694 | 0.644 | 0.000 | faster | 0.645 | 0.999 | 1.001 | 0.559 | 0.012 | faster | 0.559 | 0.999 |  |
| cbench_memref/memcpy_64_mix | 1.001 | 1.001 | 0.001 | parity | 1.001 | 1.000 | 1.003 | 1.036 | 0.045 | parity | 1.029 | 1.007 |  |
| cbench_memref/memcpy_64k | 1.877 | 0.957 | 0.021 | parity | 0.952 | 1.005 | 95.069 | 0.991 | 0.003 | parity | 0.989 | 1.002 |  |
| cbench_memref/memmem_miss_4k | 0.428 | 0.579 | 0.302 | faster | 0.417 | 1.389 | 0.187 | 0.227 | 0.004 | faster | 0.227 | 0.998 |  |
| cbench_memref/memmem_miss_64 | 0.650 | 0.501 | 0.031 | faster | 0.501 | 1.000 | 0.547 | 0.268 | 0.009 | faster | 0.268 | 1.000 |  |
| cbench_memref/memmove_128 | 0.947 | 1.001 | 0.000 | parity | 1.001 | 1.000 | 0.935 | 0.901 | 0.000 | faster | 0.901 | 1.000 |  |
| cbench_memref/memmove_144 | 0.830 | 1.167 | 0.000 | slower | 1.167 | 1.000 | 1.001 | 0.859 | 0.000 | faster | 0.859 | 1.000 |  |
| cbench_memref/memmove_16 | 0.950 | 0.900 | 0.000 | faster | 0.900 | 1.000 | 0.953 | 0.910 | 0.023 | faster | 0.927 | 0.982 |  |
| cbench_memref/memmove_256 | 0.699 | 1.001 | 0.000 | parity | 1.000 | 1.001 | 1.000 | 0.947 | 0.000 | faster | 0.947 | 1.000 |  |
| cbench_memref/memmove_4k | 0.889 | 1.007 | 0.000 | parity | 1.006 | 1.001 | 1.000 | 1.080 | 0.008 | slower | 0.965 | 1.119 |  |
| cbench_memref/memmove_64 | 0.694 | 1.000 | 0.000 | parity | 1.001 | 0.999 | 0.997 | 0.937 | 0.000 | faster | 0.937 | 1.000 |  |
| cbench_memref/memmove_64k | 0.798 | 1.007 | 0.014 | parity | 1.007 | 1.000 | 0.922 | 0.989 | 0.011 | parity | 0.991 | 0.998 |  |
| cbench_memref/memset_128 | 0.748 | 0.735 | 0.000 | faster | 0.734 | 1.001 | 0.964 | 0.852 | 0.059 | faster | 0.801 | 1.063 |  |
| cbench_memref/memset_144 | 0.870 | 1.215 | 0.000 | slower | 1.214 | 1.001 | 0.882 | 0.911 | 0.007 | faster | 0.946 | 0.963 |  |
| cbench_memref/memset_16 | 0.954 | 0.779 | 0.000 | faster | 0.779 | 1.000 | 1.049 | 0.452 | 0.010 | faster | 0.452 | 1.000 |  |
| cbench_memref/memset_16_mix | 1.005 | 1.000 | 0.000 | parity | 1.000 | 1.000 | 1.002 | 0.995 | 0.000 | parity | 0.995 | 1.000 |  |
| cbench_memref/memset_256 | 0.751 | 1.001 | 0.000 | parity | 1.000 | 1.001 | 0.884 | 0.911 | 0.107 | faster | 0.911 | 1.000 |  |
| cbench_memref/memset_4k | 1.001 | 1.002 | 0.000 | parity | 1.000 | 1.002 | 1.038 | 1.000 | 0.000 | parity | 1.000 | 1.000 |  |
| cbench_memref/memset_4k_off | 1.004 | 1.007 | 0.000 | parity | 1.007 | 1.000 | 1.000 | 1.000 | 0.000 | parity | 1.000 | 1.000 |  |
| cbench_memref/memset_64 | 0.718 | 0.693 | 0.000 | faster | 0.694 | 0.999 | 1.001 | 0.559 | 0.013 | faster | 0.559 | 0.999 |  |
| cbench_memref/memset_64_mix | 0.997 | 1.000 | 0.000 | parity | 1.000 | 1.000 | 0.998 | 1.000 | 0.000 | parity | 1.000 | 1.000 |  |
| cbench_memref/memset_64k | 0.995 | 1.004 | 0.006 | parity | 0.999 | 1.005 | 1.012 | 0.991 | 0.038 | parity | 1.000 | 0.991 |  |
| cbench_memref/rawmemchr_16 | 0.911 | 0.799 | 0.000 | faster | 0.934 | 0.856 | 0.915 | 0.559 | 0.007 | faster | 0.559 | 0.999 |  |
| cbench_memref/rawmemchr_16_mix | 0.913 | 0.703 | 0.014 | faster | 0.802 | 0.876 | 0.935 | 0.641 | 0.000 | faster | 0.641 | 0.999 |  |
| cbench_memref/rawmemchr_4k | 0.908 | 1.090 | 0.000 | slower | 1.076 | 1.013 | 0.908 | 1.047 | 0.000 | parity | 1.047 | 1.000 |  |
| cbench_memref/rawmemchr_64 | 0.920 | 0.941 | 0.000 | faster | 1.000 | 0.941 | 0.938 | 0.730 | 0.012 | faster | 0.728 | 1.003 |  |
| cbench_memref/rawmemchr_64_mix | 0.920 | 0.811 | 0.010 | faster | 0.860 | 0.944 | 0.949 | 0.758 | 0.002 | faster | 0.758 | 1.000 |  |
| cbench_memref/rawmemchr_64k | 0.901 | 1.014 | 0.008 | parity | 1.012 | 1.002 | 0.909 | 1.064 | 0.014 | slower | 1.060 | 1.004 |  |
| cbench_memref/strcasestr_hit_4k | 0.132 | 0.128 | 0.283 | faster | 0.093 | 1.376 | 0.065 | 0.083 | 0.001 | faster | 0.083 | 1.001 |  |
| cbench_memref/strcasestr_hit_64 | 0.487 | 0.397 | 0.037 | faster | 0.427 | 0.930 | 0.452 | 0.399 | 0.027 | faster | 0.400 | 0.998 |  |
| cbench_memref/strcasestr_miss_4k | 0.079 | 0.098 | 0.408 | faster | 0.116 | 0.846 | 0.038 | 0.051 | 0.020 | faster | 0.050 | 1.012 |  |
| cbench_memref/strcasestr_miss_64 | 0.268 | 0.261 | 0.042 | faster | 0.271 | 0.963 | 0.237 | 0.218 | 0.044 | faster | 0.218 | 1.002 |  |
| cbench_memref/strcmp_16_mix | 0.716 | 0.407 | 0.057 | faster | 0.389 | 1.047 | 1.127 | 0.963 | 0.033 | parity | 1.028 | 0.937 |  |
| cbench_memref/strcmp_64_mix | 0.948 | 0.576 | 0.023 | faster | 0.566 | 1.017 | 1.165 | 0.977 | 0.300 | parity | 1.006 | 0.971 |  |
| cbench_memref/strlen_16 | 0.644 | 0.929 | 0.000 | faster | 0.928 | 1.001 | 0.904 | 0.559 | 0.017 | faster | 0.559 | 0.999 |  |
| cbench_memref/strlen_16_mix | 0.697 | 0.888 | 0.037 | faster | 0.873 | 1.017 | 0.930 | 0.703 | 0.005 | faster | 0.701 | 1.003 |  |
| cbench_memref/strlen_4k | 0.923 | 1.008 | 0.149 | parity | 1.007 | 1.001 | 0.909 | 1.008 | 0.000 | parity | 1.008 | 1.000 |  |
| cbench_memref/strlen_64 | 0.649 | 0.789 | 0.003 | faster | 0.789 | 1.000 | 0.941 | 0.730 | 0.009 | faster | 0.730 | 1.000 |  |
| cbench_memref/strlen_64_mix | 0.680 | 0.564 | 0.002 | faster | 0.565 | 0.998 | 0.951 | 0.820 | 0.002 | faster | 0.820 | 1.000 |  |
| cbench_memref/strlen_64k | 0.891 | 1.001 | 0.002 | parity | 1.002 | 0.999 | 0.901 | 1.003 | 0.036 | parity | 0.983 | 1.020 |  |
| cbench_memref/strnlen_16 | 0.809 | 0.999 | 0.057 | parity | 0.999 | 1.000 | 1.077 | 0.730 | 0.009 | faster | 0.730 | 1.000 |  |
| cbench_memref/strnlen_16_mix | 0.833 | 0.982 | 0.009 | parity | 0.982 | 1.000 | 1.059 | 0.752 | 0.000 | faster | 0.752 | 1.001 |  |
| cbench_memref/strnlen_4k | 0.813 | 0.662 | 0.002 | faster | 0.661 | 1.002 | 0.946 | 1.024 | 0.000 | parity | 1.024 | 1.000 |  |
| cbench_memref/strnlen_64 | 0.838 | 0.869 | 0.000 | faster | 0.869 | 1.000 | 0.855 | 0.858 | 0.016 | faster | 0.858 | 1.000 |  |
| cbench_memref/strnlen_64_mix | 0.851 | 0.740 | 0.009 | faster | 0.734 | 1.008 | 0.873 | 0.831 | 0.021 | faster | 0.831 | 1.000 |  |
| cbench_memref/strnlen_64k | 0.755 | 1.022 | 0.009 | parity | 1.020 | 1.002 | 0.919 | 1.021 | 0.021 | parity | 1.025 | 0.996 |  |
| cbench_memref/strstr_hit_4k | 1.022 | 1.106 | 0.840 | slower | 1.277 | 0.866 | 0.290 | 0.647 | 0.023 | faster | 0.639 | 1.012 |  |
| cbench_memref/strstr_hit_64 | 2.138 | 2.174 | 0.145 | slower | 2.398 | 0.907 | 1.262 | 1.351 | 0.003 | slower | 1.437 | 0.940 |  |
| cbench_memref/strstr_miss_4k | 0.360 | 0.412 | 0.724 | faster | 0.397 | 1.037 | 0.141 | 0.270 | 0.004 | faster | 0.271 | 0.996 |  |
| cbench_memref/strstr_miss_64 | 1.370 | 1.375 | 0.132 | slower | 1.411 | 0.974 | 0.740 | 0.987 | 0.026 | parity | 1.102 | 0.896 |  |
| cbench_memref/wcsstr_hit_4k | 0.684 | 0.669 | 0.002 | faster | 0.667 | 1.002 | 0.160 | 0.343 | 0.001 | faster | 0.345 | 0.995 |  |
| cbench_memref/wcsstr_hit_64 | 0.989 | 0.933 | 0.006 | faster | 0.921 | 1.013 | 0.646 | 0.840 | 0.001 | faster | 0.885 | 0.949 |  |
| cbench_memref/wcsstr_miss_4k | 0.732 | 0.630 | 0.001 | faster | 0.630 | 1.000 | 0.171 | 0.399 | 0.002 | faster | 0.398 | 1.002 |  |
| cbench_memref/wcsstr_miss_64 | 1.019 | 0.976 | 0.107 | parity | 0.927 | 1.053 | 0.636 | 0.839 | 0.006 | faster | 0.839 | 1.000 |  |
| cbench_opt/cold_exec_empty | - | 0.483 | 0.034 | faster | 0.419 | 1.152 | - | 0.423 | 0.128 | faster | 0.373 | 1.133 |  |
| cbench_opt/cold_exec_once | - | 0.762 | 0.028 | faster | 0.716 | 1.064 | - | 0.741 | 0.035 | faster | 0.736 | 1.007 |  |
| cbench_opt/execvp_missing | 1.008 | 1.024 | 0.021 | parity | 1.030 | 0.994 | 1.012 | 1.039 | 0.020 | parity | 1.041 | 0.998 |  |
| cbench_opt/first_bindtextdomain_gettext | - | 1.053 | 0.018 | slower | 1.010 | 1.043 | - | 1.035 | 0.034 | parity | 1.042 | 0.993 |  |
| cbench_opt/first_fnmatch | - | 0.188 | 0.051 | faster | 0.990 | 0.190 | - | 0.094 | 0.117 | faster | 0.656 | 0.143 |  |
| cbench_opt/first_gettext_nocatalog | - | 0.691 | 0.011 | faster | 0.681 | 1.014 | - | 0.615 | 0.052 | faster | 0.621 | 0.990 |  |
| cbench_opt/first_iconv_open_conv_close | - | 0.086 | 0.014 | faster | 0.082 | 1.044 | - | 0.087 | 0.107 | faster | 0.086 | 1.010 |  |
| cbench_opt/first_localtime_r_new_york | - | 1.727 | 0.027 | slower | 1.956 | 0.883 | - | 1.772 | 0.032 | slower | 2.120 | 0.836 |  |
| cbench_opt/first_mktime_new_york | - | 0.914 | 0.059 | faster | 0.773 | 1.182 | - | 0.994 | 0.111 | parity | 0.924 | 1.076 |  |
| cbench_opt/first_regcomp_regexec_regfree | - | 0.981 | 0.013 | parity | 0.910 | 1.078 | - | 0.972 | 0.038 | parity | 0.930 | 1.045 |  |
| cbench_opt/first_setlocale_en_US | - | 2.424 | 0.017 | slower | 2.225 | 1.090 | - | 2.313 | 0.027 | slower | 2.223 | 1.040 |  |
| cbench_opt/first_snprintf | - | 1.162 | 0.012 | slower | 1.126 | 1.032 | - | 0.990 | 0.229 | parity | 0.964 | 1.027 |  |
| cbench_opt/first_strcmp | - | 1.358 | 0.081 | slower | 1.474 | 0.922 | - | 1.447 | 0.245 | slower | 1.368 | 1.058 |  |
| cbench_opt/first_strcoll | - | 24.050 | 0.010 | slower | 20.884 | 1.152 | - | 25.042 | 0.117 | slower | 19.542 | 1.281 |  |
| cbench_opt/first_strftime_c | - | 0.867 | 0.039 | faster | 0.661 | 1.312 | - | 0.721 | 0.195 | faster | 0.772 | 0.934 |  |
| cbench_opt/fnmatch_names_casefold | 0.677 | 0.665 | 0.018 | faster | 0.665 | 1.000 | 0.715 | 0.738 | 0.007 | faster | 0.747 | 0.989 |  |
| cbench_opt/fnmatch_names_ch | 0.450 | 0.456 | 0.015 | faster | 0.466 | 0.978 | 0.481 | 0.471 | 0.022 | faster | 0.482 | 0.977 |  |
| cbench_opt/fnmatch_names_class | 0.506 | 0.565 | 0.015 | faster | 0.578 | 0.977 | 0.537 | 0.582 | 0.030 | faster | 0.593 | 0.981 |  |
| cbench_opt/fnmatch_names_lib_so | 0.364 | 0.414 | 0.401 | faster | 0.415 | 0.997 | 0.394 | 0.410 | 0.011 | faster | 0.417 | 0.984 |  |
| cbench_opt/fnmatch_names_period | 0.633 | 0.799 | 0.014 | faster | 0.799 | 1.000 | 0.695 | 0.762 | 0.014 | faster | 0.780 | 0.977 |  |
| cbench_opt/fnmatch_names_question | 0.371 | 0.365 | 0.038 | faster | 0.367 | 0.995 | 0.396 | 0.437 | 0.016 | faster | 0.435 | 1.004 |  |
| cbench_opt/fnmatch_names_star_c | 0.368 | 0.378 | 0.055 | faster | 0.379 | 0.997 | 0.395 | 0.394 | 0.016 | faster | 0.400 | 0.984 |  |
| cbench_opt/fnmatch_paths_leading_dir | 0.379 | 0.446 | 0.018 | faster | 0.447 | 0.999 | 0.409 | 0.437 | 0.006 | faster | 0.440 | 0.993 |  |
| cbench_opt/fnmatch_paths_pathname | 0.400 | 0.431 | 0.028 | faster | 0.431 | 0.999 | 0.425 | 0.445 | 0.010 | faster | 0.460 | 0.967 |  |
| cbench_opt/fprintf_du_line | 0.924 | 0.955 | 0.004 | parity | 0.937 | 1.019 | 0.923 | 0.844 | 0.037 | faster | 0.830 | 1.017 |  |
| cbench_opt/fprintf_ls_line | 0.744 | 0.646 | 0.003 | faster | 0.649 | 0.996 | 0.739 | 0.670 | 0.057 | faster | 0.673 | 0.996 |  |
| cbench_opt/gettext_c_locale | 0.134 | 0.125 | 0.073 | faster | 0.122 | 1.024 | 0.139 | 0.105 | 0.011 | faster | 0.105 | 0.997 |  |
| cbench_opt/gettext_found | 1.119 | 0.894 | 0.045 | faster | 0.993 | 0.900 | 1.137 | 0.899 | 0.011 | faster | 1.008 | 0.892 |  |
| cbench_opt/gettext_language_alternating | 1.051 | 0.988 | 0.028 | parity | 1.065 | 0.928 | 1.097 | 0.927 | 0.073 | faster | 1.028 | 0.902 | re-time: IVB parity, N100 faster |
| cbench_opt/gettext_language_putenv_alone | 0.655 | 0.696 | 0.186 | faster | 0.696 | 1.000 | 0.699 | 0.654 | 0.152 | faster | 0.676 | 0.967 |  |
| cbench_opt/gettext_language_steady | 1.588 | 1.045 | 0.006 | parity | 1.209 | 0.865 | 1.400 | 0.976 | 0.018 | parity | 1.094 | 0.892 | re-time: IVB parity, N100 parity |
| cbench_opt/gettext_missing | 0.262 | 0.260 | 0.026 | faster | 0.271 | 0.960 | 0.282 | 0.289 | 0.035 | faster | 0.311 | 0.931 |  |
| cbench_opt/gettext_no_catalog | 0.336 | 0.331 | 0.018 | faster | 0.325 | 1.018 | 0.365 | 0.393 | 0.045 | faster | 0.398 | 0.988 |  |
| cbench_opt/gettext_vary_msgid | 0.371 | 0.371 | 0.043 | faster | 0.397 | 0.933 | 0.402 | 0.405 | 0.019 | faster | 0.450 | 0.900 |  |
| cbench_opt/iconv_8859_15_utf8_latin | 0.354 | 0.379 | 0.032 | faster | 0.377 | 1.004 | 0.354 | 0.427 | 0.247 | faster | 0.427 | 1.000 |  |
| cbench_opt/iconv_open_conv_close_name | 0.268 | 0.259 | 0.028 | faster | 0.269 | 0.963 | 0.283 | 0.272 | 0.023 | faster | 0.284 | 0.956 |  |
| cbench_opt/iconv_utf16_utf8_mixed | 0.753 | 0.662 | 0.005 | faster | 0.643 | 1.030 | 0.753 | 0.644 | 0.000 | faster | 0.697 | 0.924 |  |
| cbench_opt/iconv_utf8_8859_15_latin_4k | 0.333 | 0.360 | 0.004 | faster | 0.361 | 0.997 | 0.333 | 0.339 | 0.015 | faster | 0.340 | 0.997 |  |
| cbench_opt/iconv_utf8_utf16_ascii_4k | 0.215 | 0.196 | 0.001 | faster | 0.197 | 0.997 | 0.215 | 0.230 | 0.000 | faster | 0.230 | 0.998 |  |
| cbench_opt/iconv_utf8_utf16_mixed_4k | 0.620 | 0.775 | 0.001 | faster | 0.608 | 1.275 | 0.620 | 0.764 | 0.000 | faster | 0.686 | 1.114 |  |
| cbench_opt/iconv_utf8_utf16_name | 0.719 | 0.841 | 0.013 | faster | 0.870 | 0.966 | 0.720 | 0.741 | 0.019 | faster | 0.796 | 0.931 |  |
| cbench_opt/iconv_utf8_utf32_mixed_4k | 0.614 | 0.725 | 0.004 | faster | 0.741 | 0.979 | 0.614 | 0.721 | 0.002 | faster | 0.716 | 1.007 |  |
| cbench_opt/localtime_r_new_york | 0.071 | 0.080 | 0.030 | faster | 0.079 | 1.007 | 0.071 | 0.074 | 0.132 | faster | 0.083 | 0.897 |  |
| cbench_opt/localtime_r_posix_rule | 0.615 | 0.733 | 0.037 | faster | 0.718 | 1.021 | 0.615 | 0.602 | 0.164 | faster | 0.694 | 0.868 |  |
| cbench_opt/mktime_new_york | 0.640 | 0.634 | 0.017 | faster | 0.626 | 1.013 | 0.652 | 0.676 | 0.029 | faster | 0.670 | 1.008 |  |
| cbench_opt/mktime_posix_rule | 0.807 | 0.826 | 0.026 | faster | 0.825 | 1.001 | 0.827 | 0.887 | 0.037 | faster | 0.868 | 1.022 |  |
| cbench_opt/mktime_utc | 0.562 | 0.569 | 0.026 | faster | 0.567 | 1.003 | 0.558 | 0.652 | 0.046 | faster | 0.630 | 1.036 |  |
| cbench_opt/ngettext_found | 0.892 | 0.832 | 0.042 | faster | 0.949 | 0.877 | 0.943 | 0.891 | 0.021 | faster | 0.978 | 0.911 |  |
| cbench_opt/re_grep_alt | 0.074 | 0.086 | 0.015 | faster | 0.085 | 1.014 | 0.069 | 0.071 | 0.043 | faster | 0.071 | 1.003 |  |
| cbench_opt/re_grep_comment | 0.206 | 0.207 | 0.009 | faster | 0.210 | 0.988 | 0.192 | 0.173 | 0.029 | faster | 0.172 | 1.007 |  |
| cbench_opt/re_grep_dotstar | 0.260 | 0.284 | 0.016 | faster | 0.284 | 1.001 | 0.245 | 0.233 | 0.027 | faster | 0.233 | 1.000 |  |
| cbench_opt/re_grep_icase_absent | 0.316 | 0.310 | 0.015 | faster | 0.304 | 1.020 | 0.309 | 0.300 | 0.005 | faster | 0.299 | 1.005 |  |
| cbench_opt/re_grep_ipv4 | 0.072 | 0.084 | 0.007 | faster | 0.085 | 0.984 | 0.070 | 0.078 | 0.015 | faster | 0.077 | 1.009 |  |
| cbench_opt/re_grep_literal | 0.527 | 0.429 | 0.015 | faster | 0.410 | 1.046 | 0.492 | 0.332 | 0.065 | faster | 0.329 | 1.009 |  |
| cbench_opt/re_mt4_distinct | 1.472 | 0.154 | 0.425 | faster | 0.158 | 0.975 | 1.564 | 0.141 | 0.019 | faster | 0.140 | 1.009 |  |
| cbench_opt/re_mt4_shared | 1.531 | 0.020 | 0.213 | faster | 0.021 | 0.935 | 1.807 | 0.023 | 0.144 | faster | 0.023 | 1.021 |  |
| cbench_opt/re_search_1byte | 0.231 | 0.181 | 0.009 | faster | 0.182 | 0.993 | 0.223 | 0.179 | 0.048 | faster | 0.183 | 0.977 |  |
| cbench_opt/re_search_sed_so | 0.565 | 0.416 | 0.007 | faster | 0.453 | 0.918 | 0.553 | 0.409 | 0.045 | faster | 0.411 | 0.996 |  |
| cbench_opt/re_search_sed_swap | 0.288 | 0.295 | 0.369 | faster | 0.293 | 1.008 | 0.284 | 0.253 | 0.038 | faster | 0.251 | 1.007 |  |
| cbench_opt/re_sed_kv | 0.215 | 0.223 | 0.013 | faster | 0.223 | 0.998 | 0.210 | 0.190 | 0.003 | faster | 0.189 | 1.006 |  |
| cbench_opt/re_sed_swap | 0.081 | 0.087 | 0.006 | faster | 0.087 | 0.997 | 0.080 | 0.081 | 0.007 | faster | 0.081 | 1.000 |  |
| cbench_opt/regcomp_regfree_ipv4 | 0.841 | 0.831 | 0.037 | faster | 0.825 | 1.007 | 0.797 | 0.830 | 0.011 | faster | 0.834 | 0.996 |  |
| cbench_opt/regcomp_regfree_sed_kv | 0.399 | 0.472 | 0.055 | faster | 0.467 | 1.011 | 0.387 | 0.426 | 0.011 | faster | 0.432 | 0.987 |  |
| cbench_opt/regexec_sed_so | 0.391 | 0.300 | 0.018 | faster | 0.301 | 0.996 | 0.369 | 0.280 | 0.137 | faster | 0.272 | 1.029 |  |
| cbench_opt/regexec_sed_swap | 0.287 | 0.289 | 0.013 | faster | 0.290 | 0.996 | 0.283 | 0.251 | 0.049 | faster | 0.249 | 1.007 |  |
| cbench_opt/second_bindtextdomain_gettext | - | 10.173 | 0.021 | slower | 6.982 | 1.457 | - | 11.541 | 0.082 | slower | 7.695 | 1.500 |  |
| cbench_opt/second_fnmatch | - | 0.491 | 0.048 | faster | 0.468 | 1.048 | - | 0.411 | 0.085 | faster | 0.437 | 0.941 |  |
| cbench_opt/second_gettext_nocatalog | - | 0.440 | 0.041 | faster | 0.413 | 1.065 | - | 0.302 | 0.109 | faster | 0.346 | 0.874 |  |
| cbench_opt/second_iconv_open_conv_close | - | 0.208 | 0.019 | faster | 0.192 | 1.084 | - | 0.206 | 0.040 | faster | 0.174 | 1.184 |  |
| cbench_opt/second_localtime_r_new_york | - | 0.905 | 0.067 | faster | 0.944 | 0.959 | - | 0.908 | 0.066 | faster | 0.898 | 1.011 |  |
| cbench_opt/second_mktime_new_york | - | 1.256 | 0.026 | slower | 1.114 | 1.127 | - | 1.150 | 0.127 | slower | 1.276 | 0.901 |  |
| cbench_opt/second_regcomp_regexec_regfree | - | 0.437 | 0.010 | faster | 0.424 | 1.030 | - | 0.425 | 0.126 | faster | 0.432 | 0.984 |  |
| cbench_opt/second_setlocale_en_US | - | 1.712 | 0.038 | slower | 1.354 | 1.265 | - | 1.311 | 0.174 | slower | 1.432 | 0.916 |  |
| cbench_opt/second_snprintf | - | 0.925 | 0.015 | faster | 0.891 | 1.038 | - | 1.003 | 0.126 | parity | 0.957 | 1.048 |  |
| cbench_opt/second_strcmp | - | 1.388 | 0.054 | slower | 1.283 | 1.082 | - | 1.432 | 0.201 | slower | 1.535 | 0.933 |  |
| cbench_opt/second_strcoll | - | 0.876 | 0.031 | faster | 0.791 | 1.107 | - | 0.869 | 0.090 | faster | 0.937 | 0.928 |  |
| cbench_opt/second_strftime_c | - | 0.738 | 0.046 | faster | 0.714 | 1.034 | - | 0.673 | 0.189 | faster | 0.672 | 1.002 |  |
| cbench_opt/snprintf_d | 0.839 | 0.823 | 0.074 | faster | 0.814 | 1.012 | 0.839 | 0.970 | 0.020 | parity | 0.971 | 0.999 |  |
| cbench_opt/snprintf_e | 0.957 | 0.885 | 0.001 | faster | 0.902 | 0.981 | 0.959 | 0.920 | 0.016 | faster | 0.938 | 0.980 |  |
| cbench_opt/snprintf_f2 | 0.454 | 0.314 | 0.001 | faster | 0.318 | 0.987 | 0.454 | 0.415 | 0.019 | faster | 0.419 | 0.990 |  |
| cbench_opt/snprintf_f_large | 0.177 | 0.222 | 0.004 | faster | 0.222 | 1.001 | 0.168 | 0.424 | 0.021 | faster | 0.426 | 0.995 |  |
| cbench_opt/snprintf_g | 0.593 | 0.423 | 0.001 | faster | 0.423 | 1.001 | 0.594 | 0.509 | 0.021 | faster | 0.516 | 0.987 |  |
| cbench_opt/snprintf_ld_large | 0.838 | 0.821 | 0.023 | faster | 0.807 | 1.017 | 0.838 | 0.888 | 0.012 | faster | 0.886 | 1.002 |  |
| cbench_opt/snprintf_ls_line | 1.013 | 0.821 | 0.003 | faster | 0.841 | 0.977 | 1.009 | 0.888 | 0.056 | faster | 0.881 | 1.008 |  |
| cbench_opt/snprintf_mixed | 0.615 | 0.446 | 0.003 | faster | 0.449 | 0.992 | 0.612 | 0.528 | 0.034 | faster | 0.553 | 0.955 |  |
| cbench_opt/snprintf_s_d_colon | 1.021 | 1.004 | 0.002 | parity | 0.933 | 1.076 | 1.019 | 0.903 | 0.064 | faster | 0.901 | 1.003 |  |
| cbench_opt/snprintf_s_path | 0.580 | 0.592 | 0.011 | faster | 0.645 | 0.918 | 0.580 | 0.587 | 0.051 | faster | 0.577 | 1.017 |  |
| cbench_opt/snprintf_x08 | 0.907 | 0.854 | 0.018 | faster | 0.901 | 0.948 | 0.907 | 0.830 | 0.015 | faster | 0.822 | 1.010 |  |
| cbench_opt/sprintf_chk_ls_nlink | 1.035 | 0.955 | 0.001 | parity | 1.141 | 0.837 | 1.033 | 0.896 | 0.070 | faster | 1.047 | 0.855 | re-time: IVB parity, N100 faster |
| cbench_opt/strcmp_equal_16 | 1.065 | 1.000 | 0.002 | parity | 1.000 | 1.000 | 1.036 | 0.891 | 0.003 | faster | 1.000 | 0.891 |  |
| cbench_opt/strcmp_late_256 | 1.458 | 0.983 | 0.003 | parity | 1.020 | 0.964 | 0.885 | 0.788 | 0.001 | faster | 0.878 | 0.897 |  |
| cbench_opt/strcmp_names | 0.943 | 0.773 | 0.057 | faster | 0.759 | 1.019 | 1.022 | 1.110 | 0.007 | slower | 1.142 | 0.972 | re-time: IVB faster, N100 slower |
| cbench_opt/strcoll_c_names | 0.820 | 0.745 | 0.041 | faster | 0.743 | 1.002 | 0.864 | 1.015 | 0.033 | parity | 1.044 | 0.972 |  |
| cbench_opt/strcoll_c_utf8_names | 0.820 | 0.753 | 0.419 | faster | 0.743 | 1.014 | 0.864 | 1.027 | 0.023 | parity | 1.045 | 0.983 |  |
| cbench_opt/strcoll_en_us_equal_16 | 0.018 | 0.015 | 0.003 | faster | 0.015 | 1.000 | 0.018 | 0.013 | 0.006 | faster | 0.013 | 1.034 |  |
| cbench_opt/strcoll_en_us_names | 0.430 | 0.452 | 0.060 | faster | 0.448 | 1.009 | 0.430 | 0.435 | 0.021 | faster | 0.435 | 1.001 |  |
| cbench_opt/strcoll_en_us_utf8_16 | 0.883 | 0.787 | 0.016 | faster | 0.784 | 1.004 | 0.878 | 0.624 | 0.042 | faster | 0.624 | 0.999 |  |
| cbench_opt/strftime_c_en_us | 0.437 | 0.417 | 0.008 | faster | 0.414 | 1.006 | 0.438 | 0.384 | 0.030 | faster | 0.390 | 0.985 |  |
| cbench_opt/strftime_date_en_us | 0.638 | 0.784 | 0.011 | faster | 0.788 | 0.995 | 0.629 | 0.599 | 0.046 | faster | 0.601 | 0.996 |  |
| cbench_opt/strftime_iso8601_tz | 0.547 | 0.574 | 0.019 | faster | 0.582 | 0.987 | 0.538 | 0.559 | 0.034 | faster | 0.559 | 1.000 |  |
| cbench_opt/strftime_log | 0.592 | 0.671 | 0.010 | faster | 0.674 | 0.996 | 0.584 | 0.609 | 0.032 | faster | 0.628 | 0.969 |  |
| cbench_opt/strftime_ls_iso | 0.553 | 0.610 | 0.023 | faster | 0.607 | 1.005 | 0.546 | 0.553 | 0.006 | faster | 0.553 | 0.999 |  |
| cbench_opt/strftime_ls_recent_c | 0.610 | 0.646 | 0.007 | faster | 0.647 | 0.998 | 0.605 | 0.608 | 0.064 | faster | 0.629 | 0.967 |  |
| cbench_opt/strftime_ls_recent_en_us | 0.610 | 0.647 | 0.015 | faster | 0.648 | 0.998 | 0.605 | 0.627 | 0.044 | faster | 0.630 | 0.995 |  |
| cbench_patterns/fnmatch_bracket | 0.827 | 0.832 | 0.002 | faster | 0.832 | 1.001 | 0.866 | 0.919 | 0.061 | faster | 0.918 | 1.001 |  |
| cbench_patterns/fnmatch_pathname | 0.809 | 0.796 | 0.006 | faster | 0.796 | 1.000 | 0.812 | 0.862 | 0.016 | faster | 0.869 | 0.992 |  |
| cbench_patterns/fnmatch_pathname_nomatch | 0.997 | 0.919 | 0.001 | faster | 0.920 | 0.999 | 1.000 | 0.994 | 0.047 | parity | 1.000 | 0.994 |  |
| cbench_patterns/fnmatch_star | 0.810 | 0.811 | 0.004 | faster | 0.808 | 1.004 | 0.868 | 0.792 | 0.088 | faster | 0.790 | 1.003 |  |
| cbench_patterns/glob_1000_nosort | 0.969 | 0.982 | 0.007 | parity | 0.986 | 0.996 | 0.969 | 0.976 | 0.019 | parity | 0.978 | 0.998 |  |
| cbench_patterns/glob_1000_sorted | 0.798 | 0.773 | 0.005 | faster | 0.782 | 0.989 | 0.851 | 0.740 | 0.014 | faster | 0.742 | 0.997 |  |
| cbench_patterns/regcomp_regexec_regfree_email | 0.281 | 0.339 | 0.014 | faster | 0.339 | 1.001 | 0.278 | 0.315 | 0.014 | faster | 0.317 | 0.993 |  |
| cbench_patterns/regexec_backref | 1.059 | 1.199 | 0.008 | slower | 1.181 | 1.015 | 1.044 | 1.000 | 0.013 | parity | 1.012 | 0.988 |  |
| cbench_patterns/regexec_class_nomatch_1k | 0.946 | 1.145 | 0.004 | slower | 1.145 | 1.000 | 0.941 | 0.863 | 0.005 | faster | 0.862 | 1.001 |  |
| cbench_patterns/regexec_literal_1k | 0.056 | 0.072 | 0.005 | faster | 0.072 | 1.004 | 0.039 | 0.048 | 0.006 | faster | 0.048 | 1.009 |  |
| cbench_patterns/regexec_pathological_4k | 0.000 | 0.000 | 0.003 | faster | 0.000 | - | 0.000 | 0.000 | 0.189 | faster | 0.000 | - |  |
| cbench_patterns/regexec_submatch_1k | 0.075 | 0.085 | 0.005 | faster | 0.084 | 1.012 | 0.073 | 0.092 | 0.020 | faster | 0.092 | 1.000 |  |
| cbench_process/cfsetspeed_cfgetospeed | 0.475 | 0.685 | 0.000 | faster | 0.685 | 1.000 | 0.475 | 0.625 | 0.000 | faster | 0.625 | 1.000 |  |
| cbench_process/fork_exit_wait | 1.087 | 1.287 | 0.027 | slower | 1.247 | 1.032 | 1.081 | 1.275 | 0.045 | slower | 1.294 | 0.985 |  |
| cbench_process/isatty_devnull | 0.960 | 0.975 | 0.045 | parity | 0.989 | 0.986 | 0.958 | 0.990 | 0.002 | parity | 0.989 | 1.001 |  |
| cbench_process/isatty_pty | 0.968 | 0.990 | 0.043 | parity | 0.971 | 1.020 | 0.956 | 0.993 | 0.009 | parity | 0.995 | 0.998 |  |
| cbench_process/openpty_close | 0.992 | 1.073 | 0.131 | slower | 0.949 | 1.130 | 0.992 | 0.981 | 0.014 | parity | 0.985 | 0.996 |  |
| cbench_process/popen_pclose_true | 1.000 | 0.993 | 0.386 | parity | 0.991 | 1.002 | 1.012 | 1.002 | 0.045 | parity | 1.001 | 1.001 |  |
| cbench_process/posix_spawn_true | 1.003 | 0.995 | 0.016 | parity | 0.991 | 1.004 | 1.003 | 1.003 | 0.023 | parity | 1.006 | 0.997 |  |
| cbench_process/posix_spawn_true_actions | 1.018 | 1.017 | 0.022 | parity | 0.997 | 1.020 | 1.010 | 0.997 | 0.036 | parity | 1.007 | 0.990 |  |
| cbench_process/posix_spawn_true_heap256m | 1.017 | 0.971 | 0.699 | parity | 1.493 | 0.651 | 1.026 | 0.994 | 0.038 | parity | 0.999 | 0.995 |  |
| cbench_process/posix_spawnp_true | 1.008 | 0.996 | 0.023 | parity | 1.005 | 0.991 | 0.998 | 1.000 | 0.023 | parity | 1.005 | 0.995 |  |
| cbench_process/system_exit0 | 1.001 | 1.002 | 0.027 | parity | 0.994 | 1.008 | 1.014 | 1.003 | 0.031 | parity | 1.011 | 0.992 |  |
| cbench_process/tcgetattr | 1.014 | 1.002 | 0.022 | parity | 0.986 | 1.016 | 1.007 | 1.009 | 0.032 | parity | 1.008 | 1.001 |  |
| cbench_process/tcsetattr_now | 0.794 | 0.720 | 0.029 | faster | 0.716 | 1.006 | 0.798 | 0.750 | 0.007 | faster | 0.749 | 1.001 |  |
| cbench_process/ttyname_r | 1.011 | 1.014 | 0.031 | parity | 1.006 | 1.008 | 1.012 | 1.015 | 0.011 | parity | 1.022 | 0.993 |  |
| cbench_process/vfork_exit_wait | 0.991 | 0.996 | 0.032 | parity | 0.993 | 1.003 | 1.000 | 1.009 | 0.041 | parity | 1.000 | 1.009 |  |
| cbench_process/waitpid_echild | 1.088 | 0.998 | 0.037 | parity | 1.014 | 0.985 | 1.090 | 1.014 | 0.001 | parity | 1.016 | 0.998 |  |
| cbench_startup/exec_hello_locale | - | 0.745 | 0.012 | faster | 0.714 | 1.043 | - | 0.661 | 0.030 | faster | 0.658 | 1.005 |  |
| cbench_startup/exec_true_echo | - | 0.585 | 0.017 | faster | 0.573 | 1.020 | - | 0.505 | 0.020 | faster | 0.508 | 0.995 |  |
| cbench_startup/exec_true_static | - | 0.402 | 0.042 | faster | 0.405 | 0.992 | - | 0.378 | 0.020 | faster | 0.373 | 1.013 |  |
| cbench_startup/spawn_bin_true | - | 1.012 | 0.025 | parity | 1.003 | 1.009 | - | 0.994 | 0.046 | parity | 1.009 | 0.986 |  |
| cbench_startup/startup_instr_c | 0.018 | 0.647 | 0.098 | faster | 0.626 | 1.034 | 0.018 | 0.560 | 0.183 | faster | 0.564 | 0.993 |  |
| cbench_startup/startup_instr_printf | 0.526 | 0.885 | 0.057 | faster | 0.851 | 1.040 | 0.493 | 0.821 | 0.120 | faster | 0.795 | 1.032 |  |
| cbench_startup/startup_instr_setloc | 0.506 | 0.876 | 0.176 | faster | 0.853 | 1.027 | 0.473 | 0.786 | 0.105 | faster | 0.822 | 0.957 |  |
| cbench_startup/startup_instr_wctype | 0.512 | 0.880 | 0.073 | faster | 0.865 | 1.017 | 0.480 | 0.835 | 0.079 | faster | 0.807 | 1.034 |  |
| cbench_stdio/fprintf_d | 0.926 | 0.920 | 0.005 | faster | 0.917 | 1.003 | 0.923 | 0.830 | 0.013 | faster | 0.828 | 1.002 |  |
| cbench_stdio/fputs_32 | 0.787 | 0.814 | 0.035 | faster | 0.822 | 0.991 | 0.845 | 0.817 | 0.051 | faster | 0.817 | 1.000 |  |
| cbench_stdio/fwrite_4k | 2.049 | 1.005 | 0.087 | parity | 0.999 | 1.006 | 1.616 | 0.856 | 0.005 | faster | 0.882 | 0.970 |  |
| cbench_stdio/putc | 0.752 | 1.000 | 0.005 | parity | 1.000 | 1.000 | 0.752 | 0.605 | 0.000 | faster | 0.605 | 1.000 |  |
| cbench_stdio/snprintf_d | 0.839 | 0.811 | 0.088 | faster | 0.827 | 0.980 | 0.839 | 0.959 | 0.025 | parity | 0.962 | 0.997 |  |
| cbench_stdio/snprintf_mixed | 0.602 | 0.434 | 0.007 | faster | 0.435 | 0.998 | 0.600 | 0.516 | 0.024 | faster | 0.542 | 0.952 |  |
| cbench_time/gmtime_r | 0.545 | 0.574 | 0.005 | faster | 0.574 | 1.001 | 0.545 | 0.590 | 0.001 | faster | 0.590 | 1.000 |  |
| cbench_time/localtime_r_new_york | 0.060 | 0.067 | 0.007 | faster | 0.067 | 1.005 | 0.060 | 0.063 | 0.037 | faster | 0.065 | 0.964 |  |
| cbench_time/localtime_r_new_york_2100 | 0.057 | 0.064 | 0.016 | faster | 0.064 | 0.997 | 0.057 | 0.060 | 0.026 | faster | 0.061 | 0.988 |  |
| cbench_time/localtime_r_posix_rule | 0.610 | 0.712 | 0.006 | faster | 0.716 | 0.995 | 0.609 | 0.616 | 0.035 | faster | 0.616 | 1.000 |  |
| cbench_time/localtime_r_utc | 0.586 | 0.796 | 0.011 | faster | 0.802 | 0.992 | 0.586 | 0.803 | 0.055 | faster | 0.812 | 0.989 |  |
| cbench_time/mktime_new_york | 0.553 | 0.532 | 0.012 | faster | 0.529 | 1.006 | 0.565 | 0.578 | 0.039 | faster | 0.578 | 0.999 |  |
| cbench_time/strftime_c | 0.474 | 0.483 | 0.008 | faster | 0.484 | 0.997 | 0.473 | 0.411 | 0.011 | faster | 0.407 | 1.010 |  |
| cbench_time/strftime_iso | 0.573 | 0.591 | 0.034 | faster | 0.605 | 0.976 | 0.562 | 0.563 | 0.006 | faster | 0.563 | 1.000 |  |
| cbench_time/timegm | 0.563 | 0.652 | 0.042 | faster | 0.659 | 0.989 | 0.563 | 0.802 | 0.016 | faster | 0.803 | 0.999 |  |
| cbench_unmeasured/comparisons_per_sort | - | 1.040 | 0.000 | parity | 1.040 | 1.000 | - | 1.040 | 0.000 | parity | 1.040 | 1.000 |  |
| cbench_unmeasured/comparisons_per_sort_e16 | - | 1.035 | 0.000 | parity | 1.035 | 1.000 | - | 1.035 | 0.000 | parity | 1.035 | 1.000 |  |
| cbench_unmeasured/comparisons_per_sort_e24 | - | 1.040 | 0.000 | parity | 1.040 | 1.000 | - | 1.040 | 0.000 | parity | 1.040 | 1.000 |  |
| cbench_unmeasured/comparisons_per_sort_e4 | - | 1.037 | 0.000 | parity | 1.037 | 1.000 | - | 1.037 | 0.000 | parity | 1.037 | 1.000 |  |
| cbench_unmeasured/comparisons_per_sort_e64 | - | 1.030 | 0.000 | parity | 1.030 | 1.000 | - | 1.030 | 0.000 | parity | 1.030 | 1.000 |  |
| cbench_unmeasured/comparisons_per_sort_e8 | - | 1.031 | 0.000 | parity | 1.031 | 1.000 | - | 1.031 | 0.000 | parity | 1.031 | 1.000 |  |
| cbench_unmeasured/fts_entry_deep2k | 0.298 | 0.497 | 0.031 | faster | 0.510 | 0.975 | 0.271 | 0.464 | 0.011 | faster | 0.466 | 0.996 |  |
| cbench_unmeasured/fts_entry_flat | 0.845 | 0.883 | 0.034 | faster | 0.897 | 0.984 | 0.825 | 0.901 | 0.010 | faster | 0.905 | 0.996 |  |
| cbench_unmeasured/qsort_c_cmp_1024 | 1.318 | 0.998 | 0.410 | parity | 1.083 | 0.921 | 1.312 | 0.822 | 0.043 | faster | 0.937 | 0.877 | re-time: IVB parity, N100 faster |
| cbench_unmeasured/qsort_c_cmp_1024_e16 | 0.758 | 0.639 | 0.005 | faster | 0.692 | 0.924 | 0.772 | 0.553 | 0.012 | faster | 0.644 | 0.859 |  |
| cbench_unmeasured/qsort_c_cmp_1024_e24 | 0.751 | 0.684 | 0.004 | faster | 0.734 | 0.932 | 0.775 | 0.587 | 0.018 | faster | 0.681 | 0.862 |  |
| cbench_unmeasured/qsort_c_cmp_1024_e4 | 1.313 | 0.713 | 0.930 | faster | 1.082 | 0.659 | 1.308 | 0.801 | 0.021 | faster | 0.913 | 0.877 | re-time: IVB faster, N100 faster |
| cbench_unmeasured/qsort_c_cmp_1024_e64 | 1.126 | 0.785 | 0.008 | faster | 0.839 | 0.936 | 1.181 | 0.788 | 0.025 | faster | 0.889 | 0.886 |  |
| cbench_unmeasured/qsort_c_cmp_1024_e8 | 1.272 | 1.010 | 0.294 | parity | 1.103 | 0.916 | 1.282 | 0.800 | 0.023 | faster | 0.930 | 0.860 | re-time: IVB parity, N100 faster |
| cbench_unmeasured/readdir_entry_flat | 1.010 | 0.957 | 0.007 | parity | 0.960 | 0.997 | 1.000 | 0.949 | 0.010 | faster | 0.957 | 0.992 |  |

### OWNER-DEFERRED: wordexp rows (timed, not judged)

| bench/row | instr IVB | time IVB | spread IVB | verdict IVB | 1105 time IVB | now/1105 IVB | instr N100 | time N100 | spread N100 | verdict N100 | 1105 time N100 | now/1105 N100 | 1106B re-time |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| cbench_opt/first_wordexp | - | 4.095 | 0.040 | slower | 3.427 | 1.195 | - | 2.228 | 0.085 | slower | 1.713 | 1.301 |  |
| cbench_opt/second_wordexp | - | 1.573 | 0.047 | slower | 1.532 | 1.027 | - | 1.341 | 0.029 | slower | 1.307 | 1.026 |  |
| cbench_opt/wordexp_arith | 1.169 | 1.479 | 0.012 | slower | 1.486 | 0.995 | 1.169 | 1.285 | 0.063 | slower | 1.219 | 1.054 |  |
| cbench_opt/wordexp_param_forms | 1.184 | 1.417 | 0.025 | slower | 1.355 | 1.046 | 1.187 | 1.122 | 0.042 | slower | 1.137 | 0.987 |  |
| cbench_opt/wordexp_plain_8 | 0.575 | 0.729 | 0.013 | faster | 0.713 | 1.022 | 0.593 | 0.569 | 0.029 | faster | 0.588 | 0.967 |  |
| cbench_opt/wordexp_quoted | 1.470 | 1.758 | 0.007 | slower | 1.734 | 1.014 | 1.478 | 1.533 | 0.050 | slower | 1.515 | 1.012 |  |
| cbench_opt/wordexp_split_var | 1.075 | 1.191 | 0.055 | slower | 1.179 | 1.010 | 1.082 | 0.965 | 0.027 | parity | 0.958 | 1.008 |  |
| cbench_opt/wordexp_tilde_var | 1.331 | 1.555 | 0.028 | slower | 1.526 | 1.019 | 1.345 | 1.277 | 0.052 | slower | 1.294 | 0.987 |  |
| cbench_patterns/wordexp_arith | 1.179 | 1.513 | 0.003 | slower | 1.543 | 0.981 | 1.189 | 1.231 | 0.051 | slower | 1.266 | 0.973 |  |
| cbench_patterns/wordexp_tilde_var | 1.364 | 1.640 | 0.057 | slower | 1.663 | 0.986 | 1.375 | 1.314 | 0.077 | slower | 1.339 | 0.982 |  |
