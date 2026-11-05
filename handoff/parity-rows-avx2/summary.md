# PARITY-ROWS-AVX2: summary

Non-libm rows above 1.05 on the AVX2 counter class (Skylake-SP host with AVX-512, run a6fc34c2, `instr_ratio_ss`:
Liella static against glibc static, user instructions per call), with the Ivy Bridge ratio from docs/benchmarks.md,
Liella and glibc instructions per call, and the story that covers the row or the reason it is not briefed.
81 rows. Source: the earlier AVX2 run's measure.tsv (scratch of PRE14-BENCH-AVX2-COUNTS); not recounted.

| Row | Bench | AVX2 ratio | Ivy Bridge ratio | Liella instr | glibc instr | Story / reason |
|---|---|---:|---:|---:|---:|---|
| `nftw_phys_1000` | cbench_fstree | 1.10 | 1.1 | 6163818.0 | 5586736.8 | PERF-NFTW-ENTRY-10 |
| `fts_physical_1000` | cbench_fstree | 1.08 | 1.09 | 6769064.5 | 6237352.2 | PERF-NFTW-ENTRY-10 |
| `re_mt4_distinct` | cbench_opt | 1.38 | 1.55 | 228644.8 | 165283.5 | not briefed: not AVX2-specific (1.38 on this class, 1.55 Ivy Bridge); four threads on one regex, a locking/allocation question, not a kernel; needs its own profile |
| `re_mt4_shared` | cbench_opt | 1.38 | 1.56 | 229891.0 | 167113.5 | not briefed: not AVX2-specific (1.38 on this class, 1.55 Ivy Bridge); four threads on one regex, a locking/allocation question, not a kernel; needs its own profile |
| `wordexp_quoted` | cbench_opt | 1.53 | 1.47 | 3765.1 | 2465.4 | owner-deferred |
| `wordexp_param_forms` | cbench_opt | 1.24 | 1.18 | 6207.7 | 5015.0 | owner-deferred |
| `wordexp_tilde_var` | cbench_opt | 1.40 | 1.33 | 4077.5 | 2919.8 | owner-deferred |
| `memcmp_64k` | cbench_memref | 1.13 | 1.0 | 9324.7 | 8240.2 | PARITY-MEMCMP-AVX2 |
| `wordexp_arith` | cbench_opt | 1.18 | 1.17 | 4127.8 | 3507.3 | owner-deferred |
| `strnlen_64k` | cbench_memref | 1.09 | 0.76 | 6219.4 | 5699.7 | PARITY-SCAN-AVX2 |
| `gettext_language_steady` | cbench_opt | 2.14 | 2.17 | 969.5 | 453.7 | PERF-GETTEXT-LANGUAGE-STEADY |
| `strlen_64k` | cbench_memref | 1.11 | 0.89 | 5177.3 | 4661.7 | PARITY-SCAN-AVX2 |
| `rawmemchr_64k` | cbench_memref | 1.10 | 0.9 | 5685.0 | 5169.4 | PARITY-SCAN-AVX2 |
| `memchr_64k` | cbench_memref | 1.08 | 0.85 | 6701.0 | 6209.5 | PARITY-SCAN-AVX2 |
| `getline_long` | cbench_hot | 1.06 | 1.2 | 7183.7 | 6788.9 | not briefed: not AVX2-specific (1.06 here, 1.20 Ivy Bridge); getline copy loop, within 6% |
| `wordexp_split_var` | cbench_opt | 1.11 | 1.07 | 3778.8 | 3398.9 | owner-deferred |
| `gettext_language_alternating` | cbench_opt | 1.39 | 1.29 | 1230.3 | 886.1 | PERF-GETTEXT-LANGUAGE-STEADY |
| `sprintf_chk_ls_nlink` | cbench_opt | 1.23 | 1.2 | 876.7 | 713.1 | not briefed: not AVX2-specific (1.23 both); printf path, not a vector kernel |
| `gettext_found` | cbench_opt | 1.36 | 1.34 | 615.7 | 453.7 | PERF-GETTEXT-LANGUAGE-STEADY |
| `putenv_remove_cycle_30` | cbench_hot | 1.12 | 0.93 | 1392.1 | 1245.6 | not briefed: env table walk, scalar; 1.12 here, 0.93 Ivy Bridge: the walk uses strncmp-like compares that fall under the strcmp family |
| `memcmp_4k` | cbench_memref | 1.24 | 1.01 | 671.1 | 542.9 | PARITY-MEMCMP-AVX2 |
| `ngettext_found` | cbench_opt | 1.18 | 1.09 | 787.9 | 669.8 | PERF-GETTEXT-LANGUAGE-STEADY |
| `unsetenv_cycle_30` | cbench_hot | 1.08 | 0.75 | 1330.1 | 1228.1 | not briefed: env table walk, scalar; as putenv_remove_cycle_30 |
| `strstr_hit_64` | cbench_memref | 1.70 | 2.17 | 168.6 | 99.4 | STRSTR-SHORT-DENSE-PARITY-4 |
| `iswalpha_l_en_us` | cbench_locale | 2.32 | 2.32 | 109.1 | 47.0 | not briefed: not AVX2-specific (same ratio on both classes); wide-ctype table lookup in en_US; overlaps PERF-WCTYPE-ENUS, extend that story |
| `iswctype_en_us` | cbench_locale | 2.42 | 2.42 | 104.1 | 43.0 | PERF-WCTYPE-ENUS |
| `iswalpha_en_us` | cbench_locale | 2.04 | 2.04 | 100.1 | 49.1 | not briefed: not AVX2-specific (same ratio on both classes); wide-ctype table lookup in en_US; overlaps PERF-WCTYPE-ENUS, extend that story |
| `strcasecmp_c_256` | cbench_locale | 1.24 | 1.16 | 251.2 | 202.1 | PARITY-STRCMP-AVX2 |
| `strcasecmp_utf8_256` | cbench_locale | 1.24 | 1.16 | 251.2 | 202.2 | PARITY-STRCMP-AVX2 |
| `strcasecmp_en_256` | cbench_locale | 1.24 | 1.16 | 251.2 | 202.3 | PARITY-STRCMP-AVX2 |
| `memrchr_4k` | cbench_hot | 1.11 | 0.86 | 458.5 | 411.6 | PARITY-SCAN-AVX2 |
| `memset_64k` | cbench_memref | 2.36 | 1.89 | 75.7 | 32.1 | PERF-MEMSET-4K-4 |
| `memset_4k` | cbench_memref | 2.83 | 2.46 | 66.9 | 23.7 | PERF-MEMSET-4K-4 |
| `strnlen_4k` | cbench_memref | 1.10 | 0.81 | 452.8 | 412.7 | PARITY-SCAN-AVX2 |
| `strlen_4k` | cbench_memref | 1.11 | 0.92 | 370.7 | 334.6 | PARITY-SCAN-AVX2 |
| `rawmemchr_4k` | cbench_memref | 1.10 | 0.91 | 396.8 | 361.6 | PARITY-SCAN-AVX2 |
| `towupper_l_en_us` | cbench_locale | 1.74 | 1.74 | 68.0 | 39.0 | not briefed: not AVX2-specific (same ratio both classes); wide-ctype lookup; extend PERF-WCTYPE-ENUS |
| `memcpy_64k` | cbench_memref | 1.49 | 1.85 | 85.5 | 57.5 | PARITY-COPY-AVX2 |
| `sys_close_ebadf` | cbench_hot | 1.06 | 1.06 | 436.9 | 411.9 | not briefed: syscall row: 25 instructions over 412 (kernel entry path), within the 6% band edge |
| `memmove_4k` | cbench_memref | 1.06 | 0.89 | 436.8 | 413.0 | PARITY-COPY-AVX2 |
| `iswalpha_utf8` | cbench_locale | 1.39 | 1.39 | 68.0 | 49.0 | not briefed: not AVX2-specific (same ratio on both classes); wide-ctype table lookup in en_US; overlaps PERF-WCTYPE-ENUS, extend that story |
| `strncasecmp_c_64` | cbench_locale | 1.27 | 0.93 | 76.0 | 60.0 | PARITY-STRCMP-AVX2 |
| `strcmp_64_mix` | cbench_memref | 1.26 | 0.95 | 71.0 | 56.3 | PARITY-STRCMP-AVX2 |
| `towupper_utf8` | cbench_locale | 1.32 | 1.32 | 53.0 | 40.0 | not briefed: not AVX2-specific (same ratio both classes); wide-ctype lookup; extend PERF-WCTYPE-ENUS |
| `__memcmpeq_tail_32` | cbench_memref | 1.54 | 1.1 | 31.3 | 20.3 | PARITY-MEMCMP-AVX2 |
| `__memcmpeq_eq_32` | cbench_memref | 1.54 | 1.1 | 31.3 | 20.4 | PARITY-MEMCMP-AVX2 |
| `__memcmpeq_head_1k` | cbench_memref | 1.56 | 1.85 | 30.3 | 19.4 | PARITY-MEMCMP-AVX2 |
| `memcmp_diff8_127` | cbench_memref | 1.43 | 1.75 | 33.4 | 23.4 | PARITY-MEMCMP-AVX2 |
| `memcmp_diff8_96` | cbench_memref | 1.43 | 1.76 | 33.3 | 23.3 | PARITY-MEMCMP-AVX2 |
| `strcmp_16_mix` | cbench_memref | 1.23 | 0.72 | 52.5 | 42.8 | PARITY-STRCMP-AVX2 |
| `strcmp_names` | cbench_opt | 1.17 | 0.94 | 67.0 | 57.3 | PARITY-STRCMP-AVX2 |
| `__memcmpeq_head_128` | cbench_memref | 1.47 | 1.85 | 28.4 | 19.3 | PARITY-MEMCMP-AVX2 |
| `strcasecmp_c_16` | cbench_locale | 1.20 | 0.98 | 54.0 | 45.0 | PARITY-STRCMP-AVX2 |
| `bcmp_eq_32` | cbench_memref | 1.40 | 1.07 | 31.4 | 22.4 | PARITY-MEMCMP-AVX2 |
| `bcmp_head_1k` | cbench_memref | 1.31 | 1.56 | 30.4 | 23.3 | PARITY-MEMCMP-AVX2 |
| `memcmp_diff8_64` | cbench_memref | 1.31 | 1.31 | 30.4 | 23.3 | PARITY-MEMCMP-AVX2 |
| `isalpha_c` | cbench_locale | 1.44 | 1.44 | 23.0 | 16.0 | not briefed: not AVX2-specific; 7 instructions over, a call-shape row (ctype); small, one story with the locale rows |
| `wcwidth_utf8` | cbench_locale | 1.16 | 1.16 | 50.0 | 43.0 | not briefed: not AVX2-specific; locale table lookup; extend PERF-WCTYPE-ENUS |
| `nl_langinfo_l_en_us` | cbench_locale | 1.19 | 1.57 | 44.0 | 37.0 | not briefed: not AVX2-specific; 7 instructions over 37 |
| `strcmp_late_256` | cbench_opt | 1.06 | 1.46 | 108.1 | 102.1 | PARITY-STRCMP-AVX2 |
| `__memcmpeq_tail_128` | cbench_memref | 1.15 | 1.11 | 39.4 | 34.3 | PARITY-MEMCMP-AVX2 |
| `bcmp_tail_32` | cbench_memref | 1.20 | 0.91 | 31.4 | 26.3 | PARITY-MEMCMP-AVX2 |
| `memcmp_16` | cbench_memref | 1.23 | 1.08 | 27.3 | 22.3 | PARITY-MEMCMP-AVX2 |
| `__memcmpeq_eq_128` | cbench_memref | 1.15 | 1.11 | 39.4 | 34.4 | PARITY-MEMCMP-AVX2 |
| `bcmp_head_128` | cbench_memref | 1.21 | 1.56 | 28.3 | 23.4 | PARITY-MEMCMP-AVX2 |
| `memcmp_16_mix` | cbench_memref | 1.13 | 1.05 | 42.3 | 37.4 | PARITY-MEMCMP-AVX2 |
| `strcmp_equal_16` | cbench_opt | 1.15 | 1.06 | 31.0 | 27.0 | PARITY-STRCMP-AVX2 |
| `strncmp_eq16` | cbench_hot | 1.09 | 1.01 | 35.1 | 32.1 | PARITY-STRCMP-AVX2 |
| `strnlen_16` | cbench_memref | 1.12 | 0.81 | 28.4 | 25.4 | PARITY-SCAN-AVX2 |
| `memset_128` | cbench_memref | 1.12 | 0.77 | 27.4 | 24.4 | PARITY-MEMSET-SMALL-AVX2 |
| `__memcmpeq_eq_8` | cbench_memref | 1.15 | 1.0 | 23.3 | 20.3 | PARITY-MEMCMP-AVX2 |
| `__memcmpeq_tail_16` | cbench_memref | 1.15 | 1.0 | 23.3 | 20.3 | PARITY-MEMCMP-AVX2 |
| `__memcmpeq_eq_16` | cbench_memref | 1.14 | 1.0 | 23.3 | 20.4 | PARITY-MEMCMP-AVX2 |
| `memcmp_256` | cbench_memref | 1.05 | 1.1 | 58.4 | 55.5 | PARITY-MEMCMP-AVX2 |
| `strnlen_16_mix` | cbench_memref | 1.08 | 0.83 | 35.3 | 32.5 | PARITY-SCAN-AVX2 |
| `memrchr_64_hit` | cbench_hot | 1.08 | 1.11 | 27.1 | 25.0 | PARITY-SCAN-AVX2 |
| `strncmp_diff8` | cbench_hot | 1.06 | 1.44 | 34.1 | 32.1 | PARITY-STRCMP-AVX2 |
| `ctype_b_loc` | cbench_hot | 1.20 | 1.2 | 12.0 | 10.0 | not briefed: not AVX2-specific; 2 instructions over a 10-instruction call |
| `memset_64` | cbench_memref | 1.11 | 0.72 | 20.3 | 18.3 | PARITY-MEMSET-SMALL-AVX2 |
| `memcpy_64` | cbench_memref | 1.05 | 0.7 | 20.4 | 19.4 | PARITY-COPY-AVX2 |
| `memmove_64` | cbench_memref | 1.05 | 0.69 | 20.3 | 19.3 | PARITY-COPY-AVX2 |
