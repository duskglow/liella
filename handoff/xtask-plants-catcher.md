# XTASK-PLANTS-CATCHER handoff

State: branch task/xtask-plants-catcher-2 (stacks on task/xtask-plants-catcher, cut from origin/next a09927ac).
Commits: aa2d1689 code (run 1), 1c191042 run-1 handoff, 444bcc87 decision row D-NEW, then the run-2 handoff/report
commit (HEAD). Status: COMPLETE, built and tested in run 2 on mei:~/w/s2.

## What the change does
- xtask/src/plants.rs: `catcher(log)` reads a plant's command log and names `<test binary> <test name>` of the first
  failing test (cargo test: last `Running ... (<deps>/<target>-<hash>)` + `test <name> ... FAILED`; `Doc-tests <crate>`;
  nextest: any failing status line via `nextest::failing_status`, now `pub(crate)`). Fallbacks: a panic in a thread
  named after a test; a panic in `main`/`<unnamed>` with its binary; the cargo rerun hint; `error: could not compile`;
  else "no test the log names". Timeout: `caught by timeout (<n>s)`.
- Each plant whose command failed (CAUGHT and both WARNs) prints `plants: <id>: caught by <...>`. EQUIVALENT/SURVIVED
  lines unchanged.
- The run writes `<dir>/results.txt`: the plants file plus a `# cargo xtask plants results (<summary>):` block, one
  `# <id>: <VERDICT>[, caught by ...]` line per plant; a rerun on it replaces the block. Agents commit it as
  handoff/<STORY>-plants.txt (docs/plants.md says so).
- Decision row: D-NEW in plan/07-decisions/x-implementation-decisions.md (no id reserved; the coordinator assigns it).

## Run 2 results (mei:~/w/s2, head 444bcc87 for code)
- xtask-plants-catcher-2-lint1: clippy, static, `cargo test -p xtask --bin xtask plants::` 17 passed (rc 0) on the
  inherited head 1c191042: the unbuilt run-1 code compiled and passed first time, nothing to fix.
- xtask-plants-catcher-2-plants1: `cargo xtask plants handoff/XTASK-PLANTS-CATCHER-plants.txt`, 7 plants: 7 CAUGHT,
  0 WARN. The tool's own new output named the catching test of each plant; the committed plants file is that run's
  results.txt (the caught-by block is the first record written by the feature itself).
- xtask-plants-catcher-2-gate1: clippy + static + `cargo xtask affected --run --base origin/next` (see the report).

## Not to redo / notes
- results.txt under --dir, not an edit of the input file: keeps the tool's "writes nothing outside --dir" rule.
- Run 1 slip: one no-op `sed -i` on docs/plants.md (identical pattern and replacement).

## Next step
None for the story. The coordinator replaces D-NEW with a reserved id (tools/assign_placeholder.sh) before landing.
