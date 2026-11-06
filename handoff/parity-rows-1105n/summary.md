# PARITY-ROWS-1105N: next non-libm parity rows by instructions:u

Counter run: label parity-rows-1105n-counts (jobs.tsv rc 0, 3473 s), `cargo xtask bench --compare --counters --runs 3` over cbench_opt, cbench_hot, cbench_memref, cbench_locale, cbench_patterns, cbench_stdio, cbench_time, cbench_malloc, cbench_netdb, cbench_fstree; origin/next d1a7f653; Skylake-class counter host. Per-call profiles: label parity-rows-1105n-percall2 (small-probe.c, memcmp_bands.c, memcmp_eq32.c, sprintf_chk.c; --base none). Raw rows: counts-rows.txt.

**Rows at or below 1.05x in instructions: 297 of 345** (295 of 345 in the 1106 round). 48 rows above; the libm tiers are not in this bench set.

| Row | Instr | Time | Story / reason not briefed |
|---|---:|---:|---|
| memcpy_64k | 99.1 | 1.01 | rep-movsb row, timing judges (ruling #3046) |
| iswctype_en_us, iswalpha_l_en_us, iswalpha_en_us, towupper_l_en_us | 2.21, 2.13, 1.86, 1.74 | 2.24, 1.71, 1.14, 1.28 | PERF-WCTYPE-ENUS (in CI) |
| gettext_language_steady, _alternating, gettext_found, ngettext_found | 1.64, 1.19, 1.33, 1.08 | 0.97, 1.03, 0.90, 0.86 | PERF-GETTEXT-LANGUAGE-STEADY-5 (in CI) |
| nl_langinfo_l_en_us, nl_langinfo_en_us, ctype_b_loc, isalpha_c, strnlen_16, strnlen_16_mix | 1.57, 1.13, 1.20, 1.44, 1.08, 1.06 | 1.62, 1.25, 0.84, 1.00, 1.11, 1.20 | PARITY-SMALL-ENTRY-1105N |
| __memcmpeq_tail_32, bcmp_tail_32, memcmp_diff8_64/96/127, __memcmpeq_head_128/1k, __memcmpeq_eq_32, bcmp_eq_32 | 1.15-1.55 | 0.19-1.25 | PARITY-MEMCMP-SMALL-EQ |
| sprintf_chk_ls_nlink | 1.17 | 1.14 | PARITY-SPRINTF-CHK-LS-2 |
| iswalpha_utf8, towupper_utf8, wcwidth_utf8 | 1.39, 1.32, 1.16 | 0.89, 0.78, 0.71 | not briefed: Liella faster in time, 10-20 instructions absolute; fold into PERF-WCTYPE-ENUS follow-up |
| strcmp_16_mix, strcmp_64_mix, strcmp_equal_16, strcmp_names, strncasecmp_c_64, strcasecmp_{c,utf8,en}_16/256 | 1.06-1.15 | 0.90-1.39 | PARITY-STRCMP-AVX2 (in CI) |
| strstr_hit_64 | 1.35 | 1.43 | open story STRSTR-SHORT-DENSE-PARITY (handoff/strstr-short-dense-parity.md) |
| re_mt4_shared, re_mt4_distinct | 1.43 | 0.03, 0.17 | not briefed: Liella far faster in time (threaded wait cost) |
| wordexp_* (quoted, tilde_var, arith, param_forms, split_var) | 1.08-1.48 | 1.12-1.72 | owner-deferred |
| sys_close_ebadf | 1.06 | 1.01 | not briefed: kernel-dominated syscall wrapper |

Not run in this round (budget): per-row annotate of the qsort_/nftw_ rows (all at or below 1.05 or covered by open stories).
Notes: briefs use percall figures only (no perf annotate). The percall script needs `--base none` when the tree equals origin/next (otherwise it asks for a primed base). The memcmp_diff8 and __memcmpeq tail rows overlap with the landed PERF-MEMCMP-TAILS; the brief says to measure what it left first.
