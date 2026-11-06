| bench/row | instr IVB | time IVB | instr N100 | time N100 | status | story (ops.db 2026-11-06 06:30Z) |
|---|---|---|---|---|---|---|
| cbench_fstree/fts_physical_1000 | 1.054 | 1.052 | 1.064 | 1.026 | re-time | PERF-NFTW-ENTRY-12 landed after the sweep (re-timed 1.04 IVB there) |
| cbench_hot/getline_long | 1.201 | 1.080 | 1.168 | 1.009 | open | TIMING-COV-GETLINE-LONG-2 (queued_ci) |
| cbench_hot/memrchr_64_hit | 1.107 | 1.276 | 1.000 | 0.777 | open | TIMING-COV-MEMCHR-SHORT-NONAVX2 (ready) |
| cbench_hot/mutex_lock_unlock_normal_mt | 0.852 | 1.000 | 0.852 | 1.199 | open | TIMING-N100-MUTEX-MT-2 (queued_ci) |
| cbench_hot/strchr_16_hit | 1.063 | 1.318 | 0.965 | 0.777 | re-time | TIMING-IVB-STRSCAN-DISPATCH-2 landed after the 1106 table |
| cbench_hot/strchr_16_miss | 0.750 | 1.162 | 0.965 | 0.728 | re-time | TIMING-IVB-STRSCAN-DISPATCH-2 landed after the 1106 table |
| cbench_hot/strchr_64_hit | 1.036 | 1.373 | 0.921 | 0.909 | re-time | TIMING-IVB-STRSCAN-DISPATCH-2 landed after the 1106 table |
| cbench_hot/strchr_64_miss | 1.036 | 1.472 | 0.921 | 0.909 | re-time | TIMING-IVB-STRSCAN-DISPATCH-2 landed after the 1106 table |
| cbench_hot/strchrnul_16_hit | 1.069 | 1.557 | 0.926 | 0.701 | re-time | TIMING-IVB-STRSCAN-DISPATCH-2 landed after the 1106 table |
| cbench_hot/strchrnul_64_hit | 1.038 | 1.534 | 0.889 | 0.820 | re-time | TIMING-IVB-STRSCAN-DISPATCH-2 landed after the 1106 table |
| cbench_hot/strchrnul_64_miss | 1.038 | 1.439 | 0.889 | 0.751 | re-time | TIMING-IVB-STRSCAN-DISPATCH-2 landed after the 1106 table |
| cbench_hot/strncmp_diff8 | 1.436 | 1.752 | 1.000 | 1.000 | re-time | TIMING-IVB-STRSCAN-DISPATCH-2 landed after the 1106 table |
| cbench_hot/strncmp_eq16 | 1.015 | 1.265 | 1.029 | 1.000 | re-time | TIMING-IVB-STRSCAN-DISPATCH-2 landed after the 1106 table |
| cbench_locale/iconv_utf8_ascii_translit_4k | 0.864 | 1.299 | 0.864 | 1.454 | re-time | TIMING-ICONV-TRANSLIT-CPI-4 landed after the 1106 table |
| cbench_locale/iswalpha_en_us | 1.856 | 1.348 | 1.857 | 1.485 | re-time | PERF-WCTYPE-ENUS-2 landed after the sweep (-19..-30 instr on en_US rows) |
| cbench_locale/iswalpha_l_en_us | 2.127 | 1.791 | 2.127 | 1.851 | re-time | PERF-WCTYPE-ENUS-2 landed after the sweep (-19..-30 instr on en_US rows) |
| cbench_locale/iswalpha_utf8 | 1.387 | 0.956 | 1.388 | 1.132 | re-time | PERF-WCTYPE-ENUS-2 landed after the sweep (-19..-30 instr on en_US rows) |
| cbench_locale/iswctype_en_us | 2.210 | 2.600 | 2.209 | 2.108 | re-time | PERF-WCTYPE-ENUS-2 landed after the sweep (-19..-30 instr on en_US rows) |
| cbench_locale/nl_langinfo_en_us | 1.129 | 1.199 | 1.129 | 1.000 | re-time | PARITY-SMALL-ENTRY-1105N-2 landed after the 1106 table |
| cbench_locale/nl_langinfo_l_en_us | 1.571 | 1.557 | 1.571 | 1.100 | re-time | PARITY-SMALL-ENTRY-1105N-2 landed after the 1106 table |
| cbench_locale/strcasecmp_c_16 | 0.979 | 1.024 | 1.080 | 1.136 | re-time | TIMING-COV-STRCMP-SHORT-AVX2-2 landed after the 1106 table |
| cbench_locale/strncasecmp_c_64 | 0.932 | 0.950 | 1.118 | 1.099 | re-time | TIMING-COV-STRCMP-SHORT-AVX2-2 landed after the 1106 table |
| cbench_locale/towupper_l_en_us | 1.743 | 1.573 | 1.743 | 1.454 | re-time | PERF-WCTYPE-ENUS-2 landed after the sweep (-19..-30 instr on en_US rows) |
| cbench_malloc/malloc_free_1024 | 0.830 | 1.119 | 0.830 | 0.738 | re-time | TIMING-IVB-MALLOC-CHAIN-2 landed after the 1106 table |
| cbench_malloc/malloc_free_128 | 0.830 | 1.190 | 0.830 | 0.738 | re-time | TIMING-IVB-MALLOC-CHAIN-2 landed after the 1106 table |
| cbench_malloc/malloc_free_16 | 0.830 | 1.121 | 0.830 | 0.738 | re-time | TIMING-IVB-MALLOC-CHAIN-2 landed after the 1106 table |
| cbench_malloc/malloc_free_256 | 0.830 | 1.184 | 0.830 | 0.719 | re-time | TIMING-IVB-MALLOC-CHAIN-2 landed after the 1106 table |
| cbench_malloc/malloc_free_32 | 0.830 | 1.170 | 0.830 | 0.738 | re-time | TIMING-IVB-MALLOC-CHAIN-2 landed after the 1106 table |
| cbench_malloc/malloc_free_64 | 0.830 | 1.183 | 0.830 | 0.719 | re-time | TIMING-IVB-MALLOC-CHAIN-2 landed after the 1106 table |
| cbench_malloc/realloc_grow_x2 | 1.107 | 1.093 | 0.982 | 0.962 | re-time | TIMING-COV-REALLOC-GROW-NONAVX2 landed after the 1106 table |
| cbench_malloc/realloc_null_24 | 0.851 | 1.063 | 0.851 | 0.912 | re-time | TIMING-IVB-MALLOC-CHAIN-2 landed after the 1106 table |
| cbench_malloc/realloc_within_usable | 0.980 | 1.186 | 0.979 | 0.944 | re-time | TIMING-IVB-MALLOC-CHAIN-2 landed after the 1106 table |
| cbench_memref/__memcmpeq_eq_128 | 1.109 | 1.349 | 1.030 | 0.847 | open | TIMING-COV-MEMCMP-NONAVX2 (ready); PARITY-MEMCMP-SMALL-EQ-2 (ready) |
| cbench_memref/__memcmpeq_eq_1k | 1.212 | 1.239 | 0.918 | 1.015 | open | TIMING-COV-MEMCMP-NONAVX2 (ready); PARITY-MEMCMP-SMALL-EQ-2 (ready) |
| cbench_memref/__memcmpeq_eq_64 | 1.375 | 1.187 | 0.960 | 0.641 | open | TIMING-COV-MEMCMP-NONAVX2 (ready); PARITY-MEMCMP-SMALL-EQ-2 (ready) |
| cbench_memref/__memcmpeq_head_128 | 1.842 | 1.768 | 1.199 | 0.730 | open | TIMING-COV-MEMCMP-NONAVX2 (ready); PARITY-MEMCMP-SMALL-EQ-2 (ready) |
| cbench_memref/__memcmpeq_head_1k | 1.845 | 1.768 | 1.199 | 0.730 | open | TIMING-COV-MEMCMP-NONAVX2 (ready); PARITY-MEMCMP-SMALL-EQ-2 (ready) |
| cbench_memref/__memcmpeq_head_8 | 0.790 | 1.101 | 0.671 | 0.638 | open | TIMING-IVB-MEMREF-ENTRY-2 (ready) |
| cbench_memref/__memcmpeq_tail_128 | 1.109 | 1.419 | 1.028 | 0.920 | open | TIMING-COV-MEMCMP-NONAVX2 (ready); PARITY-MEMCMP-SMALL-EQ-2 (ready) |
| cbench_memref/__memcmpeq_tail_32 | 1.107 | 1.091 | 1.545 | 0.801 | open | TIMING-COV-MEMCMP-NONAVX2 (ready); PARITY-MEMCMP-SMALL-EQ-2 (ready) |
| cbench_memref/__memcmpeq_tail_64 | 1.370 | 1.267 | 0.961 | 0.703 | open | TIMING-COV-MEMCMP-NONAVX2 (ready); PARITY-MEMCMP-SMALL-EQ-2 (ready) |
| cbench_memref/__memcmpeq_tail_8 | 0.783 | 1.221 | 0.670 | 0.701 | open | TIMING-IVB-MEMREF-ENTRY-2 (ready) |
| cbench_memref/bcmp_eq_128 | 1.077 | 1.444 | 0.948 | 0.920 | open | TIMING-COV-MEMCMP-NONAVX2 (ready); PARITY-MEMCMP-SMALL-EQ-2 (ready) |
| cbench_memref/bcmp_eq_1k | 1.208 | 1.249 | 0.913 | 1.015 | open | TIMING-COV-MEMCMP-NONAVX2 (ready); PARITY-MEMCMP-SMALL-EQ-2 (ready) |
| cbench_memref/bcmp_eq_64 | 1.340 | 1.200 | 0.928 | 0.703 | open | TIMING-COV-MEMCMP-NONAVX2 (ready); PARITY-MEMCMP-SMALL-EQ-2 (ready) |
| cbench_memref/bcmp_head_128 | 1.553 | 1.916 | 1.001 | 0.801 | open | TIMING-COV-MEMCMP-NONAVX2 (ready); PARITY-MEMCMP-SMALL-EQ-2 (ready) |
| cbench_memref/bcmp_head_1k | 1.559 | 1.916 | 1.001 | 0.801 | open | TIMING-COV-MEMCMP-NONAVX2 (ready); PARITY-MEMCMP-SMALL-EQ-2 (ready) |
| cbench_memref/bcmp_tail_128 | 0.960 | 1.173 | 0.844 | 0.788 | open | TIMING-IVB-MEMREF-ENTRY-2 (ready) |
| cbench_memref/bcmp_tail_64 | 1.120 | 1.056 | 0.782 | 0.587 | open | TIMING-COV-MEMCMP-NONAVX2 (ready); PARITY-MEMCMP-SMALL-EQ-2 (ready) |
| cbench_memref/memchr_16 | 1.580 | 1.584 | 0.895 | 0.637 | open | TIMING-COV-MEMCHR-SHORT-NONAVX2 (ready) |
| cbench_memref/memchr_16_mix | 1.338 | 1.171 | 0.915 | 0.617 | open | TIMING-COV-MEMCHR-SHORT-NONAVX2 (ready) |
| cbench_memref/memchr_64 | 0.780 | 1.177 | 0.728 | 0.541 | open | TIMING-IVB-MEMREF-ENTRY-2 (ready) |
| cbench_memref/memchr_64k | 0.852 | 1.006 | 0.922 | 1.169 | open | TIMING-N100-SCAN-64K-3 (verifying) |
| cbench_memref/memcmp_128 | 0.910 | 1.103 | 0.924 | 0.772 | open | TIMING-IVB-MEMREF-ENTRY-2 (ready) |
| cbench_memref/memcmp_144 | 0.988 | 1.172 | 0.721 | 0.707 | open | TIMING-IVB-MEMREF-ENTRY-2 (ready) |
| cbench_memref/memcmp_256 | 1.086 | 1.226 | 0.913 | 1.061 | open | TIMING-N100-MEMCMP (ready); TIMING-COV-MEMCMP-NONAVX2 (ready) |
| cbench_memref/memcmp_4k | 1.008 | 0.976 | 0.962 | 1.086 | open | TIMING-N100-MEMCMP (ready); TIMING-COV-MEMCMP-NONAVX2 (ready) |
| cbench_memref/memcmp_diff8_127 | 1.677 | 1.584 | 1.207 | 0.801 | open | TIMING-COV-MEMCMP-NONAVX2 (ready); PARITY-MEMCMP-SMALL-EQ-2 (ready) |
| cbench_memref/memcmp_diff8_64 | 1.321 | 1.250 | 1.249 | 0.801 | open | TIMING-COV-MEMCMP-NONAVX2 (ready); PARITY-MEMCMP-SMALL-EQ-2 (ready) |
| cbench_memref/memcmp_diff8_96 | 1.672 | 1.584 | 1.206 | 0.801 | open | TIMING-COV-MEMCMP-NONAVX2 (ready); PARITY-MEMCMP-SMALL-EQ-2 (ready) |
| cbench_memref/memcpy_144 | 0.884 | 1.010 | 1.000 | 1.077 | re-time | PARITY-COPY-AVX2-3 landed after the sweep; TIMING-N100-COPY-MID (running) |
| cbench_memref/memcpy_256 | 0.632 | 0.730 | 1.001 | 1.077 | re-time | PARITY-COPY-AVX2-3 landed after the sweep; TIMING-N100-COPY-MID (running) |
| cbench_memref/memmove_144 | 0.831 | 1.167 | 1.001 | 0.859 | open | TIMING-IVB-MEMREF-ENTRY-2 (ready) |
| cbench_memref/memset_144 | 0.866 | 1.214 | 0.884 | 0.946 | open | TIMING-IVB-MEMREF-ENTRY-2 (ready) |
| cbench_memref/rawmemchr_4k | 0.908 | 1.076 | 0.908 | 1.047 | open | TIMING-IVB-MEMREF-ENTRY-2 (ready) |
| cbench_memref/rawmemchr_64k | 0.901 | 1.012 | 0.909 | 1.060 | open | TIMING-N100-SCAN-64K-3 (verifying) |
| cbench_memref/strstr_hit_4k | 1.023 | 1.277 | 0.296 | 0.639 | open | TIMING-COV-STRSTR-NONAVX2 (verifying) |
| cbench_memref/strstr_hit_64 | 2.169 | 2.398 | 1.353 | 1.437 | open | TIMING-COV-STRSTR-NONAVX2 (verifying); landed since 1106: TIMING-N100-STRSTR-64 |
| cbench_memref/strstr_miss_64 | 1.387 | 1.411 | 0.804 | 1.102 | open | TIMING-COV-STRSTR-NONAVX2 (verifying); landed since 1106: TIMING-N100-STRSTR-64 |
| cbench_opt/first_localtime_r_new_york | - | 1.956 | - | 2.120 | out of scope | COLDSTART-PLAN-1106 |
| cbench_opt/first_setlocale_en_US | - | 2.225 | - | 2.223 | out of scope | COLDSTART-PLAN-1106 |
| cbench_opt/first_snprintf | - | 1.126 | - | 0.964 | out of scope | COLDSTART-PLAN-1106 |
| cbench_opt/first_strcmp | - | 1.474 | - | 1.368 | out of scope | COLDSTART-PLAN-1106 |
| cbench_opt/first_strcoll | - | 20.884 | - | 19.542 | out of scope | COLDSTART-PLAN-1106 |
| cbench_opt/first_wordexp | - | 3.427 | - | 1.713 | deferred | OWNER-DEFERRED (wordexp) |
| cbench_opt/gettext_language_alternating | 1.154 | 1.065 | 1.193 | 1.028 | re-time | PERF-GETTEXT-LANGUAGE-STEADY-5 landed after the sweep (-32 instr/call) |
| cbench_opt/gettext_language_steady | 1.832 | 1.209 | 1.575 | 1.094 | re-time | PERF-GETTEXT-LANGUAGE-STEADY-5 landed after the sweep (-32 instr/call) |
| cbench_opt/second_bindtextdomain_gettext | - | 6.982 | - | 7.695 | out of scope | COLDSTART-PLAN-1106 |
| cbench_opt/second_mktime_new_york | - | 1.114 | - | 1.276 | out of scope | COLDSTART-PLAN-1106 |
| cbench_opt/second_setlocale_en_US | - | 1.354 | - | 1.432 | out of scope | COLDSTART-PLAN-1106 |
| cbench_opt/second_strcmp | - | 1.283 | - | 1.535 | out of scope | COLDSTART-PLAN-1106 |
| cbench_opt/second_wordexp | - | 1.532 | - | 1.307 | deferred | OWNER-DEFERRED (wordexp) |
| cbench_opt/sprintf_chk_ls_nlink | 1.171 | 1.141 | 1.170 | 1.047 | re-time | PARITY-SPRINTF-CHK-LS-2 landed after the 1106 table |
| cbench_opt/strcmp_names | 0.943 | 0.759 | 1.062 | 1.142 | re-time | TIMING-COV-STRCMP-SHORT-AVX2-2, PARITY-STRCMP-AVX2 landed after the 1106 table |
| cbench_opt/wordexp_arith | 1.169 | 1.486 | 1.169 | 1.219 | deferred | OWNER-DEFERRED (wordexp) |
| cbench_opt/wordexp_param_forms | 1.184 | 1.355 | 1.187 | 1.137 | deferred | OWNER-DEFERRED (wordexp) |
| cbench_opt/wordexp_quoted | 1.471 | 1.734 | 1.478 | 1.515 | deferred | OWNER-DEFERRED (wordexp) |
| cbench_opt/wordexp_split_var | 1.074 | 1.179 | 1.082 | 0.958 | deferred | OWNER-DEFERRED (wordexp) |
| cbench_opt/wordexp_tilde_var | 1.331 | 1.526 | 1.345 | 1.294 | deferred | OWNER-DEFERRED (wordexp) |
| cbench_patterns/regexec_backref | 1.059 | 1.181 | 1.044 | 1.012 | new brief | TIMING-COV-REGEX-BACKREF-SCRATCH (new brief) |
| cbench_patterns/regexec_class_nomatch_1k | 0.946 | 1.145 | 0.940 | 0.862 | new brief | TIMING-COV-REGEX-FIRSTBYTE-IVB (new brief) |
| cbench_patterns/wordexp_arith | 1.178 | 1.543 | 1.190 | 1.266 | deferred | OWNER-DEFERRED (wordexp) |
| cbench_patterns/wordexp_tilde_var | 1.363 | 1.663 | 1.375 | 1.339 | deferred | OWNER-DEFERRED (wordexp) |
| cbench_process/fork_exit_wait | 1.062 | 1.247 | 1.081 | 1.294 | new brief | TIMING-COV-FORK-COW-FAULTS (new brief; stacks on COLDSTART-FORK-CHILD-RESET-2, finished) |
| cbench_process/posix_spawn_true_heap256m | 1.029 | 1.493 | 1.008 | 0.999 | new brief | TIMING-COV-SPAWN-HEAP256M-IVB (new brief: reproduce on IVB first) |
| cbench_unmeasured/qsort_c_cmp_1024 | 1.517 | 1.083 | 1.510 | 0.937 | re-time | PERF-QSORT-OFFSET-EXP-2 landed after the sweep (-8..-10% instr); residual comparison surplus is by design (PERF-QSORT-COMPARISONS) |
| cbench_unmeasured/qsort_c_cmp_1024_e4 | 1.511 | 1.082 | 1.506 | 0.913 | re-time | PERF-QSORT-OFFSET-EXP-2 landed after the sweep (-8..-10% instr); residual comparison surplus is by design (PERF-QSORT-COMPARISONS) |
| cbench_unmeasured/qsort_c_cmp_1024_e8 | 1.465 | 1.103 | 1.478 | 0.930 | re-time | PERF-QSORT-OFFSET-EXP-2 landed after the sweep (-8..-10% instr); residual comparison surplus is by design (PERF-QSORT-COMPARISONS) |

Counts: deferred 9, new brief 4, open 38, out of scope 9, re-time 38
