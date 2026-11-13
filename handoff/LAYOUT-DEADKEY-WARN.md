# LAYOUT-DEADKEY-WARN: a dead hot-text key warns; it fails only under LIELLA_PROMOTE=1

State: branch `task/layout-deadkey-warn` from origin/next (b888f8ff0). Commits: da2d80257 (code, CONTRIBUTING.md,
pending record), c636b1122 (plants file), then this note and handoff/reports/LAYOUT-DEADKEY-WARN.json.
Decision: records/pending/LAYOUT-DEADKEY-WARN-1.toml (D-NEW, series D-X, amends D-X.53 (2); owner ruling #5193).

## What changed (xtask/src/hottext.rs)
- `PROMOTE_ENV = "LIELLA_PROMOTE"`; `enum DeadKeys { Regen, Warn, Promote }`. `DeadKeys::of(regen, value)` is pure
  (Promote only for exactly `1`; the regen flag wins); `DeadKeys::from_env()` is the one reader of the process's
  environment, called by `group` and `repair_cmd`. Tests pass the mode explicitly (HOTLIST-TRAIN-REGEN-4 pattern).
- `dead_key_verdict(dead, stale, profile, file, mode) -> Result<Vec<String>, String>`: Warn prints
  `warning: dead hot-text key <key> (<tier>) in <list path>: matches no function section of the <profile>-profile
  libc.a` per key and a count line; Promote fails with the old message plus a note; Regen prints as before. A stale
  `hot-text.absent` line still fails in Warn and Promote.
- `group` (so, the gate, the plants baseline, hottext-recheck) and `check_lists` (hottext-repair's final check) use it.
- Doc comments and the main.rs command table updated; CONTRIBUTING.md names the flag (hand-written inputs paragraph).

## Survey: every place a dead key failed (brief item 1)
- `hottext::group` via `cargo xtask so` (so.rs:918), and so the gate and the plants baseline (plants.rs runs `so`
  first): changed.
- `hottext-recheck` (group again) and `hottext-repair` (`check_lists`): changed.
- `cargo xtask static`'s `hottext` check (`static_check`): no dead-key check (regen-flag naming + pins only);
  pins unchanged per the brief.
- startup/probe/data keys: `startup_ro::place` and `coldbss` have no dead-key failure; layout.rs's start-up text
  and data contracts list missing keys as information and fail only when NONE of the keys is found. Unchanged
  (that is a layout contract, item 3). `startup-state.list` (coldbss) is a hand-written data list, not a key file.

## Results (shiki role sh1; logs ~/logs/<label>.log)
- ldw-lint1 (da2d80257), rc 0: `cargo xtask clippy`, `cargo xtask static`, `cargo test -p xtask -- hottext::`
  with LIELLA_PROMOTE unset and with LIELLA_PROMOTE=1 (21 passed each; the new test included).
- ldw-plants1 (c636b1122), rc 0: 8 plants, 8 CAUGHT, 0 SURVIVED, 0 EQUIVALENT, no WARN
  (handoff/LAYOUT-DEADKEY-WARN-plants.txt). Its baseline `so` printed no dead-key warning: next has none today.
- ldw-gate1 (c636b1122, `cargo xtask affected --run --base origin/next`): escalated to FULL by itself (any
  xtask/src change does); left running at hand-back for the dispatcher to collect.

## Not done / next
- The coordinator's promotion run must set `LIELLA_PROMOTE=1` (nothing in the repo sets it).
- LT-5 deletes the key lists; this ruling is interim until then.
Plan rows: D-X.53, D-X.47, D-X.56; plan/layout-rethink.md section 3.2 (task/layout-rethink).
