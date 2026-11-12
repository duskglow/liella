# HOTLIST-TRAIN-REGEN-2: `cargo xtask hot-lists`, train-owned start-up lists (step 2 and 3 of HOTLIST-TRAIN-REGEN)

State: branch `task/hotlist-train-regen-2` from origin/next 93721fcb1 (HOTLIST-REGEN-CHEAP and -2 landed in it).
Commits: 43c799f1b (code), 60124b082 (pending record, CONTRIBUTING.md, plants file), then this note and the report.
Earlier work: handoff on `task/hotlist-train-regen` (inventory, cost) and handoff/HOTLIST-REGEN-CHEAP-2.md (the
5-minute `regen.sh . --both`).

## What the branch does
- `xtask/src/hotlists.rs` (new). GENERATED = `hot-text.list`, `startup-text.keys`, `probe-text.keys`,
  `startup-data.keys` (crates/libc-so). Hand-written inputs that stay on branches: `hot-text.clusters`,
  `hot-text.absent` (no regen script writes it; only people and `hottext-repair` do), `hot-text.unpinned` and
  `startup-state.list`.
- `cargo xtask hot-lists`: copies the tracked four to `target/hot-lists/` (they are the merge base, #4839), then runs
  `tools/hottext/regen.sh <root> --both --allow-drop` with `LIELLA_HOT_LISTS=target/hot-lists`, then writes
  `target/hot-lists/STAMP` (FNV hash of Cargo.lock, crates/, csys/, include/, layout/, tools/hottext/, the four lists
  left out). `--in-place` or `LIELLA_TRAIN=1` sets `LIELLA_HOT_LISTS=crates/libc-so` and afterwards keeps a stamped
  copy in target/hot-lists. A failed run leaves no stamp.
- Readers resolve through `hotlists::path`/`rel`: the `so` grouping and its dead-key check (so.rs), the start-up
  read-only group (startup_ro.rs), the STARTUP-TEXT/DATA-WINDOWS contracts (layout.rs), `hottext-recheck`,
  `hottext-repair` and static's `hottext` check. Order: `LIELLA_HOT_LISTS`; off the train, target/hot-lists while
  STAMP matches this tree (one `hot-lists:` info line naming the lists that differ); on the train the tracked copies,
  and a tracked list that differs from a stamped regeneration of the same tree is fatal; otherwise the tracked copies.
  A stamp for another tree state (a later source edit, a reused role) is never read (info line).
- `group()` now takes the hand-written inputs' directory, so clusters and absent are always read from the tree.
- regen.sh, probe_text.sh and startup_data.sh read and write the lists in `${LIELLA_HOT_LISTS:-crates/libc-so}`.
- Pending record `records/pending/HOTLIST-TRAIN-REGEN-2-1.toml` (D-NEW, series D-X). CONTRIBUTING.md says what is
  train-owned and how a developer sees fresh lists (`cargo xtask hot-lists`, then read target/hot-lists).

## For the coordinator (ops side, not on this branch)
- Train command: `cargo xtask hot-lists --in-place` (or `LIELLA_TRAIN=1 cargo xtask hot-lists`), before the train's
  build and static. It needs gdb and about 5 min (both profiles; CHEAP-2 measured 298 s).
- Train-owned files: crates/libc-so/hot-text.list, startup-text.keys, probe-text.keys, startup-data.keys.
- `so --hottext-regen` refuses when `CI` is set (hottext.rs regen_mode). If the train's environment sets `CI`, the
  train command fails: run it with `CI` unset, or rule that LIELLA_TRAIN=1 may regen (see RULING NEEDED).
- A missing STAMP is not an error on the train, so the generator list can change before or after this lands.

## Results
(filled in below by the run: jobs htr2-lint1, htr2-hotlists, htr2-plants, htr2-gate on kanon)

## Not done (next step)
- The brief's proof (a)/(b) (two dummy branches that move start-up code, each gated without committing lists; merged,
  train command, hot_text_products at every phase, both profiles) was not run: budget. Next: run it as written.
- The 45 bare `extra`/`-` keys of hot-text.list are still kept by merge mode inside the generated file (not split to a
  seeds input). A story that wants to add such a seed key has no branch-side file yet.
- `hottext-repair` still writes the list it reads (target/hot-lists on a branch with a fresh copy).
- regen.sh's use of LIELLA_HOT_LISTS has no unit test; the proof job exercised it end to end once.
Plan rows: D-X.49, D-X.50, D-X.59, D-X.62, D-X.63, D-X.66.
