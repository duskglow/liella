# XTASK-PLANTS-PRODUCTS-REBUILD handoff

State: branch `task/xtask-plants-products-rebuild`, code commit 4602c207 (plus this handoff commit on top). Role keke:~/w/n1 (synced tree, no .git).

## What changed
- `xtask/src/plants.rs`: for each plant, `cmd_uses_products(cmd)` decides whether the command may use the products. If it
  may, `cargo xtask so` runs in the copy after the plant is applied and before the command (log `<id>.so.log`). If the
  mutant's products do not build, the plant is judged on that log and the command is not run. A command is "unit only"
  only when it is one plain `cargo test` whose targets are all named (`--lib`, `--bin <n>`, `--test <n>`) and no named
  test target uses the products. Everything else rebuilds (shell syntax, `cargo xtask affected`, all targets, unknown
  option or target). After a products plant, the next unit-only plant first rebuilds the products unmutated
  (`<id>.so-restore.log`). Every verdict line ends `build: ...`: either "the baseline products, not rebuilt" or
  "rebuilt with the mutant in <n>s".
- `xtask/src/affected.rs`: `Repo::test_uses_products(pkg, target)` exposes the same products rule `affected --run` uses
  (target source or support names `target/liella`, `libc.a`, `liella-crt1` or `conformance`).
- `docs/plants.md`: a paragraph on which commands rebuild the products, and the new log names.
- Self-test plants: `handoff/XTASK-PLANTS-PRODUCTS-REBUILD-plants.txt` (u1 unit only, so no rebuild; pr in
  `crates/core/src/diag.rs`, seen only through `diag_products`; u2 unit only after pr, so a restore rebuild).

## Results (keke)
- `cargo xtask clippy`: clean. `cargo xtask static`: clean (job xppr-check, ~/logs/xppr-check.log).
- xtask plants unit tests: 13 passed (3 new: the toy products rebuild with a build count of 3; a mutant whose products
  do not build; the command classifier table).
- NOT VERIFIED at hand-back (started detached, for the coordinator to collect):
  - plants: label `xppr-plants`, log `~/logs/xppr-plants.log` on keke (keke). Expect u1 CAUGHT with
    `build: the baseline products, not rebuilt`, pr CAUGHT (rebuilt with the mutant), u2 CAUGHT with the restore rebuild.
  - direct-tier gate: label `xppr-gate`, log `~/logs/xppr-gate.log`
    (`cargo xtask affected --files <3 files> --tier direct --run` under `~/bin/gate 12G`).
- Not run: the same plants file under the old tool, to show pr SURVIVED there. The toy test shows that path by
  construction: its product only changes when it is rebuilt.

## Open questions / rulings
- None needed. Local tooling detail, decided here: a run of products plants pays one unmutated rebuild before the next
  unit-only plant. Ordering a plants file as unit-only plants first, then products plants, avoids that cost.

## Next step
Collect `xppr-plants` and `xppr-gate` on keke. All CAUGHT + gate green: ready. A WARN-NO-MSG on pr: check that
diag_products did not skip (it passes without a C compiler or localedef).
