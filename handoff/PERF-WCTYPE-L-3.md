# PERF-WCTYPE-L-3 handoff
State: branch task/perf-wctype-l-3 (continues task/perf-wctype-l-2). Docs-only: resolved the CI conflict in
docs/saturation/15-the-seven-benches-section-14-skipped.md by merging origin/next and keeping both sides
(next's re-measure paragraph with the neutral host wording, then the PERF-WCTYPE-L-2 negative-result section;
the L-2 heading's host name was replaced by "build host" for the lab-names check).
Decisions in force: none new. No code changed, so no plants apply (L-2 kept no code).
Not to redo: the Table3 micro-optimisations (5 of 6 rows slower, reverted).
Open: the L-2 untried ideas (a) per-view BMP bit cache, (b) flat delta table for C.UTF-8 towupper,
(c) Table3 offsets resolved once at Wide construction. Each is a new code story with an exhaustive equality test.
Job: `cargo xtask static` on the role (label perf-wctype-l-3-static).
