# XTASK-BENCH-STAGE (COMPLETE)

State: branch task/xtask-bench-stage, base origin/next ff125cce. Code commits 5c316320 (feature) and 49ec9869
(test fixture host name, plants file); this handoff in the commit after them. Built/tested on shiki role sh1.

## What it does
- `cargo xtask bench --stage <out.tar> [--only ... | --rows ... | --affected <base>]`: builds as `--build-only`,
  then tars `Cargo.toml`, `xtask/Cargo.toml`, `bench/`, `target/liella/` (minus `bench/stress/`), the selected Rust
  examples (`target/release/examples/<b>`), the running xtask binary as `xtask-bin`, and `bench-stage.manifest`
  (commit, sha256 of the source tree, profile, built_on, `product <sha256> <path>` per staged file). tar keeps mtimes.
- `./xtask-bin bench --no-build --staged` (or `--no-build` in any tree holding the manifest): checks every listed
  product's sha256 (coreutils `sha256sum`) before anything else; a mismatch or missing file is refused. In that run
  `require_fresh` / `host_build_cached` only require the program to exist (static `STAGED`). No git, no build-host
  path; header line `staged: commit ..., tree ..., profile ..., built on ...`. `--staged` without a manifest is refused.
- Commit: `git rev-parse HEAD` (+dirty), else `head=` of the role dir's `.synced` (+dirty(<hash>)), else `unknown`.
- Root discovery (main.rs): an ancestor holding `bench-stage.manifest` also counts as the tree root.
- Code: xtask/src/benchstage.rs (new), xtask/src/bench.rs (options, checks, header), xtask/src/main.rs.
- docs/benchmarks.md section 6: the two-command recipe replaces the hand-staging paragraph.

## Results (shiki, sh1)
- Direct gate xbs-gate2: `cargo test -p xtask` under ~/bin/gate 16G: 199 passed, 0 failed; peak 871M, rc 0.
  (`cargo xtask affected --base origin/next` escalates any xtask/src change to FULL (xbs-gate1, plan only, nothing run);
  FULL is the coordinator's, so the direct tests are xtask's own suite.)
- clippy and static clean (xbs-static3).
- Plants xbs-plants1 (`cargo xtask plants handoff/XTASK-BENCH-STAGE-plants.txt --dir /mnt/zram/claude/XTASK-BENCH-STAGE/plants-1`):
  5 CAUGHT, 0 SURVIVED, 0 WARN (skip-hash-check, staged-keeps-mtime-check, staged-flag-without-manifest,
  stage-includes-stress, manifest-accepts-dotdot).
- End-to-end xbs-e2e1 (scratch script, copy of the tree, real release build): `bench --stage --only cbench_startup`
  wrote a 37 MB tar (13 products); unpacked fresh, bench/*.c, budgets.tsv and Cargo.toml touched newer;
  `--no-build --staged` measured cbench_startup (all 12 budgets met), header named commit 49ec9869; without the
  manifest it refused (libc.a older than its inputs); after appending one byte to libc.a it refused by hash.
  These were functional runs on a build host, not timings.
- Tests: `benchstage::tests::staged_tree_runs_without_mtimes_and_refuses_a_changed_product`,
  `manifest_round_trips_and_rejects_bad_lines` (no spec clause: developer tooling).

## Open / for the coordinator
- RULING NEEDED: a decision row for the manifest/staging design. `ops reserve --story XTASK-BENCH-STAGE D-X` fails
  ("series must be COMPAT or D-<section>"), so no D-X row was added. Recommendation: reserve the next D-X id and
  record "staged bench trees are checked by per-product sha256 in bench-stage.manifest, not by mtimes".
- `bench --compare --no-build` (the `compare` path) is not staged-aware; it checks only that programs exist, so
  it never had the mtime problem. `--stage` is on the plain `bench` command only.
- The developer-flow line pointing to docs/benchmarks.md section 6 is the coordinator's (brief item 4).

## Not to redo
The build, the plants and the end-to-end check (all green on 49ec9869; this commit changes only handoff files).
Next step: coordinator review and landing; first real use is the next timing story (stage on a build host, scp the tar).
