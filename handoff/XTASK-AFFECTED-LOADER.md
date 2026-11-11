# XTASK-AFFECTED-LOADER: a loader-only change does not run the math crate's suites

State: branch `task/xtask-affected-loader` (from origin/next), code commit 2dbc6edac; built and tested in role
mei:~/w/s2. Brief: ~/state/briefs/XTASK-AFFECTED-LOADER.md.

## Cause
`cargo xtask affected` treats a crate root (`crates/ld/src/lib.rs`) as WIDE: every test target of every crate in
`Repo::dependents(liella-ld)`. That closure was crate-level and counted dev-dependencies transitively, so
liella-math (dev-dependency on liella-boundary with `selftest`; boundary depends on liella-ld) was a dependent and
every liella-math suite and its unit tests ran. A loader module (`init.rs`) in the direct tier did not select
liella-math; the root file did (probe job `xtask-affected-loader-list1`: lib.rs listed `lib: liella-math` and all
liella-math targets).

## Change (xtask/src/affected.rs, rule 4 note in the module doc)
* `build_closures` replaces the crate-level closure in `Repo::load`. A crate's own edges (normal, dev, build) always
  count. A dependency's onward edge `B -> C` counts only when `B`, with the features it gets in that crate's test
  build (union of every edge into `B` in the build, `workspace = true` resolved through the root manifest, plus
  `RUN_FEATURES`, the `--features` the run's second pass adds), compiles a module naming `C`. Module gates are read
  from the crate root: `#[cfg(feature = "X")]` and `#[cfg(any(feature = "X", ...))]` (`module_gates`); any other cfg
  counts as compiled. Edge kept when unsure: unknown features, features set by another crate's feature table
  (`liella/selftest`), no namer found.
* `dep_edges` generalises `link_deps` (dev and workspace tables, workspace resolution); `link_deps` and the
  layout image model are unchanged.
* The run's `--features` string is built from `RUN_FEATURES` (one source with the model).
* Applies in every tier (it is a fact about what the builds compile).

## Effect
`--files crates/ld/src/lib.rs` (direct): 88 of 257 test targets, 127 of 127 conformance programs, 3 unit-test libs,
no liella-math target or lib. Kept: liella-ld's own suites, boundary's, every conformance program, the start-up
layout tests. `crates/math/src/lib.rs` and `crates/rt/src/lib.rs` still select liella-math (and rt selects the loader).
Gate time before: DL-INIT-ORDER-2's gate (#4556) 98 min, of which liella-math 2203 s + 583 s interposed. After (one
measured run, `affected --run --files crates/ld/src/lib.rs`, job `xtask-affected-loader-ldtime-after` on mei): 435 s,
4 suites, 3712 passed, 0 failed, 127/127 conformance, peak 12G. Before and after ran on different hosts (keke, mei).

## Tests and plants
* `affected::tests::a_loader_change_does_not_select_the_math_suites` (no spec clause): loader root, `init.rs`,
  `boot.rs` select no liella-math suite (direct and wide); the root keeps loader/boundary/conformance/layout; math and
  rt roots select liella-math; `module_gates` and `dep_edges` parse cases.
* Plants: handoff/XTASK-AFFECTED-LOADER-plants.txt (narrowing removed; `any(...)` gate unread; run features widened
  with `ld`). A plant cutting every onward edge was dropped: no current test has a dependent reached only through a
  transitive normal edge, so it would survive (possible follow-up test).

## Not to redo / open
* crates/ld-so is not a workspace member: its changes escalate to FULL (product crate), unchanged.
* `crates/boundary/src/ld.rs` (direct) selects 0 test targets and 1 lib; unchanged by this story (INFORMATIONAL).

## Jobs (mei, role s2, code head 2dbc6edac; all rc 0)
* `xtask-affected-loader-lint`: `cargo clippy --workspace --all-targets --all-features -D warnings` + `cargo xtask static`.
* `xtask-affected-loader-gate`: `cargo xtask affected --base origin/next --run`: 2 suites, 344 passed, 0 failed, peak 1.4G.
* `xtask-affected-loader-plants`: 3 CAUGHT, 0 SURVIVED, 0 WARN (dir /mnt/zram/claude/XTASK-AFFECTED-LOADER/plants-1).
* Evidence only: `xtask-affected-loader-ldtime-after` (`affected --run --files crates/ld/src/lib.rs`, the after time),
  started at hand-back; probes `xtask-affected-loader-list1`, `-unit1` (unit1 had one wrong assertion, fixed).

## Next step
Land. Optional: a test with a dependent reached only through a transitive normal edge.
