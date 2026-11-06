# XTASK-AFFECTED-EXPLICIT: `cargo xtask affected` needs `--list` or `--run`

State: branch task/xtask-affected-explicit (from origin/next b59b2be4), code commits d5497a17, 9d79081f; this handoff on top.

## What changed
* `xtask/src/affected.rs`: no default mode. Neither `--list` nor `--run` -> the one-line `USAGE` on stderr, exit 2
  (`affected::main`, returned straight from `main.rs` like `plants`). `--list` and `--run` together -> exit 1.
  `--replay` needs neither (it lists and runs nothing). Args parsed by `parse_args` into `Opts`; `execute` takes the
  runner as a parameter so a unit test can prove `--run` hands the selection over and `--list` does not.
  The debuginfo hint now says `--only <target> --run`.
* `xtask/src/main.rs`: dispatch, the command table row and the usage line name `(--list | --run)`.
* Callers changed: CLAUDE.md and CONTRIBUTING.md (both showed bare `cargo xtask affected` as "run them"; now `--run`),
  CONTRIBUTING.md direct-tier step, AI_POLICY.md, README.md (adds the `--run` line), docs/architecture.md table.
  Callers already explicit, unchanged: plants.rs example, docs/plants.md, tools/probe/README.md, .config/nextest.toml,
  liella-ops ops/db.py (`--tier full --run`). Historical handoff/plan text not edited. Ops-side callers outside this repo
  (VM ~/bin/*, ops tools) were not audited beyond `grep` in liella-ops (only db.py invokes it, with `--run`).
* Tests: unit `affected_requires_list_or_run`, `affected_run_runs_the_selection_and_list_does_not`; integration
  `xtask/tests/affected_mode.rs` runs the binary (no mode: rc 2, one usage line naming both, nothing on stdout;
  `--list`: listing, rc 0, no summary; both: rc 1).

## Results (mei:~/w/s1, tree 59e25ef4)
* xae-gate3: `cargo xtask clippy` + `cargo xtask static` + `cargo test -p xtask`: rc 0 (unit 203 passed, affected_mode 3 passed).
* xae-plants2: `cargo xtask plants` 4 plants: 4 CAUGHT (no-mode-lists, no-mode-exit-0, run-does-not-run, list-runs),
  0 WARN. Plants file: mei ~/work/XTASK-AFFECTED-EXPLICIT/plants.txt.
* `affected --base origin/next` escalates this diff to FULL (main.rs, xtask/tests): the FULL gate was not run (landing).

## Decisions / open
* Local detail decided here (no D-row reserved): `--list --run` together is an error (rc 1), `--replay` exempt.
* INFORMATIONAL: any external script still calling bare `cargo xtask affected` now fails with rc 2 (intended).
* Next step: coordinator lands; the landing FULL gate covers the escalation.
