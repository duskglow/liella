# BENCH-UNMEASURED-WIDEN
Branch task/bench-unmeasured-widen, head 17c3d4ff (host keke role r3, AMD EPYC 7551 Zen 1).
Done: bench rows (qsort e4/e8/e16/e24/e64, fts_entry_deep2k), budgets rows, docs/saturation/19-*.md plus index line.
Jobs: bench-unmeasured-widen-count (rc 0, counts in section 19), BENCH-UNMEASURED-WIDEN-static (clippy -D warnings, then xtask static; rc 0).
Not done: user-mode split (counters-only lacks it). qsort gap unprofiled. No plants (bench only).
Next: PERF-QSORT-SMALL-ELEMENTS, PERF-QSORT-64B.
