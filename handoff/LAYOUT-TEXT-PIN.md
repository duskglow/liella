# LAYOUT-TEXT-PIN (brief A of LAYOUT-STABILITY-RETHINK): DO NOT LAND yet

**State (2026-11-10):** branch `task/layout-text-pin` from origin/next 9af6dff86; HEAD = the commit carrying this
note. Role mei:~/w/s2. Decision record `records/decisions/D-X/D-X.63.toml` (reserved id). Plants file
`handoff/LAYOUT-TEXT-PIN-plants.txt` (3 plants, not run to the end, see Jobs).

## What the branch does
- `xtask/src/hottext.rs`: every unlisted function section leaves GNU ld's pre-group patterns
  (`.text.unlikely.*`, `.text.startup.*`, `.text.exit.*`, `.text.hot.*` -> `.text.<f>`; a plain name that itself
  matches, e.g. `.text.exit` of the library's `exit` or `.text.*_unlikely`, -> `.text.liella_cold.<f>.liella_cold`).
  Section names come from the objects' own ELF section headers (`elf_sections`), not `readelf -SW` (cuts at
  256 bytes, #4354).
- `crates/libc-so/hot-text.clusters` (hand-kept, keys): time (local_in..days_from_civil64), strings
  (strlen..printf::vfprintf), rebind (bindtextdomain..Mo::form), float (simple_float..decimal_styles). `so` lays out
  each cluster's sections in name order and sets the head's `sh_addralign` = pow2 >= size (max 64 KiB) with
  `objcopy --set-section-alignment`. Fast sizes: time 18,090 B -> 32 KiB; strings 24,732 -> 32 KiB; rebind 38,476 ->
  64 KiB; float 16,202 -> 16 KiB.
- `crates/core/tests/hot_text_products.rs`: TEXT-PREFIX (`init_no_library_text_precedes_the_hot_group`),
  CLUSTER-WINDOW (`init_zero_fault_clusters_sit_in_one_window`), pad test (`init_zero_fault_calls_hold_at_every_text_phase`:
  crt1 `ld -r` with 0x800/0x2000/0x8000/0xF000 in `pre` and `cold`; mktime/strings/rebind 0, clusters in one window).
  xtask unit tests for the rename, cluster maths and the ELF reader (pass).

## Probe-settled facts
- The 34-byte `.text.startup` of the probe link is **crt1's `_start`** (`.text.startup._start`, liella-crt1.o), not
  libc.a: it stays (it is hot). The 7-byte `.text.exit` is **libc's own `exit`** (crate `c`, member
  `c-58626ef04aeb85cd.5re7k...rcgu.o`): rustc's `.text.<symbol>` for a function named `exit` matches ld's
  `.text.exit` pattern. Before: 154 `.text.unlikely.*` + that `.text.exit` ahead of the group (group at 0x40dbc0).
- After (fast products, probe link map): nothing of libc.a ahead of the group (only crt1's `_start`); 0
  `.text.unlikely.*`; `exit` now `.text.liella_cold.exit.liella_cold` in cold text. STARTUP-TEXT-WINDOWS 5 -> 4.

## Why it stops here (RULING NEEDED)
`cargo xtask so` (fast) fails its own contract **STARTUP-DATA-WINDOWS: 4 read-only start-up data windows > ro-budget-fast
3** (the `.rodata` byte addressed by `stream::State::drain_to` alone in window 0x780000). Cause: an input section aligned
to A raises the **`.text` output section's** alignment to A, so the 64 KiB rebind head moved `.text` from 0x401000 to
0x410000 (60 KiB gap after .init/.plt) and every later segment with it; `.rodata` rotated through the 64 KiB grid
(rethink section 2, item 2). The rethink design did not count this: any cluster alignment above 4 KiB moves the text
start by up to A - 4 KiB. A bound would move, so per the brief I stopped. Options (recommendation first):
1. Land brief B's data clusters (read-only start-up data in a pow2-aligned `.rodata1` cluster) first or together, so
   R no longer depends on the text size; then this branch's text alignment is harmless to R. **Recommended.**
2. Keep each cluster <= 32 KiB by splitting rebind into two clusters (e.g. bindtextdomain..catalog lookup, and the
   rebind-only tail from `Resolution::same_texts`), which bounds the `.text` shift to < 28 KiB, not zero: R can still
   rotate at some phase.
3. Interim rule (#4347/#4348): judge the R delta as phase jitter. Not recommended for a change that causes it.

## Not verified
After-counts of hot_text_products (both profiles), the pad test, the direct gate and the plants: all need a `so` that
passes. Release profile not built with the change. The text-class count across pads is NOT in the pad test (needs
per-class attribution, brief C's perf contracts); only mktime/strings/rebind 0 and CLUSTER-WINDOW per pad are.

## Before counts (origin/next base, mei, every run identical)
fast: true 16, locale 52, tool 55, fork 7, mktime 0, localtime 20, lt+mktime 20, float 2, strings 0, rebind 0.
release: 15, 54, 57, 6, 0, 19, 19, 4, 0, 0. Bounds: 17, 55, 58, 7, 0, 20, 20, 5, 0, 0.

## INFORMATIONAL
- `xtask/src/coldbss.rs` (lines 56, 151) reads `.bss.*` and start-up state section names from `readelf -SW`: same
  256-byte blind spot. Recommend reusing `hottext::elf_sections` (make it pub(crate)) and counting renames.
- `so.rs::sections_of` also uses `readelf -SW` (no longer used by hottext).

## Jobs (mei)
ltp-counts-before rc 0 (both profiles); ltp-counts-after rc 1 (`so` fast: STARTUP-DATA-WINDOWS); ltp-gate and
ltp-plants stopped by me with kill-label (they would fail at the same `so`). clippy and static rc 0 in the foreground.

## Next step
Owner ruling on the options above; then, for option 2, split rebind in `hot-text.clusters`, rerun
`~/work/LAYOUT-TEXT-PIN/counts.sh after` (both profiles), the plants file, and `cargo xtask affected --run --base origin/next`.
