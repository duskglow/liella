# LAYOUT-MANIFEST: one manifest of layout contracts, checked when the products are built

Id: LAYOUT-MANIFEST
Kind: code (tooling)
Size: L
Model: opus
Effort: high
Plan: handoff/layout-contracts-plan.md (sections 1, 2a A1+A3, 2b B1, 2d; proposed D-X.NEW-1). RULING 1 (rule d as a
static failure) should be settled before step 5.

## The change

1. `layout/contracts.tsv` (header row; columns `id kind subjects rule reason decision`). One row per inventory item
   L4-L13 of the plan that has a checkable rule today, each naming its existing D-row: FN-ALIGN-EXPORTS (D-4.33,
   kind `fn-align`, today `align::check`), CLASSED-XREF (D-X.34, `classed-xref`), JCC-HOT-ASM (D-4.67, `jcc`),
   COLD-BSS-LBSS (D-X.22, `section-rename`, today the `cold_bss_products.rs` test), STARTUP-NAMES-WINDOW (`object-window`,
   the startup-names object inside one 64 KiB window and in `.rodata1`), GETTEXT-SLOTS-PAGE (`object-align` 4096),
   MEMCHR-SSE2-PIN and RAWMEMCHR-SSE2-PIN (D-4.56; `section-offset`: the pinned offset and the section alignment
   >= 64 in `libc.a`), RAWMEMCHR-LOOP-32 (D-4.63; `label-align`), PRODUCT-PATH-REMAP (`remap`).
2. `xtask/src/layout.rs` and `cargo xtask layout-check [<products dir>]`: reads the manifest, dispatches each row to
   its kind (existing code is called, not copied), prints one line per contract and fails with
   `layout: <ID> BROKEN: <symbol or section>: <what moved>: <rule> (decision <D-row>)`. A row whose kind is unknown
   or whose subject is absent from the products is an error, never a skip (as `classed` does).
3. Run it at the end of `cargo xtask so` for both profiles, replacing the direct calls of `align::check`,
   `classed::check_file` and `jcc::check_archive` there (keep their own subcommands).
4. Site tags: `LAYOUT(<id>)` in a comment next to each `.p2align`/`.org`/`repr(align)` a row protects. `static`: every
   tag names a row; every row of a kind with a source site has at least one tag; a deleted tagged directive fails.
5. Rule d in `static`: a row names an existing D-row; with `.changed` (as `affected` reads it), a changed `rule` or
   a removed row without a change under `plan/07-decisions/` fails.
6. Plants: one per kind (wrong alignment, a section renamed back, `.org` offset changed, a tag removed, a manifest row
   with an unknown D-row, a budget changed without a decision change). Run through `cargo xtask plants`.
7. Docs: a "Layout contracts" section in `docs/codemap/README.md` pointing at the manifest; D-X.NEW-1.

Out of scope: the start-up window kind (LAYOUT-STARTUP-WINDOWS adds it as a row) and data windows
(LAYOUT-STARTUP-DATA).
