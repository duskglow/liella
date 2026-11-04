# ENOMEM-FS-WALK handoff (run 6)

State: branch task/enomem-fs-walk-6 (stacked on walk-5 + origin/next merge). The fs-order lint failure is fixed
(FS-ORDER markers on case_scandir, case_fts, case_nftw in crates/core/tests/enomem_fs_walk.rs).
Verified on kanon (role kn1): cargo xtask static green (fs-order ok), clippy --workspace --all-targets --all-features
-D warnings green, direct gate `enomem-fs-walk-6-gate` rc 0.

NOT PROVEN: plants. Job `enomem-fs-walk-6-plants` (baseline green) ran 3 plants with plain
`cargo test -p liella --test enomem_fs_walk`: alloc-text-wrong-errno SURVIVED, scandir-dup-failure-swallowed SURVIVED,
scandir-reserve-failure-swallowed EQUIVALENT (push? re-checks). The survivors are most likely because the failure
injection only bites in the exports-interposed pass that `cargo xtask affected` runs, not in a plain cargo test (not
confirmed: no budget left). Plants file: kanon ~/work/ENOMEM-FS-WALK-6/plants.txt.

Next step: find how `cargo xtask affected` runs the interposed pass for enomem_fs_walk (likely `--with-exports` or
the exports-interposed tier) and use that as the plant `cmd:`; rerun in a NEW --dir (plants-2). If the plants still
survive, the sweep does not reach these allocation sites (check the scandir path really goes through
crates/boundary/src/dirtree.rs alloc_text/dup_bytes) and the test needs strengthening.
Do not redo: walker fixes, fts slot-pool warm-up, kept-result handling.
