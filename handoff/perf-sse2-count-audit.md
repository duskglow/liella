# PERF-SSE2-COUNT-AUDIT (handoff, after run 2: PERF-SSE2-COUNT-AUDIT-2)

State: branch task/perf-sse2-count-audit-2 (stacks on task/perf-sse2-count-audit). Planning only, no code. COMPLETE.
Full report and summary table: handoff/reports/PERF-SSE2-COUNT-AUDIT.md.
Pending record: records/pending/PERF-SSE2-COUNT-AUDIT-1.toml (D-NEW, series D-10, amends D-10.64/D-10.65; needs an id).
Proposed briefs: handoff/PERF-SSE2-COUNT-AUDIT-2/LIBM-F64-INVHYP-SSE2-PROFILE.md (D-10.65 SSE2 gap, #5057),
handoff/PERF-SSE2-COUNT-AUDIT-2/LIBM-F64-EXP2-SSE2-DEKKER-AB.md (D-10.64 trade never measured).

Result: no decision or saturation record cites an SSE2 figure from a dropped-GLIBC_TUNABLES run, and no conclusion
changes. D-10.66 was counted through `--tunables`. D-10.64 and D-10.65 had no SSE2 figure; the run 1 recount (shiki)
supplied one. SR-25's "43.4 to 45.4" was measured 2026-10-11 (perf-ledger SSE2-GAP rows), before BENCH-FIXED-ENV,
so outside the window. The "2026-10-23" in the row dates only its `now` column.

Run 2 jobs (keke:~/w/n2): perf-sse2-audit-2-lint (clippy+static on the inherited head, rc 0),
perf-sse2-audit-2-sr25 (cbench_memref strlen_64/strnlen_64/rawmemchr_64 at default, -AVX2 and -FMA, rc 0),
perf-sse2-audit-2-static (static on the final head).
Run 1 jobs (shiki): perf-sse2-audit-sr25 rc 1 SUPERSEDED (named cbench_opt; the rows are in cbench_memref).
perf-sse2-audit-static rc 1 SUPERSEDED (the pending record lacked `date`; fixed in 61bcf6355; static2 rc 0).

Not to redo: the log scan (run 1, 869 logs); the libm recount (run 1); the SR-25 recount (run 2).
Open, for other owners: SR-25's `now` column is stale (default today 32.0/41.4/31.4 vs glibc 33.6/48.5/34.9 on Zen 1,
below glibc; the row says ABOVE). On the Zen 1 build host, Liella's default path took 1.46x glibc's time at fewer
instructions, and its SSE2 strlen_64 was faster than its AVX2 path. This is untimed on a booked host; see
docs/saturation section 13, PERF-STRLEN-NONAVX2.
Next step: the coordinator assigns the pending record's id and decides whether to cut the two proposed briefs.
