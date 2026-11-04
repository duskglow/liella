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
