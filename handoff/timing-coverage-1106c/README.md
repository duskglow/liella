# TIMING-COVERAGE-1106C: the 1106 sweep's verdicts mapped to stories

Source: results/timing-sweep-1106-table.md (build next 4d454f06; Ivy Bridge = IVB, N100 = Alder Lake-N E-cores).
Story states: ops db, 2026-11-06 ~09:30Z. "In build" = the story's landing is in f2157504..4d454f06
(handoff/timing-sweep-1106/landings-f2157504..4d454f06.txt), so the sweep already measured it. Landed on next after
4d454f06 (not timed by the sweep): TIMING-COVERAGE-1106B, COLDSTART-BINDTEXTDOMAIN-REBIND(-2).
The 1106B table itself (on next after 4d454f06) was not opened; its per-row story notes are the ones the sweep
table carries.

## 1. List (c), improved rows: close / keep / none

| row | class | time now (1105) | covering story | state | verdict |
|---|---|---|---|---|---|
| cbench_fstree/fts_physical_1000 | IVB | 1.030 (1.052) | PERF-NFTW-ENTRY-12 | landed, in build | none (target met; N100 1.038 parity) |
| cbench_hot/mutex_lock_unlock_normal_mt | N100 | 0.985 (1.199) | TIMING-N100-MUTEX-MT-2 (-1 closed) | landed, in build | none (target met) |
| cbench_hot/strchr_16_miss | IVB | 0.918 (1.162) | TIMING-IVB-STRSCAN-DISPATCH-2 | landed, in build | none; the story's other rows still slower: new brief TIMING-COV-STRSCAN-FRONTEND-SSE2 |
| cbench_locale/iconv_utf8_ascii_translit_4k | IVB | 0.492 (1.299) | TIMING-ICONV-TRANSLIT-CPI-4 | landed, in build | none (target met) |
| cbench_locale/iconv_utf8_ascii_translit_4k | N100 | 0.565 (1.454) | TIMING-ICONV-TRANSLIT-CPI-4 | landed, in build | none (target met) |
| cbench_locale/iswalpha_en_us | IVB | 0.884 (1.348) | PERF-WCTYPE-ENUS-2 | landed, in build | none (time met; instr 1.244, iswalpha_l_en_us / iswctype_en_us still slower by instructions, not list (b)) |
| cbench_locale/iswalpha_en_us | N100 | 0.988 (1.485) | PERF-WCTYPE-ENUS-2 | landed, in build | none (as above) |
| cbench_locale/nl_langinfo_l_en_us | N100 | 0.801 (1.100) | PARITY-SMALL-ENTRY-1105N-2 | landed, in build | none; IVB nl_langinfo rows still slower (group D below) |
| cbench_locale/towupper_l_en_us | N100 | 0.909 (1.454) | PERF-WCTYPE-ENUS-2 | landed, in build | none; IVB row 1.067 in list (b) |
| cbench_malloc/realloc_grow_x2 | IVB | 0.888 (1.093) | TIMING-COV-REALLOC-GROW-NONAVX2 | landed, in build | none (target met) |
| cbench_memref/strstr_miss_64 | N100 | 0.987 (1.102) | TIMING-N100-STRSTR-64 | landed, in build | none (target met). TIMING-COV-STRSTR-NONAVX2 (queued_ci) covers the IVB strstr rows only |
| cbench_opt/gettext_language_alternating | IVB | 0.988 (1.065) | PERF-GETTEXT-LANGUAGE-STEADY-5 | landed, in build | none (target met) |
| cbench_opt/gettext_language_steady | IVB | 1.045 (1.209) | PERF-GETTEXT-LANGUAGE-STEADY-5 | landed, in build | none (time parity; instr still 1.588) |
| cbench_opt/gettext_language_steady | N100 | 0.976 (1.094) | PERF-GETTEXT-LANGUAGE-STEADY-5 | landed, in build | none (time parity; instr 1.400) |
| cbench_opt/sprintf_chk_ls_nlink | IVB | 0.955 (1.141) | PARITY-SPRINTF-CHK-LS-2 | landed, in build | none (target met) |
| cbench_process/posix_spawn_true_heap256m | IVB | 0.971 (1.493), spread 0.699 | TIMING-COV-SPAWN-HEAP256M-IVB | verifying | **close** (target <= 1.05 met in the sweep; the spread says noise, which is that brief's own "does not reproduce" exit) |
| cbench_unmeasured/qsort_c_cmp_1024 | IVB | 0.998 (1.083) | PERF-QSORT-OFFSET-EXP-2 | landed, in build | none (target met; residual comparisons by design, PERF-QSORT-COMPARISONS) |
| cbench_unmeasured/qsort_c_cmp_1024_e4 | IVB | 0.713 (1.082) | PERF-QSORT-OFFSET-EXP-2 | landed, in build | none |
| cbench_unmeasured/qsort_c_cmp_1024_e8 | IVB | 1.010 (1.103) | PERF-QSORT-OFFSET-EXP-2 | landed, in build | none |

Only one open story covers a (c) row: TIMING-COV-SPAWN-HEAP256M-IVB -> close. Every other (c) row was moved by a
story that is already landed; nothing else to close.

## 2. List (b), reopen rows (instr <= 1.05, time > 1.05): covering open story or NONE

| row | class | instr | time | covering open story |
|---|---|---|---|---|
| __memcmpeq_head_8, __memcmpeq_tail_8, bcmp_tail_128, memchr_64, memcmp_128, memcmp_144, memmove_144, memset_144, rawmemchr_4k | IVB | 0.74-0.99 | 1.059-1.221 | TIMING-IVB-MEMREF-ENTRY-2 (ready; its original brief lists exactly these 9 rows) |
| memcmp_256, memcmp_4k | N100 | 0.913, 0.962 | 1.061, 1.084 | TIMING-N100-MEMCMP (ready) |
| memchr_64k, rawmemchr_64k | N100 | 0.922, 0.909 | 1.163, 1.064 | TIMING-N100-SCAN-64K-3 (queued_ci) |
| memcpy_144, memcpy_256 | N100 | 1.001 | 1.077 | NULLFOLD-COPY-MID-4 (running; its -3 hand-back: N100 12.2-12.4 cycles vs glibc 13.2-13.4) |
| memmove_4k | N100 | 1.000 | 1.080 | TIMING-N100-MEMMOVE-4K-REGRESS (running) |
| strstr_hit_4k (spread 0.840) | IVB | 1.022 | 1.106 | TIMING-COV-STRSTR-NONAVX2 (queued_ci) |
| regexec_class_nomatch_1k | IVB | 0.946 | 1.145 | TIMING-COV-REGEX-FIRSTBYTE-IVB (verifying) |
| malloc_free_16/32/64/128/256/1024, realloc_null_24, realloc_within_usable | IVB | 0.83-0.995 | 1.075-1.183 | NONE (TIMING-IVB-MALLOC-CHAIN-2 landed, IVB unchanged; latest-block fast path refused, owner #3210) -> group A |
| strchr_16_hit, strchr_64_hit, strchr_64_miss, strchrnul_16_hit, strchrnul_64_hit, strchrnul_64_miss, strncmp_eq16 | IVB | 1.000-1.015 | 1.116-1.288 | NONE (TIMING-IVB-STRSCAN-DISPATCH-2 landed in build) -> group C |
| strcmp_names, strncasecmp_c_64 | N100 | 1.022, 1.029 | 1.110, 1.050 | NONE (TIMING-COV-STRCMP-SHORT-AVX2-2, PARITY-STRCMP-AVX2 landed in build) -> group B |
| nl_langinfo_en_us, towupper_l_en_us | IVB | 0.839, 1.000 | 1.100, 1.067 | NONE (PARITY-SMALL-ENTRY-1105N-2, PERF-WCTYPE-ENUS-2 landed in build) -> group D |
| mkstemp_unlink_close | N100 | 1.029 | 1.078 (spread 0.233) | NONE: noise, item 4 |
| openpty_close | IVB | 0.992 | 1.073 (spread 0.131) | NONE: noise, item 4 |

Earlier red stories on the NONE families (red, not open; read their handoffs before redoing): PERF-HEAP-PAIR-IVB-2,
PERF-HEAP-PAIR-TIME-2 (malloc); PERF-STRCHR-NONAVX2, PERF-STRNCMP-ROWS, STR-CHR-CMP-SHORT-2 (scans);
PERF-STRCMP-NONAVX2-2, PERF-STRNCASECMP-64-AVX2 (compares); PERF-CTYPE-LANGINFO-ACCESS-5, PERF-WCTYPE-L-2 (locale).

### NONE groups ranked by gap x how common

Weight = share of Liella samples in the validation workload profiles (validation/results/profiles/*.top, the 13 Liella
workload top-25 lists; 2026-10-04 builds, so a relative weight only).

| rank | group | rows | mean time gap | workload weight | brief |
|---|---|---|---|---|---|
| A | malloc/free/realloc pair, IVB | 8 | +0.15 | high: heap symbols 19.6% bash-loop, 21.4% sed-s, 21.0% sed-utf8, 2.6% sort-n | TIMING-COV-MALLOC-PAIR-CHAIN |
| B | strcmp / strncasecmp, AVX2 class | 2 | +0.08 | medium: strcmp/strcoll 5.7% sort-utf8, 2.9% bash-loop, 1.6% ls-lR | TIMING-COV-STRCMP-NAMES-AVX2 |
| C | strchr / strchrnul / strncmp short, SSE2 class | 7 | +0.20 | low in the top-25 lists (no workload names them), but called by most programs (getenv, paths, option parsing) | TIMING-COV-STRSCAN-FRONTEND-SSE2 |
| D | nl_langinfo / towupper_l, IVB | 2 | +0.08 | low: ctype/langinfo ~1% bash-loop, printf-loop | no brief (see below) |

Group D, no brief: on the Haswell-EP role host (SSE2 class via hwcaps=-AVX2) nl_langinfo_en_us reproduces (9.04 vs
8.06 cycles at 26 vs 31 instr, front-end bound: idq_uops_not_delivered 5.4x glibc's aggregate) but towupper_l_en_us
does not (13.0 vs 14.9 cycles). One row, lowest weight: fold it into the next small-entry story rather than a brief.

## 3. Profile evidence (job shiki:tc1106c-prof, rc 0; Haswell-EP E5-2697 v3 role host, tree 4741f943)

Liella static bench vs glibc 2.39 host-dynamic, pinned to one CPU. Region counters = CBENCH_COUNTERS=1 (measured
loop only, per call: instructions / cycles). Aggregate = whole-process perf stat (glibc: aggregate counters only).
Liella samples: perf record cycles:u, perf report --sort sym. Script: handoff/timing-coverage-1106c/prof.sh.

- A, SSE2 class (hwcaps=-AVX2): malloc_free_16/64/256/1024 122.1 instr / 47.1-51.3 cycles vs 147.1 / 38.4-38.7
  (1.22-1.33x cycles at 0.83x instr); realloc_within_usable 194.1/64.9 vs 195.1/51.3 (1.27); realloc_null_24
  149.1/54.7 vs 174.1/45.9 (1.19). Aggregate resource_stalls.any 6.19e9 vs 0.69e9 (9x); stalls_ldm_pending 34.9e9 vs
  31.9e9. **The IVB gap reproduces on this build host** (the register's Skylake-SP row does not show it).
  Whole-process samples are dominated by the xthread rows the row filter did not drop (xbatch/helper 81%); free 7.7%,
  malloc 4.0%.
- B, AVX2 class: strcmp_names 64.5 / 23.6 vs 63.1 / 20.8 (1.14x cycles), branch misses 0.023 vs 0.011 per call;
  samples liella_machine_strcmp 24%. strncasecmp_c_64 70.0 / 18.1 vs 68.0 / 17.1 (1.06); idq_uops_not_delivered
  5.3e8 vs 1.8e8 (3x); samples casecmp_ord_avx2 71%, the strncasecmp export itself 17%.
- C, SSE2 class: strchr_16_hit 9.05 vs 8.55 cycles (1.06), strchr_64_hit/miss 16.2 vs 14.6 (1.11), strchrnul_16_hit
  9.05 vs 8.05 (1.12), strchrnul_64 14.1 vs 14.1 (1.00), strncmp_eq16 18.1 vs 17.1 (1.06) at equal instructions;
  idq_uops_not_delivered 2.71e9 vs 1.42e9 (1.9x), resource_stalls negligible on both: front-end bound.
- D: above.

## 4. Regressions judged noise or marginal: what settles each

- first_bindtextdomain_gettext, IVB 1.053 (spread 0.018, no instruction count: startup row). COLDSTART-BINDTEXTDOMAIN-
  REBIND(-2) landed after 4d454f06 on this very path, so any re-time must be of a build that has it. Settles it: one
  booked re-time on the other IVB timing host (same class), Liella vs glibc interleaved, plus the row's instruction
  count (`bench --compare --counters`, hwcaps=-AVX2) on next vs 4d454f06. Open a story only if time stays > 1.05 there.
- openpty_close, IVB 1.073 (spread 0.131, instr 0.992 unchanged, cycles 0.982): no landing touched
  s19_terminals.rs. Settles it: a re-time on the other IVB host; the cycles ratio below 1 already points at kernel
  time (pty setup), not user code.
- mkstemp_unlink_close, N100 1.078 (spread 0.233, 1105 0.224, instr 1.029 unchanged, cycles 1.025): no landing
  touched s13_filesystem.rs. Settles it: kernel/user split (instructions:u and :k, cycles:u and :k) on an AVX2 build
  host, then a re-time on the N100 only if user cycles exceed 1.05.
No brief for any of the three: the evidence is not there.
