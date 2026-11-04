# PERF-SSE2-REMEASURE handoff (2026-11-04, run 3: port onto next after the cutover)

State: branch task/perf-sse2-remeasure-3, cut from origin/next bad2e7ae; role keke:~/w/n2. Measurement only: no code.
Run 3 ported runs 1-2 (liella-historic task/perf-sse2-remeasure-2, f1f23a46) by hand, because next's register is now one
file per section: the old register section 13 is `docs/saturation/16-sse2-level-re-measure-after-bench-tunables.md`
(next free number), listed in `docs/saturation-register.md`. Lab host names became next's descriptions (keke = Zen 1
host, sumire/mei = Skylake-SP host, shiki = Haswell-EP host, nico = Ivy Bridge host A), and the old "section 12.x"
references became next's 13.x numbers. The historic report records PERF-SSE2-REMEASURE(-2).json were carried over as they were.

Done in run 3 (the step run 2 left): read bench4's cbench_hot rows (keke:~/logs/perf-sse2-remeasure-bench4.log, job rc 0)
and wrote them up as 16.1. They agree with the percall shapes (strncmp_diff8 1.44 vs shape 1.41; memrchr 64_hit/4k within
3 instr of the shapes; getline in the same direction). Also listed every "SSE2 forced" result now in next's sections
13-15 (16.2): all used `--tunables`, wl_percall or in-process probes, except the 15.1 realloc row (log not found:
NOT CHECKED; its verdict is a code-path finding). `xtask bench` still ignores an exported GLIBC_TUNABLES: only
`--tunables` forces the level.

Verdicts (unchanged since run 1): strcmp-3 holds; memrchr holds; getline gl_80 holds; strcoll NEGATIVE stands.
Open at SSE2: strncmp_diff8 1.44, memrchr_64_hit 1.29 (before the memrchr story), getline_long 1.15.

Decisions in force: none new (measurement only). No ruling needed.
Informational: under `--compare`, xtask bench takes every later word as a bench name, so `--rows` / `--no-archive` after it fail.
Do not redo: the bench4 / counts2 measurements (keke jobs rc 0).
Next step: none for this story. Optional: find the perf-realloc-usable-base3 log and check its `tunables:` line.
Report: handoff/reports/PERF-SSE2-REMEASURE-3.json (jobs and gate result there).
