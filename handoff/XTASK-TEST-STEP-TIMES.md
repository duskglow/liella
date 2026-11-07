# XTASK-TEST-STEP-TIMES: the gate prints each step's wall seconds

State: branch `task/xtask-test-step-times`, cut from origin/next (e0c6b146). Commits: 25eeb144 (code, docs, D-X.35),
the plants commit, then this handoff. Decision in force: **D-X.35** (plan/07-decisions/x-implementation-decisions.md).

## What changed
- `xtask/src/steptime.rs` (new): `Steps::time(name, f)` prints `step: <name> <seconds>s` when the step ends, whether
  it passed or failed. `summary()` gives `step-summary: <name> <seconds>s` lines, slowest first, then `step-total: <seconds>s`.
  `parse()` reads a `step:` line back. Names have no whitespace (any becomes `-`) and seconds have one decimal.
- `cargo xtask test` (`test_all` → `test_steps`, main.rs): every step is timed: each audit, trace, fuzz-check, each
  `*-gen-check`, `remez-check`, `tables-check`, `products`, `tests-nextest`, `tests-doc`, `tests-interposed-unchanged-scan`,
  `tests-interposed-nextest`, `tests-interposed-doc` (`*-cargo-test` without nextest), `conformance`,
  `native-examples`, `rust-dylib`, `symgap` and `status`. The summary prints even when a step fails.
- `crates/mathtools/src/bin/remez.rs`: prints one `step: remez/<fit stem> <s>s` line per fit. So the `remez-check` step's
  time minus the sum of its fit lines is the release build of the tool, and one slow fit can be told apart from a slow build.
- `affected --run` escalated to FULL (affected.rs `run_selection`) times `stubs` and `clippy-workspace`. After `cargo
  xtask test` it parses that step's slice of `target/affected.log` and prints its `step:` lines to the console as
  `test/<name>` (the per-fit `remez/<fit>` lines are printed as they are and not summed again), then its own summary.
- Docs: main.rs command table (`test` row), docs/architecture.md (`cargo xtask test` row).

## Where the CI side can read these lines (for the coordinator's trend check in ops/ci.py; liella-ops not edited)
- The gate job's console log (jobrun's `~/logs/<label>.log`, the log ops/ci.py keeps). With the FULL escalation it holds
  `step: stubs`, `step: clippy-workspace`, every `step: test/<name>`, the `step: remez/<fit>` lines, then the
  `step-summary:` lines and `step-total:`.
- `target/affected.log` in the role holds the raw lines of the child `cargo xtask test` (without the `test/` prefix,
  with its own summary).
- Regex: `^step: (\S+) ([0-9]+\.[0-9])s$` for a step, `^step-summary: (\S+) ([0-9.]+)s$`, `^step-total: ([0-9.]+)s$`.
  A trend check should key on `test/remez-check` and `remez/<fit>`. These are wall times, so the right use is a
  trend alert, not a pass/fail limit (rule 3).

## Tests and jobs (kanon, role kn1)
- Unit tests `line_format_is_fixed_and_round_trips` and `summary_is_slowest_first_with_total`: pass (job xtst-unit;
  the clippy failure in that job was fixed before commit).
- `cargo xtask clippy` + `cargo xtask static` + a smoke run of `cargo xtask remez --check <one fit>` (printed
  `step: remez/example_exp 0.4s`): job xtst-lint, rc 0.
- Plants `handoff/XTASK-TEST-STEP-TIMES-plants.txt`: job xtst-plants, 4 CAUGHT, 0 SURVIVED, no WARN lines.
- Gate `cargo xtask affected --run --base origin/next`: job xtst-gate. It escalates to FULL because xtask/src/main.rs
  is part of the gate, and was STILL RUNNING at hand-back. To re-attach on kanon: `~/bin/jobwait xtst-gate`. Its log
  (`~/logs/xtst-gate.log`) is also the first real sample of the new lines.

## First sample (xtst-gate, in progress)
The per-fit lines already name the slow fits in the remez check: `remez/fast32_lgamma 166.2s`, `fast32_erfcx4 19.1s`,
`fast32_j1 17.7s`, `fast32_j0 17.3s`, `fast32_erfcx5 14.5s`, and the rest at about 1 s or less (fits run so far). If the
remez check needs to get shorter, start with fast32_lgamma (this story does not change it).

## Open / next step
- Read the xtst-gate result. If it is green, the story is ready. Check that the log ends with the `step-summary:` block.
- Not done: a separate timing for the remez tool's build. It can be inferred as `remez-check` minus the sum of the
  `remez/*` lines.
- Not to redo: per-fit remez timing through 22 separate `cargo run` calls was considered and rejected. Each call
  costs cargo start-up, and the binary's own line costs nothing.
