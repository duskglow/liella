# DL-MULTI-WRITABLE: the self-relocator and ld.so accept any number of writable PT_LOADs

State: branch `task/dl-multi-writable-2` (run 2, continues `task/dl-multi-writable`), head: see
handoff/reports/DL-MULTI-WRITABLE-2.json. Brief: `~/state/briefs/DL-MULTI-WRITABLE.md` ("The change").
Decision: D-27.25 (amends D-27.10, `ann = SEC`; the reserved id, now also records RELRO across segments).
Plants: handoff/DL-MULTI-WRITABLE-2-plants.txt (supersedes DL-MULTI-WRITABLE-plants.txt; 5 plants).

## Done
- Run 1: `crates/ld/src/boot.rs` (`Image::writable()`, `Memory`, `Writer`: each target written through the
  segment that holds it; a target in no segment or straddling two is `Outside`), `boundary/src/bootstrap.rs`
  (`Mapped` per-segment views), boot unit tests.
- Run 2, check3's exit 92: `conformance/c/s01_static_pie_multi_writable.c` got fewer than 3 RW PT_LOADs because its
  16 KiB-aligned orphan followed libc.a's `.data` by less than a page (GNU ld kept it in that segment). It now has
  three small 16 KiB-aligned sections back to back (`.liella_seg2`, `.liella_seg3`, `.data1`): each begins ~16 KiB
  past the one before, so at least two start a new segment whatever libc.a's data size is (ld 2.42 places the two
  orphans in either order). Readelf on shiki: 3 RW PT_LOADs; the test passes under liella-rcrt1.o. The start
  itself was never the problem (the constructor ran with 2 segments).
- Run 2, RELRO across segments: `liella_rt::start::relro_pieces`/`relro_piece` protect RELRO ∩ each PT_LOAD's
  pages. ld.so uses it for objects (`reloc::protect_relro`) and for a program it relocates itself (`ld.rs`).
  The static start (`runtime.rs protect_relro`) keeps its one mprotect and walks the headers again only if that
  call fails (an unmapped page between kernel-mapped segments): no added start-up work in the usual case.
  `initial_thread` also returns the header words; no closure was added in `runtime::start` (the hot-text list
  names its closures by number: a closure there broke `cargo xtask so`'s hot-text check, check1).
- Tests: `rt start::relro_pieces_stop_at_segment_boundaries` (hole, adjacent, one segment, error, overflow);
  `ld tests/reloc.rs reloc_ldso_relro_across_segments`: `split_relro_load` rewrites a linked `libr.so`'s program
  headers (its RW PT_LOAD split in two at RELRO's last whole page; the PT_NOTE slot is reused so the table keeps
  its size; built from the tree by the test, reproducible). Under `ld.so PROG`, relocated pointers in both
  segments are right and writing RELRO in either segment faults (SIGSEGV).

## Open questions / notes
- The first run-2 commit message says "Checked on a Zen 1 host"; shiki is a Haswell-EP host (Xeon E5-2697 v3).
  Not rewritten (no history rewrite); recorded here.
- The static start's fallback (`runtime.rs`, mprotect failed -> pieces) has no end-to-end test: it needs a static
  PIE whose RELRO spans an unmapped page between kernel-mapped segments, which a header patch cannot make
  (moving segments moves code). `relro_pieces` itself is unit-tested and shared.

## Start-up counters (cbench_startup --compare --counters, shiki, Liella instructions; glibc in brackets)
| row | base 672acd9aa (built in /mnt/zram/.../base) | head after RELRO commit | final head (role ~/w/sh2) |
|---|---:|---:|---:|
| startup_instr_c | 2150 [119766] | 2157 | 2156 [119767] |
| startup_instr_setloc | 67910 [145024] | 67913 | 67924 [145025] |
| startup_instr_printf | 71703 [146800] | 71714 | 71713 [146800] |
| startup_instr_wctype | 68930 [145237] | 68942 | 68936 [145237] |
UNRESOLVED: +6..+14 on the final head. The final RELRO path adds only the mprotect result test on the usual path
(the per-segment walk is a cold function). The base was built and run from a different, longer directory than
the role (argv/env/PWD lengths enter start-up), and glibc's own column moved by 1, so the comparison is not
like for like. Next: run base and head from the SAME path (e.g. sync the base into the role after the head run, or
both under one RAM path) and attribute any remaining delta (run 1's bootstrap changes vs this run's runtime.rs)
before landing. ALWAYS FORWARD applies: no bound may be loosened.

## Not done (next run, in order)
0. Settle the start-up counters above (same path), fix any real regression.
1. A dynamic PROGRAM with 2 and 3 writable PT_LOADs under ld.so (both `ld.so PROG` and PT_INTERP): add to
   `crates/ld/tests/reloc.rs` (the corpus program with three back-to-back 16 KiB-aligned pointer sections, as the
   conformance test) and a `-nostdlib` PIE without dependencies in `ldso_run.rs` (goes through `boot::image` +
   `relocate_self`). The object path with 2 writable segments is covered by `reloc_ldso_relro_across_segments`.
2. `liella::hot_text_products` on the merged head (the coordinator's merge; not run here).
3. The brief's "malformed image with a target in no segment is still refused" end-to-end (unit tests `refusals`).

## Jobs (shiki, role sh2): see handoff/reports/DL-MULTI-WRITABLE-2.json `jobs`.

## Do not redo
- GNU ld 2.42 probes: RELRO cannot be split across PT_LOADs by source attributes or an INSERT script (run 1).
- A single 16 KiB-aligned section does not reliably split from libc.a's `.data` (run 2).
