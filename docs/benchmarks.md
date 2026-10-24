# Benchmarks: Liella against glibc 2.39

This page says where Liella is faster than glibc, where it is slower, and how to
re-measure either. Numbers come from the logs named in each section, on the CPU and commit
named there. Nothing here is a claim about whole programs: see
[What the numbers do not show](#what-the-numbers-do-not-show).

Data used (all from `main`; the logs are not in the repository):

| Class | CPU | Log | Commit | Date |
|---|---|---|---|---|
| Haswell-EP | Xeon E5-2697 v3 (Haswell, AVX2) | `baseline log (Haswell-EP, 5cc49dc)` | `5cc49dc` | 2026-10-08 |
| Broadwell-EP | Xeon E5-2699 v4 (Broadwell, AVX2) | `baseline log (Broadwell-EP, 5cc49dc)` | `5cc49dc` | 2026-10-08 |

Both runs are full benches of the same commit (161 `ratio_host` rows each; 16 benchmark
binaries; 3 to 5 adaptive rounds per row). Both machines run Ubuntu 24.04, kernel
6.8.0-146, host glibc 2.39-0ubuntu8.9. Haswell-EP is the project's parity record box and has
per-CPU regression limits in [`bench/host_limits.tsv`](../bench/host_limits.tsv); Broadwell-EP has
none yet, so its rows are reported but not gated.

`main` has moved on since `5cc49dc` (it was `ccfca68` when this page was written). A full
bench of `ccfca68` was started but did not produce numbers (its log,
`~/logs/baseline-ccfca68.log`, stops at a build-staleness error), so `5cc49dc` is the newest
full measurement. Rows touched by later merges may differ; see
[Work in review](#work-in-review).

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
* **Host-specific.** Two Intel AVX2 machines. Where rows differ between them it is noted. A
  CPU without AVX2 was measured only for a few rows on branches (see below).
* **Cold paths** (`cold_exec_empty` and the `first_` rows) include process start and are
  noisier than the hot-path rows.
* **A row is one scenario.** `regexec_pathological_4k` is a single worst case for a
  backtracking engine, not a typical regex workload.

## 2. Summary

Counts over the 161 `ratio_host` rows of each log (`5cc49dc`), using the bands above:

| CPU | faster (< 0.95) | at parity (0.95 to 1.05) | slower (> 1.05) | median row ratio |
|---|---:|---:|---:|---:|
| Haswell-EP, E5-2697 v3 | 55 | 14 | 92 | 1.14 |
| Broadwell-EP, E5-2699 v4 | 57 | 18 | 86 | 1.10 |

So Liella is faster on about a third of the rows and slower on a little over half. It is not
at glibc parity across the library yet. Per area (Haswell-EP; Broadwell-EP differs by at most 3 rows in
any area):

| Area | rows | faster | parity | slower |
|---|---:|---:|---:|---:|
| Regex, glob, fnmatch, wordexp | 43 | 23 | 0 | 20 |
| Time (`localtime_r`, `mktime`, `strftime`) | 22 | 5 | 1 | 16 |
| stdio and printf | 20 | 4 | 2 | 14 |
| Case-insensitive compare, collation, gettext | 19 | 5 | 0 | 14 |
| Locale, ctype, wide characters | 14 | 9 | 2 | 3 |
| iconv | 11 | 3 | 0 | 8 |
| Memory and string scans | 31 | 5 | 9 | 17 |
| Other (`cold_exec_empty`) | 1 | 1 | 0 | 0 |

The two CPUs agree closely. Only 8 rows are on opposite sides of 1.05 between them, all of
them small-size rows near the line (for example `snprintf_d` 1.10 on Haswell-EP, 1.03 on Broadwell-EP;
`memset_64` 1.15 and 1.00; `strnlen_4k` 1.23 and 1.03). The largest cross-CPU differences
(over 0.25 in ratio) are all in the compare/wordexp families listed in section 4b. The
`5cc49dc` Haswell-EP run reported no budget regression against the Haswell-EP limits.

Complete per-row tables are in the [appendix](#appendix-all-rows).

## 3. Where Liella is faster

Haswell-EP ratios; Broadwell-EP is within 0.05 of these unless noted. "ns" is Liella's time per call from
the same log. The "why" column is what the repository records; where I could not find a
recorded cause I say so.

| Row | Ratio | Liella ns | Why |
|---|---:|---:|---|
| `strcoll_en_us_equal_16` | 0.04 | 12 | Equal strings short-circuit; glibc does a full collation compare (about 270 ns implied by the ratio). The one-pass compare is in D-4.20 ([plan](../plan/07-decisions/s04-byte-arrays-and-strings.md)); I did not trace this row to its exact code path. A narrow case: unequal names are 1.2 to 1.6 (section 4). |
| `gettext_c_locale`, `_missing`, `_no_catalog` | 0.09, 0.43, 0.52 | 8 to 130 | Lookups without per-call file checks or key scans (`liella-historic:handoff/gettext2.md`). Rows that find a translation (`gettext_found`) are still 2.08 (section 4). |
| `localtime_r_new_york`, `_2100` | 0.11, 0.10 | 65 | A time-zone cache (`liella-historic:handoff/tcache.md`). glibc implied about 600 ns; I have not probed why glibc's is that high. `mktime_utc` is now 0.91 after MKT-1 (`liella-historic:handoff/mkt1.md`). |
| `re_mt4_shared` | 0.14 | 914,000 | Four threads running one compiled regex: glibc serialises on a lock in the shared `regex_t`, Liella takes none (`liella-historic:handoff/regex-t1.md`). `re_mt4_distinct` (separate regexes) is the fair single-engine comparison. |
| `snprintf_f_large` | 0.20 | 3,290 | Large-magnitude `%f` stops exact expansion once enough digits exist (`liella-historic:handoff/float-digits.md`). Small `%e`/`%f` rows are slower (section 4). |
| `iconv_open_conv_close_name`, `iconv_utf8_utf16_ascii_4k` | 0.26, 0.31 | 138; 2,219 | Open/close and ASCII-only 4 KiB conversion. Mixed (non-ASCII) conversion is the opposite: 1.7 to 2.6 (section 4). I did not find a recorded cause for the ASCII win. |
| `fnmatch_*` (13 of 15 rows) | 0.33 to 0.93 | 25 to 700 | A polynomial set-of-positions matcher with no backtracking (D-14.7, [plan](../plan/07-decisions/s14-searching-sorting-and-patterns.md)); casefold rows benefit from PJ-23 (`liella-historic:handoff/casefold.md`). Two fnmatch rows are slower: `fnmatch_names_class` 1.30 and `fnmatch_bracket` 1.12. |
| `newlocale_free` | 0.36 | 1,773 | Pre-decoded locale views (D-7.13) and an item-table layout (`liella-historic:handoff/lv3.md`). |
| `localeconv_en_us` | 0.45 | 5.8 | 1,880 to 62 instructions per call after LV-3 (`liella-historic:handoff/lv3-report.md`). |
| `strtod_en_us`, `_c`, `_utf8` | 0.56 to 0.58 | 56 to 58 | Recorded in `liella-historic:handoff/lv01.md` (glibc 130 ns against Liella 96 at that time). Cause of glibc's cost not probed. |
| `iswalpha_utf8`, `towupper_utf8`, `wcwidth_utf8` | 0.75 to 0.79 | 4 to 5 | Pre-decoded locale views, one table read (`liella-historic:handoff/lv2.md`). Margins are 20 to 25%, outside noise. |
| `regcomp_regfree_sed_kv`, `re_sed_swap`, `re_grep_ipv4` | 0.44, 0.66, 0.68 | 8,364; 2,319; 1,062 | Probably the regex program being derived once at compile and read in place (RX-T4, `liella-historic:handoff/regex-t4.md`) and the required-literal prefilter (RX-T1); I did not attribute these three rows individually. Many other regex rows are slower (section 4). |
| `cold_exec_empty` | 0.50 | 330,000 | Whole-process start of an empty program (a cold-path row; noisier). Cause not investigated here. |
| `regexec_pathological_4k` | ~0.00 | 8,356 | `(a|aa)*c` against 4,096 `a`s, no match. glibc takes about 51 ms (`bench/parity-measure.tsv`, Haswell-EP, 2026-10-07); Liella's matcher is automaton-based and linear-time (D-14.4), and the required-literal prefilter can reject an input with no `c` outright. One adversarial scenario: do not generalise it (the ordinary regex rows are mostly slower, section 4). |

Two comparisons worth stating plainly: the biggest ratios above are real but are mostly
cases where glibc does extra work Liella avoids (a lock, a file check, a full collation), or
Liella caches a result that glibc recomputes. They are legitimate wins for those call
patterns, not across-the-board speed.

## 4. Where glibc is faster

Largest gaps first, with Liella's absolute time so the size of the gap in nanoseconds is
visible. Haswell-EP ratios (`5cc49dc`).

| Row | Ratio | Liella ns | Known cause | Planned work |
|---|---:|---:|---|---|
| `strncasecmp_c_64`, `strcasecmp_{c,en,utf8}_{16,256}` | 9.3, 5.9 to 6.2 | 25 to 111 | Instruction count 5.3 to 7.0x glibc (`measure-1.md` finding 3). A SWAR (8 bytes at a time) ASCII fold was added in PJ-23, which took these rows from 13 to 29x down to 6 to 9x. | Closed since this snapshot: the AVX2 and SSE2 casecmp kernels were built (CASECMP-K1, CASECMP, STRCASECMP-SHORT, -2; D-4.27). Recount on 2026-10-23 (STRCASE-AVX2, Zen 1 host job `strcase-avx2-percall0`): cycles 0.93 to 0.94x glibc on the 256-byte rows, 1.29x on `strncasecmp_c_64`, 1.42x on `strcasecmp_c_16`; instructions 1.12 to 1.74x. What is left on the short rows is entry and stop cost. |
| `wordexp_*` (8 rows, two benchmark binaries) | 2.3 to 4.2 | 1,450 and up | About 4x glibc's instructions (`measure-1.md`); getenv and copy checks are 12 to 16% of the time (`liella-historic:handoff/regress.md`). | Small steps landed (PJ-8, D-X.15: 1 to 20%). No design for a larger change is recorded. |
| `re_grep_dotstar`, `regexec_submatch_1k`, `regcomp_regexec_regfree_email`, `regexec_backref`, `re_sed_kv` | 4.1, 3.2, 2.6, 1.9, 2.0 | 1,066; 115,036 | Rows that need capture groups or back-references take the general engine path. RX-CAP removed its heap allocations (143 per call on `re_sed_kv` -> 0 after the first call on a thread, `liella-historic:handoff/rx-cap.md`); what remains is the matcher itself: about 1-1.3k instructions per subject byte while states are live, and the capture pass is 70-75% of the capture rows (`liella-historic:handoff/rx-t2-plan.md`). | The lazy-DFA plan (`liella-historic:handoff/rx-t2-plan.md`): four tasks (groundwork, existence DFA, span passes, a cheaper capture pass). Its lower-bound estimate: rows without submatches can reach about 0.14-0.5x glibc; capture rows reach parity only if the capture pass also drops by 25-40%. Estimates, not measurements. |
| `iconv_utf8_utf32_mixed_4k`, `iconv_utf16_utf8_mixed`, `iconv_utf8_utf16_mixed_4k`, `iconv_8859_15_utf8_latin` | 1.6 to 2.6 | 17,559 for the largest | Instruction count 1.9 to 2.2x glibc (`measure-1.md`). Design pivots through UCS-4 per D-6.9. | None recorded beyond measurement. |
| `memmove_4k` | 2.3 (Broadwell-EP 2.1) | 121 | This row overlaps by 8 bytes and took the checked path, which the small-copy kernel did not cover (`liella-historic:handoff/mem-k1.md`: "checked path, untouched": 2.3x on Haswell-EP, 1.6x on Ivy Bridge). These ratios predate MEM-BULK and MEM-FALLBACK (2026-10-09), which send overlapping moves to the overlap-safe AVX2/SSE2 move kernels. | Done. SMALL-OPEN-ROWS (2026-10-23, job `small-open-rows-percall` on Broadwell-EP host) counted the instruction ratio at 0.98 against glibc on Broadwell E5-2699 v4. The time ratio was not re-run here. |
| `memcpy_64`, `memset_64`, `memcmp_64`, `memchr_64`, `strlen_64`, `strnlen_64`, `rawmemchr_64` | 1.1 to 1.7 | 2 to 5 | 64-byte class: dependent-load and per-call overhead dominates at these sizes. Larger sizes (`_64k`) are at parity on Haswell-EP (0.97 to 1.04; `memchr_64k` is 1.11 on Broadwell-EP). | MEM-K1 / K3 / K4 and STR-K1 are the work on exactly this class (in review). |
| `strcmp_equal_16`, `strcmp_names`, `strcmp_late_256`, `strcoll_en_us_names` | 2.2, 1.6, 1.6, 1.6 | 5 and up | One-pass compare landed (PJ-8: `strcmp_equal_16` 4.01 to 2.22). Still more instructions than glibc. | MEM-K3 small-compare kernel (in review). |
| `snprintf_e` | 2.3 | 367 | Float formatting path (`snprintf_f_en_us` 0.87 and `snprintf_g` 1.05 are fine; the gap is in the `%e` path); x86-64-v3 builds helped `snprintf_e` 3.28 to 2.68 in a measurement-only experiment (`measure-1.md` finding 4). | A multiversioned float core is flagged "worth a look"; not planned in detail. |
| `gettext_found`, `ngettext_found`, `gettext_language_*` | 1.9 to 2.1 | 143 | 2.3 to 4.2x glibc's instructions with IPC above 1: pure extra work, not stalls. Already reduced by GETTEXT-2. | None specified. |
| `mktime_new_york` | 1.8 | 282 to 303 | IPC-dominated: dependent loads and misses rather than instruction count (`measure-1.md`). | Not specified. |
| `strftime_*`, `mbrtowc_*`, `putc`, `fprintf_*`, `glob_1000_*` | 1.1 to 1.6 | 5 to 50 | Instruction counts 1.6 to 2.1x glibc (strftime, putc); see `measure-1.md`. | `mbrtowc_utf8_3` is 1.41 here; MB-K2 (in review) targets it. |

How the gap splits, from `liella-historic:handoff/measure-1.md` (Haswell-EP, static
pair, older tree than `5cc49dc`): of 85 rows above parity at that time, 72 were
instruction-count gaps (Liella executes more instructions) and 13 were IPC gaps (the same or
fewer instructions, but dependent loads, misses or mispredicts). Fixes for the first group
are "do less work"; for the second, "lay the data out differently". That analysis predates the
changes in section 5; counts have changed.

### 4b. Rows that differ between CPUs

| Row | Haswell-EP | Broadwell-EP | Note |
|---|---:|---:|---|
| `strncasecmp_c_64` | 9.34 | 7.89 | Both far above parity. |
| `strcasecmp_c_16`, `_c_256` | 6.19, 5.89 | 5.53, 5.61 | Same story. |
| `wordexp_arith`, `wordexp_tilde_var` | 4.21, 4.03 | 3.87, 3.68 | Same story. |
| `strcmp_equal_16` | 2.15 | 1.82 | Same side of parity. |
| `snprintf_d` | 1.10 | 1.03 | Near the parity band on Broadwell-EP. |
| `memset_64` | 1.15 | 1.00 | Small-size noise or microarchitecture; not investigated. |
| `strnlen_4k` | 1.23 | 1.03 | Not investigated. |
| `memchr_64k` | 1.01 | 1.11 | Opposite direction. Not investigated. |

None of these changes the picture. I did not find a recorded explanation for the small-size
rows and I have not run the probes to find one.

## 5. Work in review

The newest results are not in the tables above. `liella-historic:handoff/` and `plan/` record branches
that target the largest memory/string gaps: MEM-K1 (small copies, D-4.22), MEM-K3 (small
compares, D-4.23), STR-K1 (SSE2 terminator scans, D-4.24), MEM-K4, and MB-K2 (`mbrtowc`).
`origin/opt/mem-k4` and `origin/opt/mb-k2` exist as branches. I did not measure any of them
and quote no branch numbers. For a rough indication, `liella-historic:handoff/mem-k1.md` records on its
branch `memcpy_64` at 1.12 and `memset_64` at 1.06 against glibc on Haswell-EP, and 0.79 and 0.77
on a CPU without AVX2 (Ivy Bridge). I could not confirm which of these were already
in `5cc49dc`; compare against a fresh full bench of current `main` before relying on the
memory rows above.

Two small partial runs on Ivy Bridge (E3-1220L v2, no AVX2: it exercises the non-AVX2 fallback
paths) exist: `strk2-n1.log` and `ifunc-final-n.log`. They are branch benches of memory rows
only, so they are not quoted here beyond noting that the 64-byte memory rows differ there
(for example `memmove_4k` 1.63 on both Ivy Bridge runs against 2.30 on Haswell-EP).

## 6. How to reproduce

Needs the Rust toolchain from `rust-toolchain.toml`, a C compiler and the host glibc. Build
and run the whole bench (about 15 to 40 minutes depending on machine; Haswell-EP took 819 s for the
`5cc49dc` run):

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

## Appendix: all rows

Ratios (median Liella / glibc, `5cc49dc`), rounded to two decimals; "0.00" means below 0.005.
Sorted fastest first within each area. A row name is `benchmark/row`.

### Regex, glob, fnmatch, wordexp (43 rows)

| Row | Haswell-EP | Broadwell-EP |
|---|---:|---:|
| `cbench_patterns/regexec_pathological_4k` | 0.00 | 0.00 |
| `cbench_opt/re_mt4_shared` | 0.14 | 0.13 |
| `cbench_locale/fnmatch_en_us` | 0.33 | 0.30 |
| `cbench_opt/fnmatch_names_star_c` | 0.37 | 0.36 |
| `cbench_opt/fnmatch_names_lib_so` | 0.40 | 0.45 |
| `cbench_opt/fnmatch_names_ch` | 0.44 | 0.47 |
| `cbench_opt/regcomp_regfree_sed_kv` | 0.44 | 0.44 |
| `cbench_opt/fnmatch_paths_pathname` | 0.44 | 0.46 |
| `cbench_opt/fnmatch_paths_leading_dir` | 0.45 | 0.49 |
| `cbench_opt/fnmatch_names_question` | 0.46 | 0.51 |
| `cbench_opt/fnmatch_names_casefold` | 0.57 | 0.60 |
| `cbench_opt/re_sed_swap` | 0.66 | 0.66 |
| `cbench_opt/fnmatch_names_period` | 0.68 | 0.71 |
| `cbench_opt/re_grep_ipv4` | 0.68 | 0.66 |
| `cbench_patterns/regexec_literal_1k` | 0.72 | 0.72 |
| `cbench_patterns/fnmatch_pathname` | 0.80 | 0.70 |
| `cbench_opt/re_grep_alt` | 0.81 | 0.79 |
| `cbench_opt/re_mt4_distinct` | 0.81 | 0.81 |
| `cbench_opt/re_grep_comment` | 0.86 | 0.81 |
| `cbench_locale/fnmatch_casefold_en_32` | 0.87 | 0.91 |
| `cbench_opt/regcomp_regfree_ipv4` | 0.88 | 0.93 |
| `cbench_patterns/fnmatch_star` | 0.89 | 0.72 |
| `cbench_patterns/fnmatch_pathname_nomatch` | 0.93 | 0.78 |
| `cbench_patterns/fnmatch_bracket` | 1.12 | 1.06 |
| `cbench_opt/re_grep_literal` | 1.18 | 1.07 |
| `cbench_patterns/regexec_class_nomatch_1k` | 1.18 | 1.17 |
| `cbench_opt/re_grep_icase_absent` | 1.23 | 1.28 |
| `cbench_opt/fnmatch_names_class` | 1.30 | 1.36 |
| `cbench_patterns/glob_1000_sorted` | 1.39 | 1.29 |
| `cbench_patterns/glob_1000_nosort` | 1.59 | 1.46 |
| `cbench_patterns/regexec_backref` | 1.89 | 1.84 |
| `cbench_opt/re_sed_kv` | 2.02 | 1.94 |
| `cbench_opt/wordexp_plain_8` | 2.32 | 2.29 |
| `cbench_opt/wordexp_split_var` | 2.42 | 2.39 |
| `cbench_opt/wordexp_param_forms` | 2.58 | 2.68 |
| `cbench_patterns/regcomp_regexec_regfree_email` | 2.65 | 2.60 |
| `cbench_patterns/regexec_submatch_1k` | 3.19 | 3.10 |
| `cbench_opt/wordexp_quoted` | 3.34 | 3.24 |
| `cbench_opt/wordexp_tilde_var` | 3.64 | 3.53 |
| `cbench_opt/wordexp_arith` | 3.74 | 3.70 |
| `cbench_patterns/wordexp_tilde_var` | 4.03 | 3.68 |
| `cbench_opt/re_grep_dotstar` | 4.12 | 3.99 |
| `cbench_patterns/wordexp_arith` | 4.21 | 3.87 |

### iconv (11 rows)

| Row | Haswell-EP | Broadwell-EP |
|---|---:|---:|
| `cbench_opt/iconv_open_conv_close_name` | 0.26 | 0.25 |
| `cbench_opt/iconv_utf8_utf16_ascii_4k` | 0.31 | 0.30 |
| `cbench_opt/iconv_utf8_utf16_name` | 0.81 | 0.78 |
| `cbench_locale/iconv_utf8_ascii_translit_4k` | 1.14 | 1.08 |
| `cbench_opt/iconv_utf8_8859_15_latin_4k` | 1.42 | 1.41 |
| `cbench_locale/iconv_utf8_8859_15_4k` | 1.50 | 1.52 |
| `cbench_opt/iconv_8859_15_utf8_latin` | 1.58 | 1.60 |
| `cbench_locale/iconv_utf8_utf16_4k` | 1.71 | 1.74 |
| `cbench_opt/iconv_utf8_utf16_mixed_4k` | 2.11 | 2.12 |
| `cbench_opt/iconv_utf16_utf8_mixed` | 2.38 | 2.42 |
| `cbench_opt/iconv_utf8_utf32_mixed_4k` | 2.60 | 2.57 |

### Locale, ctype and wide characters (14 rows)

| Row | Haswell-EP | Broadwell-EP |
|---|---:|---:|
| `cbench_locale/newlocale_free` | 0.36 | 0.34 |
| `cbench_locale/localeconv_en_us` | 0.45 | 0.48 |
| `cbench_locale/strtod_utf8` | 0.56 | 0.52 |
| `cbench_locale/strtod_en_us` | 0.56 | 0.51 |
| `cbench_locale/strtod_c` | 0.57 | 0.52 |
| `cbench_locale/iswalpha_en_us` | 0.73 | 0.80 |
| `cbench_locale/iswalpha_utf8` | 0.75 | 0.84 |
| `cbench_locale/towupper_utf8` | 0.78 | 0.81 |
| `cbench_locale/wcwidth_utf8` | 0.79 | 0.80 |
| `cbench_locale/isalpha_c` | 1.00 | 0.99 |
| `cbench_locale/toupper_c` | 1.00 | 1.00 |
| `cbench_locale/mbrtowc_en_us_3` | 1.26 | 1.26 |
| `cbench_locale/nl_langinfo_en_us` | 1.31 | 1.22 |
| `cbench_locale/mbrtowc_utf8_3` | 1.41 | 1.49 |

### stdio and printf (20 rows)

| Row | Haswell-EP | Broadwell-EP |
|---|---:|---:|
| `cbench_opt/snprintf_f_large` | 0.20 | 0.21 |
| `cbench_locale/snprintf_f_en_us` | 0.87 | 0.83 |
| `cbench_opt/snprintf_ls_line` | 0.89 | 0.87 |
| `cbench_opt/snprintf_f2` | 0.92 | 0.92 |
| `cbench_opt/snprintf_ld_large` | 1.00 | 0.98 |
| `cbench_stdio/fwrite_4k` | 1.03 | 1.03 |
| `cbench_opt/snprintf_g` | 1.05 | 1.03 |
| `cbench_opt/snprintf_x08` | 1.08 | 1.02 |
| `cbench_opt/snprintf_d` | 1.10 | 1.02 |
| `cbench_opt/snprintf_s_d_colon` | 1.12 | 1.17 |
| `cbench_stdio/fputs_32` | 1.18 | 1.10 |
| `cbench_stdio/snprintf_d` | 1.19 | 1.21 |
| `cbench_stdio/putc` | 1.19 | 1.17 |
| `cbench_stdio/snprintf_mixed` | 1.36 | 1.39 |
| `cbench_opt/snprintf_s_path` | 1.37 | 1.36 |
| `cbench_opt/fprintf_ls_line` | 1.39 | 1.32 |
| `cbench_opt/snprintf_mixed` | 1.46 | 1.46 |
| `cbench_stdio/fprintf_d` | 1.54 | 1.54 |
| `cbench_opt/fprintf_du_line` | 1.56 | 1.50 |
| `cbench_opt/snprintf_e` | 2.27 | 2.37 |

### Case-insensitive compare, collation, gettext (19 rows)

| Row | Haswell-EP | Broadwell-EP |
|---|---:|---:|
| `cbench_opt/strcoll_en_us_equal_16` | 0.04 | 0.04 |
| `cbench_opt/gettext_c_locale` | 0.09 | 0.09 |
| `cbench_opt/gettext_missing` | 0.43 | 0.45 |
| `cbench_opt/gettext_no_catalog` | 0.52 | 0.54 |
| `cbench_opt/gettext_vary_msgid` | 0.77 | 0.73 |
| `cbench_opt/strcoll_en_us_utf8_16` | 1.15 | 1.02 |
| `cbench_opt/strcoll_c_utf8_names` | 1.22 | 1.17 |
| `cbench_opt/strcoll_c_names` | 1.23 | 1.16 |
| `cbench_locale/strcoll_utf8_16` | 1.24 | 1.27 |
| `cbench_opt/gettext_language_putenv_alone` | 1.45 | 1.64 |
| `cbench_opt/strcoll_en_us_names` | 1.63 | 1.62 |
| `cbench_opt/gettext_language_alternating` | 1.89 | 1.96 |
| `cbench_opt/gettext_found` | 2.08 | 2.26 |
| `cbench_opt/ngettext_found` | 2.12 | 2.16 |
| `cbench_locale/strcasecmp_c_256` | 5.89 | 5.61 |
| `cbench_locale/strcasecmp_c_16` | 6.18 | 5.53 |
| `cbench_locale/strcasecmp_utf8_256` | 6.18 | 5.99 |
| `cbench_locale/strcasecmp_en_256` | 6.19 | 6.01 |
| `cbench_locale/strncasecmp_c_64` | 9.34 | 7.89 |

### Time (22 rows)

| Row | Haswell-EP | Broadwell-EP |
|---|---:|---:|
| `cbench_time/localtime_r_new_york_2100` | 0.10 | 0.10 |
| `cbench_time/localtime_r_new_york` | 0.11 | 0.11 |
| `cbench_opt/localtime_r_new_york` | 0.13 | 0.13 |
| `cbench_opt/mktime_utc` | 0.91 | 0.87 |
| `cbench_time/timegm` | 0.91 | 0.88 |
| `cbench_time/gmtime_r` | 1.02 | 0.92 |
| `cbench_opt/strftime_c_en_us` | 1.09 | 1.01 |
| `cbench_time/localtime_r_posix_rule` | 1.10 | 1.11 |
| `cbench_opt/localtime_r_posix_rule` | 1.12 | 1.07 |
| `cbench_opt/strftime_iso8601_tz` | 1.16 | 1.11 |
| `cbench_time/localtime_r_utc` | 1.17 | 1.13 |
| `cbench_time/strftime_iso` | 1.21 | 1.20 |
| `cbench_opt/strftime_log` | 1.22 | 1.11 |
| `cbench_opt/strftime_date_en_us` | 1.23 | 1.20 |
| `cbench_opt/strftime_ls_iso` | 1.23 | 1.21 |
| `cbench_opt/strftime_ls_recent_en_us` | 1.25 | 1.25 |
| `cbench_opt/strftime_ls_recent_c` | 1.26 | 1.19 |
| `cbench_time/strftime_c` | 1.26 | 1.21 |
| `cbench_opt/mktime_posix_rule` | 1.47 | 1.39 |
| `cbench_locale/strftime_names_en_us` | 1.49 | 1.40 |
| `cbench_time/mktime_new_york` | 1.78 | 1.69 |
| `cbench_opt/mktime_new_york` | 1.86 | 1.70 |

### Memory and string scans (31 rows)

| Row | Haswell-EP | Broadwell-EP |
|---|---:|---:|
| `cbench_memref/memcpy_4k_off` | 0.78 | 0.59 |
| `cbench_memref/memset_16` | 0.81 | 0.93 |
| `cbench_memref/memcpy_16` | 0.83 | 0.74 |
| `cbench_memref/memcmp_4k` | 0.90 | 0.85 |
| `cbench_memref/memmove_64` | 0.90 | 0.80 |
| `cbench_memref/memcmp_64k` | 0.97 | 0.91 |
| `cbench_memref/memset_4k_off` | 0.99 | 0.99 |
| `cbench_memref/memmove_16` | 1.00 | 1.05 |
| `cbench_memref/memset_64k` | 1.00 | 1.01 |
| `cbench_memref/memchr_64k` | 1.01 | 1.11 |
| `cbench_memref/memcpy_64k` | 1.01 | 1.02 |
| `cbench_memref/strlen_64k` | 1.03 | 1.00 |
| `cbench_memref/rawmemchr_64k` | 1.04 | 1.01 |
| `cbench_memref/strnlen_64k` | 1.05 | 1.03 |
| `cbench_memref/memcpy_4k` | 1.10 | 1.16 |
| `cbench_memref/memset_4k` | 1.11 | 1.09 |
| `cbench_memref/memset_64` | 1.15 | 1.00 |
| `cbench_memref/memcpy_64` | 1.19 | 1.20 |
| `cbench_memref/memchr_4k` | 1.20 | 1.20 |
| `cbench_memref/strnlen_4k` | 1.23 | 1.03 |
| `cbench_memref/rawmemchr_4k` | 1.34 | 1.34 |
| `cbench_memref/memcmp_64` | 1.37 | 1.26 |
| `cbench_memref/memchr_64` | 1.43 | 1.33 |
| `cbench_memref/strlen_4k` | 1.44 | 1.42 |
| `cbench_opt/strcmp_names` | 1.55 | 1.51 |
| `cbench_memref/strnlen_64` | 1.62 | 1.50 |
| `cbench_opt/strcmp_late_256` | 1.64 | 1.69 |
| `cbench_memref/rawmemchr_64` | 1.65 | 1.58 |
| `cbench_memref/strlen_64` | 1.70 | 1.54 |
| `cbench_opt/strcmp_equal_16` | 2.15 | 1.82 |
| `cbench_memref/memmove_4k` | 2.30 | 2.14 |

### Other (1 rows)

| Row | Haswell-EP | Broadwell-EP |
|---|---:|---:|
| `cbench_opt/cold_exec_empty` | 0.50 | 0.49 |

The processor classes named on this page are listed in [Tested processors](../CONTRIBUTING.md#tested-processors).
