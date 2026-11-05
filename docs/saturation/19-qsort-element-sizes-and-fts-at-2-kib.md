## 19. qsort at five element sizes and fts under a 2 KiB path (BENCH-UNMEASURED-WIDEN, 2026-11-04)

Section 17 listed these as "Not measured". `bench/c/cbench_unmeasured.c` gains `qsort_c_cmp_1024_e4`, `_e8`, `_e16`, `_e24` and `_e64` (1,024 elements of 4, 8, 16, 24 and 64 bytes: a 4-byte key and padding that is never compared; the same plain comparator on both sides; the master array is copied each sort, as in section 17) and `fts_entry_deep2k` (`fts` `FTS_PHYSICAL|FTS_NOCHDIR` over a 1,000-file directory whose base path is 10 nested directories of 200 characters, about 2.0 KiB; per entry, 1,002 entries). Each has a `max_ns` sanity row and a report-only `ratio_host` row in `bench/budgets.tsv`. (This file is numbered 19 because section 18 was already taken.)

Counted with `cargo xtask bench --counters-only --only cbench_unmeasured` on an AMD EPYC 7551 (Zen 1) host, median of 3 counted runs per side. This is the Zen 1 host: section 17's counts were on a Skylake-SP host, so the `qsort_c_cmp_1024` and flat rows below are also the second-CPU re-count.

| row (per) | Liella instr | glibc instr | instr ratio | Liella cycles | glibc cycles | cycle ratio |
|---|---|---|---|---|---|---|
| `qsort` 1,024 ints, per comparison (the section 17 row, re-counted) | 47.9 | 31.7 | 1.51 | 24.2 | 21.8 | 1.11 |
| `qsort` 4-byte elements | 47.9 | 31.8 | 1.51 | 23.9 | 21.9 | 1.09 |
| `qsort` 8-byte elements | 47.7 | 32.4 | 1.47 | 23.6 | 22.6 | 1.05 |
| `qsort` 16-byte elements | 47.8 | 54.3 | 0.88 | 23.7 | 36.6 | 0.65 |
| `qsort` 24-byte elements | 48.4 | 55.4 | 0.87 | 25.5 | 36.2 | 0.71 |
| `qsort` 64-byte elements | 50.0 | 37.8 | 1.32 | 26.6 | 28.3 | 0.94 |
| `fts` per entry, 2 KiB base path | 5838 | 21604 | 0.27 | 3970 | 8785 | 0.45 |
| `fts` per entry, flat (base path short; re-count) | 5781 | 6894 | 0.84 | 3940 | 4341 | 0.91 |
| `readdir` per entry, flat (re-count) | 487 | 479 | 1.02 | 388 | 441 | 0.88 |

Comparisons per sort (the divisor, counted once per side with a counting comparator): Liella 9,316 (ints), 9,269 / 9,251 / 9,226 / 9,293 / 9,239 (4 / 8 / 16 / 24 / 64 bytes); glibc 8,958, 8,937 / 8,971 / 8,911 / 8,932 / 8,970. The two sides do not make the same number of comparisons (Liella about 3.5% more); each row divides by its own side's count, so the per-comparison figures are per comparison, not per sort.

What this settles and what it does not:

* The counted figure here is all instructions (user and kernel together); `--counters-only` does not print a user-mode split. `qsort` makes no system calls, so for those rows all is user, as section 17's 47.83 / 47.85 showed; for the `fts` and `readdir` rows the kernel share is large and the user ratio is **not** measured here.
* `qsort`: Liella's cost per comparison is flat across element sizes (47.7 to 50.0 instructions). glibc is 31.7 to 32.4 at 4 and 8 bytes, 37.8 at 64, and 54 to 55 at 16 and 24 bytes, so Liella is behind (1.3 to 1.5x in instructions) at 4, 8 and 64 bytes and ahead at 16 and 24. The section 17 row (47.9 vs 31.7) did not change after QSORT-C-COMPARATOR and QSORT-MERGE-INDEX: the count is the same as before the change.
* In cycles Liella is at most 1.11 of glibc (ints) and below glibc at 16, 24 and 64 bytes; the instruction gap at the small sizes is mostly hidden by IPC on this CPU.
* `fts` at a 2 KiB path: total instructions per entry are 0.27 of glibc's (5.8 thousand against 21.6 thousand), and cycles 0.45. Liella does not grow with the path length the way glibc's per-entry cost does (glibc: 6.9 thousand flat, 21.6 thousand at 2 KiB; Liella: 5.8 thousand either way). The `fts` user-mode ratio at either path length was not split out.
* Not measured: wall time (no booking); user-mode instruction splits; other element-size mixes (non-power-of-two keys at offsets other than 0, 128 bytes and up); `qsort_r`; other CPUs.

Proposed PERF story lines (rows whose ratio is above 1.10; `qsort` is user mode throughout, so the instruction ratio is the user ratio):

* PERF-QSORT-SMALL-ELEMENTS: `qsort` with 4- and 8-byte elements costs 1.5x glibc's instructions per comparison (47.9 vs 31.8 and 47.7 vs 32.4); the comparator itself is about 7 of those instructions on both sides, so about 15 extra instructions per comparison are in the sort. Where they go was not profiled here. Counted cycles are only 1.05 to 1.09 of glibc's on this CPU.
* PERF-QSORT-64B: `qsort` with 64-byte elements is 1.32x glibc's instructions (50.0 vs 37.8), though 0.94 in cycles.
* The 16- and 24-byte rows (0.88, 0.87) are the cases to keep when the above are fixed.

Bench-only change: no library code was touched, so no plants apply.

### Update: address-based positions in the index sort (PERF-QSORT-OFFSET-EXP, 2026-11-05, Zen 1 host)

Where the surplus was (disassembly of the merge loop, `merge_sort_index` with the C comparator): 31 instructions per
merge comparison besides the comparator. 11 values stay live across the indirect call and there are 6 callee-saved
registers, so 7 of the 31 are spill stores or reloads; two `lea`s add the base to each offset; bounds checks are 2 to
4 instructions. The surplus is register pressure across the call, not safety checks (the "8 check instructions and
the slice building" in section 2's row predate QSORT-COMPARE-TRAIT, which already compares by offset).

Change (adopted, owner ruling #3097): while merging, the position table holds element addresses (the base's exposed
provenance plus the offset, safe Rust; `SortCompare::gt_at`, which the boundary's C-comparator adapters override with
`with_exposed_provenance`, no new `unsafe`), turned back into offsets before the permute pass; the merge's output
index is `i + j - mid` instead of a third counter. Two values fewer stay live across the call.

Counted with `validation/wl_percall.sh` on `validation/percall/qsort.c` (`instructions:u` per sort, deterministic,
sd 0); the comparison counts are the same before and after (Liella 1,830 at 256 elements and 9,316 at 1,024; glibc
1,720 and 8,958), so the per-comparison columns divide by each side's own count:

| Elements | before | after | change | glibc | before / cmp | after / cmp | glibc / cmp | after / glibc per cmp |
|---|---|---|---|---|---|---|---|---|
| 4-byte, 256 | 90823 | 83044 | -8.6% | 59665 | 49.6 | 45.4 | 34.7 | 1.31 |
| 64-byte, 256 | 95535 | 87756 | -8.1% | 73273 | 52.2 | 48.0 | 42.6 | 1.13 |
| 4-byte, 1,024 | 442291 | 398712 | -9.9% | 284113 | 47.5 | 42.8 | 31.7 | 1.35 |
| 8-byte, 1,024 | 440583 | 397004 | -9.9% | 290558 | 47.3 | 42.6 | 32.4 | 1.31 |
| 64-byte, 1,024 | 460975 | 417396 | -9.5% | 337983 | 49.5 | 44.8 | 37.7 | 1.19 |

The comparator-only control row is unchanged (186910 on all three columns), and `wl_count sort-utf8` gives the same
output with an unchanged count. About 4.5 instructions per comparison saved; the rows stay behind glibc in
instructions. Not measured here: wall time (the next timing sweep judges the rows; revert if slower on Ivy Bridge);
the comparison-count gap (about 4% more comparisons than glibc) is a story of its own. Not tried: a ping-pong merge
over two disjoint buffers (removes the output bounds check, adds a copy on in-order merges).
