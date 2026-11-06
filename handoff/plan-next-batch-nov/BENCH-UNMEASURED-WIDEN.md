# BENCH-UNMEASURED-WIDEN: count the unmeasured cases section 17 left open (qsort sizes, fts at 2 KiB)

Id: BENCH-UNMEASURED-WIDEN
Kind: bench
Priority: 50
Size: S
Host: keke
Stacks-on: -
Model: sonnet
Effort: medium
BUDGET: 35 tool calls / 80k

## The change

Gap: `docs/saturation/17-first-counts-of-the-three-unmeasured-rows.md` lists what is "Not measured": `fts` at a
2 KiB path, `qsort` with other element sizes, and the counts on a second CPU. Its qsort row (47.8 vs 31.7 instructions
per comparison, user ratio 1.51) predates QSORT-C-COMPARATOR and QSORT-MERGE-INDEX (both landed): re-count it first.
Scope: add rows to `bench/c/cbench_unmeasured.c`: `qsort` per comparison at element sizes 4, 8, 16, 24 and 64 bytes
(1,024 elements, the same comparator work on both sides, comparison count printed for both), and `fts` per entry under
a 2 KiB base path. Matching `max_ns` and report-only `ratio_host` rows in `bench/budgets.tsv`. Count with
`cargo xtask bench --counters-only --only cbench_unmeasured` on the counter host. Write the results as a new section
file `docs/saturation/18-*.md` plus its index line (do not edit section 17), naming the processor class only, never a
lab host. Any row at a user ratio above 1.10 gets a proposed PERF story line in that section.
Files: the bench source, `bench/budgets.tsv`, the new register section. Bench only: no library code, no plants.
JOBS: bench-unmeasured-widen-count, BENCH-UNMEASURED-WIDEN-static
