# FUZZ-COVERAGE-STATUS (branch task/fuzz-coverage-status)

State: `cargo xtask fuzz` keeps the last libFuzzer status line per target (across chunks, one fold `keep_last` shared by the run
and its unit tests), prints `<target> cov=<n> ft=<n> corpus=<n> execs=<n>` at the end (also after a failing target), and
`--summary <file>` writes the rows as TSV. No status line -> `none`, never 0. `docs/fuzzing.md` describes it.
Proof: 4 unit tests in xtask/src/fuzz.rs (last line, crash run, fork-mode line, none); plants first-status-line and none-as-zero CAUGHT
(jobs fuzz-coverage-status-plants, plants2); clippy + static + fuzz tests green (gate3).
NOT DONE: the rerun of all 20 targets (30 min each, about 10 h of fuzzing) that fills the coverage column of docs/fuzzing.md. It is a
long detached job, outside this budget. Next step: on mei in RAM, `cargo xtask fuzz --all` is capped at 140 min per invocation, so run
`cargo xtask fuzz <t> --time 30 --summary <f>` per target (two queued jobs of 10 as in the PRE14 campaign), then add the cov/ft columns.
Gate notes: gate (cargo xtask affected --run, no --base) escalated to FULL and was killed (rc 143); gate2 with --base origin/next ran
all tests green (10762 passed, conformance 110) but its clippy step failed on the first draft (fixed; gate3 green). The docs commit and
the final code commit came after gate2's tree; gate3 covers clippy/static/fuzz tests on the final head.
