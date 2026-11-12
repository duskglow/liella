# LAYOUT-CHECK-VERDICT-CACHE (#5085): the layout contract check in a no-change `xtask so`

State: branch `task/layout-check-verdict-cache`, stacked on origin/next 9b2a0c029. Code: 27b54c8ce and e25ef4554
(the jcc test order, plants, pending record), then this report. Decision: `records/pending/LAYOUT-CHECK-VERDICT-CACHE-1.toml`
(D-NEW, series D-X; the coordinator assigns the id).

## Where the 126 s (here 241 s) went

Profile on keke (AMD EPYC 7551, 4-core gate slot; the brief's figures are from a 7-core slot). Fast products of
origin/next 9b2a0c029. Per-contract times are in the logs of job `lcvc-prof1`.

| contract | s |
|---|---|
| JCC-HOT-ASM | 226-230 |
| CLASSED-XREF | 10.2-10.6 |
| STARTUP-DATA-WINDOWS | 2.5-2.6 |
| PRODUCT-PATH-REMAP | 0.8 |
| the other 8 | < 0.5 each |
| whole check (`cargo xtask layout-check`) | 241 |

JCC-HOT-ASM: `objdump -d -w` over the whole 14 MB `libc.a` (849,735 lines) took 225 s (job `lcvc-prof2`). Of that
output the check reads only the 47 sections the padded files (`fold.S`, `scan.S`) declare. `objdump -d -w -j` over
those 47 took 0.08 s. This was plain waste, fixed first: `jcc::check_file` with a section set reads `objdump -h`, then
disassembles only the matching section names. The check is now 13.9-14.9 s on fast products (CLASSED-XREF 9.3-10.3 s
is the slowest) and 4.8 s on release products.

## The cache

`xtask/src/layout_verdict.rs` (module doc and the pending record list the key). A pass is stored in
`target/layout-verdicts/<sha256 key>` with a sha256 of its own text. A hit prints
`layout: verdict cache hit <key> (...)` and the stored ok lines. A miss, an unreadable or corrupt entry, or any error
while computing the key leads to a full check. Failures are never stored. Under `LIELLA_TRAIN` set and not `0`, or
with `LIELLA_LAYOUT_CACHE=0`, the cache is neither read nor written. The full check also prints
`layout: N contracts checked in X s (slowest <ID> Y s)`.

## Measurements (no-change `cargo xtask so`, keke, one 4-core slot, wall seconds)

| run | fast | release |
|---|---|---|
| before (origin/next code), measured | 448.3, 441.6 | not measured (derived about 455: the release cache-off run 409.2 - its 4.8 s check + 50.0 s whole-archive objdump of the release libc.a) |
| after, cache hit | 197.2, 210.8 (and 195.0 at the end) | 393.9, 398.7 |
| after, `LIELLA_LAYOUT_CACHE=0` (full check, JCC fix) | 215.0 | 409.2 |
| after, `LIELLA_TRAIN=1` (full check) | 212.9 | - |
| after, first run (xtask rebuilt, cache miss) | 271.2 | - |

Jobs: `lcvc-prof1` (before), `lcvc-prof2` (objdump split), `lcvc-timing` (after; logs in keke
`~/work/LAYOUT-CHECK-VERDICT-CACHE/`). Wall time on a shared host varies by about +-10 s between identical runs. The
cache saves the remaining check (14 s fast, 5 s release). The JCC fix is most of the gain (226 s fast, about 50 s release).

## Tests and plants

Tests: `layout_verdict::tests::{the_key_changes_with_every_input, the_train_and_the_opt_out_never_use_the_cache,
a_corrupt_or_foreign_entry_is_a_miss_never_a_pass}`, `jcc::tests::archive_check_disassembles_exactly_the_named_sections`.
Plants: `handoff/LAYOUT-CHECK-VERDICT-CACHE-plants.txt` (9: five key inputs left out, the train served from the
cache, the sum and the key of an entry unchecked, the jcc selection inverted). Results: see the report json and the
hand-back (jobs `lcvc-gate`, `lcvc-plants`).

## Open, not done here

- INFORMATIONAL: the rest of a no-change `xtask so` (about 195 s fast, about 390 s release on 4 cores) is outside the
  layout check and was not profiled in this story. It is the next target for #5085-style work: the `so` steps
  (archive rewrite, libm, ld.so, placement) each run again on unchanged inputs.
- CLASSED-XREF (10 s fast) runs `readelf -rW` over libc.a. It was not optimised; on a hit it is skipped anyway.
- Residual key gap (named in the record): tool data files outside the binaries and their shared libraries (gcc
  specs, ld's built-in scripts) count only through the binaries and `--version`.

Next step: none required. A follow-up story could profile the non-layout part of a no-change `xtask so`.
