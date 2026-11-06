# TIMING-COVERAGE-1106: every row slower in time mapped to a story; briefs for the uncovered groups

First line: COMPLETE: (plan written; nothing implemented)

Source: TIMING-SWEEP-1105 table (results/timing-sweep-1105-table.md on task/timing-sweep-1105-2, sweep build next
f2157504). A row is listed when its time ratio is above 1.05 on either CPU class: 98 of 414 rows. Full table, one row per
line with instr / time ratios and status: [table.md](table.md).

Status counts (98 rows): open story 33, open on one class + new brief on the other 4, re-time 13, NONE 30 (all briefed below
or ranked 7-9), wordexp deferred 9 (OWNER-DEFERRED), cold-start out of scope 9 (COLDSTART-PLAN-1106). No nscache or libm
row is above 1.05x in time in the sweep table.

Coverage sources: `ops` story states (ops.db, read-only, 2026-11-06 01:20Z; open = ready / running / verifying /
queued_ci); stories landed after f2157504 (git log --first-parent f2157504..origin/next, their handoff/reports/*.json).
A red story with no open continuation counts as not covering.

## Re-time (landed after the sweep; the next sweep re-times them, not done here)
PARITY-COPY-AVX2-3 (memcpy_144/256 N100), PERF-WCTYPE-ENUS-2 (5 iswalpha/iswctype/towupper rows), PERF-GETTEXT-LANGUAGE-
STEADY-5 (2 gettext rows), PERF-NFTW-ENTRY-12 (fts_physical_1000), PERF-QSORT-OFFSET-EXP-2 (3 qsort rows).

## Uncovered rows: groups ranked by gap x how common the call is
Frequency: exact dynamic call counts of the glibc kit over the 30 workloads (keke ~/wl/entries/l/table.tsv, WL-ENTRIES):
memcmp 46.8M calls in 13 workloads, strcmp 12.8M / 25, memchr 6.7M / 12, realloc 3.6M / 25, getdelim 2.0M / 2,
memrchr 0.59M / 5, strstr 1,963 / 3; bcmp, __memcmpeq, strcasecmp, strncasecmp, regexec, fork, posix_spawn: 0 or 1
(programs reach them through memcmp, their own code, or not at all in this set).

| Rank | Brief | Rows (class, time) | Liella / glibc instr per call on keke (Zen 1), this run | Stacks on |
|---|---|---|---|---|
| 1 | [TIMING-COV-MEMCMP-NONAVX2](TIMING-COV-MEMCMP-NONAVX2.md) | 18 memcmp / bcmp / __memcmpeq rows, IVB 1.06-1.92 | non-AVX2 path: diff8_96 45/26, bcmp_head_128 42/26, meq_head_1k 42/22, bcmp_eq_64 57/42, memcmp_256 118/107 | PARITY-MEMCMP-SMALL-EQ |
| 2 | [TIMING-COV-MEMCHR-SHORT-NONAVX2](TIMING-COV-MEMCHR-SHORT-NONAVX2.md) | memchr_16 1.58, memchr_16_mix 1.17, memrchr_64_hit 1.28 (IVB) | memchr_16 50/31, memrchr_64_hit 33/30 | TIMING-N100-SCAN-64K |
| 3 | [TIMING-COV-STRCMP-SHORT-AVX2](TIMING-COV-STRCMP-SHORT-AVX2.md) | strcmp_names 1.14, strcasecmp_c_16 1.14, strncasecmp_c_64 1.10 (N100) | strcmp_names 46.4/38.0, strcasecmp_c_16 58/53, strncasecmp_c_64 80/71 | TIMING-IVB-STRSCAN-DISPATCH-2 |
| 4 | [TIMING-COV-REALLOC-GROW-NONAVX2](TIMING-COV-REALLOC-GROW-NONAVX2.md) | realloc_grow_x2 IVB 1.09 | 287.8/199.5 per realloc (non-AVX2), 252.9/199.5 (AVX2) | TIMING-IVB-MALLOC-CHAIN-2 |
| 5 | [TIMING-COV-GETLINE-LONG](TIMING-COV-GETLINE-LONG.md) | getline_long IVB 1.08 | gl_long 5170.6/4251.8 | - |
| 6 | [TIMING-COV-STRSTR-NONAVX2](TIMING-COV-STRSTR-NONAVX2.md) | strstr_hit_64 2.40, strstr_miss_64 1.41, strstr_hit_4k 1.28 (IVB) | non-AVX2: hit_64 220/204, miss_64 245/409, hit_4k 2299/1506 | TIMING-N100-STRSTR-64 |
| 7 | not briefed | regexec_backref IVB 1.18 | REGEX-BACKREF-FIXED (red, unlanded) had 1.11 -> 1.04 instr on its branch: coordinator to decide whether to revive it | - |
| 8 | not briefed | regexec_class_nomatch_1k IVB 1.15 (instr 0.95, time only) | - | - |
| 9 | not briefed | fork_exit_wait IVB 1.25 / N100 1.29; posix_spawn_true_heap256m IVB 1.49 (cycles 1.05: kernel time) | not in the workload entry table | - |

## Method (job keke:timing-coverage-1106-percall, role keke:~/w/n4 = next 730f416b, clean)
validation/wl_percall.sh with the spec [tc1106.c](tc1106.c) (shapes copied from the bench rows of the same names) plus
validation/percall/strstr_getline.c (gl_long) and strcmp.c (strcmp_names); each spec once with the default dispatch
(AVX2 class) and once with `GLIBC_TUNABLES=glibc.cpu.hwcaps=-AVX2` (Liella's non-AVX2 builds, the path Ivy Bridge runs).
Liella single-call traces (wl_percall --trace, gdb single-step of Liella only) for realloc, getdelim, strstr, memchr.
glibc side: aggregate instruction counts only. Raw output: [percall-keke-730f416b.txt](percall-keke-730f416b.txt).
Keke is Zen 1: instruction counts per path are deterministic; cycles there say nothing about Ivy Bridge or the N100, so no
time was measured. Cycle samples by symbol were not taken: each case is one kernel (the traces give the split), and perf
sampling on the keke VM loses most samples (wl_percall.sh header).

Caveats: (1) the bcmp / __memcmpeq cases print MISMATCH because tc1106.c checks the sign; both libraries return non-zero,
which is all the interface promises (the probe's check is wrong, not the libraries); fix the check to `r != 0` before
reuse. (2) The non-AVX2 probe matches the IVB sweep instr ratios for memcmp / bcmp / memchr (1.36-1.91 vs 1.34-1.85) but
NOT for strstr (hit_64 1.08 vs 2.17 in the sweep) or realloc_grow_x2 (1.44 vs 1.11): those briefs start by reconciling.
(3) The host for the N100 timing check is taken from the hosts table (4-core host nozomi): NOT VERIFIED; check with ops.
