# XTASK-RECORDS-GEN-OUT-OF-TREE: off the train, records-gen and kernel-index write under target/records-views/

State: branch `task/xtask-records-gen-out-of-tree` (cut from origin/next). The code commit is 3ac11f804; the hand-back
head is the commit that carries this note. Decision in force: D-X.59 (amends D-X.55).

## What changed
- `cargo xtask records-gen` without `LIELLA_TRAIN=1` and without `--in-place` renders every view (38 today) to
  `target/records-views/<view path>`, prints where they went, and leaves the tracked views byte-identical. With
  `LIELLA_TRAIN=1` or `--in-place` it writes in place as before (only stale views are rewritten).
- `cargo xtask kernel-index`, and `cargo xtask tables`, which calls it, follow the same rule for
  `crates/math/KERNELS.md` (it is train-owned too, D-X.55). It writes
  `target/records-views/crates/math/KERNELS.md` off the train. `tables` strips `--in-place` before it calls the
  mathtools `tables` binary.
- `--check` is unchanged in both: it compares the tracked files. static's `records-gen` and `tables-fresh` steps are
  still fatal under LIELLA_TRAIN=1 and advisory elsewhere.
- Shared helpers: `records::{OUT_OF_TREE, in_place, in_place_env, out_root}`.
- Updated text: the xtask usage line and the command table in main.rs, the module docs of records/mod.rs and
  kernel_index.rs, and CONTRIBUTING.md (decision-register bullet). No other file under docs/ told agents to
  regenerate and then restore.

## The train: ACTION NEEDED in liella-ops (I could not change it: read-only for me)
`ops/ci.py` line 432 has `TRAIN_GENERATORS = ["cargo xtask records-gen", "cargo xtask kernel-index"]`, and
`regenerate()` (around line 1761) runs them as `{ cargo xtask records-gen && cargo xtask kernel-index; }` with no
LIELLA_TRAIN in the environment. Once this branch lands, the train would therefore write under target/. That
directory is pruned from its `find -newer target/regen.stamp`, so nothing would be committed, and the gate that
follows (LIELLA_TRAIN=1 static) would then go red on the stale views. The fix is
`TRAIN_GENERATORS = ["cargo xtask records-gen --in-place", "cargo xtask kernel-index --in-place"]` (or
`LIELLA_TRAIN=1 cargo xtask ...`). Both forms work with the current next as well: the old `records-gen` and
`kernel-index` look only for `--check` and ignore every other argument. So **change ci.py first, then land this
branch.** `regen_cmd()`'s path map (ci.py lines 428-429) names the same commands and needs the same suffix if it is
ever run.

## Rules sentences for the coordinator to change (liella-ops)
- rules/developer-flow.md lines 254-255: "To read a view locally, `cargo xtask records-gen`, then `git checkout --`
  the views before committing." Change it to: "To read a view locally, run `cargo xtask records-gen` (or
  `cargo xtask kernel-index`) and read it under `target/records-views/<same path>`; the tracked views are not
  touched, so there is nothing to restore (D-X.59)."
- rules/thread_common.txt does not mention records-gen. Grep the briefs template for "records-gen" and "checkout --"
  too: briefs that copy the old sentence should follow the line above.

## Tests and proof
- Unit tests (each names D-X.59, no spec clause): `records::tests::gen_off_train_writes_under_target_and_leaves_tracked_views`
  (one stale and one absent tracked view stay as they are; every view is written out of tree; `--check` still sees
  the stale views), `records::tests::in_place_only_on_the_train_or_when_asked`, and
  `kernel_index::tests::writes_out_of_tree_unless_in_place`.
- Plants (`cargo xtask plants`, mei:xrgo1-plants): 4 CAUGHT, 0 survived, no WARN lines
  (handoff/XTASK-RECORDS-GEN-OUT-OF-TREE-plants.txt). This includes "off-train writes in place", with msg.
- End to end on the role (mei:xrgo1-e2e): records-gen and kernel-index left the md5 of every tracked view and of
  KERNELS.md unchanged and wrote 39 files under target/records-views/.
- clippy mei:xrgo1-clippy rc 0; static mei:xrgo1-static rc 0; direct-tier gate mei:xrgo1-gate (see report json).

## Notes
- D-X.59.toml was written by hand in the canonical skeleton form, because there is no cargo on the agent host.
  `records check` in static passes on it.
- Not done: the view-ownership rule in ops/ci.py (`is_generated`) is unchanged. It still holds a branch that commits
  a view.

Next step: the coordinator changes TRAIN_GENERATORS (above), then lands this branch, then updates the
developer-flow sentence.
