# PERF-SSE2-REMEASURE handoff (2026-11-04, run 4: register section 18)

State: branch task/perf-sse2-remeasure-4 (continues task/perf-sse2-remeasure-3), origin/next b0296d28 merged; role
kanon:~/w/kn1. Measurement only: no code.

Run 4 (fixing run 3's CI red: section 16 was used twice): merged origin/next (only conflict: the index line in
`docs/saturation-register.md`), moved the section to `docs/saturation/18-sse2-level-re-measure-after-bench-tunables.md`
(the coordinator ASSIGNED 18; 17 belongs to INTEGRATE-REGISTER-PERF-2), with headings 18 / 18.1 / 18.2 and index line
"18.". Nothing on next was renumbered.

BLOCKER (still open): `cargo xtask register --check`, and so `cargo xtask static`, fail on this branch alone with
"section numbers are not contiguous: 17 is missing". Section 17 is not on next yet (INTEGRATE-REGISTER-PERF-2,
task/integrate-register-perf-2, is queued in CI). Everything else in static passes, clippy (workspace, all targets,
all features, -D warnings) passes, and the direct gate is green (kanon perf-sse2-remeasure-4-gate, rc 0, 0 suites:
docs-only change). RULING NEEDED: land INTEGRATE-REGISTER-PERF-2 first (or test this branch in CI on top of it); once 17
is on next, this branch needs only a merge of origin/next and a `cargo xtask register --check`. Developers do not merge
other stories' branches, so this run did not take 17 in.

Earlier runs (record): runs 1-2 measured (liella-historic task/perf-sse2-remeasure-2, f1f23a46); run 3 ported them onto
next, wrote up bench4's cbench_hot rows (now 18.1) and checked every "SSE2 forced" result in sections 13-15 (now 18.2):
all used `--tunables`, wl_percall or in-process probes, except the 15.1 realloc row (log not found: NOT CHECKED).

Verdicts (unchanged since run 1): strcmp-3 holds; memrchr holds; getline gl_80 holds; strcoll NEGATIVE stands.
Open at SSE2: strncmp_diff8 1.44, memrchr_64_hit 1.29 (before the memrchr story), getline_long 1.15.

Decisions in force: none new (measurement only).
Informational: under `--compare`, xtask bench takes every later word as a bench name, so `--rows` / `--no-archive` after
it fail. The register check rejects a gap, so an assigned out-of-order section number cannot pass static until the
sections before it land.
Do not redo: the bench4 / counts2 measurements (keke jobs rc 0); the rename to 18.
Next step: after INTEGRATE-REGISTER-PERF-2 lands on next, merge origin/next, run `cargo xtask register --check` and
`cargo xtask static`, re-run the direct gate. Optional: find the perf-realloc-usable-base3 log and check its `tunables:` line.
Report: handoff/reports/PERF-SSE2-REMEASURE-4.json.
