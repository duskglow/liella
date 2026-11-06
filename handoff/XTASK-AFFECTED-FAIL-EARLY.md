# XTASK-AFFECTED-FAIL-EARLY: a failed test step names its failing tests and log under its FAIL line

## State

- Branch `task/xtask-affected-fail-early`. It starts from XTASK-AFFECTED-CONFORMANCE-OUTPUT (stacked, landing), then
  `origin/next` cc39389c merged in (d0d9a142, clean). Code commit c6221233, plus this handoff commit.
- Decision in force: D-X.29 (reserved through `ops reserve`; decided locally, tooling only). It builds on D-X.27.

## What changed

- `xtask/src/affected.rs`:
  - `step_out` takes `test_crate: Option<&str>`, which replaces `allow_skip: bool`, and writes to `out: &mut dyn Write`
    (the console; a buffer in the unit tests). When a test step fails, `failed_tests_report` is printed right under the
    FAIL line, before the next step starts. The per-crate `test -p <crate>` passes (with and without the exports
    interposed) and `xtask unit tests` are test steps. Other steps and the conformance step (D-X.27) are not.
  - The report shows the failing tests, each named `<crate>[::<binary>] <test>`: at most 10, then `... and N more`. For
    each test it shows the first 5 lines of its panic, each cut at 300 characters. A test whose output has no panic
    (a signal, a timeout) gets `(no panic message in the output; see the log)`. Last comes
    `full output: <target/affected.log> (this step from line N)`. A failed step naming no test (a build error) shows
    its first `error` lines instead. A passing step prints nothing extra. The end-of-run summary is unchanged.
  - `failed_tests` reads both runners. For nextest, the binary-id names come from xtask nextest's `---- ... stdout ----`
    lines, and the panic from the failing test's output after its status line. nextest on the build hosts indents that
    output under `stdout ───` / `stderr ───` headers that do not name the test (seen in the e2e run, below), so only
    column-0 `failures:` / `test result:` lines end a section. For `cargo test`, the binary comes from the `Running`
    lines and the panic from the `---- <test> stdout ----` section.
- `plan/07-decisions/x-implementation-decisions.md`: row D-X.29.
- `handoff/XTASK-AFFECTED-FAIL-EARLY-plants.txt`: 12 plants.

## Tests (Trace: no spec clause, test-harness reporting)

- `a_failed_test_step_names_its_failures_under_its_fail_line`: three real `step_out` calls with stand-in `sh`
  commands write to one buffer: a passing step, a failing step with `cargo test` output (a lib failure and a
  `tests/sweep.rs` failure with a 6-line panic), then a passing step. The test checks the exact console lines and their
  order: the report sits between the FAIL line and the next step's line. It also checks the 5-line cap, the `crate` and
  `crate::sweep` names, the log line number, that the passing step printed one line, and that the earlier step's
  `---- old stdout ----` was not read.
- `the_failed_test_report_reads_nextest_output_and_stays_bounded`: 13 failures in nextest format: the current indented
  format, one test with the older `──── STDERR: <id> <test>` header, a SIGSEGV with no panic, and a PASS line followed
  by a stray panic that must not be attributed. The test checks the 10-test cap with `... and 3 more`, the 300-character
  cut, the no-panic note and the build-error fallback.
- `affected_fail_early_fixture`: env-gated. It fails only when `LIELLA_AFFECTED_FAIL_FIXTURE` is set; the plants use it
  for the end-to-end check.

## Results (mei, role s3)

- `mei:xafe-check2` (pre-commit; clippy -p xtask and the new unit tests at that point): rc 0.
- `mei:xafe-e2e`: `LIELLA_AFFECTED_FAIL_FIXTURE=1 cargo xtask affected --only xtask --run`, run on the code BEFORE the
  indent fix. Console under `FAIL  xtask unit tests`:
  `failing tests (1):` / `xtask::bin/xtask affected::tests::affected_fail_early_fixture` /
  `| thread '...' (1957554) panicked at xtask/src/affected.rs:2969:13:` / `| FIXTURE-PANIC-MARKER: ...` /
  `full output: /mnt/zram/claude/w/s3/target/affected.log (this step from line 1)`. Its `cargo xtask nextest` half
  showed the real nextest layout, which led to the indent fix and to the nextest unit fixture.
- `mei:xafe-check3` (on c6221233): `cargo xtask clippy && cargo xtask static && cargo test -p xtask --bin xtask failed_test`, rc 0.
- `mei:xafe-gate` (on c6221233): `cargo xtask clippy && cargo xtask static && cargo xtask affected --run --base origin/next`,
  rc 0. Result: suites 2 ok, 212 passed, 0 failed; conformance 1 passed; `affected: ok`. No peak line (jobrun, not ~/bin/gate).
- `mei:xafe-plants2` (on c6221233): `cargo xtask plants handoff/XTASK-AFFECTED-FAIL-EARLY-plants.txt --dir
  /mnt/zram/claude/XTASK-AFFECTED-FAIL-EARLY/plants-2`, rc 0. Result: 12 plants, 12 CAUGHT, 0 SURVIVED, 0 EQUIVALENT,
  0 WARN-RC-ONLY, 0 WARN-NO-MSG. Its baseline-1 is the e2e fixture run through `affected --only xtask --run` on the
  final code: it passed, so the console carried the binary-id name and the panic marker.
- SUPERSEDED: `mei:xafe-check1` (rc 127: jobrun got the command as one string), `mei:xafe-plants` (rc 101: started
  outside the role tree, because the `cd` was part of the previous `&` list. No plant ran).

## Not covered / notes

- No plant swaps `Some(c)` for `None` at the per-crate test-step call site, because no product crate has a failing
  fixture. That call site is the same one-line hookup as `xtask unit tests`, which the `xafe-xtask-step-e2e` plant
  covers. Adding an env-gated failing test to a product crate did not seem worth its gate time.
- The full-mode step (`cargo xtask test`, an escalation to FULL) does not get the report. It is one step for every
  crate, so its output has no single crate to name.
- RULING NEEDED: none. INFORMATIONAL: none. Refusals: none.

## Next step

The coordinator lands it after XTASK-AFFECTED-CONFORMANCE-OUTPUT. Nothing is pending.
