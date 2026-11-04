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

## Run 7 finding (plants)

The run-6 survivors were NOT a plan-command problem. The sweep injects through Rust's global allocator only
(`selftest::failinject`, documented header: "does NOT reach ... the caller-owned malloc family"). The three plant sites
(`dirtree::alloc_text`, `scandirat`'s `dup_bytes`, `kept.reserve(1)`) allocate through `caller_alloc::calloc` (the C
layer forwarder `liella_caller_calloc`), which the hook cannot fail in any pass (plain or exports-interposed: the
hook is in the test binary's global allocator, not in the C heap). So a wrong errno / swallowed failure there cannot be
observed by any test today. `kept.reserve(1)` is equivalent by design (the following push re-checks).

To make them catchable one needs a `cfg(feature = "selftest")` check of `failinject::should_fail` inside
`caller_alloc::{malloc,calloc,realloc}`; that edits a product-compiled file, which the failinject header says needs
coordinator/owner approval (#2101). Not done. Alternative with no product edit: none found (the symbol is a C-layer
forwarder resolved at link time).

Next step (if the ruling is yes): add the selftest-gated hook, then add `scandir`/`opendir`-with-caller-alloc-failure
cases to enomem_fs_walk.rs and rerun the three plants (run them with `cargo test -p liella --test enomem_fs_walk`
in a NEW RAM copy dir; plain cargo test suffices, the hook is in-process).
