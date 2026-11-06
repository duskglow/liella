# TIMING-COVERAGE-1106B handoff

State: branch task/timing-coverage-1106b (from origin/next c101c4c6); planning only, no code changed.
Full report: handoff/timing-coverage-1106b/README.md (table.md; four briefs TIMING-COV-*.md; job script tc1106b.sh;
raw output profile-keke-c101c4c6.txt; compare-rows.txt). JSON: handoff/reports/TIMING-COVERAGE-1106B.json.

Done: the 98 rows of the 1106 table re-checked against ops.db (read-only, 2026-11-06 ~06:30Z) and the landings on next:
open 38, re-time 38, new brief 4, deferred 9 (wordexp), out of scope 9 (cold start). Only four NONE rows were left in
scope, so four briefs (not six): TIMING-COV-FORK-COW-FAULTS (stacks on COLDSTART-FORK-CHILD-RESET-2),
TIMING-COV-REGEX-BACKREF-SCRATCH, TIMING-COV-REGEX-FIRSTBYTE-IVB, TIMING-COV-SPAWN-HEAP256M-IVB (reproduce first).

Decisions in force that the briefs checked: D-18.3 (spawn via CLONE_VM), D-18.7 / D-22.12 (fork), D-14.15 (regex capture
pass). No new D-row proposed.

Not to redo: regexec_backref is NOT covered by REGEX-BACKREF-FIXED-2 (landed before the sweep base); posix_spawn
heap256m shows no gap on Zen 1 (kernel counters equal), so do not code it before an IVB reproduction.

Open questions: nozomi = N100 is NOT VERIFIED (from the 1106 README); nico = Ivy Bridge taken from the active booking.

Next step: the coordinator submits the four briefs (rank order in README); the next coverage pass needs a newer sweep
table, since no row above 1.05x time is left uncovered.
