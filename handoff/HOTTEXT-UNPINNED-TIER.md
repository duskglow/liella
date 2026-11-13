# HOTTEXT-UNPINNED-TIER: hot-text.unpinned's tier column no longer blocks the train's regen

State: branch `task/hottext-unpinned-tier` from origin/next, code commit `af564205f`; this note and the report are
the commit on top of it. Role mei:~/w/s3.

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

## RULING NEEDED
The train's regen will keep failing static whenever it adds a Rust key without a boundary or drops a key that has an
unpinned line. This is the same coupling between a hand-written input and generated output. Recommendation: open a
story that (1) has the regen's merge mode report new boundary-less Rust keys and either keep them out of the list or
write the unpinned line itself with the reason "added by regen, boundary cost not measured yet". The ratchet allows
only removals, so that write needs a ruling. (2) Under the train, treat a line whose key the regen dropped as removable:
the train deletes the line in the same commit. A short-term alternative is a story that pins or lists the 18 keys
above with counts.

## Things not to redo
Rewriting the tier from the regen, and dropping the column, were both considered and rejected (above).

## Next step
Collect hut-gate. Then rule on the regen key-set coupling above. Plan rows: D-X.77, D-X.82, and the pending record.
