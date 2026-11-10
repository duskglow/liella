# LAYOUT-MANIFEST: layout contracts manifest, checked when the products are built

**State (2026-11-10, LAYOUT-MANIFEST-2):** branch `task/layout-manifest-2` (stacked on `task/layout-manifest`, base
0b03e0dca: before the RECORDS migration on next). Code head 36babbbfe. Role keke:~/w/r5. Decision in force: **D-X.48**
(reserved for this story). It registers D-4.33, D-X.34, D-4.67, D-X.22, D-4.56 and D-4.63. Ruling #4101 is accepted:
rule d stays a static failure. Plan: `handoff/layout-contracts-plan.md` (2a A1+A3, 2b B1, 2d).
Full report of this run: `handoff/LAYOUT-MANIFEST-2-report.md`.

## What exists
- `layout/contracts.tsv` has 10 rows, `xtask/src/layout.rs` and `cargo xtask layout-check [--static | <products dir>]`.
  `cargo xtask so` runs the check after the last product. `static` runs the `layout` check: kinds, decision ids,
  `LAYOUT(<ID>)` tags, and rule d.
- (run 2) `affected` treats the manifest and `layout.rs` as known consumers of csys symbols (`LAYOUT_CONSUMERS`). A
  csys file whose symbols they name selects `cargo xtask layout-check`, and the products are built first
  (`needs_products`). It no longer escalates to FULL. The csys symbol scan reads `layout/*.tsv`. This fixes the red
  `csys_falls_back_to_full_when_unsure` in gate lm-gate-8c57b90 (mei). New test:
  `layout_manifest_names_select_the_layout_check`.
- (run 2) Decisions are read from `plan/07-decisions/*.md` rows AND `records/decisions/**/D-*.toml` (D-X.55). For
  rule d, a change under either place counts as a decision change. Without this, after the merge with next, rule d
  would have demanded an edit to a generated view.

## Open questions / for the release manager
- **Merge with next (RULING NEEDED):** this branch predates RECORDS. `git merge-tree` against origin/next conflicts in
  `plan/07-decisions/x-implementation-decisions.md` (this story's D-X.48 row; the file is a generated view on next) and
  `xtask/src/main.rs` (adjacent lines: `mod layout;` and the new doc rows; trivial). After the merge, D-X.48 must become
  `records/decisions/D-X/D-X.48.toml` (`cargo xtask records new decision D-X.48`, body = the row's text) and the
  view edit must be dropped. The brief did not allow a merge, so this was not done.
- STARTUP-NAMES-WINDOW, GETTEXT-SLOTS-PAGE and PRODUCT-PATH-REMAP name D-X.48 (they had no D-row).
- `docs/codemap/README.md` "Layout contracts" section: that file says a scratchpad `codemap.py` generates it, so a
  regeneration may drop the section.
- Release-profile products: `layout-check` was not run on them (NOT VERIFIED).

## Next step
Merge origin/next (once allowed), convert D-X.48 to a record, run `cargo xtask records check`, static and clippy, and
then the direct gate. After that, LAYOUT-STARTUP-WINDOWS adds its kind as a row.

## Not to redo
The readelf and objdump formats of the products were probed (archive `File:` blocks, JCC `cs` prefixes on padded
instructions). Symbols are found through their section index, never by section name (`.text.sorted.h<n>.*` varies).
The plants of run 1 (`handoff/LAYOUT-MANIFEST-plants.txt`, 8 of 8 CAUGHT, mei lm-plants-8c57b90) cover the check
kinds. Its `lm-rule-d` line targets code that run 2 changed. Run 2's plants are in `handoff/LAYOUT-MANIFEST-2-plants.txt`.
