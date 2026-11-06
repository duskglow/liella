# PLAN-NEXT-BATCH-NOV handoff

State: branch task/plan-next-batch-nov, briefs commit 035b7cd7 (origin/next d67780c7 base). Planning only, no code.
Result: 5 briefs (not 10) in handoff/plan-next-batch-nov/: ARCH-STRING-PORTABLE, FUZZ-COVERAGE-STATUS,
BENCH-UNMEASURED-WIDEN, VALIDATION-ROWS-STALE, PERF-READDIR-ENTRY (stacks on PERF-FSWALK-PHYS). summary.md has the table.
RULING NEEDED: DL-THREAT-TEXT waits for R14-15 approval; T22 (stdio without heap) needs assigning, after STDIO-SHORT-IO-5;
T24 is a spec gap.
Not done (budget): a full read of the plan/07-decisions follow-ups, and of spec clauses that have no traced test.
Next step: a short planning run that scripts the "clauses with no traced test" list and briefs the other five from it.
Do not redo: the in-flight checks in summary.md ("Checked and left out"), as of 2026-11-04 17:00Z.
Jobs: PLAN-NEXT-BATCH-NOV-static (cargo xtask static) on kanon kn1.
