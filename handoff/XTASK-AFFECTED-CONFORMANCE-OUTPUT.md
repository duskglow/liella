# XTASK-AFFECTED-CONFORMANCE-OUTPUT: `cargo xtask affected` prints a failed conformance program's output

## State

- Branch `task/xtask-affected-conformance-output`, cut from `origin/next` c101c4c6. Code HEAD 4418b541 (three commits), plus this handoff commit.
- COMPLETE: gate green, plants 10/10 CAUGHT, all on keke role n2.

## What changed (D-X.27, decided locally, tooling only)

- `xtask/src/affected.rs`: the conformance step is now `run_conformance` (same `conformance/run.sh <names>` step).
  When it fails, `conformance_failures` reads the step's own slice of `target/affected.log` and prints to the console,
  per program `run.sh` names in a `conformance: FAIL <name> (...)` line:
  - `conformance FAILED: <name>: <end>`: `exit status N`, or `killed by signal N SIGxxx (status 128+N)` for a status
    above 128, plus any `stderr lacks: <text>` (EXPECT-STDERR) lines;
  - the last 40 lines of `conformance/build/<name>.log.stdout` and `.log.stderr`, each prefixed `    | `, lines cut at
    300 characters, with a note naming the whole file when lines were cut;
  - a program that did not build (or whose helper did not) gets the tail of its build log instead, because its
    stdout/stderr files are an earlier run's.
  - A passing program prints nothing. A failed step naming no program prints the step's last 40 lines.
- `conformance/c/x_affected_report.c`: env-gated fixture. It passes unless `LIELLA_CONF_REPORT_FIXTURE` is set; with
  it set, it prints 45 stdout lines and a stderr marker and exits 3.
- `docs/plants.md`: one sentence noting that `msg:` can now name a conformance program's own failure message.
- `plan/07-decisions/x-implementation-decisions.md`: row D-X.27 (reserved through `ops reserve`; D-X.26 was taken).

## Tests (Trace: no spec clause, test-harness reporting)

- `a_failed_conformance_program_reports_its_end_and_output_tails`: a stand-in `run.sh` in a temp root runs one passing,
  one failing (status 134, a missing stderr text, 50 stderr lines, one 524-character line) and one non-building
  program through the real `run_conformance`. The test checks the signal name, the tails and the cut notes, that the
  build log is shown and not the stale stdout/stderr, that the passing program is absent, that an earlier step's output
  is not read, and that a passing run returns an empty report.
- `a_conformance_step_failing_without_a_program_shows_its_end`: the fallback, plus `describe_status` cases.
- `the_conformance_report_matches_the_runner_formats`: pins the `run.sh` lines and file names the parser depends on.
  If someone changes those lines in `run.sh`, this test fails rather than the report going silent.

## Results (keke, role n2)

- `keke:xaco-gate`: `cargo xtask clippy && cargo xtask static && cargo xtask affected --run --base origin/next`, rc 0.
  Output: suites 2 ok, 209 tests passed, 0 failed; conformance 1 passed (`x_affected_report`, env unset); `affected: ok`.
  No peak line (jobrun, not ~/bin/gate).
- `keke:xaco-plants`: `cargo xtask plants handoff/XTASK-AFFECTED-CONFORMANCE-OUTPUT-plants.txt`, rc 0: 10 plants,
  10 CAUGHT, 0 SURVIVED, 0 WARN-RC-ONLY, 0 WARN-NO-MSG. `xaco-noprint` is the end-to-end plant. It drops the console
  print in `run_selection` and runs the fixture through `affected --only conformance:x_affected_report --run`; its
  `msg:` matched. That is the brief's check that `plants` matches against the new output.
- `keke:xaco-e2e` (a sample of the real console output, fixture forced to fail, rc 1 as intended):
  `conformance FAILED: x_affected_report: exit status 3, expected 0`,
  `stdout: last 40 of 45 lines (5 cut; whole file .../x_affected_report.log.stdout):`,
  `stderr (1 lines):`, `    | FIXTURE-STDERR-MARKER: ...`.
- `keke:xaco-check1` (red: comma dropped in `describe_status`, fixed in 2de5dced) and `keke:xaco-check2` (green):
  clippy, static and the xtask conformance unit tests.

## Not to redo / notes

- Only the console gets the report; `target/affected.log` is unchanged (it already names the files). That avoids any
  risk of `~/bin/test-summary` counting a program's output lines as results.
- RULING NEEDED: none. INFORMATIONAL: none. Refusals: none.

## Next step

The coordinator lands it. Nothing is pending.
