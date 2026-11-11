# XTASK-AFFECTED-LAYOUT: changes linked into libc.a select the start-up layout tests

State: branch `task/xtask-affected-layout` (from origin/next), code commit 21178e7cc; built and tested in role
shiki:~/w/sh2.

## What changed (xtask/src/affected.rs, rule 9 of the module doc)

- `LAYOUT_TESTS`: the test targets that measure a static program's whole linked image, found by what they measure:
  `liella::hot_text_products` (start-up and short-program fault bounds, hot-text range, `.rodata1`/`.data1`),
  `liella::heap_meta_products` (first allocation's text and metadata pages), `liella::cold_bss_products` (`.lbss`
  placement of libc.a's large tables). They link `-static -nostdlib` against `liella-crt1.o` + `libc.a` only, never
  ld.so or libm.a. Selecting them builds the products (`cargo xtask so`), and `so` ends with every layout contract
  (`layout::check_products`, D-X.48/D-X.49), so the start-up window budgets run too.
- `Repo::image` (`image_of`): the crates libc.a links = normal `[dependencies]` of `crates/libc-so/Cargo.toml`,
  transitively (boundary, core, kernel, rt, mem, ld, abi, legacy, math); modules a crate root declares only under
  `#[cfg(feature = "X")]` for a feature the libc product does not enable are out (boundary's `ld` and `arch`: built
  into ld.so only). Edges with `workspace = true` make a crate's features unknown, so nothing of it is left out.
- `Repo::reaches_image(f)`: an image crate's `src/**` (not test-only modules, not feature-gated-off modules),
  manifest or build script; `csys/src/**`, `csys/include/**`; `crates/libc-so/**` (hot-text list); `IMAGE_TOOLS`
  (hottext.rs, coldbss.rs, so.rs; FULL anyway). Applied to every changed file in `select_with`, in every tier, and in
  `math_coeffs` (a fit or the tables generator selects what its coefficient file selects).
- Not selected (direct tier, the default gate and CI's `--reverse`): docs, other crates (native, thread-db,
  mathtools), test files of other targets, `crates/boundary/src/ld.rs` and `arch/`. The wide tier's own rule 4
  still selects every no-section target for a boundary change, unchanged.
- Existing test `csys_change_is_full` now allows the layout tests next to FULL (FULL runs everything anyway).

## Tests and plants

- `image_changes_select_the_start_up_layout_tests`: libm source, coefficients, crates/ld, kernel, a `.S`, the math
  TSV select all three (direct and wide); ld.rs, arch/, a docs file, a math test file, native select none (direct).
- `image_model_and_layout_test_list_follow_the_tree`: image dirs in/out, `(liella-boundary, ld)` off, every
  `LAYOUT_TESTS` entry is a products target, and every products target whose own source counts `minflt` of a
  `-static` program is in the list (a new fault-bound test cannot be forgotten); parser cases for `link_deps`,
  `features_on`, `feature_gated_modules`.
- Plants: handoff/XTASK-AFFECTED-LAYOUT-plants.txt (rule removed, ld feature ignored, dev-deps counted).

## Results

All on shiki role sh2, code commit 21178e7cc:
- xtask-affected-layout-unit2: `cargo test -p xtask --bin xtask affected::` 78 passed.
- xtask-affected-layout-clippy, xtask-affected-layout-static: rc 0.
- xtask-affected-layout-gate: `cargo xtask affected --run --base origin/next` 2 suites, 343 passed, peak 1.2G.
- xtask-affected-layout-plants: 3 CAUGHT, 0 SURVIVED, no WARN lines.
- xtask-affected-layout-cost (`--only` the three targets, NOT A GATE): `test -p liella` step 50.2 s (17 tests,
  including compiling the three test binaries); products 280.4 s. A libm story already builds the products
  (special_products/elem_products are selected), so the added cost of a typical libm gate is about 50 s.
- Superseded: xtask-affected-layout-unit (first head, 3 failures: two existing tests' invariants, fixed as above).

## Open / next

- No decision record: the brief reserved no id. If the coordinator wants one, a D-X row: "rule 9: changes reaching
  libc.a select LAYOUT_TESTS (XTASK-AFFECTED-LAYOUT)".
- Next step: none for this story; land.
