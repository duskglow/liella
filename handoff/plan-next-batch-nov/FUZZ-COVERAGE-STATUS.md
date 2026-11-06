# FUZZ-COVERAGE-STATUS: record per-target fuzz coverage (cov/ft) in the campaign table

Id: FUZZ-COVERAGE-STATUS
Kind: code
Priority: 55
Size: S
Host: -
Stacks-on: -
Model: sonnet
Effort: medium
BUDGET: 35 tool calls / 80k

## The change

Gap: `docs/fuzzing.md` says "Per-target coverage is open: a rerun that keeps libFuzzer's last `cov:`/`ft:` status
line would fill it." The phase-16 gate wants corpus size, coverage and crashes per target (V-3); coverage is missing.
Scope: `cargo xtask fuzz` (xtask's fuzz module) keeps the last libFuzzer status line of each run and prints
`<target> cov=<n> ft=<n> corpus=<n> execs=<n>` at the end; a `--summary <file>` option writes the same rows as TSV.
Then one short rerun of every target (same fixed per-target budget as PRE14-FUZZ-CAMPAIGN, in RAM, detached) fills the
coverage column of `docs/fuzzing.md`. A finding is a new fix story, never fixed here; a possible security weakness goes
only to SECURITY-PRIVATE in the hand-back.
Files: xtask's fuzz command, its unit tests, `docs/fuzzing.md`. No library code.
Proof: an xtask unit test that parses recorded libFuzzer status lines (including a run that ends on a crash and one with
no status line: then the column says `none`, never 0); a plant that keeps the FIRST status line instead of the last
must fail it; `cargo xtask clippy`.
JOBS: fuzz-coverage-status-gate, fuzz-coverage-status-plants, fuzz-coverage-status-run, FUZZ-COVERAGE-STATUS-static
