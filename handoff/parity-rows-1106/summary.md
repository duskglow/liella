# PARITY-ROWS-1106: next non-libm parity rows by instructions:u

Counter run: label parity-rows-1106-counts (jobs.tsv, rc 0), `cargo xtask bench --compare --counters --runs 3`, groups cbench_opt then
cbench_hot, cbench_memref, cbench_locale, cbench_patterns, cbench_stdio, cbench_time, cbench_malloc, cbench_netdb, cbench_fstree,
origin/next 412d6fc7. Host: the dispatcher assigned the Broadwell-EP counter host (kanon), not keke or shiki, so CPU-class-dependent rows
(memset, memcmp, anything rep-based) may differ from the 1105 numbers measured elsewhere. Raw: opt.out, others.out.
Per-call profiles: parity-rows-1106-percall2 (wctype, locale, sprintf_chk, memcmp_bands, parity1105 specs; pc-*.out not copied, figures in the briefs).
parity-rows-1106-percall (first attempt, rc 1 in 1 s: cargo not on PATH in the job script) is superseded.

**Rows at or below 1.05x in instructions: 295 of 345** (up from 271 of 338 before the 1105 landings). 50 rows are above; list below
(highest instr x time first). Landed from 1105 and now at or below 1.05: glob_1000_sorted, realloc_within_usable, fwrite_4k,
fnmatch_bracket, getline_long, strncmp_*, getc/putc rows.

| Row | Instr | Time | Story / reason not briefed |
|---|---:|---:|---|
| iswctype_en_us, iswalpha_l_en_us, iswalpha_en_us, towupper_l_en_us | 2.42, 2.32, 2.04, 1.74 | 2.39, 1.82, 1.28, 1.30 | PERF-WCTYPE-ENUS (1105 wctype landed, rows still above) |
| memset_4k, memset_64k | 2.01, 1.64 | 1.12, 1.02 | PERF-MEMSET-4K-3 (1105 landed; still above on this host class) |
| sprintf_chk_ls_nlink | 1.18 | 1.16 | PERF-SPRINTF-CHK-LS |
| memcmp_diff8_127/96/64, __memcmpeq_*, bcmp_*, memcmp_4k/64k | 1.07-1.40 | 0.18-1.44 | PERF-MEMCMP-TAILS (1105 landed, tails left) |
| gettext_language_steady, _alternating, gettext_found, ngettext_found | 1.88, 1.29, 1.34, 1.08 | 1.19, 1.12, 0.93, 0.89 | not briefed: no percall spec exists for gettext and the budget ended; high value (steady 1.88x), brief next round with a new spec |
| nl_langinfo_l_en_us, nl_langinfo_en_us | 1.57, 1.13 | 1.50, 1.25 | not briefed: percall locale.c output not read within budget; next round, could fold into PERF-WCTYPE-ENUS (locale entry cost) |
| iswalpha_utf8, towupper_utf8, wcwidth_utf8, isalpha_c, ctype_b_loc | 1.39, 1.32, 1.16, 1.44, 1.20 | 0.93, 0.77, 0.78, 1.00, 0.83 | not briefed: Liella faster or equal in time; small absolute counts (10-20 instructions) |
| strcmp_16_mix, strcmp_64_mix | 1.10, 1.10 | 1.22, 1.11 | open story PERF-STRCMP-MIX (handoff exists) |
| strncasecmp_c_64, strcasecmp_{c,utf8,en}_256 | 1.07, 1.06 | 1.00, 0.90 | not briefed: just above the line, faster in time |
| sys_close_ebadf | 1.06 | 0.99 | not briefed: kernel-dominated syscall wrapper |
| strstr_hit_64 | 1.73 | 1.96 | open story STRSTR-SHORT-DENSE-PARITY (excluded) |
| nftw_phys_1000, fts_physical_1000 | 1.10, 1.08 | 1.06, 1.09 | open story PERF-NFTW-ENTRY-5 (excluded); percall nftw_phys_38 33827 / 9359 |
| wordexp_* (quoted, tilde_var, arith, param_forms, split_var) | 1.07-1.46 | 1.06-1.57 | owner-deferred |
| memcpy_64k | 92.6 | 1.04 | rep-movsb row (counter shows one rep instruction), excluded |
| re_mt4_shared, re_mt4_distinct | 1.45, 1.46 | 0.02, 0.17 | Liella far faster in time (threaded wait cost) |

glob_sorted_38 percall is 74322 / 66042 (1.13) but the bench row glob_1000_sorted is below the line; no brief.

## Notes
- Briefs carry hypotheses from percall figures only (no perf annotate, sampling on the VM loses samples); each says its first step.
- Any host-class dependence (memset) is called out in the brief.
