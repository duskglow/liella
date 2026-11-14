# PLANTS-PREPARE-TIMEOUT

State: branch task/plants-prepare-timeout. Code in xtask/src/plants.rs, docs in docs/plants.md. Done.

- `cargo xtask plants` has `--prepare-timeout <secs>` (default 3600) for the `so` build (baseline, per-plant rebuild,
  restore rebuild). `--timeout` (default 600) is unchanged and still decides CAUGHT-by-hang.
- A baseline `so` that times out: `BASELINE FAILED: so timed out after N s (prepare timeout; ...)`, exit 2.
- A mutant whose build times out: new verdict `BUILD-TIMEOUT` (not CAUGHT), counted in the summary, run exits 1.
- A restore rebuild that times out stops the run (existing ERROR path).
- Tests: 3 new in plants.rs (slow prepare not cut by the test timeout; baseline prepare timeout; plant build timeout) plus the default check.

Results (keke:~/w/n2; the dispatcher admitted this story to keke, not kanon, so the "kanon" proof ran on keke):
clippy+static+20 plants:: tests green (PPT-clippy2); direct gate 400 passed / 0 failed (PPT-gate);
plants 5 CAUGHT / 1 EQUIVALENT / 0 WARN (PPT-plants, handoff/PLANTS-PREPARE-TIMEOUT-plants.txt).

Not covered: the restore-rebuild's use of prepare_timeout is not planted (needs a plant sequence with products and non-products commands).
The kanon run is not done: kanon is not this story's role. Next step: none; the briefs' "--timeout 2400" workaround can be dropped once this lands.
