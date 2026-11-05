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


### Update: comparison counts per input pattern (PERF-QSORT-COMPARISONS, 2026-11-05, Zen 1 host): NEGATIVE

Comparator calls per `qsort`, Liella / glibc 2.39, 8-byte elements (an `int` key and the element's original index,
so the probe also checks stability; Liella was stable in every cell). Probe `keke:~/work/perf-qsort-comparisons/qcount.c`
(static binaries; the keys come from the same LCG as `validation/percall/qsort.c`, so the random 256 and 1,024 cells
are the percall cases' 1,830 / 1,720 and 9,316 / 8,958):

| pattern | 16 | 100 | 256 | 1000 | 1024 | 4096 | 10000 |
|---|---|---|---|---|---|---|---|
| random | 53 / 43 | 574 / 540 | 1830 / 1720 | 9035 / 8702 | 9316 / 8958 | 45298 / 43931 | 123256 / 120531 |
| sorted | 15 / 32 | 99 / 316 | 255 / 1024 | 999 / 4932 | 1023 / 5120 | 4095 / 24576 | 9999 / 64608 |
| reversed | 15 / 32 | 99 / 356 | 255 / 1024 | 999 / 5044 | 1023 / 5120 | 4095 / 24576 | 9999 / 69008 |
| few distinct (4 keys) | 48 / 44 | 513 / 506 | 1664 / 1628 | 8108 / 7976 | 8325 / 8198 | 40471 / 39960 | 110971 / 108777 |
| many equal (90% one key) | 25 / 36 | 446 / 479 | 1518 / 1612 | 7733 / 8160 | 8007 / 8428 | 39942 / 41817 | 108674 / 113136 |
| organ pipe | 29 / 38 | 197 / 384 | 509 / 1150 | 1997 / 5486 | 2045 / 5630 | 8189 / 26622 | 19997 / 71806 |
| sawtooth (period 32) | 15 / 32 | 400 / 447 | 1011 / 1396 | 7242 / 7816 | 6063 / 7600 | 32319 / 38464 | 104536 / 109687 |
| blocks of 16, random inside | 44 / 39 | 437 / 417 | 826 / 1229 | 6013 / 5988 | 3279 / 5974 | 13019 / 28016 | 76689 / 76796 |
| sorted, 1% swapped | 31 / 37 | 356 / 435 | 699 / 1172 | 4798 / 6499 | 4988 / 6871 | 28592 / 35708 | 81681 / 97213 |
| all equal | 15 / 32 | 99 / 316 | 255 / 1024 | 999 / 4932 | 1023 / 5120 | 4095 / 24576 | 9999 / 64608 |

Where Liella's surplus on random input comes from (1,024 elements: +358 comparisons, +4.0%). A model of
`merge_sort_index` / `insertion_index` that reproduces every Liella cell above exactly splits it in two:

* About 236: each run of 8 compares an element with its predecessor before the binary search. On random input that
  test splits the element's i + 1 possible places into 1 against i, and the search that follows is no longer
  balanced: about 17.3 comparisons per run of 8 instead of 15.6 for a balanced binary insertion.
* 127: the one-comparison "runs already in order" test before each of the 127 merges, which random input always
  fails.

Without both, the model gives 8,953, which is glibc's 8,958 and the textbook average of a top-down merge sort
(about n log2 n - 1.25 n). The same two tests are what make sorted, reversed and all-equal input cost n - 1
(glibc: about n/2 log2 n) and organ pipe, sawtooth and 1%-swapped input up to 3.6 times cheaper than glibc (the
blocks pattern too at 256, 1,024 and 4,096 elements; at 1,000 and 16 it is slightly dearer). No comparison in the
current sort is redundant (none has a result implied by earlier ones), so any saving on random input reorders the
decisions and costs on some other input. Tried in the model (150 cells: the ten patterns above plus random from a
second generator, half sorted then random, random then sorted, sorted with jitter i + rand(8), keys in 0..n/4,
sizes 9 to 10,000), random 256 / 1,024 against 1,830 / 9,316, and the worst rises:

| Candidate | random 256 / 1,024 | rises (cells, worst) |
|---|---|---|
| predecessor test only after an insertion at the end | 1,772 / 9,135 | 20: few distinct +1, 1% swapped +12, jittered +14 |
| predecessor test while end insertions are the majority in the run | 1,774 / 9,140 | 14: jittered +481 at 4,096 |
| predecessor test unless the last two insertions were not at the end | 1,778 / 9,172 | 13: random +3 at 33, jittered +234 |
| predecessor test by a tally kept across runs (1:1 / 2:1) | 1,763 / 9,082; 1,764 / 9,083 | 21 / 15: random then sorted +444; few distinct +5 |
| balanced binary insertion, no predecessor test | 1,761 / 9,080 | 94: sorted 1,791 at 1,024 (from 1,023) |
| in-order test only when a half was ordered or descending | 1,799 / 9,189 | blocks of 16: 6,224 at 1,024 (from 3,279) |
| in-order test with exponential back-off after failures | 1,804 / 9,196 | 1% swapped: 6,292 at 1,024 (from 4,988) |
| runs of 16 | 1,867 / 9,440 | random itself rises |
| runs of 5 to 7 (leaves of 4 at powers of two) | 1,795 / 9,265 | few distinct +468 at 4,096 |
| runs of 10 or 12 | 1,830 / 9,316 | random +388 at 10,000 |

Verdict: NEGATIVE. No candidate lowers random input without raising another pattern, so the strategy stays as
D-14.18 describes it (stable, as D-14.1 requires; every candidate above was stable too). The best trade found
(predecessor test only after an insertion at the end: random -1.9% at 1,024, rises of up to 14 comparisons in 20 cells) was not taken:
it would mean accepting rises, and the qsort rows' remaining gap is mostly instructions per comparison
(1.13 to 1.35x, above), not the count.
