# TIMING-SWEEP-1105: Liella vs glibc timing on Ivy Bridge and N100 (non-libm groups)

Hosts: nico = Ivy Bridge E3-1220L v2 (no AVX2); nozomi = Intel N100 (Alder Lake-N E-cores, AVX2). The N100 verdicts are for that core type only.
Source: out/counters.tsv and out/rounds.tsv of one run per host (same code, same CPUs; instruction ratios from the same run).
Tool: handoff/timing-sweep-1105-analyze.py. Time ratio = median of per-round Liella/glibc ratios; spread = (max-min)/median of rounds.
Verdicts: faster < 0.95, parity 0.95..1.05, slower > 1.05. REOPEN = instr <= 1.05 and time > 1.05. CLOSE = instr > 1.05 and time <= 1.05.
Benches were not rerun. Full per-row table: results/timing-sweep-1105-table.md (414 rows).

## Run status
- nozomi (N100): bench.log ends rc=0 (all 431 budgets met).
- nico (Ivy Bridge): bench.log ends rc=1. Cause: the bench's own budget check, not a crash or lost data:
  cbench_nscache/first_getpwnam_r_cached (290206 ns) and first_getpwnam_r_create (285679 ns) exceed 1.25 x first_getpwnam_r_uncached (272500 ns) on that host.
  The counters/rounds files were written and are complete (414 rows both hosts). That budget failure is a separate finding about the nscache rows on Ivy Bridge (ratio between first-call rows, spread not examined here); it is not investigated in this task.

## Verdict counts (of 414 rows)
- Ivy Bridge: faster 231, parity 95, slower 88.
- N100: faster 298, parity 79, slower 37.

## Spread statistics (rounds)
- Ivy Bridge: median 0.008, p90 0.267, max 1.408.
- N100: median 0.008, p90 0.074, max 0.374.
Rows with spread above ~0.1 (mostly the process/fs/thread rows) carry little weight; Ivy Bridge is noisier at p90.

## REOPEN (time disagrees with the instruction count: slower in time at <= 1.05 instructions)
Full list in the appendix. Cause-class, one line each:
- cbench_hot strchr/strchrnul 64 hit/miss, strncmp_eq16, strchr_16_miss (Ivy Bridge, time 1.16-1.53): instruction mix / layout. Same or fewer instructions but slower on a no-AVX2 core; the SSE-path loop or unaligned-load shape differs from glibc's selected variant.
- cbench_malloc malloc_free_16..1024, realloc_within_usable (Ivy Bridge, instr 0.83, time 1.12-1.19): latency/memory. Fewer instructions, longer dependency or store-to-load chain per call.
- cbench_memref memcmp/__memcmpeq/bcmp/memchr/memmove/memset small and mid sizes (Ivy Bridge, time 1.10-1.22): instruction mix / layout (branchy size dispatch vs fewer branches).
- cbench_memref strstr_hit_4k (Ivy Bridge, time 1.277, cycles 1.100): instruction mix (time and cycles disagree; the rest is frequency or measurement).
- cbench_memref memcpy_144/256, memcmp_256/4k, memchr_64k, rawmemchr_64k, strstr_miss_64 (N100, time 1.06-1.17): instruction mix / rep string (vector width choice on this core).
- cbench_hot mutex_lock_unlock_normal_mt (N100, instr 0.852, time 1.199): latency/memory (atomic on a shared line).
- cbench_locale iconv_utf8_ascii_translit_4k (both, time 1.30 / 1.45, instr 0.864): latency/memory (table lookup pattern).
- cbench_patterns regexec_class_nomatch_1k (Ivy Bridge, 1.145): instruction mix.
- cbench_process posix_spawn_true_heap256m (Ivy Bridge, time 1.493, cycles 1.054): not CPU cost; kernel/page-table time (cycles disagree); low weight.
These are cause-class guesses from the ratios and counters alone; no profiling was done in this task.

## CLOSE (more instructions but not slower)
- Locale and ctype rows (isalpha_c 1.44, iswalpha_utf8 1.39, towupper_utf8 1.32, wcwidth_utf8 1.16, strcasecmp_* 1.07-1.16, ctype_b_loc 1.20): instruction mix. Extra instructions are not on the critical path; time <= 1.05.
- memcmp/__memcmpeq/bcmp small sizes on N100 (instr 1.15-1.55, time 0.70-0.80): instruction mix (wider ops, fewer dependent steps).
- gettext_found, ngettext_found, gettext_language_alternating, strcmp_equal_16, strcmp_late_256, waitpid_echild, sys_close_ebadf, nl_langinfo, getline_long, strcmp_*_mix, sprintf_chk_ls_nlink, wordexp_split_var, fts_physical_1000 (N100): latency/memory (bound by loads or syscall, not instruction count).
- qsort_c_cmp_* (instr 1.29-1.51, time 0.84-0.94): instruction mix (comparator call overhead hidden by faster path).
- fwrite_4k (instr 2.05 / 1.64, time 0.999 / 0.882): rep string (copy done by rep movs; instruction count not comparable).
- memcpy_4k / memcpy_64k (instr 26.4 / 1.87 IVB, 10.5 / 98.1 N100, time <= 1.0): rep string. The instruction ratios there are an artefact of one side using rep movs (counted as one instruction), not a cost.
- re_mt4_distinct / re_mt4_shared (instr 1.4-1.7, time 0.02-0.16, cycles 1.0-1.2): multi-thread rows; time ratio is not comparable to the instruction counter (per-thread vs wall), so neither list applies. Cycles ratio (1.02-1.20) is the usable number; N100 re_mt4_shared at 1.203 deserves a look.

## Appendix: dis.md (generated)
    REOPEN (instr <= 1.05, time > 1.05)
    cbench_hot/mutex_lock_unlock_normal_mt | N100 | instr 0.852 | time 1.199 | cycles 1.229
    cbench_hot/strchr_16_miss | Ivy Bridge | instr 0.750 | time 1.162 | cycles 1.163
    cbench_hot/strchr_64_hit | Ivy Bridge | instr 1.036 | time 1.373 | cycles 1.374
    cbench_hot/strchr_64_miss | Ivy Bridge | instr 1.036 | time 1.472 | cycles 1.469
    cbench_hot/strchrnul_64_hit | Ivy Bridge | instr 1.038 | time 1.534 | cycles 1.533
    cbench_hot/strchrnul_64_miss | Ivy Bridge | instr 1.038 | time 1.439 | cycles 1.437
    cbench_hot/strncmp_eq16 | Ivy Bridge | instr 1.015 | time 1.265 | cycles 1.263
    cbench_locale/iconv_utf8_ascii_translit_4k | Ivy Bridge | instr 0.864 | time 1.299 | cycles 1.298
    cbench_locale/iconv_utf8_ascii_translit_4k | N100 | instr 0.864 | time 1.454 | cycles 1.453
    cbench_malloc/malloc_free_1024 | Ivy Bridge | instr 0.830 | time 1.119 | cycles 1.121
    cbench_malloc/malloc_free_128 | Ivy Bridge | instr 0.830 | time 1.190 | cycles 1.188
    cbench_malloc/malloc_free_16 | Ivy Bridge | instr 0.830 | time 1.121 | cycles 1.122
    cbench_malloc/malloc_free_256 | Ivy Bridge | instr 0.830 | time 1.184 | cycles 1.183
    cbench_malloc/malloc_free_32 | Ivy Bridge | instr 0.830 | time 1.170 | cycles 1.169
    cbench_malloc/malloc_free_64 | Ivy Bridge | instr 0.830 | time 1.183 | cycles 1.183
    cbench_malloc/realloc_null_24 | Ivy Bridge | instr 0.851 | time 1.063 | cycles 1.078
    cbench_malloc/realloc_within_usable | Ivy Bridge | instr 0.980 | time 1.186 | cycles 1.187
    cbench_memref/__memcmpeq_head_8 | Ivy Bridge | instr 0.790 | time 1.101 | cycles 1.113
    cbench_memref/__memcmpeq_tail_8 | Ivy Bridge | instr 0.783 | time 1.221 | cycles 1.197
    cbench_memref/bcmp_tail_128 | Ivy Bridge | instr 0.960 | time 1.173 | cycles 1.179
    cbench_memref/memchr_64 | Ivy Bridge | instr 0.780 | time 1.177 | cycles 1.173
    cbench_memref/memchr_64k | N100 | instr 0.922 | time 1.169 | cycles 1.129
    cbench_memref/memcmp_128 | Ivy Bridge | instr 0.910 | time 1.103 | cycles 1.094
    cbench_memref/memcmp_144 | Ivy Bridge | instr 0.988 | time 1.172 | cycles 1.128
    cbench_memref/memcmp_256 | N100 | instr 0.913 | time 1.061 | cycles 1.059
    cbench_memref/memcmp_4k | N100 | instr 0.962 | time 1.086 | cycles 1.087
    cbench_memref/memcpy_144 | N100 | instr 1.000 | time 1.077 | cycles 1.076
    cbench_memref/memcpy_256 | N100 | instr 1.001 | time 1.077 | cycles 1.077
    cbench_memref/memmove_144 | Ivy Bridge | instr 0.831 | time 1.167 | cycles 1.168
    cbench_memref/memset_144 | Ivy Bridge | instr 0.866 | time 1.214 | cycles 1.201
    cbench_memref/rawmemchr_4k | Ivy Bridge | instr 0.908 | time 1.076 | cycles 1.070
    cbench_memref/rawmemchr_64k | N100 | instr 0.909 | time 1.060 | cycles 1.058
    cbench_memref/strstr_hit_4k | Ivy Bridge | instr 1.023 | time 1.277 | cycles 1.100
    cbench_memref/strstr_miss_64 | N100 | instr 0.804 | time 1.102 | cycles 1.073
    cbench_patterns/regexec_class_nomatch_1k | Ivy Bridge | instr 0.946 | time 1.145 | cycles 1.145
    cbench_process/posix_spawn_true_heap256m | Ivy Bridge | instr 1.029 | time 1.493 | cycles 1.054
    CLOSE (instr > 1.05, time <= 1.05)
    cbench_fstree/fts_physical_1000 | N100 | instr 1.064 | time 1.026 | cycles 1.033
    cbench_hot/ctype_b_loc | Ivy Bridge | instr 1.199 | time 0.856 | cycles 0.857
    cbench_hot/ctype_b_loc | N100 | instr 1.198 | time 0.376 | cycles 0.375
    cbench_hot/getline_long | N100 | instr 1.168 | time 1.009 | cycles 0.995
    cbench_hot/strncmp_names64 | Ivy Bridge | instr 1.072 | time 1.000 | cycles 1.000
    cbench_hot/sys_close_ebadf | Ivy Bridge | instr 1.063 | time 1.026 | cycles 1.027
    cbench_hot/sys_close_ebadf | N100 | instr 1.066 | time 1.003 | cycles 1.004
    cbench_locale/isalpha_c | Ivy Bridge | instr 1.437 | time 1.000 | cycles 1.001
    cbench_locale/isalpha_c | N100 | instr 1.436 | time 0.554 | cycles 0.556
    cbench_locale/iswalpha_utf8 | Ivy Bridge | instr 1.387 | time 0.956 | cycles 0.957
    cbench_locale/nl_langinfo_en_us | N100 | instr 1.129 | time 1.000 | cycles 1.000
    cbench_locale/strcasecmp_c_256 | Ivy Bridge | instr 1.161 | time 0.971 | cycles 0.971
    cbench_locale/strcasecmp_c_256 | N100 | instr 1.072 | time 0.804 | cycles 0.803
    cbench_locale/strcasecmp_en_256 | Ivy Bridge | instr 1.161 | time 0.976 | cycles 0.977
    cbench_locale/strcasecmp_en_256 | N100 | instr 1.072 | time 0.804 | cycles 0.804
    cbench_locale/strcasecmp_utf8_256 | Ivy Bridge | instr 1.161 | time 0.976 | cycles 0.976
    cbench_locale/strcasecmp_utf8_256 | N100 | instr 1.072 | time 0.804 | cycles 0.804
    cbench_locale/towupper_utf8 | Ivy Bridge | instr 1.324 | time 0.798 | cycles 0.797
    cbench_locale/towupper_utf8 | N100 | instr 1.325 | time 0.944 | cycles 0.934
    cbench_locale/wcwidth_utf8 | Ivy Bridge | instr 1.162 | time 0.791 | cycles 0.790
    cbench_locale/wcwidth_utf8 | N100 | instr 1.163 | time 1.000 | cycles 1.000
    cbench_memref/__memcmpeq_eq_32 | Ivy Bridge | instr 1.107 | time 1.002 | cycles 1.002
    cbench_memref/__memcmpeq_eq_32 | N100 | instr 1.192 | time 0.730 | cycles 0.733
    cbench_memref/__memcmpeq_head_128 | N100 | instr 1.199 | time 0.730 | cycles 0.733
    cbench_memref/__memcmpeq_head_1k | N100 | instr 1.199 | time 0.730 | cycles 0.733
    cbench_memref/__memcmpeq_tail_32 | N100 | instr 1.545 | time 0.801 | cycles 0.805
    cbench_memref/bcmp_eq_32 | Ivy Bridge | instr 1.069 | time 1.000 | cycles 1.001
    cbench_memref/bcmp_eq_32 | N100 | instr 1.145 | time 0.703 | cycles 0.703
    cbench_memref/bcmp_tail_32 | N100 | instr 1.293 | time 0.730 | cycles 0.740
    cbench_memref/memcmp_16 | Ivy Bridge | instr 1.080 | time 0.817 | cycles 0.824
    cbench_memref/memcmp_16_mix | Ivy Bridge | instr 1.051 | time 0.924 | cycles 0.929
    cbench_memref/memcmp_diff8_127 | N100 | instr 1.207 | time 0.801 | cycles 0.806
    cbench_memref/memcmp_diff8_64 | N100 | instr 1.249 | time 0.801 | cycles 0.806
    cbench_memref/memcmp_diff8_96 | N100 | instr 1.206 | time 0.801 | cycles 0.808
    cbench_memref/memcpy_4k | Ivy Bridge | instr 26.399 | time 0.866 | cycles 0.865
    cbench_memref/memcpy_4k | N100 | instr 10.518 | time 0.479 | cycles 0.479
    cbench_memref/memcpy_64k | Ivy Bridge | instr 1.870 | time 0.952 | cycles 0.956
    cbench_memref/memcpy_64k | N100 | instr 98.133 | time 0.989 | cycles 0.987
    cbench_memref/memset_16 | N100 | instr 1.053 | time 0.452 | cycles 0.463
    cbench_memref/strcmp_16_mix | N100 | instr 1.149 | time 1.028 | cycles 1.023
    cbench_memref/strcmp_64_mix | N100 | instr 1.150 | time 1.006 | cycles 0.998
    cbench_memref/strnlen_16 | N100 | instr 1.077 | time 0.730 | cycles 0.734
    cbench_memref/strnlen_16_mix | N100 | instr 1.059 | time 0.752 | cycles 0.751
    cbench_opt/gettext_found | Ivy Bridge | instr 1.310 | time 0.993 | cycles 0.995
    cbench_opt/gettext_found | N100 | instr 1.331 | time 1.008 | cycles 1.003
    cbench_opt/gettext_language_alternating | N100 | instr 1.193 | time 1.028 | cycles 0.998
    cbench_opt/ngettext_found | Ivy Bridge | instr 1.062 | time 0.949 | cycles 0.990
    cbench_opt/ngettext_found | N100 | instr 1.076 | time 0.978 | cycles 0.978
    cbench_opt/re_mt4_distinct | Ivy Bridge | instr 1.443 | time 0.158 | cycles 1.067
    cbench_opt/re_mt4_distinct | N100 | instr 1.546 | time 0.140 | cycles 1.117
    cbench_opt/re_mt4_shared | Ivy Bridge | instr 1.447 | time 0.021 | cycles 1.016
    cbench_opt/re_mt4_shared | N100 | instr 1.693 | time 0.023 | cycles 1.203
    cbench_opt/sprintf_chk_ls_nlink | N100 | instr 1.170 | time 1.047 | cycles 1.060
    cbench_opt/strcmp_equal_16 | Ivy Bridge | instr 1.064 | time 1.000 | cycles 0.999
    cbench_opt/strcmp_equal_16 | N100 | instr 1.107 | time 1.000 | cycles 1.000
    cbench_opt/strcmp_late_256 | Ivy Bridge | instr 1.458 | time 1.020 | cycles 1.020
    cbench_opt/wordexp_split_var | N100 | instr 1.082 | time 0.958 | cycles 0.985
    cbench_process/waitpid_echild | Ivy Bridge | instr 1.088 | time 1.014 | cycles 1.002
    cbench_process/waitpid_echild | N100 | instr 1.090 | time 1.016 | cycles 1.016
    cbench_stdio/fwrite_4k | Ivy Bridge | instr 2.050 | time 0.999 | cycles 1.035
    cbench_stdio/fwrite_4k | N100 | instr 1.635 | time 0.882 | cycles 0.880
    cbench_unmeasured/qsort_c_cmp_1024 | N100 | instr 1.510 | time 0.937 | cycles 0.939
    cbench_unmeasured/qsort_c_cmp_1024_e4 | N100 | instr 1.506 | time 0.913 | cycles 0.920
    cbench_unmeasured/qsort_c_cmp_1024_e64 | Ivy Bridge | instr 1.290 | time 0.839 | cycles 0.837
    cbench_unmeasured/qsort_c_cmp_1024_e64 | N100 | instr 1.352 | time 0.889 | cycles 0.878
    cbench_unmeasured/qsort_c_cmp_1024_e8 | N100 | instr 1.478 | time 0.930 | cycles 0.913
    spread ivb: n=414 median 0.008 p90 0.267 max 1.408
    spread n100: n=414 median 0.008 p90 0.074 max 0.374
    ivb: faster 231, parity 95, slower 88 of 414
    n100: faster 298, parity 79, slower 37 of 414
