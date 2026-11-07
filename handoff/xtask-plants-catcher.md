# XTASK-PLANTS-CATCHER handoff

State: branch task/xtask-plants-catcher (from origin/next a09927ac), code commit aa2d1689 + this handoff commit.
Status: INCOMPLETE. The code is written and pushed but has NOT been compiled or tested: no cargo on the agent's
machine and no host/role was admitted for the story (brief `Host: -`; `ops log` gave no "admitted to" line).

## What was done
- xtask/src/plants.rs: `catcher(log)` reads a plant's command log and names `<test binary> <test name>` of the first
  failing test (cargo test: last `Running ... (<deps>/<target>-<hash>)` + `test <name> ... FAILED`; `Doc-tests <crate>`;
  nextest: any failing status line via `nextest::failing_status`, now `pub(crate)`). Fallbacks: a panic in a thread
  named after a test; a panic in `main`/`<unnamed>` ("a panic outside a named test") with its binary; the cargo rerun
  hint; `error: could not compile` ("the build (the mutant does not compile)"); else "no test the log names".
- Each plant whose command failed (CAUGHT and both WARNs) prints `plants: <id>: caught by <...>`; timeout prints
  `caught by timeout (<n>s)`. EQUIVALENT/SURVIVED lines unchanged.
- The run writes `<dir>/results.txt`: the plants file + a `# cargo xtask plants results (<summary>):` block, one
  `# <id>: <VERDICT>[, caught by ...]` line per plant (ERROR lines too); a rerun from it replaces the block. Agents
  commit it as handoff/<STORY>-plants.txt. Config gained `text` (the plants file as read).
- Unit tests: catcher_names_the_failing_test_of_a_cargo_test_log, catcher_names_the_first_of_several_failing_tests,
  catcher_without_a_failing_test_line (panic without a test name, abort, rerun hint, compile error, nothing),
  results_replace_an_earlier_block; a_caught_plant_exits_zero_and_is_restored checks results.txt.
- docs/plants.md: the caught-by line and results.txt.
- handoff/XTASK-PLANTS-CATCHER-plants.txt: 7 plants on the new code (cmd `cargo test -p xtask --bin xtask plants::`).

## Exact next step
On an admitted host/role: sync, `cargo xtask clippy`, `cargo xtask static`, `cargo test -p xtask --bin xtask plants::`
(fix any compile/clippy slips: the code is unbuilt), then the gate `cargo xtask affected --run --base origin/next` and
`cargo xtask plants handoff/XTASK-PLANTS-CATCHER-plants.txt --dir <new RAM dir>`, each via ~/bin/jobrun; commit the
run's results.txt over handoff/XTASK-PLANTS-CATCHER-plants.txt.

## Not to redo / notes
- Decision (local, no row added yet): results go to `<dir>/results.txt` (the tool still writes nothing outside --dir)
  rather than editing the input plants file. A D-row should be added in plan/07-decisions/ once built.
- Rule slip: one `sed -i` on docs/plants.md (a no-op substitution, identical pattern and replacement); no content changed.
