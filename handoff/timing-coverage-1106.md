# TIMING-COVERAGE-1106 handoff

State: branch task/timing-coverage-1106 (from origin/next 730f416b); planning only, no code changed.
Full report: handoff/timing-coverage-1106/README.md (table: table.md; six briefs TIMING-COV-*.md; probe spec tc1106.c;
raw counts percall-keke-730f416b.txt).

Done: all 98 sweep rows above 1.05x in time (either class) mapped: open 33, open-on-one-class 4, re-time 13, NONE 30,
wordexp deferred 9, cold-start (COLDSTART-PLAN-1106) 9. Uncovered rows grouped and ranked by gap x workload call
count (WL-ENTRIES table on keke); top 6 briefed with Liella per-call instruction profiles on keke (role n4, job
timing-coverage-1106-percall, default and GLIBC_TUNABLES=glibc.cpu.hwcaps=-AVX2 dispatch).

Decisions in force: none new (no D-row: planning output only).

Open questions / for the coordinator:
- Ranks 7-9 (regexec_backref, regexec_class_nomatch_1k, fork_exit_wait / posix_spawn_true_heap256m) are not briefed.
  regexec_backref: REGEX-BACKREF-FIXED (red, unlanded) reached 1.04 instr on its branch; recommend reviving it rather
  than a new brief.
- Revive vs new: PERF-GETLINE-LONG-2 and PERF-STRNCASECMP-64-AVX2 (both red, unlanded) hold partial work for briefs 5 and 3.
- N100 timing host taken as nozomi from the hosts table: NOT VERIFIED.

Not to redo: the row-to-story mapping (table.md), the keke per-call counts at 730f416b.
Next step: submit the six briefs (stacking as their headers say); after the queued_ci timing stories land, the next
sweep re-times the 13 re-time rows.
