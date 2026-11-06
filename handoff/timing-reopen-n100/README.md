# TIMING-REOPEN-N100: profiles of the N100 (AVX2 class, Gracemont E-core) rows slower in time, and the follow-up briefs

Source: TIMING-SWEEP-1105 table (results/timing-sweep-1105-table.md on task/timing-sweep-1105-2; sweep build next f2157504).
Profiles: booked N100 timing host, perf 6.8 named events, user mode, job timing-reopen-n100-prof (perf stat, 7 event sets
x 5 runs per row and side; Liella-only cycle samples by symbol). glibc side: aggregate counters only. Per-call method and
numbers: profile-n100-percall.txt (reproduces the sweep per-call cycles within 1%).

| Group | Brief | Rows (N100 instr / time) | Named stall (Liella vs glibc, per call) | Confidence |
|---|---|---|---|---|
| mutex mt | TIMING-N100-MUTEX-MT | mutex_lock_unlock_normal_mt 0.852 / 1.199 | ROB-full 73% vs 63% of slots, load-at-retirement-head L1-bound 1.80x; 61% of samples in unlock | high (stall), medium (site) |
| memcpy 129..256 | TIMING-N100-COPY-MID | memcpy_144 / _256 1.000 / 1.077 | non-memory scheduler full 4.97 vs 0 slots = the 1-cycle gap | high (stall); re-time first: PARITY-COPY-AVX2-3 landed after the sweep |
| memcmp | TIMING-N100-MEMCMP | memcmp_256 0.913 / 1.061, memcmp_4k 0.962 / 1.086 | 4k: one mispredict per call (122 slots, = gap); 256: memory scheduler full 15.5 vs 5.0 slots | high |
| long scan | TIMING-N100-SCAN-64K | memchr_64k 0.922 / 1.169, rawmemchr_64k 0.909 / 1.060 | L2-hit load stalls 486 vs 198 cycles (2.5x); load buffer full 14 vs 65: fewer loads in flight | high (stall), medium (cause) |
| short strstr | TIMING-N100-STRSTR-64 | strstr_miss_64 0.804 / 1.102, strstr_hit_64 1.353 / 1.437 | non-memory scheduler full 55 vs 0 slots; 26% of samples in the strstr entry | medium |

Not verified: instruction-level sites (perf annotate needs objdump, absent on the timing host); each brief's step 1 annotates
the Liella samples on a build host.

## Other N100 rows above 1.05x in time (not briefed here)
- iconv_utf8_ascii_translit_4k (1.454): TIMING-ICONV-TRANSLIT-CPI-3 (open).
- iswalpha_en_us / _l_en_us / _utf8, iswctype_en_us, towupper_l_en_us (instr 1.39-2.21): PERF-WCTYPE-ENUS(-2, landed after the sweep), PERF-WCTYPE-L-2/-3, PERF-WCTYPE-UTF8-REST-3.
- nl_langinfo_l_en_us (1.100): PERF-CTYPE-LANGINFO-ACCESS.
- strncasecmp_c_64 (1.099): PERF-STRNCASECMP-64-AVX2. strcasecmp_c_16 (1.136): no story found (not verified).
- strcmp_names, first_strcmp, second_strcmp: PARITY-STRCMP-AVX2 (landed after the sweep), PERF-STRCMP-MIX.
- gettext_language_steady, second_bindtextdomain_gettext (7.7x): PERF-GETTEXT-LANGUAGE-STEADY-5 (landed after the sweep), PERF-GETTEXT-STEADY-2; the bindtextdomain cold row is not verified covered.
- UNCOVERED (no open story found): first_/second_ cold-start rows (first_strcoll 19.5x, first_setlocale_en_US 2.22x, first_localtime_r_new_york 2.12x, first_wordexp 1.71x, second_setlocale 1.43x, second_mktime 1.28x, second_wordexp 1.31x); wordexp_arith / _param_forms / _quoted / _tilde_var (instr 1.17-1.48, time 1.14-1.52, both benches); fork_exit_wait (1.294, instr 1.081). Recommendation: one planning story for the cold-start rows and one instruction story for wordexp.
