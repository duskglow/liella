# Benchmarks: Liella against glibc 2.39

This page says where Liella is faster than glibc, where it is slower, how that changed since the
`5cc49dc` bench, and how to re-measure. Numbers come from the runs named below, on the CPU and commit
named there. Nothing here is a claim about whole programs: see
[What the numbers do not show](#what-the-numbers-do-not-show).

Data used (the logs are not in the repository):

| Class | CPU | Run | Commit | Date |
|---|---|---|---|---|
| Ivy Bridge host A | Xeon E3-1220L v2 (Ivy Bridge, 2C/4T, no AVX2) | full bench, instructions:u counted (`--counters`), 473 `ratio_host` rows, 21 benchmark programs | `a6fc34c2` (`next`) | 2026-11-05 |
| Ivy Bridge host B | Xeon E3-1265L v2 (Ivy Bridge, 4C/8T, no AVX2) | the same, repeated | `a6fc34c2` | 2026-11-05 |
| Skylake-SP | Xeon Platinum 8151 (Skylake-SP, 48 threads, AVX2 and AVX-512) | full bench of the 17 comparable programs, instructions:u counted (`--counters --runs 3`), 529 counted rows; time ratios not used (host not booked) | `a6fc34c2` (`next`) | 2026-11-05 |
| Haswell-EP | Xeon E5-2697 v3 (Haswell, AVX2) | full bench, time only, 161 rows | `5cc49dc` | 2026-10-08 |
| Broadwell-EP | Xeon E5-2699 v4 (Broadwell, AVX2) | full bench, time only, 161 rows | `5cc49dc` | 2026-10-08 |
| Ivy Bridge hosts A and B | as above | `bench/parity-measure.tsv` (BENCH-REFRESH-1023): 214 rows with instruction counts | 10-23 `next` | 2026-10-23 |

The 2026-11-05 runs are the V-4 bench: the products were built once (`cargo xtask bench --build-only`, release
profile, fat LTO) on a build host and the programs run on each timing host with `--no-build --counters`, the host
booked for the run and idle otherwise. They took 4,571 s (host A) and 4,221 s (host B). Both timing hosts are Ivy
Bridge, which has no AVX2: every number in sections 2 to 4 and the appendix is for the SSE2/fallback code paths. The
counted bench was then repeated on a Skylake-SP host (AVX2 and AVX-512) at the same commit `a6fc34c2`: its counts are in
[section 2a](#2a-avx2-class-instruction-counts); nothing else on this page uses that run. The rows' decision on host A is on cycles with no limits for that CPU (nothing gated); on host B, 13 rows
exceeded their cycle limits (limits calibrated on `main` 5236e77), listed in the run's log, not reproduced here.

**What "before" means.** The 5cc49dc page has time ratios only, from AVX2 machines. It has no instruction counts,
and I did not build 5cc49dc again, so there is no 5cc49dc instruction count to compare with. The counters
comparison below is therefore against the newest earlier counted record on the same CPU class, the 2026-10-23
`bench/parity-measure.tsv` (214 rows joined), and the 5cc49dc comparison is of time ratios across two CPU classes
(advisory, owner rule: counters decide). A true 5cc49dc instruction count would need that commit built and counted
on an Ivy Bridge host; it is not in this run.

## 1. How to read the numbers

**The ratio.** Each row is `median(Liella ns / glibc ns)` over the rounds, from the same run.
Below 1.00 means Liella is faster; 2.00 means Liella takes twice as long. The glibc side is
built from the same C source as the Liella side (same compiler flags, `-fno-builtin`
included), so both run identical benchmark code; only the library differs. Rounds alternate
between the two. See the header of [`xtask/src/bench.rs`](../xtask/src/bench.rs) and
[`bench/budgets.tsv`](../bench/budgets.tsv) (the `ratio_host` rows).

**Parity.** The project works to a ratio of 1.00 or below on every row; a row above 1.00 needs
either a plan to reach it or a proof that it cannot (owner ruling 2026-10-08). The bench
output itself still prints the older "target 1.1" (about 10% over glibc, set 2026-10-05) and
counts rows above it as informational. This page uses 1.00 as the bound and the bands
below.

**Noise.** A difference of a few percent is not a result. The project's working figure for
the median ratio is about 1 to 3% run to run; I did not re-measure it. The raw per-round
spread is larger: `liella-historic:handoff/measure-1.md` (Haswell-EP) records a median
round-to-round spread of 4.3% and a p90 of 13.7% for Liella's own time, and a code-layout
effect (the same code linked in another order) of median 1.5%, p90 5.9%, up to 13.9% on one
row. Rows within about 5% of 1.00 are therefore called **at parity** below, neither a win nor
a loss. The regression limits in `host_limits.tsv` are 1.25 times a past median: they catch
regressions, they are not targets.

**Time versus instruction gating (ICOUNT-LIMITS, owner ruling 2026-10-10).** A small
single-offset row (`memset_16` and friends: one size at most 128 B, one alignment) swings
about +-10% in time with where the linker happens to place the code, so a time limit on it
fails or passes by chance. Such rows are gated on **instructions per call** instead, in
[`bench/instr_limits.tsv`](../bench/instr_limits.tsv) (columns `cpu bench row max_instr
rationale`; `*` = every CPU). `cargo xtask bench` makes the counted run (the one `--counters`
prints) whenever a selected row has such a limit, and fails the row when the median Liella
instructions per call exceed the limit, or when the row has no counted value. Instruction
counts are near-deterministic and identical on Zen 1 and the timing processors for the same code
path; a CPU that takes another path (no AVX2) needs its own line. The time ratio of these
rows stays reported but is advisory (their `host_limits.tsv` lines are retired). Setting a limit: the larger of the current count and
glibc's, rounded up; where we are above glibc, current + 1 so a regression fails and the gap
stays visible. The owner edits the limit files.
`python3 bench/limits_from_counters.py target/liella/bench/counters.tsv [--headroom 2%|N] [--rows a,b] [--cpu MODEL]`
(the table of the last `--counters` run: every `--counters` run, `bench` or `bench --compare`, rewrites
`counters.tsv` with its own rows only and writes `counters-<bench>.tsv` per bench it counted, same columns, so a
per-bench file is the input for one bench)
prints limit lines in this format from a counted run (limit = count + max(2 instr, 2%), rounded up); with `--check` it
diffs them against the current file (LOOSE = more than 5% above the proposal, WOULD-FAIL, NEW, NO-ROW) and edits nothing.
The owner still edits the limit files.

**Bands used on this page:** faster (< 0.95), at parity (0.95 to 1.05), slower (> 1.05).

### What the numbers do not show

The first three matter for how to read any row; the rest are completeness notes.

* **Microbenchmarks, one call at a time.** Most rows time one small operation in a tight loop
  (nanoseconds to microseconds). A 2x ratio on a 5 ns call is 5 ns. The absolute time is given
  next to the biggest gaps below; read it before judging a ratio. This matters.
* **Static link.** The comparison binaries are statically linked. glibc's dynamic and static
  builds differ by a median of 1.3% (`measure-1.md` finding 2: static/dynamic median 0.987,
  25 of 123 rows differ by more than 5%); gap sizes move for some rows (for example
  `strftime_c_en_us` 1.10 to 1.77, `strcmp_equal_16` 2.10 to 1.69 in that analysis) but no row
  changed sides of parity. Liella's own dynamic loader is not built yet, so a Liella
  dynamic-link measurement does not exist. This matters for a few rows.
* **Single-threaded**, except the `re_mt4_*` rows (four threads). Nothing here measures
  contention in malloc or stdio under load. This matters if you care about threaded programs.
* **Host-specific.** The 2026-11-05 timed run is on two Ivy Bridge machines (no AVX2: the SSE2 and
  fallback paths); the AVX2 class has instruction counts only (section 2a). The 5cc49dc run was on two AVX2 machines (Haswell-EP, Broadwell-EP). The two
  are different CPU classes, so a time ratio does not compare directly across them (section 3).
* **Cold paths** (`cold_exec_empty` and the `first_` rows) include process start and are
  noisier than the hot-path rows.
* **A row is one scenario.** `regexec_pathological_4k` is a single worst case for a
  backtracking engine, not a typical regex workload.


## 2. Summary

**Against glibc, now (a6fc34c2, Ivy Bridge host A; host B within a row or two).** Bands as in section 1.

| Measure | rows | faster (< 0.95) | at parity | slower (> 1.05) |
|---|---:|---:|---:|---:|
| Instructions per call, all counted rows | 542 | 235 | 63 | 244 |
| Instructions per call, without the libm rows | 373 | 234 | 60 | 79 |
| Instructions per call, libm rows (`cbench_libm`) | 169 | 1 | 3 | 165 |
| Time ratio (advisory), host A, `ratio_host` rows | 466 | 173 | 60 | 233 |
| Time ratio (advisory), host B | 466 | 175 | 58 | 233 |

Median instructions-per-call ratio without libm: 0.86. The libm rows dominate the "slower" column: see section 4.

**Before and after.**

| Comparison | basis | better | same | worse |
|---|---|---:|---:|---:|
| Liella instructions per call, now against 2026-10-23, same CPU class (+-3%) | counters, 214 rows with a count in both | 43 | 163 | 8 |
| Time ratio against glibc, now (Ivy Bridge) against 5cc49dc (Haswell-EP) (+-5%) | time, advisory, different CPU class, 159 common rows | 122 | 24 | 13 |

The 329 rows with no earlier count (new rows, most of libm; 543 rows were counted, 542 of them with an instruction ratio) are in the appendix as "no earlier count".

**By counters, the rows that got worse since 2026-10-23** (instructions per call, Ivy Bridge, > 3%):

| Row | 10-23 | now | change | now vs glibc (instr) |
|---|---:|---:|---:|---:|
| `cbench_locale/iswalpha_en_us` | 74 | 100 | +35% | 2.04 |
| `cbench_locale/iswalpha_l_en_us` | 83 | 109 | +31% | 2.32 |
| `cbench_opt/gettext_language_steady` | 854 | 1004 | +18% | 2.17 |
| `cbench_opt/gettext_language_alternating` | 1115 | 1240 | +11% | 1.29 |
| `cbench_opt/sprintf_chk_ls_nlink` | 821 | 883 | +8% | 1.20 |
| `cbench_locale/iswctype_en_us` | 99 | 104 | +5% | 2.42 |
| `cbench_locale/isalpha_c` | 22 | 23 | +5% | 1.44 |
| `cbench_opt/re_mt4_distinct` | 216466 | 225564 | +4% | 1.55 |

These eight are the regressions by counters. The `iswalpha`, `iswctype` and `isalpha_c` rows and the two
`gettext_language_*` rows are also among the slowest non-libm rows against glibc (section 4). I did not investigate why
they rose; each is an open question for the story that owns the row (`docs/saturation/05-*.md`, `12`, `13`, `14`).

**By time against 5cc49dc, the rows more than 5% higher than the Haswell-EP ratio** (advisory; Ivy Bridge against
Haswell-EP is not like for like, and the instruction ratio next to each shows the counted position):

| Row | 5cc49dc Haswell-EP | 5cc49dc Broadwell-EP | now (host A) | now instr vs glibc |
|---|---:|---:|---:|---:|
| `cbench_locale/iswalpha_en_us` | 0.73 | 0.80 | 1.43 | 2.04 |
| `cbench_locale/iswalpha_utf8` | 0.75 | 0.84 | 0.95 | 1.39 |
| `cbench_opt/gettext_c_locale` | 0.09 | 0.09 | 0.11 | 0.13 |
| `cbench_locale/localeconv_en_us` | 0.45 | 0.48 | 0.53 | 0.49 |
| `cbench_locale/iconv_utf8_ascii_translit_4k` | 1.14 | 1.08 | 1.34 | 0.86 |
| `cbench_opt/fnmatch_names_period` | 0.68 | 0.71 | 0.79 | 0.63 |
| `cbench_opt/fnmatch_names_casefold` | 0.57 | 0.60 | 0.67 | 0.68 |
| `cbench_opt/iconv_utf8_utf16_name` | 0.81 | 0.78 | 0.94 | 0.82 |
| `cbench_memref/memcpy_4k_off` | 0.78 | 0.59 | 0.89 | 0.99 |
| `cbench_memref/memcmp_4k` | 0.90 | 0.85 | 1.02 | 1.01 |
| `cbench_memref/memmove_64` | 0.90 | 0.80 | 1.00 | 0.69 |
| `cbench_opt/snprintf_f_large` | 0.20 | 0.21 | 0.22 | 0.18 |
| `cbench_opt/regcomp_regfree_sed_kv` | 0.44 | 0.44 | 0.47 | 0.40 |

Where the instruction ratio is below 1.00 the higher time ratio is probably the change of CPU class, not a Liella
regression; that is a reading, not a measurement (no 5cc49dc build was run on Ivy Bridge). The rows that are higher in
time and above 1.05 in instructions (`iswalpha_en_us`, `iswalpha_utf8`) are real and are listed in section 4b.

Per benchmark program, instruction-ratio bands against glibc now (Ivy Bridge host A):

| Program | faster | parity | slower |
|---|---:|---:|---:|
| `cbench_fstree` | 1 | 8 | 2 |
| `cbench_hot` | 40 | 10 | 8 |
| `cbench_libm` | 1 | 3 | 165 |
| `cbench_locale` | 16 | 4 | 13 |
| `cbench_malloc` | 13 | 1 | 1 |
| `cbench_memref` | 66 | 18 | 31 |
| `cbench_opt` | 66 | 4 | 14 |
| `cbench_patterns` | 8 | 2 | 3 |
| `cbench_process` | 2 | 12 | 2 |
| `cbench_startup` | 4 | 0 | 0 |
| `cbench_stdio` | 5 | 0 | 1 |
| `cbench_time` | 9 | 0 | 0 |
| `cbench_unmeasured` | 4 | 1 | 4 |

## 2a. AVX2-class instruction counts

The same counted bench (`cargo xtask bench --compare --counters --runs 3`, 17 of the 18 C programs;
`a6fc34c2`, release profile) on a Skylake-SP host (Xeon Platinum 8151, AVX2 and AVX-512), run on a shared host that was not booked for timing. Instruction counts do not depend on load, so they hold; **the time ratios of this run
are not used anywhere on this page** (host not booked). The count per row is `instr_ratio_ss`: Liella static against
glibc static, user-mode instructions per call. Bands as in section 1. The Ivy Bridge column is the "now instr vs glibc"
column of the appendix (host A).

| Measure | rows | faster (< 0.95) | at parity | slower (> 1.05) |
|---|---:|---:|---:|---:|
| Instructions per call, all counted rows, AVX2 class | 529 | 187 | 90 | 252 |
| Instructions per call, without the libm rows, AVX2 class | 360 | 186 | 87 | 87 |
| Instructions per call, libm rows, AVX2 class | 169 | 1 | 3 | 165 |
| (Ivy Bridge, section 2, for comparison: all / without libm) | 542 / 373 | 235 / 234 | 63 / 60 | 244 / 79 |

Median instructions-per-call ratio without libm: 0.93 (Ivy Bridge 0.86). The libm rows are 165 slower on both classes. The two row sets differ by 13 rows (`cbench_nscache` cannot be compared, see
section 5; I did not identify the other four), so the totals are not a like-for-like difference; the per-row comparison
below is: all 529 rows of this run are in the appendix of the Ivy Bridge run.

**What the counter counts for a rep string instruction (PARITY-COPY-AVX2 probe, 2026-11-05).** `perf stat -e
instructions:u` of a loop of 100,000 `rep movsb` (and, separately, `rep stosb`) at 0, 1, 64, 1024, 4096, 16384 and 65536
bytes counted 9.00 instructions per iteration at every size on a Skylake-SP host (Xeon Platinum 8151), a Haswell-EP host
(E5-2697 v3) and an Ivy Bridge host (E3-1265L V2): the loop's 8 plus **one** for the `rep` instruction, whatever its
length. On all three classes a `rep movsb`/`rep stosb` is one instruction, so a row where either side runs one counts
that side's setup only, not the copy. Such rows are judged by timing on a booked timing host (Liella and the reference
on the same host, interleaved, same-host ratio; cycles there are that measurement), not by instructions (owner ruling,
2026-11-05: timing is the final arbiter). Rows where neither side runs a rep string instruction keep instruction counts
as the development gate, and the periodic timing sweep gives their verdict. Copy rows this concerns (the side
that runs `rep movsb` follows from its per-size count; Liella's dispatch is in `crates/boundary/src/cpu.rs`):
`memcpy_4k` (Skylake-SP: Liella's `rep movsb` window starts at 4 KiB on an ERMS CPU with AVX-512, 73 instructions;
Ivy Bridge: the reference's count of about 27 against Liella's 718-instruction SSE2 loop, which runs below
`COPY_ERMS_MIN` = 6144), `memcpy_64k` (both sides `rep movsb` on Skylake-SP: 85.5 against 57.5 is setup against setup;
Ivy Bridge 100 against 54, the same), and on Haswell-EP 16 and 64 KiB (the reference's 44 is setup; Liella's 80 at 16 KiB
is its `rep movsb` window, 5718 at 64 KiB its vector loop). `memmove_4k`, `memcpy_64`, `memmove_64` and the 1 KiB copies
run vector code on both sides: developed and gated on instructions, verdict from the timing sweep.

**Where the two classes give a different verdict.** 89 of the 529 rows change band between the Ivy Bridge and the
AVX2 class (bands on instructions per call vs glibc): faster to parity 31, faster to slower 21, slower to parity 13,
parity to faster 10, parity to slower 9, slower to faster 5. The pattern in the lists below: on Ivy Bridge Liella's string and memory rows mostly use fewer
instructions than glibc's; on the AVX2 class many of them are at parity or above 1.05 (glibc may use AVX2 or AVX-512
variants there; not separated), mostly on the 16-byte to 64 KiB `strlen`, `strnlen`, `memchr`, `rawmemchr`, `memset`, `memcmp` and
`strcmp` rows. The reverse (slower on Ivy Bridge, parity or better here) happens for `memchr`, `bcmp`, `memcmp` and
`strstr` rows. These are instruction counts, not times: a shorter vector loop can still be slower per call, and no AVX2
time was taken at this commit.

The rows at **parity on one class and above 1.05 on the other** (22):

| Row | Ivy Bridge instr vs glibc | AVX2-class instr vs glibc | AVX2-class Liella instr per call |
|---|---:|---:|---:|
| `strchr_16_hit` (cbench_hot) | 1.06 | 1.00 | 28 |
| `strchrnul_16_hit` (cbench_hot) | 1.07 | 1.04 | 25 |
| `strncmp_eq16` (cbench_hot) | 1.01 | 1.09 | 35 |
| `strncmp_names64` (cbench_hot) | 1.07 | 1.02 | 42 |
| `strcasecmp_c_16` (cbench_locale) | 0.98 | 1.20 | 54 |
| `realloc_grow_x2` (cbench_malloc) | 1.11 | 0.99 | 2035 |
| `__memcmpeq_eq_16` (cbench_memref) | 1.00 | 1.14 | 23 |
| `__memcmpeq_eq_1k` (cbench_memref) | 1.21 | 1.04 | 172 |
| `__memcmpeq_eq_64` (cbench_memref) | 1.37 | 1.04 | 25 |
| `__memcmpeq_eq_8` (cbench_memref) | 1.00 | 1.15 | 23 |
| `__memcmpeq_tail_16` (cbench_memref) | 1.00 | 1.15 | 23 |
| `__memcmpeq_tail_64` (cbench_memref) | 1.38 | 1.04 | 25 |
| `bcmp_eq_128` (cbench_memref) | 1.08 | 1.00 | 39 |
| `bcmp_eq_1k` (cbench_memref) | 1.21 | 1.04 | 172 |
| `bcmp_eq_64` (cbench_memref) | 1.34 | 0.97 | 25 |
| `memchr_16` (cbench_memref) | 1.58 | 1.04 | 25 |
| `memchr_16_mix` (cbench_memref) | 1.33 | 1.03 | 32 |
| `memcmp_16_mix` (cbench_memref) | 1.05 | 1.13 | 42 |
| `memcmp_4k` (cbench_memref) | 1.01 | 1.24 | 671 |
| `memcmp_64k` (cbench_memref) | 1.00 | 1.13 | 9325 |
| `strcmp_64_mix` (cbench_memref) | 0.95 | 1.26 | 71 |
| `fwrite_4k` (cbench_stdio) | 2.05 | 1.05 | 1095 |

The rows that are faster than glibc (< 0.95) on Ivy Bridge and slower (> 1.05) on the AVX2 class (19): `memrchr_4k`,
`putenv_remove_cycle_30`, `unsetenv_cycle_30`, `strncasecmp_c_64`, `bcmp_tail_32`, `memchr_64k`, `memmove_4k`, `memset_128`,
`memset_64`, `rawmemchr_4k`, `rawmemchr_64k`, `strcmp_16_mix`, `strlen_4k`, `strlen_64k`, `strnlen_16`, `strnlen_16_mix`,
`strnlen_4k`, `strnlen_64k`, `strcmp_names`. The reverse (slower than glibc on Ivy Bridge, faster on the AVX2 class, 5):
`nl_langinfo_en_us`, `bcmp_tail_64`, `memcpy_4k`, `strstr_hit_4k`, `strstr_miss_64`. (`memcpy_4k` is 26.3 on Ivy Bridge and
0.17 here; I did not look into why, and a count ratio that far from 1 on a plain copy needs a look before it is trusted.)
The full row-by-row list of all 89 is in the run's scratch output, not in the repository.

## 3. Where Liella is faster

The shape of the 5cc49dc page holds: 234 of the 373 non-libm counted rows are faster by instructions and the median
instruction ratio is 0.86. The reasons recorded for the largest wins are unchanged and live in
[`docs/saturation-register.md`](saturation-register.md) (per-row tables) and in the ratio tables of the appendix: equal-string collation, gettext
lookups, the time-zone cache, `re_mt4_*` (four threads on one compiled regex), large-magnitude `%f`, iconv open and
ASCII conversion, `fnmatch`, `newlocale`, `localeconv`, `strtod`. Each of these rows is still well below 1.00 in the
appendix (`fnmatch_*`, `gettext_c_locale` 0.11, `localeconv_en_us` 0.53, `snprintf_f_large` 0.22,
`regcomp_regfree_sed_kv` 0.47 in time on host A). No new claim is made about why.

The rows that moved most in the right direction since 2026-10-23 by counters are the 43 "better" rows in the appendix
(instructions per call more than 3% lower).


## 4. Where glibc is faster, and what could not be improved

Counted rows slower than glibc by instructions (> 1.05): 244 of 542. Of them 165 are libm rows. This section is the
"could not improve" list: each group has its reason or its open story as recorded in
[`docs/saturation/`](saturation-register.md) (section numbers below are that register's files), or says that no row
exists.

### 4a. libm (`cbench_libm`): 169 rows, 165 slower by instructions

Median instructions-per-call ratio against glibc: 9.7 for the 88 double-precision rows, 18.1 for the 41 `float` rows
and 504 for the 40 `long double` rows (host A instruction counts). Only 1 libm row is faster
(`log10f`, 0.84) and `add_baseline` and the two `mxcsr_*` harness rows sit at 1.00 by construction.
This is the largest open gap of the library. It is not a saturation claim: the math functions are the subject of the
open `MATH-M5-*` and `MATH-M6-*` stories (f64 `exp`/`log`/`pow` gap breakdown: register section 16; other math stories are held or landing through the math integration branch). The long-double and `float`
gaps are not measured to a cause here.

### 4b. Everything else: 79 rows slower by instructions

| Group (rows, instr ratio range) | Reason or open story as recorded |
|---|---|
| `memcmp_*`, `bcmp_*`, `__memcmpeq_*` (21 rows, 1.07 to 1.85) | Register 03 records the small compares at or near an instruction floor (load/compare/branch ladder; fused flag test and `memcmp`-equal specialisations measured negative). The `*_head_*`, `*_diff8_*` and 64/128-byte rows are above that: open, register 13 to 15 rank them. |
| `memcpy_4k` 26.3, `memcpy_64k` 1.85, `memset_4k` 2.46, `memset_64k` 1.89 | Large-block rows. The time ratios are 0.86 to 1.04 (parity or faster), so on this CPU the count is not a time loss; why the instruction count is higher was not investigated here. Register 03, 08, 10, 12, 13, 15. |
| `memchr_16`, `strstr_hit_64`, `strstr_miss`, `wcsstr_miss`, `memrchr`, `strchr`, `strchrnul`, `strncmp_*`, `strcmp_late_256` | Register 03 (kernel floor), 13 to 15 (ranked stories), 18 (SSE2 re-measure). `wcsstr_miss` and `strncmp_names64` have no register row: open. |
| `iswalpha_*`, `iswctype_en_us`, `towupper_*`, `isalpha_c`, `wcwidth_utf8`, `nl_langinfo_l_en_us`, `ctype_b_loc` (11 rows, 1.13 to 2.42) | Register 05, 12 to 15. Up to 35% more instructions than on 2026-10-23 for `iswalpha`: a regression, open. |
| `strcasecmp_*` (3 rows, 1.16) | Register 07 (AVX2 kernel built; the Ivy Bridge SSE2 path is the one measured here). `strcasecmp_utf8`: no register row. |
| `gettext_found`, `gettext_language_*`, `ngettext` (1.09 to 2.17) | Register 05, 12 to 14. |
| `wordexp_*` (7 rows, 1.07 to 1.47) | Register 07: "no design for a larger change is recorded". Open. |
| `regexec_backref`, `re_mt4_shared`, `re_mt4_distinct` (1.06 to 1.56 by instructions; time 0.02 to 0.16) | Register 06, 12 to 14. The `re_mt4_*` rows count more instructions across four threads yet their time ratios are 0.02 and 0.16 (faster than glibc); instructions are not what decides these rows. |
| `fwrite_4k` 2.05 | Register 02, 13 to 15. Time ratio 0.99. |
| `qsort_c_cmp_1024*` (4 rows, 1.29 to 1.51) | Register 17 and 19 (first counts and element sizes). Open. |
| `fts_physical`, `nftw_phys`, `getline_long`, `sys_close_ebadf`, `sprintf_chk_ls_nlink` (1.06 to 1.20) | Register 13 to 15 (kernel-shared or small); `sprintf_chk_ls_nlink` rose 8% since 10-23 (register 14). |
| `realloc_grow_x2` 1.11, `fork_exit_wait` 1.06, `waitpid_echild` 1.09 | No register row: open. Small gaps (5% to 10%); not investigated. |

This table groups by name and cites the register's files; I did not re-read every row's "tried, measured negative"
column, so the register is the authority on what was tried per row.

## 5. Not measured in this run

* No AVX2 **time** measurement at `a6fc34c2` (the AVX2 class was counted, not timed; the 5cc49dc Haswell-EP and
  Broadwell-EP times are the only AVX2 timings). No `--v3` or `--sorted` comparison on any class. The AVX2-class run was
  on a CPU that also has AVX-512, and the run did not separate the AVX2 kernels from AVX-512 ones on either side:
  glibc may have chosen an AVX-512 (EVEX) variant there. A `--tunables glibc.cpu.hwcaps=-AVX512F` count would split them.
* `cbench_nscache` (it includes a Liella-only header, so the host build cannot compile it and the command stops) and
  the glibc column of `cbench_netdb` (static host build fails) have no AVX2-class comparison.
* No AMD host. The saturation register's Zen 1 numbers are from earlier runs and are not refreshed here.
* The `5cc49dc` instruction counts (section 1 of this page).


## 6. How to reproduce

Needs the Rust toolchain from `rust-toolchain.toml`, a C compiler and the host glibc. Build
and run the whole bench (about 15 to 40 minutes depending on machine; Haswell-EP took 819 s for the
`5cc49dc` run; the 2026-11-05 run took 4,571 s and 4,221 s on Ivy Bridge hosts, counters on):

```
cargo xtask bench
```

Run named benchmark binaries only (the names are in `bench/budgets.tsv`; each must have rows
there):

```
cargo xtask bench --only cbench_opt,cbench_patterns
```

Compare against the host glibc and print the ratio table (`--runs N` sets the number of
rounds, `--counters` adds per-row instruction counts and IPC from `perf stat`, using aggregate
counters of the glibc-linked reference only; the summary table then ends with instructions per
operation for Liella and glibc and their ratio):

```
cargo xtask bench --compare --counters --runs 5 cbench_opt
cargo xtask bench --compare --only cbench_opt,cbench_patterns
```

In compare mode `--only a,b` and positional names are the same list; any other argument starting
with `-` is an error, never a benchmark name.

Measure a lower dispatch level (BENCH-TUNABLES): `--tunables <value>` sets `GLIBC_TUNABLES` for both the
Liella and the glibc side, in `cargo xtask bench` and in `--compare`, and the output prints a `tunables:` line
so a reader sees which level ran, for example
`cargo xtask bench --compare --counters --runs 1 --tunables glibc.cpu.hwcaps=-AVX2 cbench_opt`. The benchmark
environment is fixed (BENCH-FIXED-ENV), so a `GLIBC_TUNABLES=... cargo xtask bench` in the caller's environment is
dropped and measures the full-width paths on both sides; use the option. Compare the instruction counts with and
without the option to confirm the level changed.

Where a row's limit lives: [`bench/host_limits.tsv`](../bench/host_limits.tsv) is keyed by the
exact `model name` string from `/proc/cpuinfo`. A CPU with no entry is measured but not
gated, which is why Broadwell-EP's rows are all "not gated" in its log. To add a CPU, run a full
bench of `main` on it and set each row's limit to 1.25 times its median (the rule recorded in
the file's header).

Which flags exist is defined in [`xtask/src/bench.rs`](../xtask/src/bench.rs); read it before
relying on a flag from this page. The raw logs of the two runs above are not in the repository.

Timing hosts have no compiler. A timing-host run is two commands (XTASK-BENCH-STAGE):

```bash
# on a build host, in the tree to measure (the same selection flags as the measuring run):
cargo xtask bench --stage /tmp/stage.tar [--only <benches> | --rows <bench>/<row>,...]
# on the timing host, with the host booked:
mkdir ~/work/<story>/tree && tar -xf stage.tar -C ~/work/<story>/tree && cd ~/work/<story>/tree
./xtask-bin bench --no-build --staged --counters [--only <benches> | --rows ...]
```

`--stage` builds as `--build-only` does, then writes one tarball: the workspace `Cargo.toml` and `xtask/Cargo.toml`,
`bench/`, `target/liella/` (without the generated `bench/stress/`), the selected Rust bench examples, the xtask
binary as `xtask-bin`, and `bench-stage.manifest` (commit, a sha256 of the source tree, build profile, built-on
host, and the sha256 of every staged product), with the build's mtimes. A `--no-build` run in a tree that holds the
manifest (`--staged` requires one) checks every product against its sha256 instead of comparing mtimes, needs no
git and no source tree, and prints a `staged: commit ...` header line naming what it timed; a product that does not
match is refused. The commit comes from git, or from the `head=` line of a role directory's `.synced` file.

A run in a staged tree writes its tables (`bench_times.tsv`, `rounds.tsv`, `ratio_host.tsv`, `decision.tsv`,
`counters.tsv`, `counters-<bench>.tsv`, and for `--compare` `measure.tsv` and `<bench>.compare.tsv`) to `bench-out/` at
the staged root, never into `target/liella/` (XTASK-BENCH-OUTPUTS), so the same unpacked tree runs any number of times;
`scp` the tables from `<dir>/bench-out/`. `--stage` leaves the build host's own run tables out of the tarball. Outside a
staged tree the tables stay in `target/liella/bench/`.

## Appendix: all rows

One table per benchmark program. Columns: the 5cc49dc time ratio on each AVX2 machine (`-` = the row did not exist);
Liella instructions per call on the Ivy Bridge hosts in the 2026-10-23 record and now (host A), and the ratio of
those instructions to glibc's; the time ratio now on host A and host B; and the change in instructions per call
against 2026-10-23 (better / same / worse, +-3%). Time ratios are advisory; "no earlier count" means the row is not in
the 2026-10-23 record.


### cbench_fstree

| Row | 5cc49dc Haswell-EP time | 5cc49dc Broadwell-EP time | 10-23 Ivy instr | now Ivy instr | now instr vs glibc | now time vs glibc (E3-1220L v2) | now time vs glibc (E3-1265L v2) | change in instr vs 10-23 |
|---|---:|---:|---:|---:|---:|---:|---:|---|
| `fdopendir_readdir_closedir_99` | - | - | 165998 | 165681 | 0.99 | 0.92 | 0.94 | same (-0%) |
| `fts_nochdir_flat_1000` | - | - | - | 6857291 | 0.87 | - | - | no earlier count |
| `fts_physical_1000` | - | - | 6794347 | 6843262 | 1.09 | - | - | same (+1%) |
| `getcwd` | - | - | 2233 | 2232 | 1.02 | - | - | same (-0%) |
| `getmntent_50_lines` | - | - | 153772 | 154651 | 1.04 | - | - | same (+1%) |
| `mkstemp_unlink_close` | - | - | 72102 | 72702 | 1.03 | - | - | same (+1%) |
| `nftw_phys_1000` | - | - | 6281495 | 6211097 | 1.10 | - | - | same (-1%) |
| `opendir_readdir_closedir_1000` | - | - | 1612967 | 1620165 | 1.00 | 0.94 | 0.94 | same (+0%) |
| `readdir_entry` | - | - | 1596 | 1606 | 1.00 | 0.94 | 0.94 | same (+1%) |
| `realpath_5` | - | - | 21377 | 21373 | 1.04 | - | - | same (-0%) |
| `scandir_alphasort_1000` | - | - | 2739238 | 2727314 | 0.97 | - | - | same (-0%) |

### cbench_hot

| Row | 5cc49dc Haswell-EP time | 5cc49dc Broadwell-EP time | 10-23 Ivy instr | now Ivy instr | now instr vs glibc | now time vs glibc (E3-1220L v2) | now time vs glibc (E3-1265L v2) | change in instr vs 10-23 |
|---|---:|---:|---:|---:|---:|---:|---:|---|
| `ctype_b_loc` | - | - | 14 | 12 | 1.20 | 0.86 | 0.86 | better (-14%) |
| `ctype_get_mb_cur_max` | - | - | 10 | 10 | 0.91 | 1.00 | 1.00 | same (+0%) |
| `errno_location` | - | - | 9 | 9 | 0.90 | 1.00 | 1.00 | same (+0%) |
| `fwrite_unlocked_16` | - | - | 84 | 83 | 0.73 | 0.71 | 0.71 | same (-0%) |
| `fwrite_unlocked_64` | - | - | 93 | 91 | 0.70 | 0.73 | 0.73 | same (-2%) |
| `getc` | - | - | 51 | 22 | 0.82 | 1.00 | 1.00 | better (-56%) |
| `getc_unlocked` | - | - | 44 | 15 | 1.00 | 0.89 | 0.89 | better (-66%) |
| `getenv_hit_last` | - | - | 1000 | 1000 | 0.46 | 0.57 | 0.57 | same (-0%) |
| `getenv_hit_last_2000` | - | - | 11194 | 11191 | 0.91 | - | - | same (-0%) |
| `getenv_hit_last_300` | - | - | 1825 | 1824 | 0.90 | - | - | same (-0%) |
| `getenv_hit_mid` | - | - | 572 | 572 | 0.75 | 0.94 | 0.93 | same (-0%) |
| `getenv_hit_mid_2000` | - | - | 5641 | 5639 | 0.92 | - | - | same (-0%) |
| `getenv_hit_mid_300` | - | - | 958 | 957 | 0.93 | - | - | same (-0%) |
| `getenv_miss` | - | - | 575 | 575 | 0.42 | 0.51 | 0.51 | same (-0%) |
| `getenv_miss_2000` | - | - | 11123 | 11119 | 0.91 | - | - | same (-0%) |
| `getenv_miss_300` | - | - | 1753 | 1752 | 0.90 | - | - | same (-0%) |
| `getline_80` | - | - | 308 | 238 | 0.99 | - | - | better (-23%) |
| `getline_long` | - | - | 9868 | 9450 | 1.20 | - | - | better (-4%) |
| `memcpy_chk_64` | - | - | 27 | 27 | 0.69 | 0.80 | 0.80 | same (+0%) |
| `mempcpy_16` | - | - | 21 | 21 | 0.95 | 1.00 | 1.00 | same (+0%) |
| `mempcpy_64` | - | - | 23 | 23 | 0.72 | 0.80 | 0.80 | same (+0%) |
| `memrchr_4k` | - | - | 1201 | 984 | 0.86 | 0.89 | 0.88 | better (-18%) |
| `memrchr_64` | - | - | 53 | 48 | 0.91 | 1.00 | 1.00 | better (-10%) |
| `memrchr_64_hit` | - | - | 36 | 31 | 1.11 | 1.28 | 1.28 | better (-14%) |
| `mutex_lock_unlock_normal` | - | - | 64 | 64 | 0.89 | 0.95 | 0.95 | same (+0%) |
| `mutex_lock_unlock_normal_mt` | - | - | 63 | 63 | 0.85 | 1.00 | 1.00 | same (-0%) |
| `mutex_lock_unlock_recursive` | - | - | 83 | 83 | 0.92 | 0.81 | 0.81 | same (+0%) |
| `mutex_lock_unlock_recursive_mt` | - | - | 82 | 82 | 0.88 | 0.96 | 0.96 | same (+0%) |
| `putc_unlocked` | - | - | 40 | 17 | 0.90 | 0.87 | 0.86 | better (-57%) |
| `putenv_2000` | - | - | 11467 | 11464 | 0.13 | - | - | same (-0%) |
| `putenv_30` | - | - | 1237 | 1237 | 0.79 | - | - | same (-0%) |
| `putenv_remove_cycle_2000` | - | - | 11622 | 11619 | 0.13 | - | - | same (-0%) |
| `putenv_remove_cycle_30` | - | - | 1398 | 1398 | 0.93 | - | - | same (-0%) |
| `setenv_2000` | - | - | 11780 | 11776 | 0.09 | - | - | same (-0%) |
| `setenv_30` | - | - | 1543 | 1543 | 0.60 | - | - | same (-0%) |
| `setenv_300` | - | - | 2411 | 2410 | 0.12 | - | - | same (-0%) |
| `setenv_getenv_2000` | - | - | 17420 | 17414 | 0.13 | - | - | same (-0%) |
| `setenv_getenv_300` | - | - | 3366 | 3365 | 0.16 | - | - | same (-0%) |
| `strchr_16_hit` | - | - | 41 | 34 | 1.06 | 1.32 | 1.32 | better (-17%) |
| `strchr_16_miss` | - | - | 56 | 42 | 0.75 | 1.16 | 1.16 | better (-25%) |
| `strchr_64_hit` | - | - | 75 | 58 | 1.04 | 1.38 | 1.37 | better (-23%) |
| `strchr_64_miss` | - | - | 78 | 58 | 1.04 | 1.47 | 1.47 | better (-26%) |
| `strchrnul_16_hit` | - | - | 39 | 31 | 1.07 | 1.56 | 1.55 | better (-21%) |
| `strchrnul_16_miss` | - | - | 51 | 39 | 0.74 | 1.00 | 1.00 | better (-24%) |
| `strchrnul_64_hit` | - | - | 73 | 55 | 1.04 | 1.53 | 1.53 | better (-25%) |
| `strchrnul_64_miss` | - | - | 73 | 55 | 1.04 | 1.44 | 1.44 | better (-25%) |
| `strcpy_chk_16` | - | - | 40 | 40 | 0.49 | 0.60 | 0.60 | same (-0%) |
| `strcpy_chk_64` | - | - | 54 | 54 | 0.59 | 0.60 | 0.60 | same (+0%) |
| `strncmp_diff8` | - | - | 56 | 56 | 1.44 | 1.75 | 1.75 | same (+0%) |
| `strncmp_eq16` | - | - | 67 | 67 | 1.01 | 1.27 | 1.26 | same (+0%) |
| `strncmp_names64` | - | - | 89 | 89 | 1.07 | 1.00 | 1.00 | same (+0%) |
| `sys_close_ebadf` | - | - | 425 | 425 | 1.06 | 1.00 | 0.98 | same (-0%) |
| `sys_fstat` | - | - | 1194 | 1193 | 1.01 | 1.02 | 0.99 | same (-0%) |
| `sys_read_1` | - | - | 932 | 931 | 1.01 | 1.03 | 1.02 | same (-0%) |
| `unsetenv_cycle_2000` | - | - | 11554 | 11551 | 0.11 | - | - | same (-0%) |
| `unsetenv_cycle_30` | - | - | 1333 | 1333 | 0.75 | - | - | same (-0%) |
| `unsetenv_miss_2000` | - | - | 11350 | 11346 | 0.14 | - | - | same (-0%) |
| `unsetenv_miss_30` | - | - | 802 | 802 | 0.59 | - | - | same (-0%) |

### cbench_libm

| Row | 5cc49dc Haswell-EP time | 5cc49dc Broadwell-EP time | 10-23 Ivy instr | now Ivy instr | now instr vs glibc | now time vs glibc (E3-1220L v2) | now time vs glibc (E3-1265L v2) | change in instr vs 10-23 |
|---|---:|---:|---:|---:|---:|---:|---:|---|
| `acos` | - | - | - | 1569 | 18.75 | 41.60 | 40.56 | no earlier count |
| `acosf` | - | - | - | 1557 | 25.44 | 50.73 | 50.68 | no earlier count |
| `acosf128` | - | - | - | 222761 | 23.31 | 23.29 | 23.26 | no earlier count |
| `acosh` | - | - | - | 1451 | 14.68 | 16.82 | 16.79 | no earlier count |
| `acoshf` | - | - | - | 1446 | 18.11 | 26.35 | 26.32 | no earlier count |
| `acoshf128` | - | - | - | 102032 | 9.70 | 12.81 | 12.80 | no earlier count |
| `acoshl` | - | - | - | 101875 | 1190.46 | 438.45 | 440.73 | no earlier count |
| `acosl` | - | - | - | 222595 | 4974.30 | 498.42 | 497.86 | no earlier count |
| `add_baseline` | - | - | - | 4 | 1.00 | - | - | no earlier count |
| `asin` | - | - | - | 1560 | 19.43 | 40.95 | 39.90 | no earlier count |
| `asinf` | - | - | - | 1551 | 28.97 | 61.95 | 62.27 | no earlier count |
| `asinf128` | - | - | - | 219163 | 23.78 | 23.29 | 23.25 | no earlier count |
| `asinh` | - | - | - | 1511 | 13.11 | 15.95 | 15.86 | no earlier count |
| `asinhf` | - | - | - | 1484 | 15.72 | 22.34 | 22.33 | no earlier count |
| `asinhf128` | - | - | - | 107572 | 9.28 | 11.77 | 11.74 | no earlier count |
| `asinhl` | - | - | - | 107399 | 1460.20 | 507.93 | 506.97 | no earlier count |
| `asinl` | - | - | - | 218997 | 2125.15 | 1020.93 | 1030.82 | no earlier count |
| `atan` | - | - | - | 1392 | 12.68 | 19.50 | 19.42 | no earlier count |
| `atan2` | - | - | - | 1618 | 8.17 | 12.35 | 12.26 | no earlier count |
| `atan2f` | - | - | - | 1621 | 10.96 | 17.94 | 17.99 | no earlier count |
| `atan2f128` | - | - | - | 209163 | 33.02 | 30.39 | 30.38 | no earlier count |
| `atan2l` | - | - | - | 208922 | 4674.19 | 561.21 | 563.67 | no earlier count |
| `atanf` | - | - | - | 1380 | 20.70 | 33.26 | 33.36 | no earlier count |
| `atanf128` | - | - | - | 201956 | 34.42 | 34.27 | 34.26 | no earlier count |
| `atanh` | - | - | - | 1514 | 11.58 | 17.65 | 17.63 | no earlier count |
| `atanhf` | - | - | - | 1496 | 12.92 | 17.82 | 17.91 | no earlier count |
| `atanhf128` | - | - | - | 94456 | 8.56 | 13.41 | 13.40 | no earlier count |
| `atanhl` | - | - | - | 94299 | 928.21 | 384.72 | 385.47 | no earlier count |
| `atanl` | - | - | - | 201788 | 9327.77 | 576.03 | 574.50 | no earlier count |
| `cbrt` | - | - | - | 1070 | 7.21 | 10.20 | 10.18 | no earlier count |
| `cbrtf` | - | - | - | 162 | 1.18 | 1.24 | 1.24 | no earlier count |
| `cbrtf128` | - | - | - | 46153 | 8.38 | 6.55 | 6.59 | no earlier count |
| `cbrtl` | - | - | - | 45986 | 291.19 | 90.61 | 90.99 | no earlier count |
| `cexp` | - | - | - | 144404 | 503.31 | 870.02 | 866.91 | no earlier count |
| `cexpf` | - | - | - | 144374 | 785.79 | 764.66 | 766.20 | no earlier count |
| `cexpl` | - | - | - | 144218 | 123.74 | 130.94 | 130.78 | no earlier count |
| `clog` | - | - | - | 92103 | 197.17 | 264.24 | 264.61 | no earlier count |
| `clogf` | - | - | - | 92077 | 333.51 | 302.48 | 301.73 | no earlier count |
| `clogl` | - | - | - | 299756 | 981.96 | 384.14 | 383.01 | no earlier count |
| `cos` | - | - | - | 336 | 2.66 | 4.01 | 3.92 | no earlier count |
| `cosf` | - | - | - | 167 | 3.28 | 3.67 | 3.67 | no earlier count |
| `cosf128` | - | - | - | 97215 | 13.92 | 25.45 | 25.44 | no earlier count |
| `cosh` | - | - | - | 1527 | 16.12 | 25.76 | 25.57 | no earlier count |
| `coshf` | - | - | - | 1507 | 18.20 | 30.90 | 30.92 | no earlier count |
| `coshf128` | - | - | - | 51998 | 7.12 | 8.49 | 8.53 | no earlier count |
| `coshl` | - | - | - | 51781 | 460.74 | 139.67 | 139.83 | no earlier count |
| `cosl` | - | - | - | 97047 | 89.67 | 142.17 | 142.12 | no earlier count |
| `cpow` | - | - | - | 441774 | 569.43 | 620.11 | 617.72 | no earlier count |
| `cpowf` | - | - | - | 442374 | 922.05 | 685.20 | 685.13 | no earlier count |
| `cpowl` | - | - | - | 441661 | 282.09 | 203.98 | 204.31 | no earlier count |
| `csin` | - | - | - | 158333 | 350.66 | 452.41 | 452.81 | no earlier count |
| `csinf` | - | - | - | 158313 | 467.09 | 399.83 | 400.41 | no earlier count |
| `csqrt` | - | - | - | 50741 | 350.86 | 203.72 | 204.49 | no earlier count |
| `csqrtf` | - | - | - | 50903 | 410.24 | 146.89 | 146.76 | no earlier count |
| `csqrtl` | - | - | - | 51952 | 270.83 | 105.18 | 104.25 | no earlier count |
| `erf` | - | - | - | 212269 | 1260.73 | 1796.49 | 1807.95 | no earlier count |
| `erfc` | - | - | - | 212973 | 1236.07 | 1772.16 | 1772.20 | no earlier count |
| `erfcf` | - | - | - | 212952 | 1473.06 | 1935.99 | 1935.40 | no earlier count |
| `erfcf128` | - | - | - | 213115 | 11.63 | 17.12 | 17.17 | no earlier count |
| `erfcl` | - | - | - | 212954 | 1108.31 | 366.29 | 366.84 | no earlier count |
| `erff` | - | - | - | 212255 | 1503.11 | 1958.58 | 1950.65 | no earlier count |
| `erff128` | - | - | - | 212406 | 11.52 | 17.01 | 17.00 | no earlier count |
| `erfl` | - | - | - | 212244 | 1121.17 | 367.23 | 366.81 | no earlier count |
| `exp` | - | - | - | 182 | 2.35 | 2.90 | 1.86 | no earlier count |
| `exp10` | - | - | - | 420 | 7.48 | 12.96 | 12.68 | no earlier count |
| `exp10f` | - | - | - | 69 | 1.61 | 2.60 | 2.59 | no earlier count |
| `exp10f128` | - | - | - | 136831 | 10.30 | 14.31 | 14.28 | no earlier count |
| `exp10l` | - | - | - | 136603 | 1651.45 | 558.79 | 558.40 | no earlier count |
| `exp2` | - | - | - | 186 | 3.65 | 5.44 | 5.38 | no earlier count |
| `exp2f` | - | - | - | 70 | 1.67 | 2.30 | 2.30 | no earlier count |
| `exp2f128` | - | - | - | 41094 | 5.64 | 7.35 | 7.32 | no earlier count |
| `exp2l` | - | - | - | 40863 | 643.51 | 217.55 | 217.44 | no earlier count |
| `exp_overflow_errno` | - | - | - | 250 | 2.49 | 2.18 | 2.19 | no earlier count |
| `exp_qnan` | - | - | - | 53 | 1.06 | 0.85 | 0.85 | no earlier count |
| `exp_small` | - | - | - | 182 | 2.35 | 2.91 | 2.92 | no earlier count |
| `exp_tiny` | - | - | - | 484 | 11.22 | 7.85 | 7.87 | no earlier count |
| `exp_upward` | - | - | - | 408 | 5.29 | 12.56 | 12.39 | no earlier count |
| `expf` | - | - | - | 70 | 1.63 | 2.10 | 2.10 | no earlier count |
| `expf128` | - | - | - | 40416 | 6.05 | 7.89 | 7.87 | no earlier count |
| `expf_qnan` | - | - | - | 114 | 4.74 | 3.42 | 3.42 | no earlier count |
| `expf_upward` | - | - | - | 244 | 5.65 | 17.27 | 17.24 | no earlier count |
| `expl` | - | - | - | 40291 | 486.98 | 155.98 | 155.91 | no earlier count |
| `expm1` | - | - | - | 1372 | 13.15 | 15.58 | 15.51 | no earlier count |
| `expm1f` | - | - | - | 161 | 1.76 | 1.59 | 1.60 | no earlier count |
| `expm1f128` | - | - | - | 41356 | 4.44 | 6.88 | 6.85 | no earlier count |
| `expm1l` | - | - | - | 41132 | 581.88 | 159.70 | 159.59 | no earlier count |
| `hypot` | - | - | - | 3814 | 59.71 | 42.09 | 40.78 | no earlier count |
| `hypotf` | - | - | - | 48 | 1.37 | 1.73 | 1.73 | no earlier count |
| `hypotf128` | - | - | - | 7389 | 1.43 | 0.89 | 0.89 | no earlier count |
| `hypotl` | - | - | - | 4481 | 42.32 | 18.75 | 18.91 | no earlier count |
| `j0` | - | - | - | 142601 | 306.76 | 527.80 | 528.59 | no earlier count |
| `j0f` | - | - | - | 143326 | 478.64 | 779.50 | 777.41 | no earlier count |
| `j0f128` | - | - | - | 142768 | 4.17 | 8.24 | 8.23 | no earlier count |
| `j0l` | - | - | - | 142592 | 58.88 | 92.64 | 92.44 | no earlier count |
| `j1` | - | - | - | 142653 | 303.52 | 531.42 | 535.52 | no earlier count |
| `j1f` | - | - | - | 143369 | 471.76 | 772.38 | 769.96 | no earlier count |
| `j1f128` | - | - | - | 142819 | 4.14 | 8.16 | 8.17 | no earlier count |
| `j1l` | - | - | - | 142642 | 58.74 | 92.45 | 92.39 | no earlier count |
| `jn` | - | - | - | 142821 | 132.51 | 211.42 | 211.85 | no earlier count |
| `jnf` | - | - | - | 143541 | 193.90 | 300.75 | 300.09 | no earlier count |
| `jnf128` | - | - | - | 142989 | 1.92 | 3.66 | 3.66 | no earlier count |
| `jnl` | - | - | - | 142794 | 28.54 | 43.61 | 43.65 | no earlier count |
| `lgamma` | - | - | - | 586184 | 1428.99 | 1410.71 | 1407.06 | no earlier count |
| `lgammaf` | - | - | - | 587969 | 2037.94 | 2260.96 | 2260.86 | no earlier count |
| `lgammaf128` | - | - | - | 586316 | 10.80 | 14.05 | 14.05 | no earlier count |
| `lgammal` | - | - | - | 586155 | 1340.07 | 627.23 | 630.11 | no earlier count |
| `log` | - | - | - | 218 | 2.79 | 5.16 | 5.17 | no earlier count |
| `log10` | - | - | - | 243 | 2.05 | 3.52 | 3.52 | no earlier count |
| `log10f` | - | - | - | 78 | 0.84 | 1.21 | 1.22 | no earlier count |
| `log10f128` | - | - | - | 191781 | 23.41 | 38.88 | 38.87 | no earlier count |
| `log10l` | - | - | - | 191500 | 4317.25 | 1668.30 | 1667.33 | no earlier count |
| `log1p` | - | - | - | 1336 | 12.98 | 22.51 | 22.34 | no earlier count |
| `log1pf` | - | - | - | 174 | 2.00 | 2.17 | 2.17 | no earlier count |
| `log1pf128` | - | - | - | 85029 | 9.72 | 16.46 | 16.45 | no earlier count |
| `log1pl` | - | - | - | 84863 | 1839.48 | 701.83 | 701.70 | no earlier count |
| `log2` | - | - | - | 229 | 2.75 | 4.46 | 4.48 | no earlier count |
| `log2f` | - | - | - | 78 | 1.56 | 2.36 | 2.36 | no earlier count |
| `log2f128` | - | - | - | 95453 | 12.93 | 20.50 | 20.48 | no earlier count |
| `log2l` | - | - | - | 95282 | 2147.64 | 805.31 | 805.45 | no earlier count |
| `log_upward` | - | - | - | 390 | 4.99 | 15.11 | 15.01 | no earlier count |
| `logf` | - | - | - | 77 | 1.54 | 2.59 | 2.57 | no earlier count |
| `logf128` | - | - | - | 87106 | 11.18 | 19.53 | 19.56 | no earlier count |
| `logl` | - | - | - | 86934 | 1835.22 | 762.39 | 762.09 | no earlier count |
| `mxcsr_pair` | - | - | - | 3 | 1.00 | - | - | no earlier count |
| `mxcsr_wrapper_seq` | - | - | - | 11 | 1.00 | - | - | no earlier count |
| `pow` | - | - | - | 482 | 2.52 | 3.28 | 3.29 | no earlier count |
| `pow_upward` | - | - | - | 917 | 4.78 | 7.45 | 7.45 | no earlier count |
| `powf` | - | - | - | 250 | 2.74 | 4.46 | 4.46 | no earlier count |
| `powf128` | - | - | - | 123707 | 6.38 | 10.13 | 10.15 | no earlier count |
| `powl` | - | - | - | 123444 | 185.55 | 93.82 | 93.80 | no earlier count |
| `sin` | - | - | - | 340 | 2.78 | 4.09 | 4.04 | no earlier count |
| `sin_huge` | - | - | - | 1795 | 4.72 | 5.42 | 5.40 | no earlier count |
| `sin_upward` | - | - | - | 958 | 7.28 | 8.39 | 8.39 | no earlier count |
| `sinf` | - | - | - | 167 | 3.30 | 3.73 | 3.74 | no earlier count |
| `sinf128` | - | - | - | 97215 | 13.92 | 25.20 | 25.22 | no earlier count |
| `sinf_upward` | - | - | - | 248 | 4.88 | 13.49 | 13.48 | no earlier count |
| `sinh` | - | - | - | 1550 | 9.58 | 11.50 | 11.46 | no earlier count |
| `sinhf` | - | - | - | 1521 | 10.45 | 11.85 | 11.83 | no earlier count |
| `sinhf128` | - | - | - | 52027 | 5.12 | 7.21 | 7.20 | no earlier count |
| `sinhl` | - | - | - | 51802 | 409.07 | 145.15 | 145.15 | no earlier count |
| `sinl` | - | - | - | 97051 | 89.69 | 142.51 | 142.27 | no earlier count |
| `sqrt` | - | - | - | 33 | 1.94 | 1.50 | 1.50 | no earlier count |
| `sqrtf` | - | - | - | 31 | 1.82 | 2.09 | 2.09 | no earlier count |
| `sqrtf128` | - | - | - | 7475 | 3.90 | 1.73 | 1.73 | no earlier count |
| `sqrtl` | - | - | - | 83 | 2.77 | 3.58 | 3.58 | no earlier count |
| `tan` | - | - | - | 497 | 4.00 | 5.04 | 4.96 | no earlier count |
| `tanf` | - | - | - | 173 | 1.63 | 1.59 | 1.59 | no earlier count |
| `tanf128` | - | - | - | 105937 | 13.21 | 21.31 | 21.34 | no earlier count |
| `tanh` | - | - | - | 1549 | 10.41 | 13.09 | 13.04 | no earlier count |
| `tanhf` | - | - | - | 1522 | 11.59 | 13.61 | 13.61 | no earlier count |
| `tanhf128` | - | - | - | 60654 | 6.04 | 8.03 | 8.03 | no earlier count |
| `tanhl` | - | - | - | 60437 | 520.83 | 171.12 | 171.05 | no earlier count |
| `tanl` | - | - | - | 105772 | 101.08 | 148.50 | 148.46 | no earlier count |
| `tgamma` | - | - | - | 624832 | 951.08 | 944.97 | 947.67 | no earlier count |
| `tgammaf` | - | - | - | 626626 | 1242.05 | 1275.92 | 1269.63 | no earlier count |
| `tgammaf128` | - | - | - | 624999 | 20.08 | 25.60 | 25.59 | no earlier count |
| `tgammal` | - | - | - | 624819 | 830.91 | 344.82 | 344.78 | no earlier count |
| `y0` | - | - | - | 595972 | 1221.59 | 1553.15 | 1555.01 | no earlier count |
| `y0f` | - | - | - | 596822 | 1883.97 | 2266.82 | 2272.31 | no earlier count |
| `y0f128` | - | - | - | 596111 | 16.48 | 23.82 | 23.81 | no earlier count |
| `y0l` | - | - | - | 595931 | 241.14 | 284.04 | 283.58 | no earlier count |
| `y1` | - | - | - | 596293 | 1223.15 | 1563.91 | 1580.73 | no earlier count |
| `y1f` | - | - | - | 597162 | 1871.48 | 2301.78 | 2298.68 | no earlier count |
| `y1f128` | - | - | - | 596461 | 16.38 | 23.57 | 23.59 | no earlier count |
| `y1l` | - | - | - | 596278 | 241.17 | 283.92 | 283.31 | no earlier count |
| `yn` | - | - | - | 599433 | 552.50 | 681.17 | 680.08 | no earlier count |
| `ynf` | - | - | - | 600274 | 808.91 | 937.96 | 934.71 | no earlier count |
| `ynf128` | - | - | - | 599580 | 8.13 | 11.69 | 11.69 | no earlier count |
| `ynl` | - | - | - | 599400 | 118.75 | 140.56 | 140.72 | no earlier count |

### cbench_locale

| Row | 5cc49dc Haswell-EP time | 5cc49dc Broadwell-EP time | 10-23 Ivy instr | now Ivy instr | now instr vs glibc | now time vs glibc (E3-1220L v2) | now time vs glibc (E3-1265L v2) | change in instr vs 10-23 |
|---|---:|---:|---:|---:|---:|---:|---:|---|
| `fnmatch_casefold_en_32` | 0.87 | 0.91 | 7631 | 7571 | 0.97 | 0.82 | 0.82 | same (-1%) |
| `fnmatch_en_us` | 0.33 | 0.30 | 428 | 406 | 0.28 | 0.31 | 0.31 | better (-5%) |
| `iconv_utf8_8859_15_4k` | 1.50 | 1.52 | 51623 | 51619 | 0.32 | 0.28 | 0.28 | same (-0%) |
| `iconv_utf8_ascii_translit_4k` | 1.14 | 1.08 | 396249 | 396210 | 0.86 | 1.34 | 1.34 | same (-0%) |
| `iconv_utf8_utf16_4k` | 1.71 | 1.74 | 74665 | 74651 | 0.60 | 0.64 | 0.64 | same (-0%) |
| `isalpha_c` | 1.00 | 0.99 | 22 | 23 | 1.44 | 1.00 | 1.00 | worse (+5%) |
| `iswalpha_en_us` | 0.73 | 0.80 | 74 | 100 | 2.04 | 1.43 | 1.44 | worse (+35%) |
| `iswalpha_l_en_us` | - | - | 83 | 109 | 2.32 | 1.91 | 1.90 | worse (+31%) |
| `iswalpha_utf8` | 0.75 | 0.84 | 81 | 68 | 1.39 | 0.95 | 0.96 | better (-16%) |
| `iswctype_en_us` | - | - | 99 | 104 | 2.42 | 2.80 | 2.80 | worse (+5%) |
| `localeconv_en_us` | 0.45 | 0.48 | 56 | 56 | 0.49 | 0.53 | 0.53 | same (+0%) |
| `mbrtowc_en_us_3` | 1.26 | 1.26 | 217 | 217 | 0.89 | 0.89 | 0.89 | same (+0%) |
| `mbrtowc_utf8_1` | - | - | 40 | 40 | 0.20 | 0.28 | 0.28 | same (+0%) |
| `mbrtowc_utf8_2` | - | - | 53 | 53 | 0.24 | 0.30 | 0.30 | same (+0%) |
| `mbrtowc_utf8_3` | 1.41 | 1.49 | 217 | 217 | 0.89 | 0.91 | 0.91 | same (-0%) |
| `newlocale_free` | 0.36 | 0.34 | 16035 | 16034 | 0.35 | 0.34 | 0.34 | same (-0%) |
| `nl_langinfo_en_us` | 1.31 | 1.22 | 35 | 35 | 1.13 | 1.20 | 1.20 | same (+0%) |
| `nl_langinfo_l_en_us` | - | - | 44 | 44 | 1.57 | 1.56 | 1.55 | same (+0%) |
| `snprintf_f_en_us` | 0.87 | 0.83 | 1128 | 1135 | 0.44 | 0.31 | 0.31 | same (+1%) |
| `strcasecmp_c_16` | 6.18 | 5.53 | 162 | 94 | 0.98 | 1.02 | 1.02 | better (-42%) |
| `strcasecmp_c_256` | 5.89 | 5.61 | 407 | 368 | 1.16 | 0.96 | 0.95 | better (-10%) |
| `strcasecmp_en_256` | 6.19 | 6.01 | 407 | 368 | 1.16 | 0.97 | 0.95 | better (-10%) |
| `strcasecmp_utf8_256` | 6.18 | 5.99 | 407 | 368 | 1.16 | 0.97 | 0.95 | better (-10%) |
| `strcoll_utf8_16` | 1.24 | 1.27 | 58 | 57 | 0.58 | 0.73 | 0.73 | same (-2%) |
| `strftime_names_en_us` | 1.49 | 1.40 | 355 | 355 | 1.01 | 0.96 | 0.96 | same (+0%) |
| `strncasecmp_c_64` | 9.34 | 7.89 | 158 | 123 | 0.93 | 0.95 | 0.88 | better (-22%) |
| `strtod_c` | 0.57 | 0.52 | 285 | 285 | 0.32 | 0.22 | 0.22 | same (+0%) |
| `strtod_en_us` | 0.56 | 0.51 | 285 | 285 | 0.32 | 0.22 | 0.22 | same (+0%) |
| `strtod_utf8` | 0.56 | 0.52 | 285 | 285 | 0.32 | 0.22 | 0.22 | same (-0%) |
| `toupper_c` | 1.00 | 1.00 | 21 | 21 | 1.05 | 1.00 | 1.00 | same (+0%) |
| `towupper_l_en_us` | - | - | 67 | 68 | 1.74 | 1.57 | 1.57 | same (+1%) |
| `towupper_utf8` | 0.78 | 0.81 | 69 | 53 | 1.32 | 0.80 | 0.80 | better (-23%) |
| `wcwidth_utf8` | 0.79 | 0.80 | 55 | 50 | 1.16 | 0.79 | 0.79 | better (-9%) |

### cbench_malloc

| Row | 5cc49dc Haswell-EP time | 5cc49dc Broadwell-EP time | 10-23 Ivy instr | now Ivy instr | now instr vs glibc | now time vs glibc (E3-1220L v2) | now time vs glibc (E3-1265L v2) | change in instr vs 10-23 |
|---|---:|---:|---:|---:|---:|---:|---:|---|
| `calloc_free_4096` | - | - | - | 218 | 0.54 | 0.85 | 0.85 | no earlier count |
| `calloc_free_64` | - | - | - | 172 | 0.61 | 0.69 | 0.69 | no earlier count |
| `heap_mix_real` | - | - | - | 152 | 0.75 | 0.68 | 0.68 | no earlier count |
| `lifo_batch64` | - | - | - | 8342 | 0.73 | 0.96 | 0.95 | no earlier count |
| `malloc_free_1024` | - | - | - | 122 | 0.83 | 1.11 | 1.11 | no earlier count |
| `malloc_free_128` | - | - | - | 122 | 0.83 | 1.18 | 1.18 | no earlier count |
| `malloc_free_16` | - | - | - | 122 | 0.83 | 1.11 | 1.12 | no earlier count |
| `malloc_free_256` | - | - | - | 122 | 0.83 | 1.17 | 1.17 | no earlier count |
| `malloc_free_32` | - | - | - | 122 | 0.83 | 1.18 | 1.18 | no earlier count |
| `malloc_free_64` | - | - | - | 122 | 0.83 | 1.17 | 1.17 | no earlier count |
| `realloc_grow_16step` | - | - | - | 5826 | 0.54 | 0.53 | 0.53 | no earlier count |
| `realloc_grow_x2` | - | - | - | 2304 | 1.11 | 1.06 | 1.07 | no earlier count |
| `realloc_moving` | - | - | - | 448 | 0.60 | 0.60 | 0.60 | no earlier count |
| `realloc_null_24` | - | - | - | 148 | 0.85 | 1.05 | 1.07 | no earlier count |
| `realloc_within_usable` | - | - | - | 191 | 0.98 | 1.19 | 1.19 | no earlier count |

### cbench_memref

| Row | 5cc49dc Haswell-EP time | 5cc49dc Broadwell-EP time | 10-23 Ivy instr | now Ivy instr | now instr vs glibc | now time vs glibc (E3-1220L v2) | now time vs glibc (E3-1265L v2) | change in instr vs 10-23 |
|---|---:|---:|---:|---:|---:|---:|---:|---|
| `__memcmpeq_eq_128` | - | - | - | 71 | 1.11 | 1.40 | 1.40 | no earlier count |
| `__memcmpeq_eq_16` | - | - | - | 23 | 1.00 | 0.73 | 0.73 | no earlier count |
| `__memcmpeq_eq_1k` | - | - | - | 390 | 1.21 | 1.25 | 1.25 | no earlier count |
| `__memcmpeq_eq_32` | - | - | - | 31 | 1.10 | 1.00 | 1.00 | no earlier count |
| `__memcmpeq_eq_64` | - | - | - | 55 | 1.37 | 1.19 | 1.19 | no earlier count |
| `__memcmpeq_eq_8` | - | - | - | 23 | 1.00 | 0.80 | 0.80 | no earlier count |
| `__memcmpeq_head_128` | - | - | - | 39 | 1.85 | 1.84 | 1.85 | no earlier count |
| `__memcmpeq_head_16` | - | - | - | 18 | 0.79 | 1.00 | 1.00 | no earlier count |
| `__memcmpeq_head_1k` | - | - | - | 39 | 1.85 | 1.84 | 1.85 | no earlier count |
| `__memcmpeq_head_32` | - | - | - | 18 | 0.86 | 0.85 | 0.85 | no earlier count |
| `__memcmpeq_head_64` | - | - | - | 18 | 0.86 | 0.85 | 0.85 | no earlier count |
| `__memcmpeq_head_8` | - | - | - | 18 | 0.79 | 1.10 | 1.10 | no earlier count |
| `__memcmpeq_tail_128` | - | - | - | 71 | 1.11 | 1.42 | 1.42 | no earlier count |
| `__memcmpeq_tail_16` | - | - | - | 23 | 1.00 | 0.70 | 0.70 | no earlier count |
| `__memcmpeq_tail_1k` | - | - | - | 57 | 0.18 | 0.26 | 0.26 | no earlier count |
| `__memcmpeq_tail_32` | - | - | - | 31 | 1.10 | 1.00 | 1.00 | no earlier count |
| `__memcmpeq_tail_64` | - | - | - | 55 | 1.38 | 1.20 | 1.20 | no earlier count |
| `__memcmpeq_tail_8` | - | - | - | 18 | 0.79 | 1.11 | 1.11 | no earlier count |
| `bcmp_eq_128` | - | - | - | 71 | 1.08 | 1.55 | 1.55 | no earlier count |
| `bcmp_eq_16` | - | - | - | 23 | 0.92 | 0.80 | 0.80 | no earlier count |
| `bcmp_eq_1k` | - | - | - | 390 | 1.21 | 1.27 | 1.27 | no earlier count |
| `bcmp_eq_32` | - | - | - | 31 | 1.07 | 1.09 | 1.09 | no earlier count |
| `bcmp_eq_64` | - | - | - | 55 | 1.34 | 1.27 | 1.26 | no earlier count |
| `bcmp_eq_8` | - | - | - | 23 | 0.80 | 0.89 | 0.89 | no earlier count |
| `bcmp_head_128` | - | - | - | 39 | 1.56 | 1.92 | 1.92 | no earlier count |
| `bcmp_head_16` | - | - | - | 18 | 0.72 | 0.83 | 0.83 | no earlier count |
| `bcmp_head_1k` | - | - | - | 39 | 1.56 | 1.92 | 1.92 | no earlier count |
| `bcmp_head_32` | - | - | - | 18 | 0.72 | 0.83 | 0.83 | no earlier count |
| `bcmp_head_64` | - | - | - | 18 | 0.73 | 0.83 | 0.83 | no earlier count |
| `bcmp_head_8` | - | - | - | 18 | 0.55 | 0.91 | 0.91 | no earlier count |
| `bcmp_tail_128` | - | - | - | 71 | 0.96 | 1.17 | 1.17 | no earlier count |
| `bcmp_tail_16` | - | - | - | 23 | 0.80 | 0.54 | 0.54 | no earlier count |
| `bcmp_tail_1k` | - | - | - | 57 | 0.17 | 0.25 | 0.26 | no earlier count |
| `bcmp_tail_32` | - | - | - | 31 | 0.91 | 0.73 | 0.73 | no earlier count |
| `bcmp_tail_64` | - | - | - | 55 | 1.12 | 1.00 | 1.00 | no earlier count |
| `bcmp_tail_8` | - | - | - | 18 | 0.55 | 0.83 | 0.83 | no earlier count |
| `memchr_16` | - | - | - | 46 | 1.58 | - | - | no earlier count |
| `memchr_16_mix` | - | - | - | 53 | 1.33 | 1.24 | 1.23 | no earlier count |
| `memchr_4k` | 1.20 | 1.20 | - | 968 | 0.84 | 0.91 | 0.91 | no earlier count |
| `memchr_64` | 1.43 | 1.33 | - | 42 | 0.78 | 1.11 | 1.11 | no earlier count |
| `memchr_64_mix` | - | - | - | 49 | 0.74 | 0.98 | 0.97 | no earlier count |
| `memchr_64k` | 1.01 | 1.11 | - | 14905 | 0.85 | 1.00 | 1.00 | no earlier count |
| `memcmp_128` | - | - | - | 62 | 0.94 | 1.22 | 1.22 | no earlier count |
| `memcmp_144` | - | - | - | 81 | 1.01 | 1.32 | 1.32 | no earlier count |
| `memcmp_16` | - | - | - | 27 | 1.08 | 0.90 | 0.90 | no earlier count |
| `memcmp_16_mix` | - | - | - | 42 | 1.05 | 1.00 | 1.00 | no earlier count |
| `memcmp_256` | - | - | - | 118 | 1.10 | 1.40 | 1.40 | no earlier count |
| `memcmp_4k` | 0.90 | 0.85 | - | 1198 | 1.01 | 1.02 | 1.02 | no earlier count |
| `memcmp_64` | 1.37 | 1.26 | - | 42 | 1.02 | 1.00 | 1.00 | no earlier count |
| `memcmp_64_mix` | - | - | - | 57 | 1.02 | 1.01 | 1.01 | no earlier count |
| `memcmp_64k` | 0.97 | 0.91 | - | 18508 | 1.00 | 0.97 | 0.97 | no earlier count |
| `memcmp_diff8_127` | - | - | - | 44 | 1.75 | 1.61 | 1.61 | no earlier count |
| `memcmp_diff8_64` | - | - | - | 33 | 1.31 | 1.15 | 1.16 | no earlier count |
| `memcmp_diff8_96` | - | - | - | 44 | 1.76 | 1.61 | 1.61 | no earlier count |
| `memcpy_128` | - | - | - | 35 | 0.94 | 0.93 | 0.93 | no earlier count |
| `memcpy_144` | - | - | - | 53 | 0.88 | 1.01 | 1.01 | no earlier count |
| `memcpy_16` | 0.83 | 0.74 | - | 18 | 0.95 | 0.70 | 0.70 | no earlier count |
| `memcpy_16_mix` | - | - | - | 29 | 1.00 | 1.00 | 1.00 | no earlier count |
| `memcpy_256` | - | - | - | 53 | 0.63 | 0.70 | 0.70 | no earlier count |
| `memcpy_4k` | 1.10 | 1.16 | - | 718 | 26.33 | 0.86 | 0.86 | no earlier count |
| `memcpy_4k_off` | 0.78 | 0.59 | - | 18 | 0.99 | 0.89 | 0.89 | no earlier count |
| `memcpy_64` | 1.19 | 1.20 | - | 20 | 0.70 | 0.60 | 0.60 | no earlier count |
| `memcpy_64_mix` | - | - | - | 30 | 1.00 | 1.00 | 1.00 | no earlier count |
| `memcpy_64k` | 1.01 | 1.02 | - | 100 | 1.85 | 0.96 | 0.94 | no earlier count |
| `memmem_miss_4k` | - | - | - | 2961 | 0.43 | - | - | no earlier count |
| `memmem_miss_64` | - | - | - | 186 | 0.65 | - | - | no earlier count |
| `memmove_128` | - | - | - | 35 | 0.95 | 1.00 | 1.00 | no earlier count |
| `memmove_144` | - | - | - | 53 | 0.83 | 1.17 | 1.17 | no earlier count |
| `memmove_16` | 1.00 | 1.05 | - | 18 | 0.94 | 1.00 | 1.00 | no earlier count |
| `memmove_256` | - | - | - | 53 | 0.70 | 1.00 | 1.00 | no earlier count |
| `memmove_4k` | 2.30 | 2.14 | - | 707 | 0.89 | 1.01 | 1.12 | no earlier count |
| `memmove_64` | 0.90 | 0.80 | - | 20 | 0.69 | 1.00 | 1.00 | no earlier count |
| `memmove_64k` | - | - | - | 9848 | 0.80 | - | - | no earlier count |
| `memset_128` | - | - | - | 27 | 0.77 | 0.73 | 0.73 | no earlier count |
| `memset_144` | - | - | - | 39 | 0.87 | - | - | no earlier count |
| `memset_16` | 0.81 | 0.93 | - | 20 | 0.96 | 0.78 | 0.78 | no earlier count |
| `memset_16_mix` | - | - | - | 20 | 1.00 | 1.08 | 1.08 | no earlier count |
| `memset_256` | - | - | - | 39 | 0.75 | - | - | no earlier count |
| `memset_4k` | 1.11 | 1.09 | - | 67 | 2.46 | 1.03 | 1.03 | no earlier count |
| `memset_4k_off` | 0.99 | 0.99 | - | 14 | 1.00 | 1.01 | 1.01 | no earlier count |
| `memset_64` | 1.15 | 1.00 | - | 20 | 0.72 | 0.69 | 0.69 | no earlier count |
| `memset_64_mix` | - | - | - | 19 | 1.00 | 1.08 | 1.08 | no earlier count |
| `memset_64k` | 1.00 | 1.01 | - | 85 | 1.89 | 1.00 | 1.00 | no earlier count |
| `rawmemchr_16` | - | - | - | 30 | 0.91 | - | - | no earlier count |
| `rawmemchr_16_mix` | - | - | - | 37 | 0.91 | 0.84 | 0.84 | no earlier count |
| `rawmemchr_4k` | 1.34 | 1.34 | - | 938 | 0.91 | 0.99 | 0.99 | no earlier count |
| `rawmemchr_64` | 1.65 | 1.58 | - | 45 | 0.92 | 0.94 | 0.94 | no earlier count |
| `rawmemchr_64_mix` | - | - | - | 52 | 0.92 | 0.90 | 0.89 | no earlier count |
| `rawmemchr_64k` | 1.04 | 1.01 | - | 13915 | 0.90 | 1.01 | 1.01 | no earlier count |
| `strcasestr_hit_4k` | - | - | - | 4246 | 0.13 | - | - | no earlier count |
| `strcasestr_hit_64` | - | - | - | 426 | 0.49 | - | - | no earlier count |
| `strcasestr_miss_4k` | - | - | - | 4139 | 0.08 | - | - | no earlier count |
| `strcasestr_miss_64` | - | - | - | 318 | 0.27 | - | - | no earlier count |
| `strcmp_16_mix` | - | - | - | 56 | 0.72 | 0.38 | 0.39 | no earlier count |
| `strcmp_64_mix` | - | - | - | 94 | 0.95 | 0.57 | 0.57 | no earlier count |
| `strlen_16` | - | - | - | 27 | 0.65 | - | - | no earlier count |
| `strlen_16_mix` | - | - | - | 34 | 0.69 | 0.87 | 0.87 | no earlier count |
| `strlen_4k` | 1.44 | 1.42 | - | 586 | 0.92 | 1.01 | 1.01 | no earlier count |
| `strlen_64` | 1.70 | 1.54 | - | 44 | 0.65 | 0.79 | 0.79 | no earlier count |
| `strlen_64_mix` | - | - | - | 51 | 0.68 | 0.56 | 0.56 | no earlier count |
| `strlen_64k` | 1.03 | 1.00 | - | 8281 | 0.89 | 1.00 | 1.00 | no earlier count |
| `strnlen_16` | - | - | - | 42 | 0.81 | - | - | no earlier count |
| `strnlen_16_mix` | - | - | - | 49 | 0.83 | 1.08 | 1.07 | no earlier count |
| `strnlen_4k` | 1.23 | 1.03 | - | 676 | 0.81 | 0.80 | 0.80 | no earlier count |
| `strnlen_64` | 1.62 | 1.50 | - | 62 | 0.84 | 0.93 | 0.93 | no earlier count |
| `strnlen_64_mix` | - | - | - | 69 | 0.85 | 0.76 | 0.76 | no earlier count |
| `strnlen_64k` | 1.05 | 1.03 | - | 9331 | 0.76 | 1.02 | 1.02 | no earlier count |
| `strstr_hit_4k` | - | - | - | 2378 | 1.06 | - | - | no earlier count |
| `strstr_hit_64` | - | - | - | 216 | 2.17 | - | - | no earlier count |
| `strstr_miss_4k` | - | - | - | 2344 | 0.36 | - | - | no earlier count |
| `strstr_miss_64` | - | - | - | 262 | 1.24 | - | - | no earlier count |
| `wcsstr_hit_4k` | - | - | - | 19659 | 0.68 | - | - | no earlier count |
| `wcsstr_hit_64` | - | - | - | 491 | 1.04 | - | - | no earlier count |
| `wcsstr_miss_4k` | - | - | - | 19670 | 0.73 | - | - | no earlier count |
| `wcsstr_miss_64` | - | - | - | 501 | 1.05 | - | - | no earlier count |

### cbench_opt

| Row | 5cc49dc Haswell-EP time | 5cc49dc Broadwell-EP time | 10-23 Ivy instr | now Ivy instr | now instr vs glibc | now time vs glibc (E3-1220L v2) | now time vs glibc (E3-1265L v2) | change in instr vs 10-23 |
|---|---:|---:|---:|---:|---:|---:|---:|---|
| `execvp_missing` | - | - | 17549 | 17469 | 1.01 | - | - | same (-0%) |
| `fnmatch_names_casefold` | 0.57 | 0.60 | 1480 | 1445 | 0.68 | 0.67 | 0.67 | same (-2%) |
| `fnmatch_names_ch` | 0.44 | 0.47 | 750 | 766 | 0.45 | 0.46 | 0.46 | same (+2%) |
| `fnmatch_names_class` | 1.30 | 1.36 | 2264 | 911 | 0.51 | 0.58 | 0.58 | better (-60%) |
| `fnmatch_names_lib_so` | 0.40 | 0.45 | 483 | 442 | 0.36 | 0.41 | 0.41 | better (-9%) |
| `fnmatch_names_period` | 0.68 | 0.71 | 655 | 653 | 0.63 | 0.79 | 0.79 | same (-0%) |
| `fnmatch_names_question` | 0.46 | 0.51 | 580 | 527 | 0.37 | 0.37 | 0.36 | better (-9%) |
| `fnmatch_names_star_c` | 0.37 | 0.36 | 582 | 573 | 0.37 | 0.38 | 0.38 | same (-1%) |
| `fnmatch_paths_leading_dir` | 0.45 | 0.49 | 468 | 445 | 0.38 | 0.45 | 0.45 | better (-5%) |
| `fnmatch_paths_pathname` | 0.44 | 0.46 | 746 | 694 | 0.40 | 0.43 | 0.43 | better (-7%) |
| `fprintf_du_line` | 1.56 | 1.50 | 849 | 848 | 0.92 | 0.94 | 0.94 | same (-0%) |
| `fprintf_ls_line` | 1.39 | 1.32 | 1899 | 1897 | 0.74 | 0.65 | 0.64 | same (-0%) |
| `gettext_c_locale` | 0.09 | 0.09 | 107 | 105 | 0.13 | 0.11 | 0.11 | same (-2%) |
| `gettext_found` | 2.08 | 2.26 | 608 | 618 | 1.34 | 1.01 | 1.03 | same (+2%) |
| `gettext_language_alternating` | 1.89 | 1.96 | 1115 | 1240 | 1.29 | 1.16 | 1.16 | worse (+11%) |
| `gettext_language_putenv_alone` | 1.45 | 1.64 | 332 | 331 | 0.66 | 0.69 | 0.70 | same (-0%) |
| `gettext_language_steady` | - | - | 854 | 1004 | 2.17 | 1.62 | 1.61 | worse (+18%) |
| `gettext_missing` | 0.43 | 0.45 | 846 | 863 | 0.29 | 0.28 | 0.28 | same (+2%) |
| `gettext_no_catalog` | 0.52 | 0.54 | 703 | 718 | 0.34 | 0.35 | 0.35 | same (+2%) |
| `gettext_vary_msgid` | 0.77 | 0.73 | 740 | 754 | 0.42 | 0.42 | 0.42 | same (+2%) |
| `iconv_8859_15_utf8_latin` | 1.58 | 1.60 | 38097 | 38091 | 0.35 | 0.38 | 0.38 | same (-0%) |
| `iconv_open_conv_close_name` | 0.26 | 0.25 | 1541 | 1540 | 0.29 | 0.27 | 0.27 | same (-0%) |
| `iconv_utf16_utf8_mixed` | 2.38 | 2.42 | 82638 | 82625 | 0.73 | 0.64 | 0.64 | same (-0%) |
| `iconv_utf8_8859_15_latin_4k` | 1.42 | 1.41 | 51873 | 51865 | 0.33 | 0.36 | 0.36 | same (-0%) |
| `iconv_utf8_utf16_ascii_4k` | 0.31 | 0.30 | 28454 | 28450 | 0.22 | 0.20 | 0.20 | same (-0%) |
| `iconv_utf8_utf16_mixed_4k` | 2.11 | 2.12 | 70754 | 70743 | 0.60 | 0.61 | 0.61 | same (-0%) |
| `iconv_utf8_utf16_name` | 0.81 | 0.78 | 830 | 829 | 0.82 | 0.94 | 0.93 | same (-0%) |
| `iconv_utf8_utf32_mixed_4k` | 2.60 | 2.57 | 69066 | 69054 | 0.59 | 0.74 | 0.74 | same (-0%) |
| `localtime_r_new_york` | 0.13 | 0.13 | 392 | 392 | 0.07 | 0.08 | 0.08 | same (+0%) |
| `localtime_r_posix_rule` | 1.12 | 1.07 | 394 | 394 | 0.61 | 0.72 | 0.71 | same (-0%) |
| `mktime_new_york` | 1.86 | 1.70 | 949 | 948 | 0.64 | 0.63 | 0.63 | same (-0%) |
| `mktime_posix_rule` | 1.47 | 1.39 | 1058 | 1058 | 0.81 | 0.83 | 0.83 | same (-0%) |
| `mktime_utc` | 0.91 | 0.87 | 662 | 662 | 0.56 | 0.57 | 0.57 | same (-0%) |
| `ngettext_found` | 2.12 | 2.16 | 801 | 808 | 1.09 | 0.99 | 0.99 | same (+1%) |
| `re_grep_alt` | 0.81 | 0.79 | 694 | 696 | 0.07 | 0.09 | 0.09 | same (+0%) |
| `re_grep_comment` | 0.86 | 0.81 | 578 | 580 | 0.21 | 0.21 | 0.21 | same (+0%) |
| `re_grep_dotstar` | 4.12 | 3.99 | 764 | 765 | 0.26 | 0.28 | 0.28 | same (+0%) |
| `re_grep_icase_absent` | 1.23 | 1.28 | 1956 | 1956 | 0.32 | 0.31 | 0.31 | same (+0%) |
| `re_grep_ipv4` | 0.68 | 0.66 | 1347 | 1348 | 0.07 | 0.08 | 0.08 | same (+0%) |
| `re_grep_literal` | 1.18 | 1.07 | 417 | 419 | 0.53 | 0.40 | 0.40 | same (+0%) |
| `re_mt4_distinct` | 0.81 | 0.81 | 216466 | 225564 | 1.55 | 0.16 | 0.23 | worse (+4%) |
| `re_mt4_shared` | 0.14 | 0.13 | 219721 | 224643 | 1.56 | 0.02 | 0.02 | same (+2%) |
| `re_search_1byte` | - | - | 335 | 336 | 0.23 | - | - | same (+0%) |
| `re_search_sed_so` | - | - | 684 | 685 | 0.56 | - | - | same (+0%) |
| `re_search_sed_swap` | - | - | 6488 | 6604 | 0.29 | - | - | same (+2%) |
| `re_sed_kv` | 2.02 | 1.94 | 2720 | 2773 | 0.21 | 0.22 | 0.22 | same (+2%) |
| `re_sed_swap` | 0.66 | 0.66 | 3257 | 3322 | 0.08 | 0.09 | 0.09 | same (+2%) |
| `regcomp_regfree_ipv4` | 0.88 | 0.93 | 154913 | 154886 | 0.84 | 0.83 | 0.83 | same (-0%) |
| `regcomp_regfree_sed_kv` | 0.44 | 0.44 | 83443 | 83417 | 0.40 | 0.47 | 0.47 | same (-0%) |
| `regexec_sed_so` | - | - | 391 | 392 | 0.39 | - | - | same (+0%) |
| `regexec_sed_swap` | - | - | 6380 | 6496 | 0.29 | - | - | same (+2%) |
| `snprintf_d` | 1.10 | 1.02 | 495 | 494 | 0.84 | 0.81 | 0.81 | same (-0%) |
| `snprintf_e` | 2.27 | 2.37 | 1563 | 1570 | 0.96 | 0.91 | 0.91 | same (+0%) |
| `snprintf_f2` | 0.92 | 0.92 | 1121 | 1128 | 0.46 | 0.32 | 0.32 | same (+1%) |
| `snprintf_f_large` | 0.20 | 0.21 | 19278 | 19280 | 0.18 | 0.22 | 0.22 | same (+0%) |
| `snprintf_g` | 1.05 | 1.03 | 1307 | 1314 | 0.59 | 0.43 | 0.43 | same (+1%) |
| `snprintf_ld_large` | 1.00 | 0.98 | 555 | 554 | 0.84 | 0.81 | 0.81 | same (-0%) |
| `snprintf_ls_line` | 0.89 | 0.87 | 2428 | 2424 | 0.99 | 0.82 | 0.82 | same (-0%) |
| `snprintf_mixed` | 1.46 | 1.46 | 2017 | 2018 | 0.62 | 0.45 | 0.45 | same (+0%) |
| `snprintf_s_d_colon` | 1.12 | 1.17 | 960 | 962 | 1.00 | 0.90 | 0.90 | same (+0%) |
| `snprintf_s_path` | 1.37 | 1.36 | 233 | 234 | 0.58 | 0.60 | 0.59 | same (+0%) |
| `snprintf_x08` | 1.08 | 1.02 | 535 | 534 | 0.91 | 0.91 | 0.91 | same (-0%) |
| `sprintf_chk_ls_nlink` | - | - | 821 | 883 | 1.20 | 1.09 | 1.09 | worse (+8%) |
| `strcmp_equal_16` | 2.15 | 1.82 | 34 | 33 | 1.06 | 1.00 | 1.00 | same (-3%) |
| `strcmp_late_256` | 1.64 | 1.69 | 198 | 175 | 1.46 | 1.02 | 1.02 | better (-12%) |
| `strcmp_names` | 1.55 | 1.51 | 68 | 66 | 0.94 | 0.76 | 0.75 | same (-2%) |
| `strcoll_c_names` | 1.23 | 1.16 | 84 | 82 | 0.82 | 0.74 | 0.74 | same (-1%) |
| `strcoll_c_utf8_names` | 1.22 | 1.17 | 84 | 82 | 0.82 | 0.74 | 0.74 | same (-1%) |
| `strcoll_en_us_equal_16` | 0.04 | 0.04 | 63 | 63 | 0.02 | 0.01 | 0.01 | same (+0%) |
| `strcoll_en_us_names` | 1.63 | 1.62 | 168 | 168 | 0.43 | 0.45 | 0.44 | same (+0%) |
| `strcoll_en_us_utf8_16` | 1.15 | 1.02 | 45897 | 31186 | 0.88 | 0.78 | 0.78 | better (-32%) |
| `strftime_c_en_us` | 1.09 | 1.01 | 1565 | 1564 | 0.44 | 0.41 | 0.41 | same (-0%) |
| `strftime_date_en_us` | 1.23 | 1.20 | 910 | 910 | 0.64 | 0.79 | 0.79 | same (-0%) |
| `strftime_iso8601_tz` | 1.16 | 1.11 | 757 | 757 | 0.55 | 0.59 | 0.59 | same (-0%) |
| `strftime_log` | 1.22 | 1.11 | 823 | 822 | 0.59 | 0.68 | 0.67 | same (-0%) |
| `strftime_ls_iso` | 1.23 | 1.21 | 532 | 532 | 0.55 | 0.61 | 0.61 | same (-0%) |
| `strftime_ls_recent_c` | 1.26 | 1.19 | 477 | 477 | 0.61 | 0.64 | 0.65 | same (-0%) |
| `strftime_ls_recent_en_us` | 1.25 | 1.25 | 477 | 477 | 0.61 | 0.65 | 0.65 | same (-0%) |
| `wordexp_arith` | 3.74 | 3.70 | 4220 | 4152 | 1.17 | 1.46 | 1.46 | same (-2%) |
| `wordexp_param_forms` | 2.58 | 2.68 | 6377 | 6244 | 1.18 | 1.35 | 1.36 | same (-2%) |
| `wordexp_plain_8` | 2.32 | 2.29 | 3799 | 3797 | 0.57 | 0.72 | 0.72 | same (-0%) |
| `wordexp_quoted` | 3.34 | 3.24 | 3843 | 3791 | 1.47 | 1.73 | 1.73 | same (-1%) |
| `wordexp_split_var` | 2.42 | 2.39 | 3949 | 3805 | 1.07 | 1.11 | 1.11 | better (-4%) |
| `wordexp_tilde_var` | 3.64 | 3.53 | 4216 | 4100 | 1.33 | 1.52 | 1.53 | same (-3%) |

### cbench_patterns

| Row | 5cc49dc Haswell-EP time | 5cc49dc Broadwell-EP time | 10-23 Ivy instr | now Ivy instr | now instr vs glibc | now time vs glibc (E3-1220L v2) | now time vs glibc (E3-1265L v2) | change in instr vs 10-23 |
|---|---:|---:|---:|---:|---:|---:|---:|---|
| `fnmatch_bracket` | 1.12 | 1.06 | 948 | 786 | 0.83 | 0.83 | 0.83 | better (-17%) |
| `fnmatch_pathname` | 0.80 | 0.70 | 582 | 498 | 0.81 | 0.80 | 0.80 | better (-14%) |
| `fnmatch_pathname_nomatch` | 0.93 | 0.78 | 382 | 329 | 1.00 | 0.92 | 0.92 | better (-14%) |
| `fnmatch_star` | 0.89 | 0.72 | 380 | 358 | 0.81 | 0.81 | 0.81 | better (-6%) |
| `glob_1000_nosort` | 1.59 | 1.46 | 2544889 | 2602182 | 0.97 | 0.98 | 0.98 | same (+2%) |
| `glob_1000_sorted` | 1.39 | 1.29 | 3611283 | 2937921 | 0.80 | 0.78 | 0.78 | better (-19%) |
| `regcomp_regexec_regfree_email` | 2.65 | 2.60 | 146752 | 146670 | 0.28 | 0.34 | 0.34 | same (-0%) |
| `regexec_backref` | 1.89 | 1.84 | 24791 | 23299 | 1.06 | 1.17 | 1.18 | better (-6%) |
| `regexec_class_nomatch_1k` | 1.18 | 1.17 | 10265 | 8267 | 0.95 | 1.14 | 1.14 | better (-19%) |
| `regexec_literal_1k` | 0.72 | 0.72 | 1622 | 1629 | 0.06 | 0.07 | 0.07 | same (+0%) |
| `regexec_pathological_4k` | 0.00 | 0.00 | 1979 | 1986 | 0.00 | 0.00 | 0.00 | same (+0%) |
| `regexec_submatch_1k` | 3.19 | 3.10 | 32640 | 32775 | 0.07 | 0.08 | 0.08 | same (+0%) |
| `wordexp_arith` | 4.21 | 3.87 | 4254 | 4185 | 1.18 | 1.50 | 1.50 | same (-2%) |
| `wordexp_tilde_var` | 4.03 | 3.68 | 4201 | 4085 | 1.36 | 1.65 | 1.64 | same (-3%) |

### cbench_process

| Row | 5cc49dc Haswell-EP time | 5cc49dc Broadwell-EP time | 10-23 Ivy instr | now Ivy instr | now instr vs glibc | now time vs glibc (E3-1220L v2) | now time vs glibc (E3-1265L v2) | change in instr vs 10-23 |
|---|---:|---:|---:|---:|---:|---:|---:|---|
| `cfsetspeed_cfgetospeed` | - | - | - | 86 | 0.47 | - | - | no earlier count |
| `fork_exit_wait` | - | - | - | 174864 | 1.06 | - | - | no earlier count |
| `isatty_devnull` | - | - | - | 479 | 0.96 | - | - | no earlier count |
| `isatty_pty` | - | - | - | 948 | 0.97 | - | - | no earlier count |
| `openpty_close` | - | - | - | 82912 | 0.99 | - | - | no earlier count |
| `popen_pclose_true` | - | - | - | 88260 | 1.02 | - | - | no earlier count |
| `posix_spawn_true` | - | - | - | 73627 | 1.00 | 1.00 | 1.00 | no earlier count |
| `posix_spawn_true_actions` | - | - | - | 75212 | 1.02 | - | - | no earlier count |
| `posix_spawn_true_heap256m` | - | - | - | 75639 | 1.03 | - | - | no earlier count |
| `posix_spawnp_true` | - | - | - | 73944 | 1.01 | 1.00 | 1.00 | no earlier count |
| `system_exit0` | - | - | - | 80333 | 1.01 | - | - | no earlier count |
| `tcgetattr` | - | - | - | 977 | 1.01 | - | - | no earlier count |
| `tcsetattr_now` | - | - | - | 3107 | 0.79 | - | - | no earlier count |
| `ttyname_r` | - | - | - | 17317 | 1.01 | - | - | no earlier count |
| `vfork_exit_wait` | - | - | - | 50948 | 1.01 | - | - | no earlier count |
| `waitpid_echild` | - | - | - | 793 | 1.09 | - | - | no earlier count |

### cbench_startup

| Row | 5cc49dc Haswell-EP time | 5cc49dc Broadwell-EP time | 10-23 Ivy instr | now Ivy instr | now instr vs glibc | now time vs glibc (E3-1220L v2) | now time vs glibc (E3-1265L v2) | change in instr vs 10-23 |
|---|---:|---:|---:|---:|---:|---:|---:|---|
| `startup_instr_c` | - | - | - | 2141 | 0.02 | - | - | no earlier count |
| `startup_instr_printf` | - | - | - | 77565 | 0.53 | - | - | no earlier count |
| `startup_instr_setloc` | - | - | - | 73648 | 0.51 | - | - | no earlier count |
| `startup_instr_wctype` | - | - | - | 74662 | 0.51 | - | - | no earlier count |

### cbench_stdio

| Row | 5cc49dc Haswell-EP time | 5cc49dc Broadwell-EP time | 10-23 Ivy instr | now Ivy instr | now instr vs glibc | now time vs glibc (E3-1220L v2) | now time vs glibc (E3-1265L v2) | change in instr vs 10-23 |
|---|---:|---:|---:|---:|---:|---:|---:|---|
| `fprintf_d` | 1.54 | 1.54 | 636 | 640 | 0.93 | 0.90 | 0.90 | same (+1%) |
| `fputs_32` | 1.18 | 1.10 | 134 | 133 | 0.79 | 0.81 | 0.81 | same (-1%) |
| `fwrite_4k` | 1.03 | 1.03 | 1482 | 1376 | 2.05 | 0.99 | 1.01 | better (-7%) |
| `putc` | 1.19 | 1.17 | 47 | 24 | 0.75 | 1.00 | 1.00 | better (-49%) |
| `snprintf_d` | 1.19 | 1.21 | 495 | 494 | 0.84 | 0.81 | 0.81 | same (-0%) |
| `snprintf_mixed` | 1.36 | 1.39 | 2033 | 2034 | 0.61 | 0.43 | 0.43 | same (+0%) |

### cbench_time

| Row | 5cc49dc Haswell-EP time | 5cc49dc Broadwell-EP time | 10-23 Ivy instr | now Ivy instr | now instr vs glibc | now time vs glibc (E3-1220L v2) | now time vs glibc (E3-1265L v2) | change in instr vs 10-23 |
|---|---:|---:|---:|---:|---:|---:|---:|---|
| `gmtime_r` | 1.02 | 0.92 | 203 | 203 | 0.55 | 0.57 | 0.57 | same (-0%) |
| `localtime_r_new_york` | 0.11 | 0.11 | 394 | 394 | 0.06 | 0.07 | 0.07 | same (-0%) |
| `localtime_r_new_york_2100` | 0.10 | 0.10 | 392 | 392 | 0.06 | 0.06 | 0.07 | same (-0%) |
| `localtime_r_posix_rule` | 1.10 | 1.11 | 394 | 394 | 0.61 | 0.71 | 0.73 | same (-0%) |
| `localtime_r_utc` | 1.17 | 1.13 | 382 | 382 | 0.59 | 0.80 | 0.81 | same (-0%) |
| `mktime_new_york` | 1.78 | 1.69 | 854 | 854 | 0.55 | 0.53 | 0.53 | same (-0%) |
| `strftime_c` | 1.26 | 1.21 | 1079 | 1079 | 0.47 | 0.48 | 0.48 | same (-0%) |
| `strftime_iso` | 1.21 | 1.20 | 756 | 756 | 0.57 | 0.60 | 0.61 | same (-0%) |
| `timegm` | 0.91 | 0.88 | 380 | 380 | 0.56 | 0.65 | 0.65 | same (-0%) |

### cbench_unmeasured

| Row | 5cc49dc Haswell-EP time | 5cc49dc Broadwell-EP time | 10-23 Ivy instr | now Ivy instr | now instr vs glibc | now time vs glibc (E3-1220L v2) | now time vs glibc (E3-1265L v2) | change in instr vs 10-23 |
|---|---:|---:|---:|---:|---:|---:|---:|---|
| `fts_entry_deep2k` | - | - | - | 6924 | 0.31 | 0.53 | 0.51 | no earlier count |
| `fts_entry_flat` | - | - | - | 6866 | 0.87 | 0.93 | 0.90 | no earlier count |
| `qsort_c_cmp_1024` | - | - | - | 48 | 1.51 | 1.07 | 1.09 | no earlier count |
| `qsort_c_cmp_1024_e16` | - | - | - | 48 | 0.87 | 0.69 | 0.69 | no earlier count |
| `qsort_c_cmp_1024_e24` | - | - | - | 48 | 0.86 | 0.73 | 0.73 | no earlier count |
| `qsort_c_cmp_1024_e4` | - | - | - | 48 | 1.51 | 1.07 | 0.78 | no earlier count |
| `qsort_c_cmp_1024_e64` | - | - | - | 50 | 1.29 | 0.84 | 0.84 | no earlier count |
| `qsort_c_cmp_1024_e8` | - | - | - | 48 | 1.47 | 1.10 | 1.11 | no earlier count |
| `readdir_entry_flat` | - | - | - | 1616 | 1.00 | 0.95 | 0.96 | no earlier count |

