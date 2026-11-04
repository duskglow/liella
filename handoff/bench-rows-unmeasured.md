# BENCH-ROWS-UNMEASURED (handoff)

State: branch task/bench-rows-unmeasured. Bench-only: `bench/c/cbench_unmeasured.c`, rows in `bench/budgets.tsv` (max_ns + report-only ratio_host), limits in `bench/instr_limits.tsv` (Xeon 8151 only), register section 17 (renumbered on next).
Counts were on sumire (the dispatcher's role), not keke; keke's CPU has no limit rows yet.
Results (all-mode instr per op, Liella vs glibc): qsort 47.83 vs 31.71 per comparison; readdir 1611 vs 1604 (user 54.3 vs 46.1); fts 6830 vs 7891 (user 1490 vs 792, 1.88x).
The register's "about 22" glibc instr per comparison is not reproduced (31.7 here, incl. comparator and copy).
Not to redo: the counts. No plants: no code changed.
Next step (a story, not done): why fts user instructions are 1.88x glibc on a flat FTS_NOCHDIR walk; qsort 16 instr/comparison gap.
