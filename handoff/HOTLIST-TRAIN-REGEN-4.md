# HOTLIST-TRAIN-REGEN-4: hot-lists tests independent of the caller's environment

State: branch `task/hotlist-train-regen-4`, on top of `task/hotlist-train-regen-2` (eb52e8287). Commits:
353138e51 (fix), ea4bbdf7c (cache test, plants file), then this note and the report record.
What the branch does as a whole: handoff/HOTLIST-TRAIN-REGEN-2.md (still current apart from the resolver's signature).

## The red and its cause
CI RED on eb52e8287: `hotlists::tests::branch_reads_the_fresh_copy_with_an_info_line` failed under the CI gate, which
runs with `LIELLA_TRAIN=1`. The test passed the train flag explicitly to `resolve`, but its last two assertions went
through `rel()` -> `path()` -> `dir()`, and `dir()` read the process's `LIELLA_TRAIN` (and `LIELLA_HOT_LISTS`). Under
the gate it took the train path and failed on the stale tracked list the test had set up. `dir()`'s `OnceLock` also
ignored its root: the first root's answer was returned for every later root.

## Fix (xtask/src/hotlists.rs)
- `Mode { over, train }`: the `LIELLA_HOT_LISTS` override and the train flag. `resolve(root, &Mode)` takes it
  explicitly; only `dir()` builds it, with `Mode::from_env()`.
- `rel()` is a wrapper over `rel_in(root, rel, lists)`, which takes the directory of the generated lists. The tests use
  `rel_in` with a `resolve` under their own mode: no hotlists test reads `LIELLA_TRAIN`, `LIELLA_HOT_LISTS` or `CI`.
- `dir()` remembers the root it cached; another root is resolved for itself. New test
  `the_cached_directory_belongs_to_its_root` (compares `dir` with `resolve` under the same process mode, so it holds
  in any environment).
- Readers' tests checked: the layout, startup_ro, hottext and so tests test pure helpers and never reach `dir()`;
  codemap, kernel_index and records take the train flag as a parameter in their tests.

## Results (mei role s1; logs ~/logs/<label>.log, script ~/work/hotlist-train-regen-4/lint.sh)
- htr4-lint1 (353138e51), rc 0: `cargo xtask clippy`, `cargo xtask static`, then `cargo test -p xtask` with
  `LIELLA_TRAIN=1 CI=1 LIELLA_HOT_LISTS=/nonexistent-hot-lists` (378 + 3 passed) and with all three unset (378 + 3 passed).
- htr4-lint2 (ea4bbdf7c, the final code), rc 0: the same script; 379 + 3 passed in both environments, the new
  cache test included.
- htr4-plants1 (ea4bbdf7c), rc 0: 7 plants, 7 CAUGHT, 0 SURVIVED, no WARN (handoff/HOTLIST-TRAIN-REGEN-4-plants.txt),
  run with `LIELLA_TRAIN=1` set for the whole run as in the CI gate. `train-from-process` puts the leak back (the
  resolver also reads the process's `LIELLA_TRAIN`) and is caught by `branch_reads_the_fresh_copy_with_an_info_line`.
- No `affected` gate: after the red, the brief's rule is to run only the failing suite (the xtask tests), and the
  previous branch gate escalated to FULL for any xtask source change. Later commits touch only handoff/.
- Step 0 of the brief (lint on the inherited head before any change) was not run separately: the change touches only
  hotlists.rs, and htr4-lint1 ran clippy and static on the first fix commit.

## Not done / next (unchanged from -2, HOTLIST-TRAIN-REGEN-3 owns them)
- The seed-key split, a fatal missing STAMP under `LIELLA_TRAIN=1` (#5162) and a regen.sh test are HOTLIST-TRAIN-REGEN-3.
- The brief's proof (a)/(b) of HOTLIST-TRAIN-REGEN-2 (two dummy branches, train command, both profiles) is still unrun.
- INFORMATIONAL: `hottext::tests::regen_mode_is_one_explicit_flag_for_regen_sh_only` branches on the process's `CI`
  (it passes either way, but each environment checks only half of it). Passing `CI` in explicitly, as here, would
  fix that; it is outside this story's scope.
Plan rows: D-X.49, D-X.50, D-X.59, D-X.62, D-X.63, D-X.66, D-X.82 (the train-owned lists).
