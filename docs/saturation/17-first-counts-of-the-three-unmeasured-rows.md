## 17. First counts of the three unmeasured rows (BENCH-ROWS-UNMEASURED, 2026-10-24)

New C bench `bench/c/cbench_unmeasured.c` (rows `qsort_c_cmp_1024`, `readdir_entry_flat`, `fts_entry_flat`), counted with `cargo xtask bench --counters-only --only cbench_unmeasured` on Skylake-SP host (Intel Xeon Platinum 8151; the dispatcher gave this run a Skylake-SP host role, not Zen 1 host), median of 3 counted runs per side, glibc = the host dynamic build of the same file. "All" counts user and kernel instructions per operation (the number the limits in `bench/instr_limits.tsv` gate); "user" counts user mode only (from the `ctr` lines of the same program run directly, 3 runs each, spread under 0.1%). Nothing was timed.

| row (per) | Liella all | glibc all | Liella user | glibc user | user ratio |
|---|---|---|---|---|---|
| `qsort` C comparator, 1,024 ints (per comparison; 10,000-odd comparisons per sort, the same count for both) | 47.83 | 31.71 | 47.85 | 31.68 | 1.51 |
| `readdir` per entry, flat 1,000-file directory (rewinddir then to the end) | 1611 | 1604 | 54.3 | 46.1 | 1.18 |
| `fts` per entry, `FTS_PHYSICAL\|FTS_NOCHDIR`, flat 1,000-file directory | 6830 | 7891 | 1490 | 792 | 1.88 |

What this settles and what it does not:

* `qsort`: Liella is 47.8 instructions per comparison; glibc is 31.7 in this program. The register's "about 22" for glibc was a number from a note and is **not reproduced**: here glibc is 31.7, and that figure includes the plain C comparator (about 7 instructions), the 4 KiB copy per sort (about 130 instructions, under 0.02 per comparison) and the loop. The gap is 16 instructions per comparison (1.51x). Whether comparing by offset in the boundary would close it is untested; the design question stays open.
* `readdir`: the user-mode ratio re-counts at 1.18 (the old 1.17 stands, on a different tree: 54.3 vs 46.1 per entry, against 75.9 vs 64.8 in the old note). In total instructions the two are equal (1611 vs 1604) because the kernel's `getdents64` work dominates, as the register said.
* `fts`: user-mode instructions per entry are 1.88x glibc's (1490 vs 792) on this flat walk with `FTS_NOCHDIR`; total instructions are lower than glibc's (6830 vs 7891) because Liella's run spends fewer kernel instructions. This differs from the "about 1.2x" of the FTS-SHARED-PATH note, which measured a different shape (`FTS_PHYSICAL` over the 1,000-entry tree with its 10 subdirectories, no `FTS_NOCHDIR`); the two are not comparable and the gap is not attributed here. It is a candidate for a story, not a conclusion.
* Not measured: wall time (no booking), the `fts` row at a 2 KiB path, `qsort` with other element sizes or on a CPU other than the Xeon 8151, Zen 1 host's count.

Bench-only change: no code was touched, so no plants apply. The three rows have `max_ns` sanity rows and report-only `ratio_host` rows in `bench/budgets.tsv` (the counters table needs a `ratio_host` row to count both sides) and instruction limits at the count plus about 2%.

Follow-up (PERF-READDIR-ENTRY, 2026-11-04, Skylake-SP host, user-mode instructions per entry from the `ctr` lines, 3 runs per side, spread 0): `readdir_entry_flat` 50.29 -> 46.32 against glibc's 46.12 (1.004). Two changes: the hit path asks `lock_alone` (one relaxed load) instead of `alone`, and `readdir`/`readdir64` return the record address from a frameless hit path with every miss in one cold call that also sets `errno` (no stack frame or hoisted error constant per entry). The `getdents64` record walk and its bounds checks are unchanged. `fts_entry_flat` (1493 vs 794 user) and the walk rows' totals did not move; the fts gap is not in readdir. Left on the 41-instruction hit path: the handle's null, alignment and magic tests, the `Option<DirStream>` test, and a `start <= end` compare in the record slice that cannot fail once `pos < len` (a record-walk change, not tried).
