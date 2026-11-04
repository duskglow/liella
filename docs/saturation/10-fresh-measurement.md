## 10. Fresh measurement (BENCH-REFRESH-1023-2, 2026-10-23)

Current `next` (programs built on Haswell-EP host, role sh2, release profile, no `--v3`) against the host glibc 2.39 on the idle bare-metal timing hosts Ivy Bridge host A (E3-1220L v2) and Ivy Bridge host B (E3-1265L v2), both Ivy Bridge, turbo off, TIMING-READY. `cargo xtask bench --compare --counters --runs 5`: ns per operation, median of 5 rounds, ratio = Liella (static) / glibc (dynamic); `i` = user+kernel instructions per operation ratio from the in-process counters. Basis: **measured**. Raw tables: `handoff/bench-refresh-1023-logs/`. Hosts: 7 benches (opt, locale, patterns, stdio, time, memref, malloc) on both Ivy Bridge host A and Ivy Bridge host B (the two agree to within noise except where the spread column is large); `cbench_hot` on Ivy Bridge host A only; `cbench_fstree` on Ivy Bridge host B only. Ratios are per host and never mixed.

Spread is the Liella min-max over the 5 rounds as a percentage of the median (largest over the listed rows); above 10% the row is **NOISY** and is reported as measured, not rerun. The decision basis is the time ratio; cycles columns for `hot` (Ivy Bridge host A) and `fstree` (Ivy Bridge host B) are in `handoff/bench-refresh-1023-logs/Ivy Bridge host A-hot-measure.tsv` and `Ivy Bridge host B-fstree-measure.tsv`; `bench/parity-measure.tsv` was NOT refreshed (47 columns there, 38 written by this tool: a schema decision is needed). The cycles columns of the other 7 benches were lost when run 1 stopped (measure.tsv was only written at the end; fixed by the per-bench write), so those rows carry time and instruction ratios only.

| Register row | Old (10-07/08 unless noted) | New (host) | Moved? | Spread / noise |
|---|---|---|---|---|
| `readdir` loop | t 1.01, 0.99 (Zen 1 host); i 1.17 (user instr) | t 0.95, 0.95; i 1.01, 1.01 (Ivy Bridge host B) | no (within 6%) | 0.9% |
| `fts` flat walk | i about 1.2x | t 1.08, 1.08; i 1.09, 1.12 (Ivy Bridge host B) | i 1.2 -> 1.09: better | 1.8% |
| `memcmp_16/64/128` | instr only (Zen 1) | t 0.90, 1.27, 1.61; i 1.08, 1.22, 1.11 (Ivy Bridge host A) | new time ratio | 0.0% |
| `memcpy`/`memset`/`memmove` 16, 64 | instr only (Zen 1) | t 0.70, 0.78, 1.00, 0.60, 0.69, 1.00; i 0.95, 0.95, 0.94, 0.69, 0.72, 0.70 (Ivy Bridge host A) | new time ratio | 0.0% |
| `strlen_64`, `strnlen_64`, `rawmemchr_64` | instr only (Zen 1) | t 1.20, 1.41, 1.28; i 0.91, 1.39, 1.34 (Ivy Bridge host A) | new time ratio | 0.1% |
| `strncmp` equal-16, diff-8, names-64 | instr only (Zen 1) | t 1.21, 1.75, 1.04; i 1.02, 1.44, 1.07 (Ivy Bridge host A) | new time ratio: ABOVE glibc on two | 0.1% |
| `strcmp` names, equal-16, late-256 | t 1.49, 2.10, 1.70; i 1.51, 2.00, 1.93 | t 0.78, 1.10, 1.24; i 0.96, 1.10, 1.65 (Ivy Bridge host A) | MOVED: names 1.49 -> 0.78, equal-16 2.10 -> 1.10, late-256 1.70 -> 1.24 | 0.7% |
| `strstr` short/64/4k | cycles 50.6 vs 22.2 | t 2.30, 1.25; i 2.16, 2.23 (no row: strstr_hit_short) (Ivy Bridge host A) | see table (time, not cycles) | 46.4% NOISY |
| `getline` long, 80 | instr gap only | t 1.10, 1.30; i 1.25, 1.29 (Ivy Bridge host A) | new time ratio | 3.4% |
| `malloc`+`free` pair | i 0.82 | t 1.16; i 0.83 (Ivy Bridge host A) | i 0.82 -> 0.83: same | 0.4% |
| `iswalpha`, `towupper`, `wcwidth` (UTF-8) | t 0.89, 0.89, 0.77; i 1.5, 1.6, 1.3 | t 1.26, 1.03, 0.84; i 1.65, 1.72, 1.28 (Ivy Bridge host A) | MOVED: iswalpha 0.89 -> 1.26, towupper 0.89 -> 1.03, wcwidth 0.77 -> 0.84 (worse) | 0.3% |
| `mbrtowc` UTF-8 | t 1.655; i 1.84 | t 0.28, 0.30, 0.91; i 0.20, 0.24, 0.89 (Ivy Bridge host A) | MOVED: faster than glibc on 1 and 2 byte, 0.91 on 3 | 0.3% |
| `strftime` iso, `%c`, log | t 1.78 to 2.26; i 1.4 to 1.7 | t 0.60, 0.41, 0.67; i 0.55, 0.44, 0.59 (Ivy Bridge host A) | MOVED: 1.78-2.26 -> 0.41-0.67 | 3.1% |
| `gettext` found / steady | instr +0.4 to 0.6% accepted | t 0.99, 1.21, 0.97; i 1.32, 1.84, 1.08 (Ivy Bridge host A) | see spread: gettext_found and _steady are NOISY | 63.8% NOISY |
| `localtime_r` New York | t 0.13 | t 0.08; i 0.07 (Ivy Bridge host A) | MOVED: 0.13 -> 0.08 (faster) | 4.0% |
| `snprintf` `%d`, `%x08`, `%ld` large | t 1.13, 1.06, 1.00; i 0.96, 1.04, 0.91 | t 0.82, 0.86, 0.81; i 0.85, 0.91, 0.84 (Ivy Bridge host A) | MOVED: now 0.81-0.86 | 3.0% |
| `snprintf` `%e` | t 3.28 (2.68 v3) | t 0.90; i 0.96 (Ivy Bridge host A) | MOVED: 3.28 -> 0.90 (name match: check that the row is the same shape) | 0.0% |
| regex captures / backrefs | t 3.1 to 5.8 | t 1.30, 0.08, 0.40, 0.22, 0.30, 0.47; i 1.13, 0.07, 0.52, 0.21, 0.39, 0.40 (Ivy Bridge host A) | MOVED: 3.1-5.8 -> 0.08-1.30 | 15.0% NOISY |
| `wordexp` | t 2.9 to 4.2 | t 1.57, 1.46, 0.72, 1.76, 1.14, 1.43; i 1.37, 1.19, 0.57, 1.49, 1.11, 1.21 (Ivy Bridge host A) | MOVED: 2.9-4.2 -> 0.72-1.76 | 1.2% |
| `fnmatch` names | t 0.54 to 0.68 | t 0.37, 0.43, 0.84, 1.26, 0.64; i 0.37, 0.44, 0.63, 1.26, 0.69 (Ivy Bridge host A) | MOVED: spread 0.37-1.26 (class 1.26 is above glibc) | 1.9% |
| iconv UTF-8 to UTF-16 ASCII 4k | t 0.31 | t 0.20; i 0.22 (Ivy Bridge host A) | MOVED: 0.31 -> 0.20 (faster) | 1.6% |
| iconv mixed 4k | t 2.1 to 2.5 | t 0.61, 0.64, 0.74; i 0.60, 0.73, 0.59 (Ivy Bridge host A) | MOVED: 2.1-2.5 -> 0.61-0.74 | 0.5% |

Not measured: `qsort` with a C comparator (no such row in the nine benches), `getline` instruction gap rows beyond the above, `memcpy` non-AVX2 policy rows (own handoff note). Programs built for x86-64-v3 were not timed (Ivy Bridge lacks AVX2; no the AVX2 timing hosts run was made in this run).
