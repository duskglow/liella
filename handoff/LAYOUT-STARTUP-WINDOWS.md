# LAYOUT-STARTUP-WINDOWS: the start-up text window contract, checked at product build

**State (2026-11-10, LAYOUT-STARTUP-WINDOWS-2):** branch `task/layout-startup-windows-2` (continues
`task/layout-startup-windows`, stacked on `task/layout-manifest-2`), merged with origin/next 03c9a4972 (post-RECORDS).
Code head e13b5ad6b (second merge with next b14f639cc, where LAYOUT-MANIFEST landed); handoff commits follow it. Role shiki:~/w/sh2 (Skylake-SP). Decision **D-X.49** is
`records/decisions/D-X/D-X.49.toml`. Plan: `handoff/layout-contracts-plan.md` findings 1-3, section 3. Full report:
`handoff/LAYOUT-STARTUP-WINDOWS-report.md`.

## What exists
- `layout/contracts.tsv` row STARTUP-TEXT-WINDOWS, kind `startup-text-windows` (`xtask/src/layout.rs`), rule
  `keys=crates/libc-so/startup-text.keys; window=65536; budget-fast=5; budget-release=4; grouped=error`. `cargo xtask so`
  runs it last; `cargo xtask layout-check <dir>` reads `<dir>/.profile`. Failure: over budget, a function outside the
  hot group straddling a window boundary, or a libc.a start-up function outside the group (GROUPED).
- `tools/hottext/regen.sh` also traces `page_faults true` and writes `crates/libc-so/startup-text.keys` (`@generated`).

## Decisions in force
- D-X.49 (this story), a record since the merge. D-X.48 landed on next with LAYOUT-MANIFEST; this branch takes next's record. (Before that landing, D-X.48 (LAYOUT-MANIFEST, stacked underneath) is a record on this branch
  too: the merge took next's generated view, and the D-X.48 contract rows of `layout/contracts.tsv` need the record for
  the `layout` static check. Its body is LAYOUT-MANIFEST's row text, with story LAYOUT-MANIFEST.)

## Not to redo
- Merge with next is done (views are next's; main.rs keeps next's 31 static checks with `layout`).
- Release budget on the merged head is measured: 4 windows (101 of 101 keys), fast 5 (107 of 107), every contract holds
  (lsw2-measure-8e5a92e87). The earlier "110 of 110" fast count was on the pre-merge base.
- lsw-key-removed was re-planted at tier `extra`, profile `-` (`handoff/LAYOUT-STARTUP-WINDOWS-2-plants.txt`).

## Open questions
None.

## Next step
Land once lsw2-gate-e13b5ad6b and lsw2-plants-e13b5ad6b (shiki) are green. A later raise of the `hot_text_products.rs` bounds quotes this check (D-X.49 rule d).
