# Timing sweep 1106E: non-libm rows, Ivy Bridge and N100 (Alder Lake-N), after the 11-06 landings

Build: origin/next 34c98f61 (42 landings after the 1106 build 4d454f06; list in
handoff/timing-sweep-1106e/landings-4d454f06..34c98f61.txt). Staged once with `cargo xtask bench --stage <tar>
--only <17 groups>` (76 products matched bench-stage.manifest), unpacked whole on each timing host, and run
unchanged with `./xtask-bin bench --no-build --staged --counters --only <17 groups>`: Liella and glibc on the
same host, rounds interleaved, V-4 procedure, the same 17 groups and the same command as the 1106 sweep (aio,
fstree, hot, locale, malloc, memref, netdb, nscache, opt, patterns, process, services, startup, stdio, thread,
time, unmeasured). Both runs ran on booked hosts with nothing else queued.

Hosts (CPU classes):
- Ivy Bridge (Xeon E3-1265L v2 class, no AVX2). The same host as the 1106 Ivy Bridge run.
- N100 (Intel Alder Lake-N, 4 E-cores, AVX2). Its verdicts are for that core type, not for big cores. The same
  host as the 1106 N100 run.

Columns: `instr` = Liella/glibc instructions per call on that class (counts of this run); `time` = median
Liella/glibc time ratio over the interleaved rounds; `spread` = (max - min) / median of the round ratios for the
row; verdict faster (< 0.95), parity, slower (> 1.05); `1106 time` and `now/1106` = the same row in
results/timing-sweep-1106-table.md and the change. Analyzer: handoff/timing-sweep-1106e/analyze.py (checked first
on the 1106 raw data: it reproduces the 1106 table, drift median 1.000, p10 0.999, p90 1.001).

## Summary

| | faster | parity | slower | spread median | spread p90 | spread max |
|---|---|---|---|---|---|---|
| Ivy Bridge now (407 judged rows) | 252 | 99 | 56 | 0.009 | 0.094 | 0.929 |
| Ivy Bridge 1106 (404) | 238 | 97 | 69 | 0.010 | 0.101 | 0.930 |
| N100 now (407 judged rows) | 313 | 74 | 20 | 0.012 | 0.105 | 0.319 |
| N100 1106 (404) | 307 | 74 | 23 | 0.015 | 0.105 | 0.300 |

Sweep-to-sweep drift (now/1106 over the 403 rows in both): Ivy Bridge median 1.000 (p10 0.895, p90 1.017); N100
median 1.000 (p10 0.924, p90 1.015). A change inside the row's spread is not a finding. The 8 wordexp rows are
OWNER-DEFERRED: timed and listed apart at the end of the table, not counted above. Three rows are new: the two
memmove overlap rows (BENCH-MEMMOVE-OVERLAP-SHORT) and cbench_startup/startup_kinstr_c.

The Ivy Bridge bench exited 1, as in 1106 and 1105: the run finished and wrote every table; the exit is the
bench's own per-host budget check (1.25 x the ratio calibrated on main 5236e77), which flagged 4 rows, down from
11 in 1106: __memcmpeq_eq_128 (cycles 1.270), __memcmpeq_tail_1k (cycles 1.062), cbench_malloc/xthread_free_64
(ns 0.654, faster than glibc) and cbench_hot/getenv_hit_mid (cycles 0.954, faster than glibc). The N100 run exited 0
(all 433 budgets met).

## (a) Regressions: slower now, parity or faster in 1106

| row | class | instr now (1106) | time now (1106) | spread | likely cause |
|---|---|---|---|---|---|
| cbench_opt/first_mktime_new_york | Ivy Bridge | - | 4.885 (0.914) | 0.025 | **coldstart-second-latency-4** (a86c9677): the only landing between the builds that changed the mktime/time-zone C path (crates/legacy/src/time_compat.rs and crates/legacy/src/recheck.rs; no other landing touched crates/core/src/time or crates/boundary/src/posix/s23_time.rs). Liella 8.8 us against glibc 1.8 us per first call, every round. Real, on both classes: open a story. |
| cbench_opt/first_mktime_new_york | N100 | - | 4.800 (0.994) | 0.060 | Same row and same cause as above. |
| cbench_opt/first_snprintf | N100 | - | 1.263 (0.990) | 0.179 | Also worse on Ivy Bridge (1.162 -> 1.449, spread 0.017; slower in both sweeps, so not in this list there). Landings on the first-call stdio path: **coldstart-fork-child-reset-2** (bf079683, crates/legacy/src/stdio_compat.rs) and coldstart-second-latency-4 (a86c9677, crates/legacy/src/recheck.rs). Most likely bf079683, the only one that changed stdio code. Real on Ivy Bridge (spread 0.017); open a story with the mktime row. |
| cbench_opt/gettext_language_steady | Ivy Bridge | 1.587 (1.588) | 1.051 (1.045) | 0.005 | Marginal (0.001 over the line, +0.006 since 1106), instructions unchanged. Landing on the gettext path: coldstart-second-latency-4 (a86c9677, recheck.rs). Not a finding at this size; re-time with the next sweep. |
| cbench_locale/strncasecmp_c_64 | N100 | 1.029 (1.029) | 1.050 (1.050) | 0.000 | Not a regression: 1.05018 now and 1.05018 in 1106 (identical medians of 3 rounds); listed only because the 1106 table rounds it to 1.050 (parity). No change. |

## (b) Reopen and (c) improved

List (b) (instructions at or below 1.05, time above 1.05) has 36 entries, 31 Ivy Bridge and 5 N100 (1106: 39).
Most were on the 1106 list with the same time: Ivy Bridge malloc_free_* and the memcmp/memmove/memset/rawmemchr
mid sizes, strchr/strchrnul. New on it: first_mktime_new_york and first_snprintf have no instruction count, so
they are in (a) only.

List (c) has 21 rows that went from slower to parity or better. The 11-06 string/memory kernels got their verdict:
- parity-memcmp-small-eq-2 / timing-cov-memcmp-nonavx2 / timing-ivb-memref-entry-2: __memcmpeq_eq_64, _tail_8,
  _tail_32, _tail_64, _tail_128, _head_8, bcmp_eq_64, bcmp_tail_64, memcmp_144 on Ivy Bridge now faster or parity
  (time 1.055 to 1.421 before). __memcmpeq_eq_128 and bcmp_eq_128 on Ivy Bridge are still slower.
- timing-cov-memchr-short-nonavx2-4: memchr_16_mix on Ivy Bridge 1.174 -> 0.949.
- timing-n100-memcmp-3: memcmp_4k on N100 1.084 -> 1.015.
- sse2-copy-mid-split-2 / nullfold-copy-mid-4: memcpy_144 and memcpy_256 on N100 1.077 -> 0.925.
- timing-n100-memmove-4k-regress: memmove_4k on N100 1.080 -> 0.971 (the 1106 regression is fixed).
- timing-cov-regex-firstbyte-ivb-2: regexec_class_nomatch_1k on Ivy Bridge 1.145 -> 0.263.
- timing-cov-fork-cow-faults: fork_exit_wait 1.287 -> 0.888 (Ivy Bridge) and 1.275 -> 0.915 (N100).
- strncmp_eq16 on Ivy Bridge 1.158 -> 1.000 (timing-cov-strcmp-names-avx2-2 or timing-cov-strscan-frontend-sse2).
- Noise rows from the 1106 list (a) are back: openpty_close (Ivy Bridge), mkstemp_unlink_close (N100),
  first_bindtextdomain_gettext (Ivy Bridge, 1.053 -> 0.992; the 1106 marginal regression did not hold).
Open stories on these rows may close (coordinator's call).

New rows: memmove_overlap8_4k is faster on Ivy Bridge (0.781) and at parity on N100 (0.992); memmove_overlap64_4k is
at parity on Ivy Bridge (0.948, 0.002 under the line) and faster on N100 (0.520). Their instruction ratios (26 on Ivy
Bridge, 10 for overlap64 on N100) are far from their time ratios: instructions mislead on these rows, judge them by
time. startup_kinstr_c: faster on both (0.541, 0.472).

## Lists (analyzer output)

## REGRESSIONS (slower now, parity/faster in 1106): 5
- cbench_locale/strncasecmp_c_64 | N100 | instr 1.029 (1106 1.029) | time 1.050 (1106 1.050) | spread 0.000 | cycles 1.049
- cbench_opt/first_mktime_new_york | Ivy Bridge | instr - (1106 -) | time 4.885 (1106 0.914) | spread 0.025 | cycles -
- cbench_opt/first_mktime_new_york | N100 | instr - (1106 -) | time 4.800 (1106 0.994) | spread 0.060 | cycles -
- cbench_opt/first_snprintf | N100 | instr - (1106 -) | time 1.263 (1106 0.990) | spread 0.179 | cycles -
- cbench_opt/gettext_language_steady | Ivy Bridge | instr 1.587 (1106 1.588) | time 1.051 (1106 1.045) | spread 0.005 | cycles 1.215
## REOPEN (instr <= 1.05, time > 1.05): 36
- cbench_hot/strchr_16_hit | Ivy Bridge | instr 1.000 (1106 1.000) | time 1.116 (1106 1.116) | spread 0.002 | cycles 1.116
- cbench_hot/strchr_64_hit | Ivy Bridge | instr 1.000 (1106 1.000) | time 1.135 (1106 1.135) | spread 0.000 | cycles 1.135
- cbench_hot/strchr_64_miss | Ivy Bridge | instr 1.000 (1106 1.000) | time 1.289 (1106 1.288) | spread 0.005 | cycles 1.286
- cbench_hot/strchrnul_16_hit | Ivy Bridge | instr 1.000 (1106 1.000) | time 1.110 (1106 1.221) | spread 0.000 | cycles 1.118
- cbench_hot/strchrnul_64_hit | Ivy Bridge | instr 1.000 (1106 1.000) | time 1.269 (1106 1.267) | spread 0.005 | cycles 1.274
- cbench_hot/strchrnul_64_miss | Ivy Bridge | instr 1.000 (1106 1.000) | time 1.194 (1106 1.188) | spread 0.005 | cycles 1.188
- cbench_locale/nl_langinfo_en_us | Ivy Bridge | instr 0.839 (1106 0.839) | time 1.100 (1106 1.100) | spread 0.000 | cycles 1.100
- cbench_locale/strncasecmp_c_64 | N100 | instr 1.029 (1106 1.029) | time 1.050 (1106 1.050) | spread 0.000 | cycles 1.049
- cbench_locale/towupper_l_en_us | Ivy Bridge | instr 1.000 (1106 1.000) | time 1.067 (1106 1.067) | spread 0.000 | cycles 1.066
- cbench_malloc/malloc_free_1024 | Ivy Bridge | instr 0.830 (1106 0.830) | time 1.107 (1106 1.112) | spread 0.091 | cycles 1.107
- cbench_malloc/malloc_free_128 | Ivy Bridge | instr 0.830 (1106 0.830) | time 1.178 (1106 1.179) | spread 0.000 | cycles 1.184
- cbench_malloc/malloc_free_16 | Ivy Bridge | instr 0.830 (1106 0.830) | time 1.114 (1106 1.118) | spread 0.001 | cycles 1.118
- cbench_malloc/malloc_free_256 | Ivy Bridge | instr 0.830 (1106 0.830) | time 1.179 (1106 1.179) | spread 0.044 | cycles 1.175
- cbench_malloc/malloc_free_32 | Ivy Bridge | instr 0.830 (1106 0.830) | time 1.284 (1106 1.170) | spread 0.003 | cycles 1.290
- cbench_malloc/malloc_free_64 | Ivy Bridge | instr 0.830 (1106 0.830) | time 1.170 (1106 1.181) | spread 0.004 | cycles 1.173
- cbench_malloc/realloc_null_24 | Ivy Bridge | instr 0.857 (1106 0.857) | time 1.168 (1106 1.075) | spread 0.002 | cycles 1.188
- cbench_malloc/realloc_within_usable | Ivy Bridge | instr 0.995 (1106 0.995) | time 1.299 (1106 1.183) | spread 0.000 | cycles 1.302
- cbench_memref/__memcmpeq_eq_128 | Ivy Bridge | instr 0.955 (1106 1.109) | time 1.263 (1106 1.350) | spread 0.000 | cycles 1.270
- cbench_memref/__memcmpeq_eq_1k | Ivy Bridge | instr 0.963 (1106 1.212) | time 1.077 (1106 1.247) | spread 0.394 | cycles 1.102
- cbench_memref/bcmp_eq_128 | Ivy Bridge | instr 0.926 (1106 1.076) | time 1.260 (1106 1.444) | spread 0.050 | cycles 1.265
- cbench_memref/bcmp_eq_1k | Ivy Bridge | instr 0.960 (1106 1.208) | time 1.065 (1106 1.231) | spread 0.297 | cycles 1.080
- cbench_memref/bcmp_tail_128 | Ivy Bridge | instr 0.826 (1106 0.960) | time 1.101 (1106 1.174) | spread 0.002 | cycles 1.055
- cbench_memref/memchr_16 | Ivy Bridge | instr 1.001 (1106 1.517) | time 1.251 (1106 1.583) | spread 0.000 | cycles 1.242
- cbench_memref/memchr_64 | Ivy Bridge | instr 0.780 (1106 0.744) | time 1.117 (1106 1.059) | spread 0.000 | cycles 1.116
- cbench_memref/memchr_64k | N100 | instr 0.770 (1106 0.922) | time 1.066 (1106 1.163) | spread 0.028 | cycles 1.039
- cbench_memref/memcmp_128 | Ivy Bridge | instr 0.895 (1106 0.909) | time 1.052 (1106 1.105) | spread 0.052 | cycles 1.047
- cbench_memref/memcmp_256 | Ivy Bridge | instr 1.010 (1106 1.086) | time 1.162 (1106 1.257) | spread 0.028 | cycles 1.132
- cbench_memref/memcmp_256 | N100 | instr 0.913 (1106 0.913) | time 1.061 (1106 1.061) | spread 0.000 | cycles 1.059
- cbench_memref/memmove_144 | Ivy Bridge | instr 0.798 (1106 0.830) | time 1.167 (1106 1.167) | spread 0.000 | cycles 1.165
- cbench_memref/memset_144 | Ivy Bridge | instr 0.824 (1106 0.870) | time 1.215 (1106 1.215) | spread 0.000 | cycles 1.209
- cbench_memref/rawmemchr_4k | Ivy Bridge | instr 0.908 (1106 0.908) | time 1.078 (1106 1.090) | spread 0.017 | cycles 1.071
- cbench_memref/rawmemchr_64k | N100 | instr 0.822 (1106 0.909) | time 1.064 (1106 1.064) | spread 0.012 | cycles 1.056
- cbench_memref/strstr_hit_4k | Ivy Bridge | instr 0.989 (1106 1.022) | time 1.104 (1106 1.106) | spread 0.929 | cycles 1.100
- cbench_memref/strstr_miss_64 | Ivy Bridge | instr 0.908 (1106 1.370) | time 1.087 (1106 1.375) | spread 0.003 | cycles 1.084
- cbench_opt/strcmp_names | N100 | instr 1.022 (1106 1.022) | time 1.111 (1106 1.110) | spread 0.006 | cycles 1.121
- cbench_patterns/regexec_backref | Ivy Bridge | instr 1.014 (1106 1.059) | time 1.146 (1106 1.199) | spread 0.028 | cycles 1.130
## IMPROVED (slower in 1106, parity/faster now): 21
- cbench_fstree/mkstemp_unlink_close | N100 | instr 1.030 (1106 1.029) | time 0.924 (1106 1.078) | spread 0.058 | cycles 1.023
- cbench_hot/strncmp_eq16 | Ivy Bridge | instr 1.015 (1106 1.015) | time 1.000 (1106 1.158) | spread 0.004 | cycles 0.999
- cbench_memref/__memcmpeq_eq_64 | Ivy Bridge | instr 0.949 (1106 1.373) | time 0.934 (1106 1.188) | spread 0.000 | cycles 0.929
- cbench_memref/__memcmpeq_head_8 | Ivy Bridge | instr 0.786 (1106 0.786) | time 1.000 (1106 1.100) | spread 0.000 | cycles 1.003
- cbench_memref/__memcmpeq_tail_128 | Ivy Bridge | instr 0.954 (1106 1.109) | time 1.049 (1106 1.421) | spread 0.000 | cycles 1.048
- cbench_memref/__memcmpeq_tail_32 | Ivy Bridge | instr 1.071 (1106 1.107) | time 0.836 (1106 1.093) | spread 0.002 | cycles 0.842
- cbench_memref/__memcmpeq_tail_64 | Ivy Bridge | instr 0.949 (1106 1.370) | time 0.813 (1106 1.265) | spread 0.000 | cycles 0.812
- cbench_memref/__memcmpeq_tail_8 | Ivy Bridge | instr 0.783 (1106 0.790) | time 0.900 (1106 1.221) | spread 0.000 | cycles 0.892
- cbench_memref/bcmp_eq_64 | Ivy Bridge | instr 0.928 (1106 1.342) | time 0.876 (1106 1.199) | spread 0.000 | cycles 0.880
- cbench_memref/bcmp_tail_64 | Ivy Bridge | instr 0.778 (1106 1.124) | time 0.766 (1106 1.055) | spread 0.000 | cycles 0.773
- cbench_memref/memchr_16_mix | Ivy Bridge | instr 0.910 (1106 1.285) | time 0.949 (1106 1.174) | spread 0.075 | cycles 0.947
- cbench_memref/memcmp_144 | Ivy Bridge | instr 0.913 (1106 0.988) | time 1.043 (1106 1.173) | spread 0.044 | cycles 1.087
- cbench_memref/memcmp_4k | N100 | instr 0.872 (1106 0.962) | time 1.015 (1106 1.084) | spread 0.002 | cycles 1.016
- cbench_memref/memcpy_144 | N100 | instr 0.922 (1106 1.001) | time 0.925 (1106 1.077) | spread 0.000 | cycles 0.926
- cbench_memref/memcpy_256 | N100 | instr 0.921 (1106 1.001) | time 0.925 (1106 1.077) | spread 0.002 | cycles 0.921
- cbench_memref/memmove_4k | N100 | instr 1.036 (1106 1.000) | time 0.971 (1106 1.080) | spread 0.005 | cycles 0.987
- cbench_opt/first_bindtextdomain_gettext | Ivy Bridge | instr - (1106 -) | time 0.992 (1106 1.053) | spread 0.012 | cycles -
- cbench_patterns/regexec_class_nomatch_1k | Ivy Bridge | instr 0.207 (1106 0.946) | time 0.263 (1106 1.145) | spread 0.002 | cycles 0.266
- cbench_process/fork_exit_wait | Ivy Bridge | instr 1.054 (1106 1.087) | time 0.888 (1106 1.287) | spread 0.026 | cycles 0.986
- cbench_process/fork_exit_wait | N100 | instr 1.084 (1106 1.081) | time 0.915 (1106 1.275) | spread 0.014 | cycles 1.052
- cbench_process/openpty_close | Ivy Bridge | instr 0.992 (1106 0.992) | time 0.944 (1106 1.073) | spread 0.080 | cycles 0.982
## rows not in the 1106 table: 6
- cbench_memref/memmove_overlap64_4k | Ivy Bridge | instr 26.160 (1106 -) | time 0.948 (1106 -) | spread 0.003 | cycles 0.975
- cbench_memref/memmove_overlap64_4k | N100 | instr 10.196 (1106 -) | time 0.520 (1106 -) | spread 0.000 | cycles 0.520
- cbench_memref/memmove_overlap8_4k | Ivy Bridge | instr 25.869 (1106 -) | time 0.781 (1106 -) | spread 0.000 | cycles 0.784
- cbench_memref/memmove_overlap8_4k | N100 | instr 1.000 (1106 -) | time 0.992 (1106 -) | spread 0.013 | cycles 0.988
- cbench_startup/startup_kinstr_c | Ivy Bridge | instr 0.226 (1106 -) | time 0.541 (1106 -) | spread 0.240 | cycles -
- cbench_startup/startup_kinstr_c | N100 | instr 0.235 (1106 -) | time 0.472 (1106 -) | spread 0.040 | cycles -
spread ivb: n=417 median 0.009 p90 0.094 max 0.929
drift now/1106 ivb: n=403 median 1.000 p10 0.895 p90 1.017
ivb now: faster 252, parity 99, slower 56 of 407; 1106: faster 238, parity 97, slower 69 of 404
spread n100: n=417 median 0.012 p90 0.105 max 0.319
drift now/1106 n100: n=403 median 1.000 p10 0.924 p90 1.015
n100 now: faster 313, parity 74, slower 20 of 407; 1106: faster 307, parity 74, slower 23 of 404

## Table

| bench/row | instr IVB | time IVB | spread IVB | verdict IVB | 1106 time IVB | now/1106 IVB | instr N100 | time N100 | spread N100 | verdict N100 | 1106 time N100 | now/1106 N100 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| cbench_fstree/fdopendir_readdir_closedir_99 | 1.000 | 0.947 | 0.010 | faster | 0.945 | 1.002 | 1.000 | 0.955 | 0.012 | parity | 0.941 | 1.015 |
| cbench_fstree/fts_nochdir_flat_1000 | 0.846 | 0.890 | 0.012 | faster | 0.891 | 0.999 | 0.830 | 0.912 | 0.014 | faster | 0.920 | 0.991 |
| cbench_fstree/fts_physical_1000 | 1.055 | 1.028 | 0.025 | parity | 1.030 | 0.998 | 1.064 | 1.027 | 0.031 | parity | 1.038 | 0.989 |
| cbench_fstree/getcwd | 1.016 | 1.001 | 0.037 | parity | 1.005 | 0.996 | 1.025 | 1.006 | 0.010 | parity | 1.007 | 0.999 |
| cbench_fstree/getmntent_50_lines | 1.037 | 0.972 | 0.017 | parity | 0.974 | 0.998 | 1.041 | 0.830 | 0.068 | faster | 0.863 | 0.962 |
| cbench_fstree/mkstemp_unlink_close | 1.028 | 1.030 | 0.116 | parity | 1.035 | 0.996 | 1.030 | 0.924 | 0.058 | faster | 1.078 | 0.858 |
| cbench_fstree/nftw_phys_1000 | 1.024 | 1.001 | 0.008 | parity | 0.992 | 1.009 | 1.027 | 1.002 | 0.025 | parity | 0.996 | 1.006 |
| cbench_fstree/opendir_readdir_closedir_1000 | 0.999 | 0.943 | 0.007 | faster | 0.939 | 1.004 | 0.997 | 0.944 | 0.010 | faster | 0.942 | 1.002 |
| cbench_fstree/readdir_entry | 1.000 | 0.943 | 0.009 | faster | 0.941 | 1.002 | 0.998 | 0.947 | 0.006 | faster | 0.944 | 1.003 |
| cbench_fstree/realpath_5 | 1.038 | 1.013 | 0.005 | parity | 1.024 | 0.989 | 1.047 | 1.027 | 0.011 | parity | 1.030 | 0.997 |
| cbench_fstree/scandir_alphasort_1000 | 0.969 | 0.934 | 0.004 | faster | 0.928 | 1.007 | 0.974 | 0.932 | 0.002 | faster | 0.930 | 1.002 |
| cbench_hot/ctype_b_loc | 1.199 | 0.858 | 0.000 | faster | 0.858 | 1.000 | 1.198 | 0.376 | 0.000 | faster | 0.376 | 1.000 |
| cbench_hot/ctype_get_mb_cur_max | 0.909 | 1.000 | 0.000 | parity | 1.000 | 1.000 | 0.908 | 0.499 | 0.000 | faster | 0.499 | 0.999 |
| cbench_hot/errno_location | 0.900 | 1.000 | 0.000 | parity | 1.000 | 1.000 | 0.899 | 0.499 | 0.000 | faster | 0.499 | 0.999 |
| cbench_hot/fwrite_unlocked_16 | 0.729 | 0.706 | 0.017 | faster | 0.701 | 1.007 | 0.729 | 0.601 | 0.002 | faster | 0.602 | 0.999 |
| cbench_hot/fwrite_unlocked_64 | 0.704 | 0.724 | 0.037 | faster | 0.714 | 1.014 | 0.757 | 0.637 | 0.002 | faster | 0.637 | 1.000 |
| cbench_hot/getc | 0.817 | 1.000 | 0.002 | parity | 1.000 | 1.000 | 0.815 | 0.643 | 0.000 | faster | 0.643 | 1.000 |
| cbench_hot/getc_unlocked | 1.000 | 0.881 | 0.015 | faster | 0.882 | 0.999 | 0.999 | 0.423 | 0.000 | faster | 0.423 | 0.999 |
| cbench_hot/getenv_hit_last | 0.464 | 0.572 | 0.005 | faster | 0.576 | 0.992 | 0.805 | 0.678 | 0.004 | faster | 0.677 | 1.001 |
| cbench_hot/getenv_hit_last_2000 | 0.914 | 0.948 | 0.118 | faster | 0.919 | 1.032 | 0.921 | 0.886 | 0.001 | faster | 0.886 | 1.000 |
| cbench_hot/getenv_hit_last_300 | 0.899 | 1.009 | 0.004 | parity | 1.012 | 0.997 | 0.943 | 0.501 | 0.008 | faster | 0.500 | 1.001 |
| cbench_hot/getenv_hit_mid | 0.754 | 0.933 | 0.109 | faster | 0.941 | 0.991 | 0.844 | 0.773 | 0.000 | faster | 0.773 | 1.000 |
| cbench_hot/getenv_hit_mid_2000 | 0.918 | 0.960 | 0.030 | parity | 0.956 | 1.004 | 0.923 | 0.905 | 0.003 | faster | 0.905 | 1.000 |
| cbench_hot/getenv_hit_mid_300 | 0.928 | 1.034 | 0.003 | parity | 1.033 | 1.001 | 0.958 | 0.588 | 0.007 | faster | 0.588 | 1.000 |
| cbench_hot/getenv_miss | 0.417 | 0.513 | 0.002 | faster | 0.513 | 1.000 | 0.460 | 0.374 | 0.001 | faster | 0.374 | 1.000 |
| cbench_hot/getenv_miss_2000 | 0.914 | 0.948 | 0.116 | faster | 0.923 | 1.027 | 0.915 | 0.887 | 0.001 | faster | 0.887 | 1.000 |
| cbench_hot/getenv_miss_300 | 0.900 | 0.922 | 0.005 | faster | 1.001 | 0.922 | 0.904 | 0.494 | 0.007 | faster | 0.494 | 1.000 |
| cbench_hot/getline_80 | 0.992 | 0.999 | 0.022 | parity | 1.010 | 0.989 | 1.018 | 1.005 | 0.031 | parity | 0.986 | 1.019 |
| cbench_hot/getline_long | 1.194 | 1.078 | 0.046 | slower | 1.121 | 0.962 | 1.148 | 0.994 | 0.019 | parity | 1.011 | 0.983 |
| cbench_hot/memcpy_chk_64 | 0.692 | 0.799 | 0.002 | faster | 0.799 | 1.000 | 0.931 | 0.637 | 0.000 | faster | 0.637 | 1.000 |
| cbench_hot/mempcpy_16 | 0.955 | 1.000 | 0.000 | parity | 1.000 | 1.000 | 0.954 | 0.546 | 0.000 | faster | 0.546 | 0.999 |
| cbench_hot/mempcpy_64 | 0.719 | 0.799 | 0.000 | faster | 0.799 | 1.000 | 1.000 | 0.701 | 0.000 | faster | 0.701 | 1.000 |
| cbench_hot/memrchr_4k | 0.860 | 0.887 | 0.006 | faster | 0.885 | 1.002 | 0.897 | 1.037 | 0.000 | parity | 1.037 | 1.000 |
| cbench_hot/memrchr_64 | 0.906 | 1.000 | 0.000 | parity | 1.000 | 1.000 | 0.846 | 0.667 | 0.000 | faster | 0.667 | 1.000 |
| cbench_hot/memrchr_64_hit | 1.107 | 1.277 | 0.003 | slower | 1.277 | 1.000 | 1.000 | 0.777 | 0.000 | faster | 0.777 | 1.000 |
| cbench_hot/mutex_lock_unlock_normal | 0.847 | 0.953 | 0.002 | parity | 0.951 | 1.002 | 0.847 | 0.667 | 0.000 | faster | 0.667 | 1.000 |
| cbench_hot/mutex_lock_unlock_normal_mt | 0.824 | 0.820 | 0.223 | faster | 0.820 | 0.999 | 0.825 | 0.985 | 0.002 | parity | 0.985 | 1.000 |
| cbench_hot/mutex_lock_unlock_recursive | 0.878 | 0.780 | 0.003 | faster | 0.780 | 1.000 | 0.878 | 0.616 | 0.000 | faster | 0.616 | 1.000 |
| cbench_hot/mutex_lock_unlock_recursive_mt | 0.850 | 0.961 | 0.002 | parity | 0.961 | 1.000 | 0.849 | 0.704 | 0.018 | faster | 0.701 | 1.004 |
| cbench_hot/putc_unlocked | 0.897 | 0.862 | 0.007 | faster | 0.863 | 0.999 | 0.897 | 0.466 | 0.000 | faster | 0.466 | 1.000 |
| cbench_hot/putenv_2000 | 0.130 | 0.184 | 0.003 | faster | 0.185 | 0.996 | 0.138 | 0.202 | 0.164 | faster | 0.199 | 1.013 |
| cbench_hot/putenv_30 | 0.791 | 0.846 | 0.005 | faster | 0.845 | 1.002 | 0.860 | 0.732 | 0.148 | faster | 0.679 | 1.079 |
| cbench_hot/putenv_remove_cycle_2000 | 0.134 | 0.192 | 0.006 | faster | 0.192 | 1.001 | 0.147 | 0.232 | 0.200 | faster | 0.228 | 1.016 |
| cbench_hot/putenv_remove_cycle_30 | 0.930 | 0.979 | 0.002 | parity | 0.988 | 0.991 | 1.023 | 0.892 | 0.065 | faster | 0.840 | 1.061 |
| cbench_hot/setenv_2000 | 0.091 | 0.127 | 0.003 | faster | 0.127 | 0.998 | 0.146 | 0.219 | 0.002 | faster | 0.219 | 1.001 |
| cbench_hot/setenv_30 | 0.603 | 0.593 | 0.010 | faster | 0.566 | 1.048 | 0.851 | 0.583 | 0.021 | faster | 0.575 | 1.014 |
| cbench_hot/setenv_300 | 0.119 | 0.125 | 0.005 | faster | 0.127 | 0.987 | 0.190 | 0.132 | 0.008 | faster | 0.133 | 0.995 |
| cbench_hot/setenv_getenv_2000 | 0.129 | 0.171 | 0.009 | faster | 0.172 | 0.994 | 0.201 | 0.265 | 0.109 | faster | 0.292 | 0.906 |
| cbench_hot/setenv_getenv_300 | 0.159 | 0.167 | 0.020 | faster | 0.168 | 0.993 | 0.246 | 0.173 | 0.122 | faster | 0.174 | 0.997 |
| cbench_hot/strchr_16_hit | 1.000 | 1.116 | 0.002 | slower | 1.116 | 1.000 | 0.965 | 0.777 | 0.000 | faster | 0.777 | 1.000 |
| cbench_hot/strchr_16_miss | 0.714 | 0.918 | 0.000 | faster | 0.918 | 1.000 | 0.965 | 0.728 | 0.000 | faster | 0.728 | 1.000 |
| cbench_hot/strchr_64_hit | 1.000 | 1.135 | 0.000 | slower | 1.135 | 1.000 | 0.921 | 0.909 | 0.000 | faster | 0.909 | 1.000 |
| cbench_hot/strchr_64_miss | 1.000 | 1.289 | 0.005 | slower | 1.288 | 1.001 | 0.921 | 0.909 | 0.000 | faster | 0.909 | 1.000 |
| cbench_hot/strchrnul_16_hit | 1.000 | 1.110 | 0.000 | slower | 1.221 | 0.909 | 0.926 | 0.701 | 0.000 | faster | 0.701 | 1.000 |
| cbench_hot/strchrnul_16_miss | 0.698 | 0.813 | 0.002 | faster | 0.750 | 1.084 | 0.926 | 0.701 | 0.000 | faster | 0.701 | 1.000 |
| cbench_hot/strchrnul_64_hit | 1.000 | 1.269 | 0.005 | slower | 1.267 | 1.001 | 0.889 | 0.820 | 0.000 | faster | 0.820 | 0.999 |
| cbench_hot/strchrnul_64_miss | 1.000 | 1.194 | 0.005 | slower | 1.188 | 1.005 | 0.889 | 0.751 | 0.000 | faster | 0.751 | 1.000 |
| cbench_hot/strcpy_chk_16 | 0.488 | 0.599 | 0.002 | faster | 0.600 | 0.999 | 0.590 | 0.442 | 0.030 | faster | 0.442 | 0.999 |
| cbench_hot/strcpy_chk_64 | 0.587 | 0.599 | 0.003 | faster | 0.599 | 1.001 | 0.586 | 0.512 | 0.109 | faster | 0.499 | 1.027 |
| cbench_hot/strncmp_diff8 | 1.436 | 1.417 | 0.004 | slower | 1.585 | 0.894 | 1.000 | 1.000 | 0.000 | parity | 1.000 | 1.000 |
| cbench_hot/strncmp_eq16 | 1.015 | 1.000 | 0.004 | parity | 1.158 | 0.864 | 1.029 | 1.000 | 0.000 | parity | 1.000 | 1.000 |
| cbench_hot/strncmp_names64 | 1.072 | 0.968 | 0.002 | parity | 0.971 | 0.997 | 0.977 | 0.846 | 0.000 | faster | 0.846 | 1.000 |
| cbench_hot/sys_close_ebadf | 1.063 | 1.002 | 0.049 | parity | 1.002 | 1.000 | 1.066 | 1.003 | 0.001 | parity | 1.004 | 0.999 |
| cbench_hot/sys_fstat | 1.007 | 1.005 | 0.031 | parity | 1.014 | 0.991 | 1.008 | 1.001 | 0.021 | parity | 1.002 | 0.999 |
| cbench_hot/sys_read_1 | 1.012 | 1.009 | 0.049 | parity | 1.021 | 0.988 | 1.012 | 1.003 | 0.000 | parity | 1.003 | 1.000 |
| cbench_hot/unsetenv_cycle_2000 | 0.109 | 0.154 | 0.004 | faster | 0.154 | 0.999 | 0.145 | 0.219 | 0.191 | faster | 0.219 | 0.999 |
| cbench_hot/unsetenv_cycle_30 | 0.750 | 0.833 | 0.003 | faster | 0.827 | 1.007 | 0.988 | 0.883 | 0.079 | faster | 0.836 | 1.056 |
| cbench_hot/unsetenv_miss_2000 | 0.135 | 0.196 | 0.005 | faster | 0.196 | 0.999 | 0.150 | 0.230 | 0.124 | faster | 0.237 | 0.968 |
| cbench_hot/unsetenv_miss_30 | 0.592 | 0.609 | 0.002 | faster | 0.599 | 1.017 | 0.657 | 0.548 | 0.004 | faster | 0.546 | 1.003 |
| cbench_locale/fnmatch_casefold_en_32 | 0.964 | 0.804 | 0.007 | faster | 0.837 | 0.961 | 0.943 | 0.984 | 0.003 | parity | 0.988 | 0.996 |
| cbench_locale/fnmatch_en_us | 0.282 | 0.312 | 0.023 | faster | 0.312 | 1.000 | 0.299 | 0.323 | 0.008 | faster | 0.323 | 1.001 |
| cbench_locale/iconv_utf8_8859_15_4k | 0.321 | 0.277 | 0.002 | faster | 0.277 | 1.000 | 0.321 | 0.349 | 0.005 | faster | 0.351 | 0.995 |
| cbench_locale/iconv_utf8_ascii_translit_4k | 0.410 | 0.492 | 0.020 | faster | 0.492 | 1.000 | 0.410 | 0.563 | 0.003 | faster | 0.565 | 0.997 |
| cbench_locale/iconv_utf8_utf16_4k | 0.485 | 0.567 | 0.001 | faster | 0.568 | 0.999 | 0.485 | 0.743 | 0.003 | faster | 0.737 | 1.008 |
| cbench_locale/isalpha_c | 1.437 | 1.000 | 0.000 | parity | 1.000 | 1.000 | 1.436 | 0.554 | 0.000 | faster | 0.554 | 1.001 |
| cbench_locale/iswalpha_en_us | 1.244 | 0.886 | 0.001 | faster | 0.884 | 1.002 | 1.245 | 0.990 | 0.003 | parity | 0.988 | 1.002 |
| cbench_locale/iswalpha_l_en_us | 1.489 | 1.330 | 0.000 | slower | 1.330 | 1.000 | 1.489 | 1.448 | 0.000 | slower | 1.448 | 1.000 |
| cbench_locale/iswalpha_utf8 | 1.346 | 0.955 | 0.000 | parity | 0.954 | 1.001 | 1.347 | 1.061 | 0.002 | slower | 1.063 | 0.998 |
| cbench_locale/iswctype_en_us | 1.768 | 2.090 | 0.000 | slower | 2.090 | 1.000 | 1.767 | 1.840 | 0.000 | slower | 1.840 | 1.000 |
| cbench_locale/localeconv_en_us | 0.491 | 0.532 | 0.152 | faster | 0.532 | 1.001 | 0.491 | 0.520 | 0.000 | faster | 0.520 | 1.000 |
| cbench_locale/mbrtowc_en_us_3 | 0.889 | 0.884 | 0.001 | faster | 0.885 | 0.999 | 0.889 | 0.838 | 0.000 | faster | 0.840 | 0.998 |
| cbench_locale/mbrtowc_utf8_1 | 0.202 | 0.277 | 0.000 | faster | 0.277 | 0.998 | 0.202 | 0.228 | 0.004 | faster | 0.229 | 0.997 |
| cbench_locale/mbrtowc_utf8_2 | 0.241 | 0.300 | 0.002 | faster | 0.299 | 1.002 | 0.241 | 0.268 | 0.002 | faster | 0.268 | 1.000 |
| cbench_locale/mbrtowc_utf8_3 | 0.889 | 0.910 | 0.001 | faster | 0.910 | 1.000 | 0.889 | 0.856 | 0.001 | faster | 0.858 | 0.998 |
| cbench_locale/newlocale_free | 0.346 | 0.337 | 0.019 | faster | 0.341 | 0.987 | 0.291 | 0.321 | 0.018 | faster | 0.309 | 1.038 |
| cbench_locale/nl_langinfo_en_us | 0.839 | 1.100 | 0.000 | slower | 1.100 | 1.000 | 0.839 | 0.701 | 0.000 | faster | 0.701 | 1.000 |
| cbench_locale/nl_langinfo_l_en_us | 1.250 | 1.221 | 0.005 | slower | 1.221 | 1.000 | 1.250 | 0.801 | 0.000 | faster | 0.801 | 1.001 |
| cbench_locale/snprintf_f_en_us | 0.442 | 0.301 | 0.003 | faster | 0.299 | 1.008 | 0.442 | 0.414 | 0.015 | faster | 0.410 | 1.010 |
| cbench_locale/strcasecmp_c_16 | 0.979 | 1.022 | 0.002 | parity | 1.022 | 1.000 | 1.060 | 1.066 | 0.000 | slower | 1.066 | 1.000 |
| cbench_locale/strcasecmp_c_256 | 1.161 | 0.971 | 0.000 | parity | 0.971 | 1.000 | 1.068 | 0.804 | 0.003 | faster | 0.803 | 1.002 |
| cbench_locale/strcasecmp_en_256 | 1.161 | 0.976 | 0.000 | parity | 0.976 | 1.000 | 1.068 | 0.803 | 0.002 | faster | 0.803 | 1.000 |
| cbench_locale/strcasecmp_utf8_256 | 1.161 | 0.975 | 0.000 | parity | 0.976 | 0.999 | 1.068 | 0.803 | 0.001 | faster | 0.803 | 1.000 |
| cbench_locale/strcoll_utf8_16 | 0.576 | 0.729 | 0.366 | faster | 0.731 | 0.998 | 0.897 | 0.866 | 0.117 | faster | 0.866 | 1.000 |
| cbench_locale/strftime_names_en_us | 1.011 | 0.954 | 0.004 | parity | 0.958 | 0.996 | 1.011 | 0.936 | 0.038 | faster | 0.937 | 0.998 |
| cbench_locale/strncasecmp_c_64 | 0.932 | 0.950 | 0.001 | parity | 0.947 | 1.004 | 1.029 | 1.050 | 0.000 | slower | 1.050 | 1.000 |
| cbench_locale/strtod_c | 0.318 | 0.222 | 0.016 | faster | 0.223 | 0.995 | 0.316 | 0.245 | 0.005 | faster | 0.244 | 1.005 |
| cbench_locale/strtod_en_us | 0.318 | 0.223 | 0.015 | faster | 0.226 | 0.986 | 0.316 | 0.246 | 0.012 | faster | 0.244 | 1.009 |
| cbench_locale/strtod_utf8 | 0.318 | 0.221 | 0.015 | faster | 0.225 | 0.983 | 0.316 | 0.245 | 0.007 | faster | 0.245 | 1.001 |
| cbench_locale/toupper_c | 1.050 | 1.000 | 0.000 | parity | 1.000 | 1.000 | 1.049 | 0.554 | 0.000 | faster | 0.554 | 1.001 |
| cbench_locale/towupper_l_en_us | 1.000 | 1.067 | 0.000 | slower | 1.067 | 1.000 | 1.000 | 0.909 | 0.000 | faster | 0.909 | 1.000 |
| cbench_locale/towupper_utf8 | 1.324 | 0.845 | 0.003 | faster | 0.845 | 1.000 | 1.325 | 1.023 | 0.004 | parity | 1.026 | 0.997 |
| cbench_locale/wcwidth_utf8 | 1.162 | 0.791 | 0.001 | faster | 0.790 | 1.002 | 1.163 | 1.000 | 0.000 | parity | 1.000 | 1.000 |
| cbench_malloc/calloc_free_4096 | 0.440 | 0.832 | 0.001 | faster | 0.831 | 1.001 | 0.441 | 0.874 | 0.003 | faster | 0.870 | 1.004 |
| cbench_malloc/calloc_free_64 | 0.608 | 0.694 | 0.002 | faster | 0.698 | 0.995 | 0.608 | 0.597 | 0.003 | faster | 0.599 | 0.997 |
| cbench_malloc/heap_mix_real | 0.735 | 0.672 | 0.010 | faster | 0.670 | 1.003 | 0.736 | 0.697 | 0.008 | faster | 0.697 | 1.000 |
| cbench_malloc/heap_mix_real_2t | - | 0.427 | 0.069 | faster | 0.448 | 0.952 | - | 0.562 | 0.023 | faster | 0.562 | 1.001 |
| cbench_malloc/lifo_batch64 | 0.735 | 0.968 | 0.038 | parity | 0.960 | 1.008 | 0.735 | 0.702 | 0.013 | faster | 0.698 | 1.006 |
| cbench_malloc/malloc_free_1024 | 0.830 | 1.107 | 0.091 | slower | 1.112 | 0.996 | 0.830 | 0.738 | 0.006 | faster | 0.739 | 0.999 |
| cbench_malloc/malloc_free_128 | 0.830 | 1.178 | 0.000 | slower | 1.179 | 0.999 | 0.830 | 0.738 | 0.003 | faster | 0.740 | 0.998 |
| cbench_malloc/malloc_free_16 | 0.830 | 1.114 | 0.001 | slower | 1.118 | 0.997 | 0.830 | 0.738 | 0.000 | faster | 0.739 | 0.999 |
| cbench_malloc/malloc_free_256 | 0.830 | 1.179 | 0.044 | slower | 1.179 | 1.000 | 0.830 | 0.719 | 0.003 | faster | 0.719 | 1.000 |
| cbench_malloc/malloc_free_32 | 0.830 | 1.284 | 0.003 | slower | 1.170 | 1.098 | 0.830 | 0.743 | 0.004 | faster | 0.742 | 1.001 |
| cbench_malloc/malloc_free_64 | 0.830 | 1.170 | 0.004 | slower | 1.181 | 0.991 | 0.830 | 0.719 | 0.004 | faster | 0.722 | 0.996 |
| cbench_malloc/realloc_grow_16step | 0.539 | 0.544 | 0.018 | faster | 0.542 | 1.004 | 0.524 | 0.503 | 0.002 | faster | 0.503 | 1.000 |
| cbench_malloc/realloc_grow_x2 | 0.962 | 0.915 | 0.027 | faster | 0.888 | 1.031 | 0.827 | 0.769 | 0.029 | faster | 0.788 | 0.976 |
| cbench_malloc/realloc_moving | 0.564 | 0.592 | 0.000 | faster | 0.570 | 1.039 | 0.564 | 0.540 | 0.010 | faster | 0.543 | 0.995 |
| cbench_malloc/realloc_null_24 | 0.857 | 1.168 | 0.002 | slower | 1.075 | 1.086 | 0.856 | 0.810 | 0.149 | faster | 0.919 | 0.882 |
| cbench_malloc/realloc_within_usable | 0.995 | 1.299 | 0.000 | slower | 1.183 | 1.098 | 0.995 | 0.866 | 0.105 | faster | 0.905 | 0.956 |
| cbench_malloc/xthread_free_64 | - | 0.654 | 0.009 | faster | 0.654 | 1.000 | - | 0.466 | 0.003 | faster | 0.468 | 0.996 |
| cbench_memref/__memcmpeq_eq_128 | 0.955 | 1.263 | 0.000 | slower | 1.350 | 0.936 | 1.028 | 0.920 | 0.002 | faster | 0.849 | 1.084 |
| cbench_memref/__memcmpeq_eq_16 | 1.005 | 0.801 | 0.000 | faster | 0.729 | 1.099 | 0.769 | 0.457 | 0.012 | faster | 0.419 | 1.090 |
| cbench_memref/__memcmpeq_eq_1k | 0.963 | 1.077 | 0.394 | slower | 1.247 | 0.864 | 0.918 | 1.015 | 0.000 | parity | 1.015 | 1.000 |
| cbench_memref/__memcmpeq_eq_32 | 1.075 | 1.002 | 0.000 | parity | 1.002 | 1.000 | 1.149 | 0.701 | 0.003 | faster | 0.730 | 0.960 |
| cbench_memref/__memcmpeq_eq_64 | 0.949 | 0.934 | 0.000 | faster | 1.188 | 0.786 | 0.963 | 0.703 | 0.007 | faster | 0.641 | 1.097 |
| cbench_memref/__memcmpeq_eq_8 | 1.001 | 0.890 | 0.000 | faster | 0.801 | 1.110 | 0.854 | 0.502 | 0.019 | faster | 0.457 | 1.099 |
| cbench_memref/__memcmpeq_head_128 | 1.282 | 1.538 | 0.001 | slower | 1.768 | 0.870 | 1.194 | 0.819 | 0.013 | faster | 0.732 | 1.119 |
| cbench_memref/__memcmpeq_head_16 | 0.783 | 0.910 | 0.000 | faster | 1.000 | 0.910 | 0.670 | 0.547 | 0.020 | faster | 0.638 | 0.857 |
| cbench_memref/__memcmpeq_head_1k | 1.288 | 1.538 | 0.001 | slower | 1.768 | 0.870 | 1.202 | 0.819 | 0.011 | faster | 0.730 | 1.122 |
| cbench_memref/__memcmpeq_head_32 | 0.860 | 0.770 | 0.000 | faster | 0.847 | 0.909 | 0.902 | 0.547 | 0.016 | faster | 0.638 | 0.857 |
| cbench_memref/__memcmpeq_head_64 | 0.860 | 0.770 | 0.000 | faster | 0.847 | 0.909 | 0.808 | 0.638 | 0.010 | faster | 0.638 | 1.001 |
| cbench_memref/__memcmpeq_head_8 | 0.786 | 1.000 | 0.000 | parity | 1.100 | 0.909 | 0.668 | 0.547 | 0.020 | faster | 0.638 | 0.857 |
| cbench_memref/__memcmpeq_tail_128 | 0.954 | 1.049 | 0.000 | parity | 1.421 | 0.738 | 1.030 | 0.849 | 0.000 | faster | 0.918 | 0.925 |
| cbench_memref/__memcmpeq_tail_16 | 1.001 | 0.729 | 0.000 | faster | 0.801 | 0.909 | 0.852 | 0.458 | 0.000 | faster | 0.502 | 0.911 |
| cbench_memref/__memcmpeq_tail_1k | 0.963 | 1.043 | 0.000 | parity | 0.264 | 3.952 | 0.247 | 0.194 | 0.000 | faster | 0.194 | 1.002 |
| cbench_memref/__memcmpeq_tail_32 | 1.071 | 0.836 | 0.002 | faster | 1.093 | 0.765 | 1.496 | 0.595 | 0.002 | faster | 0.801 | 0.743 |
| cbench_memref/__memcmpeq_tail_64 | 0.949 | 0.813 | 0.000 | faster | 1.265 | 0.643 | 0.960 | 0.641 | 0.000 | faster | 0.703 | 0.911 |
| cbench_memref/__memcmpeq_tail_8 | 0.783 | 0.900 | 0.000 | faster | 1.221 | 0.738 | 0.670 | 0.458 | 0.010 | faster | 0.701 | 0.653 |
| cbench_memref/bcmp_eq_128 | 0.926 | 1.260 | 0.050 | slower | 1.444 | 0.873 | 0.948 | 0.847 | 0.002 | faster | 0.918 | 0.923 |
| cbench_memref/bcmp_eq_16 | 0.922 | 0.729 | 0.000 | faster | 0.704 | 1.035 | 0.660 | 0.387 | 0.005 | faster | 0.419 | 0.923 |
| cbench_memref/bcmp_eq_1k | 0.960 | 1.065 | 0.297 | slower | 1.231 | 0.865 | 0.913 | 1.015 | 0.000 | parity | 1.015 | 1.000 |
| cbench_memref/bcmp_eq_32 | 1.035 | 0.919 | 0.000 | faster | 1.002 | 0.917 | 1.111 | 0.638 | 0.009 | faster | 0.703 | 0.908 |
| cbench_memref/bcmp_eq_64 | 0.928 | 0.876 | 0.000 | faster | 1.199 | 0.730 | 0.928 | 0.641 | 0.016 | faster | 0.703 | 0.911 |
| cbench_memref/bcmp_eq_8 | 0.799 | 0.801 | 0.000 | faster | 0.782 | 1.024 | 0.793 | 0.458 | 0.000 | faster | 0.502 | 0.911 |
| cbench_memref/bcmp_head_128 | 1.080 | 1.462 | 0.000 | slower | 1.915 | 0.763 | 1.001 | 0.730 | 0.008 | faster | 0.801 | 0.911 |
| cbench_memref/bcmp_head_16 | 0.724 | 0.693 | 0.000 | faster | 0.917 | 0.756 | 0.565 | 0.419 | 0.014 | faster | 0.638 | 0.657 |
| cbench_memref/bcmp_head_1k | 1.084 | 1.462 | 0.000 | slower | 1.915 | 0.763 | 0.998 | 0.730 | 0.008 | faster | 0.801 | 0.911 |
| cbench_memref/bcmp_head_32 | 0.722 | 0.693 | 0.000 | faster | 0.917 | 0.756 | 0.753 | 0.458 | 0.000 | faster | 0.701 | 0.653 |
| cbench_memref/bcmp_head_64 | 0.724 | 0.693 | 0.000 | faster | 0.917 | 0.756 | 0.669 | 0.547 | 0.016 | faster | 0.701 | 0.780 |
| cbench_memref/bcmp_head_8 | 0.550 | 0.751 | 0.000 | faster | 1.000 | 0.751 | 0.566 | 0.419 | 0.010 | faster | 0.638 | 0.657 |
| cbench_memref/bcmp_tail_128 | 0.826 | 1.101 | 0.002 | slower | 1.174 | 0.938 | 0.841 | 0.847 | 0.009 | faster | 0.786 | 1.078 |
| cbench_memref/bcmp_tail_16 | 0.796 | 0.668 | 0.000 | faster | 0.617 | 1.083 | 0.724 | 0.458 | 0.000 | faster | 0.419 | 1.092 |
| cbench_memref/bcmp_tail_1k | 0.937 | 1.020 | 0.018 | parity | 0.255 | 4.002 | 0.238 | 0.191 | 0.000 | faster | 0.191 | 1.003 |
| cbench_memref/bcmp_tail_32 | 0.887 | 0.717 | 0.024 | faster | 0.801 | 0.895 | 1.245 | 0.653 | 0.000 | faster | 0.730 | 0.895 |
| cbench_memref/bcmp_tail_64 | 0.778 | 0.766 | 0.000 | faster | 1.055 | 0.726 | 0.786 | 0.639 | 0.000 | faster | 0.587 | 1.089 |
| cbench_memref/bcmp_tail_8 | 0.548 | 0.819 | 0.000 | faster | 0.917 | 0.893 | 0.565 | 0.458 | 0.057 | faster | 0.585 | 0.782 |
| cbench_memref/memchr_16 | 1.001 | 1.251 | 0.000 | slower | 1.583 | 0.790 | 0.892 | 0.637 | 0.011 | faster | 0.637 | 1.000 |
| cbench_memref/memchr_16_mix | 0.910 | 0.949 | 0.075 | faster | 1.174 | 0.808 | 0.916 | 0.617 | 0.012 | faster | 0.624 | 0.989 |
| cbench_memref/memchr_4k | 0.836 | 0.786 | 0.016 | faster | 0.837 | 0.939 | 0.781 | 0.988 | 0.001 | parity | 1.036 | 0.953 |
| cbench_memref/memchr_64 | 0.780 | 1.117 | 0.000 | slower | 1.059 | 1.055 | 0.729 | 0.541 | 0.011 | faster | 0.541 | 1.001 |
| cbench_memref/memchr_64_mix | 0.745 | 0.894 | 0.023 | faster | 0.858 | 1.042 | 0.695 | 0.464 | 0.028 | faster | 0.464 | 1.001 |
| cbench_memref/memchr_64k | 0.852 | 1.010 | 0.011 | parity | 1.006 | 1.004 | 0.770 | 1.066 | 0.028 | slower | 1.163 | 0.917 |
| cbench_memref/memcmp_128 | 0.895 | 1.052 | 0.052 | slower | 1.105 | 0.952 | 0.924 | 0.772 | 0.015 | faster | 0.772 | 1.000 |
| cbench_memref/memcmp_144 | 0.913 | 1.043 | 0.044 | parity | 1.173 | 0.889 | 0.721 | 0.707 | 0.001 | faster | 0.707 | 1.000 |
| cbench_memref/memcmp_16 | 1.080 | 0.819 | 0.000 | faster | 0.817 | 1.002 | 0.932 | 0.549 | 0.009 | faster | 0.549 | 1.000 |
| cbench_memref/memcmp_16_mix | 1.050 | 0.924 | 0.000 | faster | 0.924 | 0.999 | 0.940 | 0.550 | 0.032 | faster | 0.553 | 0.995 |
| cbench_memref/memcmp_256 | 1.010 | 1.162 | 0.028 | slower | 1.257 | 0.924 | 0.913 | 1.061 | 0.000 | slower | 1.061 | 1.000 |
| cbench_memref/memcmp_4k | 0.950 | 0.992 | 0.000 | parity | 0.976 | 1.017 | 0.872 | 1.015 | 0.002 | parity | 1.084 | 0.937 |
| cbench_memref/memcmp_64 | 1.023 | 0.938 | 0.000 | faster | 0.936 | 1.002 | 0.998 | 0.728 | 0.009 | faster | 0.730 | 0.998 |
| cbench_memref/memcmp_64_mix | 1.018 | 0.959 | 0.014 | parity | 0.960 | 0.999 | 1.002 | 0.998 | 0.009 | parity | 0.998 | 1.000 |
| cbench_memref/memcmp_64k | 0.945 | 0.961 | 0.008 | parity | 0.972 | 0.989 | 0.943 | 0.830 | 0.011 | faster | 0.831 | 0.998 |
| cbench_memref/memcmp_diff8_127 | 1.396 | 1.334 | 0.000 | slower | 1.583 | 0.843 | 1.207 | 0.801 | 0.007 | faster | 0.801 | 1.001 |
| cbench_memref/memcmp_diff8_64 | 1.317 | 1.251 | 0.000 | slower | 1.251 | 1.000 | 1.251 | 0.801 | 0.009 | faster | 0.801 | 1.001 |
| cbench_memref/memcmp_diff8_96 | 1.400 | 1.334 | 0.000 | slower | 1.583 | 0.843 | 1.203 | 0.801 | 0.007 | faster | 0.801 | 1.001 |
| cbench_memref/memcpy_128 | 0.947 | 0.998 | 0.002 | parity | 1.000 | 0.998 | 0.933 | 0.752 | 0.004 | faster | 0.752 | 1.001 |
| cbench_memref/memcpy_144 | 0.850 | 1.010 | 0.001 | parity | 1.011 | 0.999 | 0.922 | 0.925 | 0.000 | faster | 1.077 | 0.859 |
| cbench_memref/memcpy_16 | 0.949 | 0.777 | 0.003 | faster | 0.779 | 0.997 | 0.949 | 0.404 | 0.007 | faster | 0.404 | 1.001 |
| cbench_memref/memcpy_16_mix | 1.001 | 1.001 | 0.000 | parity | 1.001 | 1.000 | 1.000 | 1.010 | 0.026 | parity | 1.016 | 0.994 |
| cbench_memref/memcpy_256 | 0.609 | 0.751 | 0.040 | faster | 0.740 | 1.015 | 0.921 | 0.925 | 0.002 | faster | 1.077 | 0.859 |
| cbench_memref/memcpy_4k | 26.421 | 0.863 | 0.016 | faster | 0.867 | 0.995 | 10.313 | 0.551 | 0.175 | faster | 0.476 | 1.157 |
| cbench_memref/memcpy_4k_off | 0.992 | 0.885 | 0.000 | faster | 0.889 | 0.996 | 1.005 | 1.048 | 0.001 | parity | 1.048 | 1.000 |
| cbench_memref/memcpy_64 | 0.697 | 0.642 | 0.004 | faster | 0.644 | 0.997 | 1.001 | 0.559 | 0.008 | faster | 0.559 | 0.999 |
| cbench_memref/memcpy_64_mix | 1.001 | 1.001 | 0.001 | parity | 1.001 | 1.000 | 1.001 | 1.030 | 0.031 | parity | 1.036 | 0.994 |
| cbench_memref/memcpy_64k | 1.873 | 0.969 | 0.023 | parity | 0.957 | 1.013 | 95.572 | 0.986 | 0.192 | parity | 0.991 | 0.995 |
| cbench_memref/memmem_miss_4k | 0.428 | 0.417 | 0.023 | faster | 0.579 | 0.720 | 0.187 | 0.227 | 0.003 | faster | 0.227 | 0.999 |
| cbench_memref/memmem_miss_64 | 0.651 | 0.501 | 0.027 | faster | 0.501 | 1.000 | 0.547 | 0.268 | 0.006 | faster | 0.268 | 1.000 |
| cbench_memref/memmove_128 | 0.947 | 1.001 | 0.000 | parity | 1.001 | 1.000 | 0.935 | 0.901 | 0.000 | faster | 0.901 | 1.000 |
| cbench_memref/memmove_144 | 0.798 | 1.167 | 0.000 | slower | 1.167 | 1.000 | 0.922 | 0.859 | 0.001 | faster | 0.859 | 1.000 |
| cbench_memref/memmove_16 | 0.950 | 0.900 | 0.000 | faster | 0.900 | 1.000 | 0.950 | 0.925 | 0.034 | faster | 0.910 | 1.017 |
| cbench_memref/memmove_256 | 0.672 | 1.001 | 0.001 | parity | 1.001 | 1.000 | 0.922 | 0.947 | 0.000 | faster | 0.947 | 1.000 |
| cbench_memref/memmove_4k | 0.891 | 1.007 | 0.000 | parity | 1.007 | 1.000 | 1.036 | 0.971 | 0.005 | parity | 1.080 | 0.899 |
| cbench_memref/memmove_64 | 0.696 | 1.000 | 0.000 | parity | 1.000 | 1.000 | 1.005 | 0.937 | 0.000 | faster | 0.937 | 1.000 |
| cbench_memref/memmove_64k | 0.798 | 1.009 | 0.010 | parity | 1.007 | 1.002 | 0.925 | 0.996 | 0.005 | parity | 0.989 | 1.007 |
| cbench_memref/memmove_overlap64_4k | 26.160 | 0.948 | 0.003 | faster | - | - | 10.196 | 0.520 | 0.000 | faster | - | - |
| cbench_memref/memmove_overlap8_4k | 25.869 | 0.781 | 0.000 | faster | - | - | 1.000 | 0.992 | 0.013 | parity | - | - |
| cbench_memref/memset_128 | 0.746 | 0.735 | 0.000 | faster | 0.735 | 1.000 | 0.964 | 0.801 | 0.070 | faster | 0.852 | 0.941 |
| cbench_memref/memset_144 | 0.824 | 1.215 | 0.000 | slower | 1.215 | 1.000 | 1.088 | 0.911 | 0.013 | faster | 0.911 | 1.000 |
| cbench_memref/memset_16 | 0.954 | 0.779 | 0.000 | faster | 0.779 | 1.000 | 1.053 | 0.452 | 0.012 | faster | 0.452 | 1.000 |
| cbench_memref/memset_16_mix | 1.002 | 1.000 | 0.000 | parity | 1.000 | 1.000 | 1.002 | 0.995 | 0.001 | parity | 0.995 | 1.000 |
| cbench_memref/memset_256 | 0.712 | 1.001 | 0.135 | parity | 1.001 | 1.000 | 1.088 | 0.822 | 0.228 | faster | 0.911 | 0.902 |
| cbench_memref/memset_4k | 1.004 | 1.002 | 0.006 | parity | 1.002 | 1.000 | 1.040 | 1.000 | 0.000 | parity | 1.000 | 1.000 |
| cbench_memref/memset_4k_off | 1.006 | 1.007 | 0.000 | parity | 1.007 | 1.000 | 1.000 | 1.000 | 0.000 | parity | 1.000 | 1.000 |
| cbench_memref/memset_64 | 0.716 | 0.693 | 0.000 | faster | 0.693 | 1.001 | 0.998 | 0.559 | 0.008 | faster | 0.559 | 0.999 |
| cbench_memref/memset_64_mix | 1.001 | 1.000 | 0.000 | parity | 1.000 | 1.000 | 1.001 | 1.000 | 0.000 | parity | 1.000 | 1.000 |
| cbench_memref/memset_64k | 1.011 | 0.997 | 0.011 | parity | 1.004 | 0.993 | 1.027 | 1.007 | 0.051 | parity | 0.991 | 1.017 |
| cbench_memref/rawmemchr_16 | 0.908 | 0.799 | 0.002 | faster | 0.799 | 1.000 | 0.915 | 0.559 | 0.005 | faster | 0.559 | 0.999 |
| cbench_memref/rawmemchr_16_mix | 0.914 | 0.700 | 0.015 | faster | 0.703 | 0.996 | 0.935 | 0.639 | 0.013 | faster | 0.641 | 0.997 |
| cbench_memref/rawmemchr_4k | 0.908 | 1.078 | 0.017 | slower | 1.090 | 0.989 | 0.864 | 1.035 | 0.004 | parity | 1.047 | 0.988 |
| cbench_memref/rawmemchr_64 | 0.919 | 0.941 | 0.000 | faster | 0.941 | 1.000 | 0.943 | 0.728 | 0.009 | faster | 0.730 | 0.998 |
| cbench_memref/rawmemchr_64_mix | 0.920 | 0.816 | 0.017 | faster | 0.811 | 1.006 | 0.949 | 0.758 | 0.023 | faster | 0.758 | 1.000 |
| cbench_memref/rawmemchr_64k | 0.901 | 1.011 | 0.013 | parity | 1.014 | 0.997 | 0.822 | 1.064 | 0.012 | slower | 1.064 | 1.000 |
| cbench_memref/strcasestr_hit_4k | 0.132 | 0.090 | 0.391 | faster | 0.128 | 0.704 | 0.065 | 0.083 | 0.003 | faster | 0.083 | 1.002 |
| cbench_memref/strcasestr_hit_64 | 0.487 | 0.363 | 0.102 | faster | 0.397 | 0.915 | 0.452 | 0.399 | 0.031 | faster | 0.399 | 1.000 |
| cbench_memref/strcasestr_miss_4k | 0.079 | 0.072 | 0.560 | faster | 0.098 | 0.732 | 0.038 | 0.050 | 0.018 | faster | 0.051 | 0.975 |
| cbench_memref/strcasestr_miss_64 | 0.272 | 0.254 | 0.030 | faster | 0.261 | 0.972 | 0.237 | 0.215 | 0.055 | faster | 0.218 | 0.988 |
| cbench_memref/strcmp_16_mix | 0.717 | 0.391 | 0.032 | faster | 0.407 | 0.961 | 1.129 | 0.947 | 0.019 | faster | 0.963 | 0.983 |
| cbench_memref/strcmp_64_mix | 0.948 | 0.561 | 0.024 | faster | 0.576 | 0.973 | 1.165 | 0.992 | 0.043 | parity | 0.977 | 1.015 |
| cbench_memref/strlen_16 | 0.646 | 0.929 | 0.000 | faster | 0.929 | 1.000 | 0.911 | 0.559 | 0.008 | faster | 0.559 | 0.999 |
| cbench_memref/strlen_16_mix | 0.696 | 0.888 | 0.029 | faster | 0.888 | 1.000 | 0.928 | 0.701 | 0.010 | faster | 0.703 | 0.997 |
| cbench_memref/strlen_4k | 0.923 | 1.008 | 0.107 | parity | 1.008 | 1.000 | 0.909 | 1.008 | 0.000 | parity | 1.008 | 1.000 |
| cbench_memref/strlen_64 | 0.650 | 0.789 | 0.009 | faster | 0.789 | 1.000 | 0.941 | 0.728 | 0.013 | faster | 0.730 | 0.998 |
| cbench_memref/strlen_64_mix | 0.680 | 0.564 | 0.002 | faster | 0.564 | 1.000 | 0.951 | 0.820 | 0.012 | faster | 0.820 | 1.000 |
| cbench_memref/strlen_64k | 0.891 | 1.002 | 0.004 | parity | 1.001 | 1.001 | 0.901 | 0.985 | 0.016 | parity | 1.003 | 0.982 |
| cbench_memref/strnlen_16 | 0.809 | 1.000 | 0.000 | parity | 0.999 | 1.001 | 1.075 | 0.728 | 0.009 | faster | 0.730 | 0.998 |
| cbench_memref/strnlen_16_mix | 0.832 | 0.999 | 0.003 | parity | 0.982 | 1.017 | 1.063 | 0.752 | 0.004 | faster | 0.752 | 1.001 |
| cbench_memref/strnlen_4k | 0.813 | 0.662 | 0.002 | faster | 0.662 | 1.000 | 0.946 | 1.024 | 0.003 | parity | 1.024 | 1.000 |
| cbench_memref/strnlen_64 | 0.838 | 0.869 | 0.001 | faster | 0.869 | 1.000 | 0.854 | 0.856 | 0.013 | faster | 0.858 | 0.998 |
| cbench_memref/strnlen_64_mix | 0.851 | 0.740 | 0.007 | faster | 0.740 | 1.000 | 0.874 | 0.829 | 0.016 | faster | 0.831 | 0.998 |
| cbench_memref/strnlen_64k | 0.755 | 1.022 | 0.010 | parity | 1.022 | 1.000 | 0.919 | 1.028 | 0.031 | parity | 1.021 | 1.007 |
| cbench_memref/strstr_hit_4k | 0.989 | 1.104 | 0.929 | slower | 1.106 | 0.998 | 0.290 | 0.646 | 0.024 | faster | 0.647 | 0.999 |
| cbench_memref/strstr_hit_64 | 1.555 | 1.675 | 0.007 | slower | 2.174 | 0.771 | 1.262 | 1.350 | 0.009 | slower | 1.351 | 0.999 |
| cbench_memref/strstr_miss_4k | 0.347 | 0.405 | 0.926 | faster | 0.412 | 0.984 | 0.141 | 0.270 | 0.002 | faster | 0.270 | 1.000 |
| cbench_memref/strstr_miss_64 | 0.908 | 1.087 | 0.003 | slower | 1.375 | 0.791 | 0.740 | 0.988 | 0.033 | parity | 0.987 | 1.001 |
| cbench_memref/wcsstr_hit_4k | 0.684 | 0.667 | 0.002 | faster | 0.669 | 0.998 | 0.160 | 0.344 | 0.001 | faster | 0.343 | 1.003 |
| cbench_memref/wcsstr_hit_64 | 0.987 | 0.927 | 0.013 | faster | 0.933 | 0.994 | 0.644 | 0.829 | 0.276 | faster | 0.840 | 0.987 |
| cbench_memref/wcsstr_miss_4k | 0.732 | 0.630 | 0.002 | faster | 0.630 | 1.001 | 0.171 | 0.399 | 0.003 | faster | 0.399 | 0.999 |
| cbench_memref/wcsstr_miss_64 | 1.019 | 0.966 | 0.094 | parity | 0.976 | 0.990 | 0.636 | 0.839 | 0.007 | faster | 0.839 | 1.000 |
| cbench_opt/cold_exec_empty | - | 0.475 | 0.035 | faster | 0.483 | 0.984 | - | 0.367 | 0.060 | faster | 0.423 | 0.868 |
| cbench_opt/cold_exec_once | - | 0.750 | 0.028 | faster | 0.762 | 0.984 | - | 0.711 | 0.037 | faster | 0.741 | 0.960 |
| cbench_opt/execvp_missing | 1.008 | 1.018 | 0.038 | parity | 1.024 | 0.994 | 1.013 | 1.031 | 0.052 | parity | 1.039 | 0.993 |
| cbench_opt/first_bindtextdomain_gettext | - | 0.992 | 0.012 | parity | 1.053 | 0.942 | - | 0.956 | 0.051 | parity | 1.035 | 0.924 |
| cbench_opt/first_fnmatch | - | 0.190 | 0.024 | faster | 0.188 | 1.010 | - | 0.095 | 0.165 | faster | 0.094 | 1.011 |
| cbench_opt/first_gettext_nocatalog | - | 0.681 | 0.023 | faster | 0.691 | 0.985 | - | 0.608 | 0.021 | faster | 0.615 | 0.989 |
| cbench_opt/first_iconv_open_conv_close | - | 0.086 | 0.020 | faster | 0.086 | 0.995 | - | 0.087 | 0.101 | faster | 0.087 | 1.000 |
| cbench_opt/first_localtime_r_new_york | - | 1.811 | 0.012 | slower | 1.727 | 1.049 | - | 1.855 | 0.067 | slower | 1.772 | 1.047 |
| cbench_opt/first_mktime_new_york | - | 4.885 | 0.025 | slower | 0.914 | 5.345 | - | 4.800 | 0.060 | slower | 0.994 | 4.829 |
| cbench_opt/first_regcomp_regexec_regfree | - | 0.991 | 0.010 | parity | 0.981 | 1.010 | - | 0.948 | 0.064 | faster | 0.972 | 0.975 |
| cbench_opt/first_setlocale_en_US | - | 2.174 | 0.021 | slower | 2.424 | 0.897 | - | 2.171 | 0.066 | slower | 2.313 | 0.939 |
| cbench_opt/first_snprintf | - | 1.449 | 0.017 | slower | 1.162 | 1.247 | - | 1.263 | 0.179 | slower | 0.990 | 1.275 |
| cbench_opt/first_strcmp | - | 1.292 | 0.030 | slower | 1.358 | 0.951 | - | 1.444 | 0.104 | slower | 1.447 | 0.998 |
| cbench_opt/first_strcoll | - | 23.930 | 0.027 | slower | 24.050 | 0.995 | - | 20.146 | 0.213 | slower | 25.042 | 0.804 |
| cbench_opt/first_strftime_c | - | 0.909 | 0.020 | faster | 0.867 | 1.048 | - | 0.783 | 0.225 | faster | 0.721 | 1.086 |
| cbench_opt/fnmatch_names_casefold | 0.677 | 0.663 | 0.032 | faster | 0.665 | 0.997 | 0.715 | 0.739 | 0.007 | faster | 0.738 | 1.001 |
| cbench_opt/fnmatch_names_ch | 0.450 | 0.456 | 0.034 | faster | 0.456 | 1.000 | 0.481 | 0.473 | 0.020 | faster | 0.471 | 1.003 |
| cbench_opt/fnmatch_names_class | 0.509 | 0.582 | 0.012 | faster | 0.565 | 1.031 | 0.541 | 0.591 | 0.021 | faster | 0.582 | 1.016 |
| cbench_opt/fnmatch_names_lib_so | 0.364 | 0.411 | 0.119 | faster | 0.414 | 0.992 | 0.394 | 0.409 | 0.020 | faster | 0.410 | 0.998 |
| cbench_opt/fnmatch_names_period | 0.633 | 0.791 | 0.021 | faster | 0.799 | 0.989 | 0.695 | 0.774 | 0.019 | faster | 0.762 | 1.016 |
| cbench_opt/fnmatch_names_question | 0.371 | 0.365 | 0.081 | faster | 0.365 | 0.999 | 0.396 | 0.432 | 0.014 | faster | 0.437 | 0.988 |
| cbench_opt/fnmatch_names_star_c | 0.368 | 0.427 | 0.381 | faster | 0.378 | 1.131 | 0.395 | 0.397 | 0.027 | faster | 0.394 | 1.007 |
| cbench_opt/fnmatch_paths_leading_dir | 0.379 | 0.451 | 0.052 | faster | 0.446 | 1.011 | 0.409 | 0.441 | 0.019 | faster | 0.437 | 1.009 |
| cbench_opt/fnmatch_paths_pathname | 0.400 | 0.431 | 0.081 | faster | 0.431 | 0.999 | 0.425 | 0.449 | 0.010 | faster | 0.445 | 1.009 |
| cbench_opt/fprintf_du_line | 0.924 | 0.931 | 0.003 | faster | 0.955 | 0.975 | 0.923 | 0.828 | 0.021 | faster | 0.844 | 0.981 |
| cbench_opt/fprintf_ls_line | 0.744 | 0.644 | 0.011 | faster | 0.646 | 0.996 | 0.739 | 0.658 | 0.019 | faster | 0.670 | 0.983 |
| cbench_opt/gettext_c_locale | 0.134 | 0.125 | 0.014 | faster | 0.125 | 0.999 | 0.139 | 0.106 | 0.129 | faster | 0.105 | 1.008 |
| cbench_opt/gettext_found | 1.119 | 0.905 | 0.383 | faster | 0.894 | 1.012 | 1.137 | 0.916 | 0.034 | faster | 0.899 | 1.019 |
| cbench_opt/gettext_language_alternating | 1.051 | 0.983 | 0.034 | parity | 0.988 | 0.995 | 1.103 | 0.962 | 0.046 | parity | 0.927 | 1.038 |
| cbench_opt/gettext_language_putenv_alone | 0.655 | 0.697 | 0.170 | faster | 0.696 | 1.001 | 0.707 | 0.682 | 0.219 | faster | 0.654 | 1.042 |
| cbench_opt/gettext_language_steady | 1.587 | 1.051 | 0.005 | slower | 1.045 | 1.006 | 1.400 | 0.977 | 0.021 | parity | 0.976 | 1.001 |
| cbench_opt/gettext_missing | 0.262 | 0.255 | 0.018 | faster | 0.260 | 0.981 | 0.282 | 0.291 | 0.030 | faster | 0.289 | 1.007 |
| cbench_opt/gettext_no_catalog | 0.336 | 0.335 | 0.046 | faster | 0.331 | 1.013 | 0.365 | 0.395 | 0.062 | faster | 0.393 | 1.006 |
| cbench_opt/gettext_vary_msgid | 0.372 | 0.364 | 0.027 | faster | 0.371 | 0.982 | 0.402 | 0.404 | 0.022 | faster | 0.405 | 0.998 |
| cbench_opt/iconv_8859_15_utf8_latin | 0.354 | 0.378 | 0.004 | faster | 0.379 | 0.997 | 0.354 | 0.330 | 0.303 | faster | 0.427 | 0.772 |
| cbench_opt/iconv_open_conv_close_name | 0.268 | 0.254 | 0.003 | faster | 0.259 | 0.980 | 0.283 | 0.272 | 0.044 | faster | 0.272 | 1.000 |
| cbench_opt/iconv_utf16_utf8_mixed | 0.753 | 0.662 | 0.004 | faster | 0.662 | 0.999 | 0.753 | 0.643 | 0.002 | faster | 0.644 | 0.998 |
| cbench_opt/iconv_utf8_8859_15_latin_4k | 0.333 | 0.360 | 0.001 | faster | 0.360 | 1.000 | 0.333 | 0.340 | 0.014 | faster | 0.339 | 1.002 |
| cbench_opt/iconv_utf8_utf16_ascii_4k | 0.215 | 0.196 | 0.001 | faster | 0.196 | 1.002 | 0.215 | 0.229 | 0.001 | faster | 0.230 | 0.998 |
| cbench_opt/iconv_utf8_utf16_mixed_4k | 0.620 | 0.666 | 0.001 | faster | 0.775 | 0.859 | 0.620 | 0.764 | 0.000 | faster | 0.764 | 1.000 |
| cbench_opt/iconv_utf8_utf16_name | 0.719 | 0.875 | 0.013 | faster | 0.841 | 1.040 | 0.720 | 0.746 | 0.026 | faster | 0.741 | 1.006 |
| cbench_opt/iconv_utf8_utf32_mixed_4k | 0.614 | 0.726 | 0.003 | faster | 0.725 | 1.001 | 0.614 | 0.721 | 0.001 | faster | 0.721 | 1.000 |
| cbench_opt/localtime_r_new_york | 0.071 | 0.079 | 0.028 | faster | 0.080 | 0.993 | 0.071 | 0.074 | 0.137 | faster | 0.074 | 1.005 |
| cbench_opt/localtime_r_posix_rule | 0.615 | 0.723 | 0.037 | faster | 0.733 | 0.986 | 0.615 | 0.622 | 0.145 | faster | 0.602 | 1.033 |
| cbench_opt/mktime_new_york | 0.582 | 0.597 | 0.010 | faster | 0.634 | 0.941 | 0.593 | 0.625 | 0.030 | faster | 0.676 | 0.925 |
| cbench_opt/mktime_posix_rule | 0.805 | 0.828 | 0.020 | faster | 0.826 | 1.003 | 0.826 | 0.882 | 0.027 | faster | 0.887 | 0.994 |
| cbench_opt/mktime_utc | 0.562 | 0.571 | 0.025 | faster | 0.569 | 1.003 | 0.558 | 0.646 | 0.038 | faster | 0.652 | 0.991 |
| cbench_opt/ngettext_found | 0.898 | 0.870 | 0.037 | faster | 0.832 | 1.046 | 0.943 | 0.886 | 0.013 | faster | 0.891 | 0.995 |
| cbench_opt/re_grep_alt | 0.074 | 0.087 | 0.021 | faster | 0.086 | 1.007 | 0.069 | 0.071 | 0.051 | faster | 0.071 | 1.003 |
| cbench_opt/re_grep_comment | 0.207 | 0.211 | 0.011 | faster | 0.207 | 1.019 | 0.193 | 0.174 | 0.057 | faster | 0.173 | 1.007 |
| cbench_opt/re_grep_dotstar | 0.260 | 0.284 | 0.366 | faster | 0.284 | 0.998 | 0.246 | 0.238 | 0.054 | faster | 0.233 | 1.020 |
| cbench_opt/re_grep_icase_absent | 0.317 | 0.308 | 0.022 | faster | 0.310 | 0.995 | 0.310 | 0.302 | 0.008 | faster | 0.300 | 1.007 |
| cbench_opt/re_grep_ipv4 | 0.073 | 0.084 | 0.012 | faster | 0.084 | 1.004 | 0.070 | 0.078 | 0.011 | faster | 0.078 | 0.998 |
| cbench_opt/re_grep_literal | 0.525 | 0.420 | 0.003 | faster | 0.429 | 0.979 | 0.492 | 0.327 | 0.091 | faster | 0.332 | 0.985 |
| cbench_opt/re_mt4_distinct | 1.453 | 0.140 | 0.391 | faster | 0.154 | 0.907 | 1.556 | 0.140 | 0.221 | faster | 0.141 | 0.993 |
| cbench_opt/re_mt4_shared | 1.566 | 0.019 | 0.188 | faster | 0.020 | 0.961 | 1.697 | 0.026 | 0.275 | faster | 0.023 | 1.136 |
| cbench_opt/re_search_1byte | 0.223 | 0.176 | 0.027 | faster | 0.181 | 0.971 | 0.223 | 0.179 | 0.025 | faster | 0.179 | 1.000 |
| cbench_opt/re_search_sed_so | 0.565 | 0.467 | 0.006 | faster | 0.416 | 1.122 | 0.553 | 0.408 | 0.031 | faster | 0.409 | 0.998 |
| cbench_opt/re_search_sed_swap | 0.289 | 0.300 | 0.013 | faster | 0.295 | 1.016 | 0.285 | 0.252 | 0.042 | faster | 0.253 | 0.997 |
| cbench_opt/re_sed_kv | 0.215 | 0.223 | 0.009 | faster | 0.223 | 1.002 | 0.210 | 0.189 | 0.009 | faster | 0.190 | 0.996 |
| cbench_opt/re_sed_swap | 0.081 | 0.088 | 0.009 | faster | 0.087 | 1.017 | 0.080 | 0.081 | 0.007 | faster | 0.081 | 0.999 |
| cbench_opt/regcomp_regfree_ipv4 | 0.839 | 0.814 | 0.019 | faster | 0.831 | 0.979 | 0.797 | 0.820 | 0.010 | faster | 0.830 | 0.988 |
| cbench_opt/regcomp_regfree_sed_kv | 0.398 | 0.464 | 0.013 | faster | 0.472 | 0.983 | 0.387 | 0.427 | 0.016 | faster | 0.426 | 1.002 |
| cbench_opt/regexec_sed_so | 0.390 | 0.299 | 0.008 | faster | 0.300 | 0.996 | 0.369 | 0.275 | 0.233 | faster | 0.280 | 0.983 |
| cbench_opt/regexec_sed_swap | 0.287 | 0.295 | 0.018 | faster | 0.289 | 1.021 | 0.283 | 0.249 | 0.009 | faster | 0.251 | 0.992 |
| cbench_opt/second_bindtextdomain_gettext | - | 1.567 | 0.029 | slower | 10.173 | 0.154 | - | 1.626 | 0.122 | slower | 11.541 | 0.141 |
| cbench_opt/second_fnmatch | - | 0.484 | 0.022 | faster | 0.491 | 0.985 | - | 0.463 | 0.075 | faster | 0.411 | 1.127 |
| cbench_opt/second_gettext_nocatalog | - | 0.325 | 0.044 | faster | 0.440 | 0.738 | - | 0.308 | 0.112 | faster | 0.302 | 1.020 |
| cbench_opt/second_iconv_open_conv_close | - | 0.192 | 0.022 | faster | 0.208 | 0.924 | - | 0.179 | 0.099 | faster | 0.206 | 0.870 |
| cbench_opt/second_localtime_r_new_york | - | 0.905 | 0.051 | faster | 0.905 | 1.000 | - | 0.897 | 0.137 | faster | 0.908 | 0.988 |
| cbench_opt/second_mktime_new_york | - | 1.193 | 0.046 | slower | 1.256 | 0.950 | - | 1.217 | 0.119 | slower | 1.150 | 1.059 |
| cbench_opt/second_regcomp_regexec_regfree | - | 0.428 | 0.007 | faster | 0.437 | 0.980 | - | 0.426 | 0.166 | faster | 0.425 | 1.003 |
| cbench_opt/second_setlocale_en_US | - | 1.717 | 0.043 | slower | 1.712 | 1.003 | - | 1.545 | 0.215 | slower | 1.311 | 1.178 |
| cbench_opt/second_snprintf | - | 0.946 | 0.023 | faster | 0.925 | 1.022 | - | 0.909 | 0.109 | faster | 1.003 | 0.907 |
| cbench_opt/second_strcmp | - | 1.371 | 0.111 | slower | 1.388 | 0.988 | - | 1.455 | 0.062 | slower | 1.432 | 1.016 |
| cbench_opt/second_strcoll | - | 0.923 | 0.028 | faster | 0.876 | 1.054 | - | 0.888 | 0.245 | faster | 0.869 | 1.021 |
| cbench_opt/second_strftime_c | - | 0.874 | 0.016 | faster | 0.738 | 1.184 | - | 0.679 | 0.194 | faster | 0.673 | 1.008 |
| cbench_opt/snprintf_d | 0.839 | 0.827 | 0.105 | faster | 0.823 | 1.004 | 0.839 | 0.965 | 0.011 | parity | 0.970 | 0.994 |
| cbench_opt/snprintf_e | 0.957 | 0.888 | 0.003 | faster | 0.885 | 1.003 | 0.959 | 0.927 | 0.009 | faster | 0.920 | 1.008 |
| cbench_opt/snprintf_f2 | 0.454 | 0.313 | 0.001 | faster | 0.314 | 0.998 | 0.454 | 0.417 | 0.030 | faster | 0.415 | 1.005 |
| cbench_opt/snprintf_f_large | 0.177 | 0.222 | 0.006 | faster | 0.222 | 0.998 | 0.169 | 0.425 | 0.020 | faster | 0.424 | 1.002 |
| cbench_opt/snprintf_g | 0.593 | 0.420 | 0.001 | faster | 0.423 | 0.992 | 0.594 | 0.509 | 0.018 | faster | 0.509 | 1.000 |
| cbench_opt/snprintf_ld_large | 0.838 | 0.815 | 0.282 | faster | 0.821 | 0.993 | 0.838 | 0.890 | 0.012 | faster | 0.888 | 1.002 |
| cbench_opt/snprintf_ls_line | 1.013 | 0.817 | 0.002 | faster | 0.821 | 0.995 | 1.009 | 0.864 | 0.048 | faster | 0.888 | 0.973 |
| cbench_opt/snprintf_mixed | 0.615 | 0.445 | 0.001 | faster | 0.446 | 0.999 | 0.612 | 0.529 | 0.041 | faster | 0.528 | 1.002 |
| cbench_opt/snprintf_s_d_colon | 1.021 | 0.973 | 0.001 | parity | 1.004 | 0.969 | 1.019 | 0.869 | 0.068 | faster | 0.903 | 0.962 |
| cbench_opt/snprintf_s_path | 0.580 | 0.592 | 0.005 | faster | 0.592 | 1.000 | 0.580 | 0.580 | 0.053 | faster | 0.587 | 0.988 |
| cbench_opt/snprintf_x08 | 0.907 | 0.849 | 0.005 | faster | 0.854 | 0.994 | 0.907 | 0.828 | 0.027 | faster | 0.830 | 0.998 |
| cbench_opt/sprintf_chk_ls_nlink | 1.035 | 0.946 | 0.019 | faster | 0.955 | 0.991 | 1.033 | 0.890 | 0.072 | faster | 0.896 | 0.993 |
| cbench_opt/strcmp_equal_16 | 1.064 | 1.000 | 0.363 | parity | 1.000 | 1.000 | 1.036 | 0.891 | 0.016 | faster | 0.891 | 1.000 |
| cbench_opt/strcmp_late_256 | 1.458 | 0.982 | 0.362 | parity | 0.983 | 0.999 | 0.885 | 0.788 | 0.001 | faster | 0.788 | 1.001 |
| cbench_opt/strcmp_names | 0.943 | 0.771 | 0.037 | faster | 0.773 | 0.998 | 1.022 | 1.111 | 0.006 | slower | 1.110 | 1.001 |
| cbench_opt/strcoll_c_names | 0.820 | 0.756 | 0.027 | faster | 0.745 | 1.014 | 0.864 | 1.011 | 0.028 | parity | 1.015 | 0.996 |
| cbench_opt/strcoll_c_utf8_names | 0.820 | 0.746 | 0.425 | faster | 0.753 | 0.991 | 0.864 | 1.008 | 0.016 | parity | 1.027 | 0.982 |
| cbench_opt/strcoll_en_us_equal_16 | 0.018 | 0.015 | 0.009 | faster | 0.015 | 0.999 | 0.018 | 0.013 | 0.004 | faster | 0.013 | 1.033 |
| cbench_opt/strcoll_en_us_names | 0.430 | 0.447 | 0.404 | faster | 0.452 | 0.989 | 0.430 | 0.433 | 0.011 | faster | 0.435 | 0.996 |
| cbench_opt/strcoll_en_us_utf8_16 | 0.883 | 0.780 | 0.018 | faster | 0.787 | 0.991 | 0.878 | 0.624 | 0.009 | faster | 0.624 | 1.000 |
| cbench_opt/strftime_c_en_us | 0.437 | 0.429 | 0.016 | faster | 0.417 | 1.029 | 0.438 | 0.385 | 0.034 | faster | 0.384 | 1.002 |
| cbench_opt/strftime_date_en_us | 0.638 | 0.751 | 0.011 | faster | 0.784 | 0.958 | 0.629 | 0.592 | 0.044 | faster | 0.599 | 0.988 |
| cbench_opt/strftime_iso8601_tz | 0.547 | 0.579 | 0.014 | faster | 0.574 | 1.009 | 0.538 | 0.547 | 0.037 | faster | 0.559 | 0.979 |
| cbench_opt/strftime_log | 0.592 | 0.628 | 0.013 | faster | 0.671 | 0.936 | 0.584 | 0.609 | 0.037 | faster | 0.609 | 1.000 |
| cbench_opt/strftime_ls_iso | 0.553 | 0.606 | 0.031 | faster | 0.610 | 0.993 | 0.546 | 0.556 | 0.029 | faster | 0.553 | 1.006 |
| cbench_opt/strftime_ls_recent_c | 0.610 | 0.732 | 0.018 | faster | 0.646 | 1.134 | 0.605 | 0.609 | 0.074 | faster | 0.608 | 1.002 |
| cbench_opt/strftime_ls_recent_en_us | 0.610 | 0.738 | 0.010 | faster | 0.647 | 1.141 | 0.605 | 0.627 | 0.030 | faster | 0.627 | 0.999 |
| cbench_patterns/fnmatch_bracket | 0.827 | 0.832 | 0.002 | faster | 0.832 | 1.000 | 0.866 | 0.908 | 0.030 | faster | 0.919 | 0.988 |
| cbench_patterns/fnmatch_pathname | 0.809 | 0.796 | 0.003 | faster | 0.796 | 1.000 | 0.812 | 0.866 | 0.032 | faster | 0.862 | 1.005 |
| cbench_patterns/fnmatch_pathname_nomatch | 0.997 | 0.920 | 0.001 | faster | 0.919 | 1.001 | 1.000 | 1.000 | 0.025 | parity | 0.994 | 1.006 |
| cbench_patterns/fnmatch_star | 0.810 | 0.809 | 0.002 | faster | 0.811 | 0.998 | 0.868 | 0.771 | 0.071 | faster | 0.792 | 0.974 |
| cbench_patterns/glob_1000_nosort | 0.967 | 0.982 | 0.004 | parity | 0.982 | 1.000 | 0.969 | 0.975 | 0.009 | parity | 0.976 | 0.999 |
| cbench_patterns/glob_1000_sorted | 0.797 | 0.773 | 0.005 | faster | 0.773 | 1.000 | 0.851 | 0.743 | 0.007 | faster | 0.740 | 1.005 |
| cbench_patterns/regcomp_regexec_regfree_email | 0.281 | 0.341 | 0.012 | faster | 0.339 | 1.006 | 0.279 | 0.315 | 0.013 | faster | 0.315 | 1.000 |
| cbench_patterns/regexec_backref | 1.014 | 1.146 | 0.028 | slower | 1.199 | 0.956 | 1.001 | 0.965 | 0.012 | parity | 1.000 | 0.965 |
| cbench_patterns/regexec_class_nomatch_1k | 0.207 | 0.263 | 0.002 | faster | 1.145 | 0.229 | 0.196 | 0.174 | 0.028 | faster | 0.863 | 0.201 |
| cbench_patterns/regexec_literal_1k | 0.056 | 0.070 | 0.007 | faster | 0.072 | 0.969 | 0.039 | 0.048 | 0.003 | faster | 0.048 | 1.005 |
| cbench_patterns/regexec_pathological_4k | 0.000 | 0.000 | 0.012 | faster | 0.000 | - | 0.000 | 0.000 | 0.045 | faster | 0.000 | - |
| cbench_patterns/regexec_submatch_1k | 0.075 | 0.085 | 0.013 | faster | 0.085 | 1.001 | 0.073 | 0.090 | 0.032 | faster | 0.092 | 0.980 |
| cbench_process/cfsetspeed_cfgetospeed | 0.475 | 0.685 | 0.000 | faster | 0.685 | 1.000 | 0.475 | 0.627 | 0.006 | faster | 0.625 | 1.004 |
| cbench_process/fork_exit_wait | 1.054 | 0.888 | 0.026 | faster | 1.287 | 0.690 | 1.084 | 0.915 | 0.014 | faster | 1.275 | 0.717 |
| cbench_process/isatty_devnull | 0.959 | 0.976 | 0.094 | parity | 0.975 | 1.001 | 0.958 | 0.989 | 0.004 | parity | 0.990 | 0.999 |
| cbench_process/isatty_pty | 0.968 | 0.984 | 0.038 | parity | 0.990 | 0.994 | 0.956 | 0.990 | 0.007 | parity | 0.993 | 0.997 |
| cbench_process/openpty_close | 0.992 | 0.944 | 0.080 | faster | 1.073 | 0.880 | 0.992 | 0.979 | 0.011 | parity | 0.981 | 0.998 |
| cbench_process/popen_pclose_true | 1.011 | 1.001 | 0.032 | parity | 0.993 | 1.008 | 1.004 | 0.992 | 0.032 | parity | 1.002 | 0.990 |
| cbench_process/posix_spawn_true | 1.011 | 0.998 | 0.643 | parity | 0.995 | 1.003 | 1.002 | 1.000 | 0.022 | parity | 1.003 | 0.997 |
| cbench_process/posix_spawn_true_actions | 1.016 | 1.005 | 0.009 | parity | 1.017 | 0.988 | 1.023 | 1.000 | 0.009 | parity | 0.997 | 1.003 |
| cbench_process/posix_spawn_true_heap256m | 1.017 | 1.010 | 0.471 | parity | 0.971 | 1.040 | 1.015 | 0.988 | 0.025 | parity | 0.994 | 0.994 |
| cbench_process/posix_spawnp_true | 1.006 | 0.979 | 0.027 | parity | 0.996 | 0.983 | 1.011 | 1.004 | 0.039 | parity | 1.000 | 1.004 |
| cbench_process/system_exit0 | 1.016 | 1.000 | 0.034 | parity | 1.002 | 0.998 | 1.018 | 0.993 | 0.030 | parity | 1.003 | 0.990 |
| cbench_process/tcgetattr | 1.014 | 0.990 | 0.048 | parity | 1.002 | 0.988 | 1.007 | 1.011 | 0.010 | parity | 1.009 | 1.002 |
| cbench_process/tcsetattr_now | 0.794 | 0.721 | 0.028 | faster | 0.720 | 1.002 | 0.798 | 0.750 | 0.003 | faster | 0.750 | 1.000 |
| cbench_process/ttyname_r | 1.011 | 1.011 | 0.023 | parity | 1.014 | 0.997 | 1.012 | 1.004 | 0.008 | parity | 1.015 | 0.989 |
| cbench_process/vfork_exit_wait | 0.994 | 0.996 | 0.049 | parity | 0.996 | 1.000 | 1.007 | 0.998 | 0.063 | parity | 1.009 | 0.989 |
| cbench_process/waitpid_echild | 1.088 | 1.007 | 0.059 | parity | 0.998 | 1.009 | 1.090 | 1.015 | 0.003 | parity | 1.014 | 1.001 |
| cbench_startup/exec_hello_locale | - | 0.724 | 0.019 | faster | 0.745 | 0.972 | - | 0.644 | 0.018 | faster | 0.661 | 0.974 |
| cbench_startup/exec_true_echo | - | 0.581 | 0.058 | faster | 0.585 | 0.994 | - | 0.505 | 0.028 | faster | 0.505 | 1.000 |
| cbench_startup/exec_true_static | - | 0.404 | 0.041 | faster | 0.402 | 1.005 | - | 0.368 | 0.016 | faster | 0.378 | 0.974 |
| cbench_startup/spawn_bin_true | - | 1.003 | 0.018 | parity | 1.012 | 0.991 | - | 0.997 | 0.038 | parity | 0.994 | 1.003 |
| cbench_startup/startup_instr_c | 0.018 | 0.569 | 0.134 | faster | 0.647 | 0.880 | 0.018 | 0.479 | 0.319 | faster | 0.560 | 0.855 |
| cbench_startup/startup_instr_printf | 0.516 | 0.775 | 0.099 | faster | 0.885 | 0.876 | 0.489 | 0.718 | 0.084 | faster | 0.821 | 0.874 |
| cbench_startup/startup_instr_setloc | 0.496 | 0.786 | 0.187 | faster | 0.876 | 0.898 | 0.469 | 0.701 | 0.056 | faster | 0.786 | 0.892 |
| cbench_startup/startup_instr_wctype | 0.502 | 0.762 | 0.097 | faster | 0.880 | 0.866 | 0.476 | 0.751 | 0.086 | faster | 0.835 | 0.900 |
| cbench_startup/startup_kinstr_c | 0.226 | 0.541 | 0.240 | faster | - | - | 0.235 | 0.472 | 0.040 | faster | - | - |
| cbench_stdio/fprintf_d | 0.926 | 0.867 | 0.015 | faster | 0.920 | 0.942 | 0.923 | 0.828 | 0.003 | faster | 0.830 | 0.998 |
| cbench_stdio/fputs_32 | 0.787 | 0.819 | 0.037 | faster | 0.814 | 1.006 | 0.845 | 0.806 | 0.019 | faster | 0.817 | 0.986 |
| cbench_stdio/fwrite_4k | 2.049 | 0.994 | 0.043 | parity | 1.005 | 0.989 | 1.627 | 0.869 | 0.017 | faster | 0.856 | 1.015 |
| cbench_stdio/putc | 0.752 | 1.000 | 0.005 | parity | 1.000 | 1.000 | 0.752 | 0.605 | 0.000 | faster | 0.605 | 1.000 |
| cbench_stdio/snprintf_d | 0.839 | 0.801 | 0.013 | faster | 0.811 | 0.987 | 0.839 | 0.954 | 0.008 | parity | 0.959 | 0.995 |
| cbench_stdio/snprintf_mixed | 0.602 | 0.434 | 0.002 | faster | 0.434 | 1.000 | 0.600 | 0.520 | 0.016 | faster | 0.516 | 1.007 |
| cbench_time/gmtime_r | 0.545 | 0.574 | 0.001 | faster | 0.574 | 1.000 | 0.545 | 0.590 | 0.051 | faster | 0.590 | 1.000 |
| cbench_time/localtime_r_new_york | 0.060 | 0.067 | 0.058 | faster | 0.067 | 1.005 | 0.060 | 0.063 | 0.164 | faster | 0.063 | 0.993 |
| cbench_time/localtime_r_new_york_2100 | 0.057 | 0.064 | 0.061 | faster | 0.064 | 0.997 | 0.057 | 0.060 | 0.144 | faster | 0.060 | 1.006 |
| cbench_time/localtime_r_posix_rule | 0.610 | 0.717 | 0.018 | faster | 0.712 | 1.008 | 0.609 | 0.613 | 0.132 | faster | 0.616 | 0.996 |
| cbench_time/localtime_r_utc | 0.586 | 0.802 | 0.012 | faster | 0.796 | 1.007 | 0.586 | 0.804 | 0.018 | faster | 0.803 | 1.001 |
| cbench_time/mktime_new_york | 0.498 | 0.494 | 0.003 | faster | 0.532 | 0.929 | 0.508 | 0.547 | 0.027 | faster | 0.578 | 0.946 |
| cbench_time/strftime_c | 0.474 | 0.484 | 0.014 | faster | 0.483 | 1.001 | 0.473 | 0.412 | 0.034 | faster | 0.411 | 1.002 |
| cbench_time/strftime_iso | 0.573 | 0.599 | 0.005 | faster | 0.591 | 1.013 | 0.562 | 0.562 | 0.004 | faster | 0.563 | 0.998 |
| cbench_time/timegm | 0.563 | 0.659 | 0.008 | faster | 0.652 | 1.010 | 0.563 | 0.800 | 0.036 | faster | 0.802 | 0.998 |
| cbench_unmeasured/comparisons_per_sort | - | 1.040 | 0.000 | parity | 1.040 | 1.000 | - | 1.040 | 0.000 | parity | 1.040 | 1.000 |
| cbench_unmeasured/comparisons_per_sort_e16 | - | 1.035 | 0.000 | parity | 1.035 | 1.000 | - | 1.035 | 0.000 | parity | 1.035 | 1.000 |
| cbench_unmeasured/comparisons_per_sort_e24 | - | 1.040 | 0.000 | parity | 1.040 | 1.000 | - | 1.040 | 0.000 | parity | 1.040 | 1.000 |
| cbench_unmeasured/comparisons_per_sort_e4 | - | 1.037 | 0.000 | parity | 1.037 | 1.000 | - | 1.037 | 0.000 | parity | 1.037 | 1.000 |
| cbench_unmeasured/comparisons_per_sort_e64 | - | 1.030 | 0.000 | parity | 1.030 | 1.000 | - | 1.030 | 0.000 | parity | 1.030 | 1.000 |
| cbench_unmeasured/comparisons_per_sort_e8 | - | 1.031 | 0.000 | parity | 1.031 | 1.000 | - | 1.031 | 0.000 | parity | 1.031 | 1.000 |
| cbench_unmeasured/fts_entry_deep2k | 0.297 | 0.502 | 0.043 | faster | 0.497 | 1.010 | 0.271 | 0.467 | 0.025 | faster | 0.464 | 1.006 |
| cbench_unmeasured/fts_entry_flat | 0.843 | 0.897 | 0.038 | faster | 0.883 | 1.016 | 0.825 | 0.897 | 0.013 | faster | 0.901 | 0.996 |
| cbench_unmeasured/qsort_c_cmp_1024 | 1.318 | 0.974 | 0.618 | parity | 0.998 | 0.976 | 1.312 | 0.794 | 0.052 | faster | 0.822 | 0.965 |
| cbench_unmeasured/qsort_c_cmp_1024_e16 | 0.758 | 0.638 | 0.021 | faster | 0.639 | 0.999 | 0.772 | 0.553 | 0.012 | faster | 0.553 | 1.000 |
| cbench_unmeasured/qsort_c_cmp_1024_e24 | 0.752 | 0.682 | 0.008 | faster | 0.684 | 0.997 | 0.775 | 0.587 | 0.018 | faster | 0.587 | 1.001 |
| cbench_unmeasured/qsort_c_cmp_1024_e4 | 1.314 | 0.994 | 0.366 | parity | 0.713 | 1.393 | 1.308 | 0.789 | 0.022 | faster | 0.801 | 0.985 |
| cbench_unmeasured/qsort_c_cmp_1024_e64 | 1.126 | 0.781 | 0.014 | faster | 0.785 | 0.995 | 1.181 | 0.798 | 0.012 | faster | 0.788 | 1.012 |
| cbench_unmeasured/qsort_c_cmp_1024_e8 | 1.273 | 1.019 | 0.007 | parity | 1.010 | 1.009 | 1.282 | 0.797 | 0.016 | faster | 0.800 | 0.997 |
| cbench_unmeasured/readdir_entry_flat | 0.999 | 0.958 | 0.006 | parity | 0.957 | 1.001 | 1.000 | 0.953 | 0.003 | parity | 0.949 | 1.004 |

### OWNER-DEFERRED: wordexp rows (timed, not judged)

| bench/row | instr IVB | time IVB | spread IVB | verdict IVB | 1106 time IVB | now/1106 IVB | instr N100 | time N100 | spread N100 | verdict N100 | 1106 time N100 | now/1106 N100 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| cbench_opt/first_wordexp | - | 2.989 | 0.021 | slower | 4.095 | 0.730 | - | 1.366 | 0.152 | slower | 2.228 | 0.613 |
| cbench_opt/second_wordexp | - | 1.576 | 0.032 | slower | 1.573 | 1.002 | - | 1.249 | 0.168 | slower | 1.341 | 0.931 |
| cbench_opt/wordexp_arith | 1.169 | 1.435 | 0.008 | slower | 1.479 | 0.970 | 1.169 | 1.233 | 0.060 | slower | 1.285 | 0.959 |
| cbench_opt/wordexp_param_forms | 1.184 | 1.400 | 0.395 | slower | 1.417 | 0.988 | 1.187 | 1.137 | 0.023 | slower | 1.122 | 1.013 |
| cbench_opt/wordexp_plain_8 | 0.575 | 0.721 | 0.013 | faster | 0.729 | 0.989 | 0.593 | 0.587 | 0.076 | faster | 0.569 | 1.032 |
| cbench_opt/wordexp_quoted | 1.470 | 1.736 | 0.003 | slower | 1.758 | 0.987 | 1.478 | 1.479 | 0.074 | slower | 1.533 | 0.965 |
| cbench_opt/wordexp_split_var | 1.074 | 1.115 | 0.009 | slower | 1.191 | 0.936 | 1.082 | 0.956 | 0.097 | parity | 0.965 | 0.991 |
| cbench_opt/wordexp_tilde_var | 1.331 | 1.525 | 0.008 | slower | 1.555 | 0.981 | 1.346 | 1.279 | 0.053 | slower | 1.277 | 1.002 |
| cbench_patterns/wordexp_arith | 1.179 | 1.513 | 0.001 | slower | 1.513 | 1.000 | 1.189 | 1.235 | 0.035 | slower | 1.231 | 1.003 |
| cbench_patterns/wordexp_tilde_var | 1.362 | 1.639 | 0.009 | slower | 1.640 | 1.000 | 1.375 | 1.312 | 0.031 | slower | 1.314 | 0.998 |
