# HOTTEXT-UNPINNED-TIER: hot-text.unpinned's tier column no longer blocks the train's regen

State: code commit `af564205f` (first run, branch `task/hottext-unpinned-tier`, role mei:~/w/s3). The continuation
HOTTEXT-UNPINNED-TIER-2 (branch `task/hottext-unpinned-tier-2`, same commits plus handoff updates, role keke:~/w/n1)
changed no code: it re-ran clippy, static, the gate and the plants on its head (see "Continuation" below).
Pending decision record: `records/pending/HOTTEXT-UNPINNED-TIER-1.toml` (D-NEW, series D-X; the coordinator assigns the id).

## What changed
- `xtask/src/hottext.rs::check_pinned` no longer compares the tier column of `crates/libc-so/hot-text.unpinned`
  with the key's tier in `hot-text.list`. The parse format is unchanged (`<key>\t<tier>\t<reason>`, the tier must
  still be a tier name); the column is now a note (the tier when the line was written). A key without a fixed
  boundary still needs a line, and a line whose key gained a boundary or left the list still fails.
- File header of `hot-text.unpinned` and the `UNPINNED_LIST` doc comment say so.
- Pending record `records/pending/HOTTEXT-UNPINNED-TIER-1.toml` (series D-X, amends D-X.77 (1)).
- Test `a_key_that_moves_tier_in_a_regen_leaves_static_green` (new); `startup_keys_without_a_fixed_boundary_fail_unless_listed` updated.

## Why this option (brief item 1)
The only reader of the tier column was the comparison. Since ruling #5024 every tier is checked the same way
(`INLINE_NEVER_TIERS` = all tiers), and none of the accepted reasons (#4997) depends on the tier, so asking for a
key to be "justified again for its new tier" asked for nothing new. Rejected options:
(a) Have the regen rewrite the column. The file is a hand-written input read from the tracked tree, so off the
train every branch that ran `hot-lists` would get a diff in a hand-written file, and the train would have to commit
a file it does not own.
(b) Drop the column. That edits every line, so it would conflict with every in-flight branch that removes a line,
and removing lines is the ratchet's only edit.

## Results
- clippy + static on the role: green (job hut-clippy).
- Plants (`cargo xtask plants`, job hut-plants): 2/2 CAUGHT, no WARN lines (`handoff/HOTTEXT-UNPINNED-TIER-plants.txt`).
  The plants baseline ran `cargo test -p xtask --bin xtask hottext::` green.
- Gate (job hut-gate, `cargo xtask affected --run --base origin/next`): the tool escalated it to FULL, because xtask
  and a product linker input changed. It was STILL RUNNING at hand-back. Re-attach: on mei, `~/bin/jobwait hut-gate`,
  then `~/bin/test-summary ~/logs/hut-gate.log`.
- Train sequence on next + this branch (job hut-train-seq; a RAM copy of the role;
  `cargo xtask hot-lists --in-place` for both profiles, then `LIELLA_TRAIN=1 cargo xtask static`). The regen moved one
  unpinned key's tier, `catalog::textdomain` (extra -> tool, the train 654 case), and static raised no tier error for
  it. **Static still failed (rc 1) for reasons outside this story.** The regen added 22 keys and dropped 2:
  - 18 keys have no fixed boundary and no unpinned line: 15 tool (`nscache::*`, `names::cache::Table::*`,
    `nss::passwd`, ...), 2 locale (`text::builtin_charset`, `CtypeFile::wide`) and 1 extra (`Heap::locate_in`).
    17 of them are confirmed absent from the tracked list. `builtin_charset` was not classified, but static on the
    tracked list passes, so the regen brought it in.
  - 2 unpinned lines name keys the regen dropped (`global_locale::setlocale::<..>`, `iconv::Conv::to_words`).
  So brief item 2 (the train's sequence passes) is NOT met. The brief forbids changing which keys are listed or
  pinned, so these were left alone.

## Regen key-set coupling (RULED, #5205)
Ruling #5205 (coordinator, 11-10): no new story for the 18 boundary-less keys and the 2 dropped-key lines. The
train's hot-list step is paused (`ci_train_hot_lists=0`) until LT-5 deletes it (owner rulings #5192/#5193,
plan/layout-rethink.md). hut-train-seq is SUPERSEDED for that reason; the tier change lands on its own.
The first run's analysis, kept for LT-5: the train's regen will keep failing static whenever it adds a Rust key without a boundary or drops a key that has an
unpinned line. This is the same coupling between a hand-written input and generated output. Recommendation: open a
story that (1) has the regen's merge mode report new boundary-less Rust keys and either keep them out of the list or
write the unpinned line itself with the reason "added by regen, boundary cost not measured yet". The ratchet allows
only removals, so that write needs a ruling. (2) Under the train, treat a line whose key the regen dropped as removable:
the train deletes the line in the same commit. A short-term alternative is a story that pins or lists the 18 keys
above with counts.

## Things not to redo
Rewriting the tier from the regen, and dropping the column, were both considered and rejected (above).

## Continuation (HOTTEXT-UNPINNED-TIER-2, keke:~/w/n1)
- Step 0 on the inherited head: `cargo xtask clippy` + `cargo xtask static` green (job hottext-unpinned-tier-2-lint).
- hut-train-seq (mei, rc 1): the 20 key-set errors above; SUPERSEDED by ruling #5205, not a defect of this change.
- hut-gate (mei): left no jobs.tsv row and an empty log; superseded by the gate below.
- Gate: job hottext-unpinned-tier-2-gate, `cargo xtask affected --run --base origin/next` (the tool escalates to FULL:
  xtask and a product linker input changed). Plants: job hottext-unpinned-tier-2-plants, same plants file as the
  first run. Results in handoff/reports/HOTTEXT-UNPINNED-TIER-2.json and the hand-back.

## Next step
None for this story: land it once the gate and plants are green. The key-set coupling goes with LT-5 (#5205).
Plan rows: D-X.77, D-X.82, the pending record.
