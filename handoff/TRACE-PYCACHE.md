# TRACE-PYCACHE
Branch task/trace-pycache. Change: tools/hottext/{regen,probe_text,startup_data}.sh run python3 -B; so.rs sets
PYTHONDONTWRITEBYTECODE=1 for the regen.sh launch; labnames::walk skips __pycache__ dirs and *.pyc.
Tests (xtask labnames): walk_skips_build_output (extended), hottext_scripts_run_python_without_bytecode.
Proof: no-.git copy, `cargo xtask so && cargo xtask static`: no __pycache__/*.pyc left, static passes (job TRACE-PYCACHE-e2e, E2E-OK).
Plants (3/3 CAUGHT, job TRACE-PYCACHE-plants2; the first run's walk-dir survived and the test fixture was fixed).
Gate: `affected --run --base origin/next` escalated to FULL (tools/hottext/*.sh are unmapped), killed; ran
`cargo test -p xtask --bin xtask` instead (rc 0, label TRACE-PYCACHE-xtasktests).
Not verified: clippy/static ran on the first commit only (second commit is test-fixture only; the xtask tests compiled it).
Other tree walkers (policy.rs, compat_ids.rs etc.) were not changed: they filter by extension.
Next: none; land.

## Run 2 (TRACE-PYCACHE-2)
Inherited head was already fixed (walk-dir fixture adds a non-.pyc file inside __pycache__). On that tree: lint (static + clippy) rc 0
(TRACE-PYCACHE-2-lint), `cargo test -p xtask --bin xtask` rc 0 (TRACE-PYCACHE-2-gate; affected escalates to FULL for tools/hottext/*.sh),
plants 3/3 CAUGHT (TRACE-PYCACHE-2-plants2; handoff/TRACE-PYCACHE-plants.txt). TRACE-PYCACHE-2-plants (rc 2) was SUPERSEDED: its baseline
`cargo xtask so` hit the 600 s per-command timeout on a loaded host; rerun with --timeout 1800.
