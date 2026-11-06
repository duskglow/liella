# HELD-REDS-TRIAGE summary (planning; no product code changed; JOBS: none)

Method: per branch, diff against its own merge base (historic/task/<b> against historic/next, or origin/task/<b> against
origin/next), then every added line (comparing trimmed text) looked up in the same file on origin/next 0005889c; register
rows looked up by text windows across docs/saturation/*; named functions/rows/tests grepped on next. A branch with 0 own
commits is already an ancestor of its remote's next. Added-line counts are "present/added" (non-handoff files).

Verdicts: 22 CONTAINED, 4 OBSOLETE (all 26 close), 1 NEEDED (MATH-M6-F32-BENCH-2, small), 0 UNSURE.
The pre-existing `handoff/cutover-reds-triage.md` (11-04) said "check" for FPENV-HONOR-4 and F32-BENCH-2; answered here.

| Story | Branch | Verdict | Evidence |
|---|---|---|---|
| BENCH-ROWS-UNMEASURED | task/bench-rows-unmeasured | CONTAINED | bench/c/cbench_unmeasured.c 137/138, budgets.tsv 6/6; its register section is docs/saturation/17-first-counts-of-the-three-unmeasured-rows.md (11/11 long lines) |
| COMPAT-CONDVAR-OLD-3 | task/compat-condvar-old-3 | CONTAINED | 0 own commits: merged into historic next (a90f10ab "Merge task/compat-condvar-old-3"); D-22.13 and tools/probe/compat/compat_condvar.c on next |
| COMPAT-NSS-RESOLV-4 | task/compat-nss-resolv-4 | CONTAINED | 291/313 lines; the rest are reworded comments; the five __nss_* compat stubs (group/hosts/passwd lookup, database_lookup, next) are in s16_nameservices.rs and absent from compat-missing.tsv; oracle probe 160/160 |
| FMA-DID-COMMENTS | task/fma-did-comments | OBSOLETE | comment-only (11 lines), cites D-10.23 (that id is cpow on next); next's comments cite D-10.25 (MATH-F64-FMA-PATH-5) in cpu.rs, fast64.rs |
| GATE-THREADS-REQUIRED-2 | task/gate-threads-required-2 | OBSOLETE | branch is handoff/report only (0 own commits vs historic next); next has `threads-required = 4` in .config/nextest.toml and the GATE-TEST-THREADS test-thread policy in xtask/src/nextest.rs |
| HOSTCONF-DIAGNOSTICS-2 | task/hostconf-diagnostics-2 | CONTAINED | 782/788 lines (nss.rs 222/222, hostconf.rs 205/205, tests 100%); next merged it ("Merge remote-tracking branch origin/task/hostconf-diagnostics-2 into task/integrate-hconf") |
| HOSTCONF-OPTIONS-PROBE-3 | task/hostconf-options-probe-3 | CONTAINED | 491/511 (the 14 hostconf.rs lines are the old on/off parser, rewritten as on_off/on_off_rest, hostconf.rs:131-136); "Merge task/hostconf-options-probe-3 into the hostconf integration" on next |
| MATH-M6-F32-BENCH-2 | task/math-m6-f32-bench-2 | NEEDED | all code on next (2078/2123) except validation/percall/math_f32.c (40 lines, file absent on next); brief handoff/held-reds-triage/MATH-M6-F32-BENCH-2-R.md |
| MATH-M6-F32-POWF | task/math-m6-f32-powf | CONTAINED | pow_core and powf tests (incl. powf(1e-40,0.3), powf(2.5,-97.3)) on next; 21 missing fast32.rs lines are doc-comment rewordings |
| MATH-M6-F32-REST-3 | task/math-m6-f32-rest-3 | CONTAINED | expm1_core, cbrt_core and tables on next (326/337); missing lines are comments and a ALL-array/names list now longer (powf added) |
| MATH-M6-F64-GAP-ASSEMBLY-2 | task/math-m6-f64-gap-assembly-2 | CONTAINED | register lines 69/69 in docs/saturation/16-f64-exp-log-pow-gap-breakdown.md; fast64.rs 184/201 (log_assemble, log_kernel, exp_nearest on next); s10_math.tsv/gen differences are the later hot.rs dispatch |
| MATH-M6-F64-GAP-BREAKDOWN | task/math-m6-f64-gap-breakdown | CONTAINED | only change is the register section, present as docs/saturation/16-f64-exp-log-pow-gap-breakdown.md (69/69) |
| MATH-M6-F64-GAP-EXP-REDUCE-2 | task/math-m6-f64-gap-exp-reduce-2 | CONTAINED | fast64.rs 280/299, register 69/69; Cody-Waite reduction on next; remaining lines are rewordings/dispatch moved to hot.rs |
| MATH-M6-F64-GAP-ROUND-TEST-2 | task/math-m6-f64-gap-round-test-2 | CONTAINED | fast64.rs 459/490 (`near`, `log_eps`, `log_assemble`, `log2_nearest` on next); the 5 missing s10_math.tsv rows are `elem_fast` wiring replaced by hot::exp_nearest etc. (hot.rs:2514) |
| MATH-M6-FPENV-HONOR-4 | task/math-m6-fpenv-honor-4 | CONTAINED | 2081/2118; D-10.22 on next carries the ruling (reworded, longer); spec/behaviour/math-ftz-daz.md 30/32. One 7-line paragraph in spec/behaviour/libm-exact.md is not on next: folded into the F32-BENCH-2-R brief |
| MATH-M6-SQRT-FTZ-6 | task/math-m6-sqrt-ftz-6 | OBSOLETE | its COMPAT-11 row and differences row (hardware sqrt only when DAZ off) were reversed by the 2026-10-23 owner ruling, now D-10.22 on next (sqrt honours the caller's FTZ/DAZ, sqrtsd in place); code lines otherwise 254/290 present |
| NS-NETWORKS-DNS-2 | task/ns-networks-dns-2 | CONTAINED | 390/393; the compat and differences rows are on next as COMPAT-13 (compat-flags.md:33, differences-from-glibc.md:148); commit 8b1bfb4e "Port NS-NETWORKS-DNS-2" |
| PERF-CTYPE-LOC | task/perf-ctype-loc | CONTAINED | locale.rs 10/10; row 7 (N3-7) in docs/saturation/13 and 15 |
| PERF-FNMATCH-PATHNAME-3 | task/perf-fnmatch-pathname-3 | CONTAINED | fnmatch.rs 55/55, ctype.rs, tests 35/35; register Row 8 status for -PATHNAME-2 on next (14-next-ranked...md:139); the one absent line is the run-1 status that the -2 line supersedes; complex.rs/s10_math.tsv differences are merged cpow work that next has moved past (b4d46ff1) |
| PERF-GETC-PUTC-TRIMS-2 | task/perf-getc-putc-trims-2 | CONTAINED | only docs/saturation-register.md changed; 36/36 long lines present in docs/saturation/13-15 |
| PERF-HEAP-PAIR-IVB-3 | task/perf-heap-pair-ivb-3 | CONTAINED | the negative (K_HINTV, #1710, reverted, "do not retry") is in docs/saturation/14 row 2 (run 3 text) |
| PERF-MEMSET-4K-SSE2-2 | task/perf-memset-4k-sse2-2 | CONTAINED | scan.S 46/46, hot.rs/ffi.rs/conv.rs 100%; "14.1a Row 9 ... PERF-MEMSET-4K-SSE2-2 result" at docs/saturation/14:120 |
| PERF-STRSTR-LONG | task/perf-strstr-long | CONTAINED | strstr_find.rs 23/23, scan.S 75/77 (2 comment lines; 128-byte group loop at scan.S:255/355); row 3 in docs/saturation/15 |
| PERF-WCTYPE-UTF8-REST-3 | task/perf-wctype-utf8-rest-3 | CONTAINED | view.rs 19/19; register rows 18/18 present in docs/saturation/15 |
| PLAN-NEXT-BATCH-1024-2 | task/plan-next-batch-1024-2 | CONTAINED | planning only (handoff/report); the plan is plan/next-batch-1024-2.md on next; branch is an ancestor of historic next |
| RC-RECOUNT-1024-4 | task/rc-recount-1024-4 | OBSOLETE | report-only recount of a 10-24 head against main (3 handoff files, no code); superseded by the cutover rebuild of the root; nothing to port |
| TIME-SPEC-GAPS-SETTLE-3 | task/time-spec-gaps-settle-3 | CONTAINED | 387/394; carried and merged ("Merge .../task/time-spec-gaps-settle-3 into task/integrate-hconf"); getdate.rs 18/18, differences rows 4/4; missing 6 test lines are the old harness (timeout/scratch), reworded |
