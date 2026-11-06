# PLAN-BATCH-UNTESTED-NONLOADER (planning) - handoff

State: branch task/plan-batch-untested-nonloader, cut from origin/next c3424c99. Planning only: no library or test
code changed. Briefs: handoff/plan-batch-untested-nonloader/{PROFILING-GMON-CLAUSES,FINITE-ALIAS-CONTRACT,
TRACE-TAG-COVERED-STUBS}.md. Report: handoff/reports/PLAN-BATCH-UNTESTED-NONLOADER.json.

## How the list was made

`cargo xtask trace --untested` is NOT on origin/next (SPEC-UNTESTED-CLAUSES is queued_ci, its branch unmerged); I did
not read that branch (others' work). Stand-in, run in scratch: every one of the 1269 clause IDs in spec/clauses.tsv is
named somewhere under crates/*/tests, so "untested" = the 41 clauses still pending as `clause!` stubs (no test fn of
that name). 15 are §27 (phase 14, excluded). The 26 others:

| Clause(s) | Outcome |
|---|---|
| s28_1_c_1, s28_1_c_2, s28_1_c_4, s28_3_c_0 | brief PROFILING-GMON-CLAUSES (stacks on PROFIL-SIGPROF-HISTOGRAM) |
| s10_0_c2_0 | brief FINITE-ALIAS-CONTRACT |
| s22_5_c_1_2, s24_5_c_3, s0_6_c0_6_0 | brief TRACE-TAG-COVERED-STUBS (existing tests lack the tag; one gap each for §24, §0) |
| s28_1_c_3 | open story PROFIL-SIGPROF-HISTOGRAM (queued_ci) |
| s3_4_c_0, s3_4_f_0 | pending by decision D-3.8 (alloca is a compiler builtin) |
| s16_1_g_3 | nscd is a documented difference (docs/differences-from-glibc.md "nscd" row); no Liella nscd client to test |
| s22_10_c_0..c_5 | libthread_db: no Liella code (td_ta_new only in spec/symbols.tsv); also debugger-side |
| s29_1_c_1, s29_1_c_2, s29_2_c_0 | RPC/XDR/NIS: story LEGACY-RPC-NIS-STUBS (red 11-04, not closed) |
| s29_4_c_0 | removed functions: REGEXP-H-* line (red 10-24), clause too broad for one story |

Only three briefs qualify; not padded to eight.

## Open questions / next step

- Coordinator: SPEC-UNTESTED-CLAUSES' `--untested` definition may differ from the stand-in (its picks included
  `s22_n_2`, a Notes clause with a test fn). Re-run `cargo xtask trace --untested` once it lands and diff with the
  table above.
- Next step: submit the three briefs; PROFILING-GMON-CLAUSES only after PROFIL-SIGPROF-HISTOGRAM lands.
- Do not redo: the scan (scratch script, 1 file read per test file; result above).
