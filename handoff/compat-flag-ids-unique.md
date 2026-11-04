# COMPAT-FLAG-IDS-UNIQUE (run 4)

State: branch task/compat-flag-ids-unique-4 (continues -3), origin/next merged.
Done: `compat_ids` static check (one id per option, no gaps/dupes); fnmatch option is COMPAT-13, short-write COMPAT-14 (next's
row, kept), `__libc_freeres` stays COMPAT-11. Merge conflicts in docs/compat-flags.md and xtask/src/main.rs resolved by keeping
both sides (next's `compatprobe`/`compat_objects` modules, `fs-order` check, plus `compat-ids`; check array length 20).
Merge briefly left duplicate `mod` lines (E0428), fixed in the next commit.
Verified on shiki:~/w/sh2: `cargo xtask static` rc 0, workspace clippy -D warnings rc 0, gate job compat-flag-ids-unique-4-gate.
Plants for the check (duplicate id, gap) were CAUGHT in -3 (compatflagidsunique3-plants); compat_ids.rs unchanged since.
Not to redo: renumbering. Note: perror option (COMPAT-15) is not in this tree; its branch takes it.
Next: none beyond CI.

## Run 5
Merged origin/next again. next now holds COMPAT-13 (h_errno/networks), 14 (short write), 15 (perror), 16 (memstream); its own
fnmatch row (old id COMPAT-11) duplicated freeres. Resolution: fnmatch option is now COMPAT-17 (compat-flags.md row, differences row,
fnmatch_differential.rs comment); next's COMPAT-11 fnmatch row and our duplicate COMPAT-14 dropped; xtask usage line = next's.
Ids 1-17 unique. Job: compat-flag-ids-unique-5-gate on kanon (static + workspace clippy + affected --base origin/next).
No code change since the plants in -3 (compat_ids.rs untouched), so no new plants. NOT VERIFIED at hand-back: gate result.
Not to redo: renumbering beyond 17. Next: CI.

## Run 6
Merged origin/next again (right before push). next's syslog option took COMPAT-17, so the fnmatch option is now COMPAT-18
(compat-flags.md row, differences row, fnmatch_differential.rs comment); next's duplicate COMPAT-14 row dropped (ours identical).
Ids 1-18 unique. Job: compat-flag-ids-unique-6-static on shiki (cargo xtask static). Next: CI.
