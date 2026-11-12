# PERF-SSE2-COUNT-AUDIT (handoff)

State: branch task/perf-sse2-count-audit, planning only (no code). Full report: handoff/reports/PERF-SSE2-COUNT-AUDIT.md.
Pending record: records/pending/PERF-SSE2-COUNT-AUDIT-1.toml (D-NEW, series D-10, amends D-10.64/D-10.65).
Decisions read: D-10.54/61/62/64/65/66/77/90, D-3.17, D-4.57/61/62, SR-20..57 (SSE2 figures), D-X.39.

Result: no decision or saturation record cites an SSE2 figure from a dropped-GLIBC_TUNABLES run. D-10.66's SSE2
counts came through `--tunables` (the keke log prints `-FMA (both sides)`). D-10.64 and D-10.65 had no SSE2 counts. The recount
on next (shiki Haswell-EP, `--tunables glibc.cpu.hwcaps=-FMA`) supplies them. No conclusion changes.

Jobs (shiki:~/w/sh1): perf-sse2-audit-counts2 rc 0 (default and -FMA cbench_libm counts). perf-sse2-audit-sr25 rc 1
(wrong bench: those rows are not in cbench_opt). perf-sse2-audit-static rc 1 (pending record lacked `date`, fixed).
perf-sse2-audit-static2 rc 0 on the final code head.

Not to redo: the log scan (869 bench logs on keke/mei/shiki/kanon, `tunables:` line per run); scripts were in
the scratch directory (scan.sh, recscan.py), not in the repo.

Open / next step: SR-25 "SSE2-GAP instr 43.4 to 45.4" (2026-10-23, inside the window before `--tunables` existed) is
NOT VERIFIED. Recount strlen_64/strnlen_64/rawmemchr_64 in their memref bench with `--tunables glibc.cpu.hwcaps=-AVX2`.
Follow-up proposals (D-10.65 SSE2 path +51..64 over FMA; D-10.64's unmeasured Dekker trade) are in the report.
Host caveat: the recount ran on shiki (Haswell-EP), not keke (Zen 1). glibc's -FMA figures agree within 0.2.
