# DL-MULTI-WRITABLE: the self-relocator accepts any number of writable PT_LOADs

State: branch `task/dl-multi-writable` (from origin/next), run 1 stopped at the context checkpoint. INCOMPLETE.
Brief: `~/state/briefs/DL-MULTI-WRITABLE.md`. Decision: D-27.25 (amends D-27.10, `ann = SEC`).

## Done (run 1)
- `crates/ld/src/boot.rs`: `Image<'h>` keeps the program headers; `Image::writable()` lists every writable
  `PT_LOAD` (no copy, no allocation). `image` needs >= 1 writable `PT_LOAD` and refuses one whose end overflows.
  `tables(dynamic, &img)` refuses a table overlapping ANY writable segment. New `Memory` trait (one view per
  segment) and `Writer` (locates each target's segment with `holds`, last segment tried first, writes through
  that segment's view). A target in no segment, or straddling two, is `Outside`.
- `crates/boundary/src/bootstrap.rs`: `Mapped(bias)` implements `Memory` (one `bytes_at_mut` view per store,
  UNSAFE(slice)); `Relocated` carries the headers and `phnum` instead of one segment; pass 2 (`irelative`) uses
  one `Writer` (views are still made after each resolver returns).
- ld.so's program path (`boundary/src/ld.rs` `run`) calls `boot::image` unchanged, so it accepts many writable
  segments too. `liella_ld::reloc` (ld.so's general relocator) and the image reader (`elf.rs image_span_kind`)
  already handled any number.
- Tests: boot unit tests `writable_segments_each_relocated` (4 segments, gaps and adjacent), `refusals` (gap,
  past the second segment, straddling, nothing written outside), `dynamic_and_headers` (table overlapping the
  second segment). The test memory's views run past each segment's end, so only the relocator's own check refuses.
  `conformance/c/s01_static_pie_multi_writable.c`: s01_static_pie with pointers in three writable PT_LOADs (16 KiB
  `.data1` + 16 KiB orphan `.liella_seg3`; probe on mei, GNU ld 2.42, gives 3 RW PT_LOADs), exits 92 if the
  linker did not split. Plants: `handoff/DL-MULTI-WRITABLE-plants.txt` (3 plants, msg: regexes).

## Open failure (check3, mei)
`conformance:s01_static_pie_multi_writable` exits 92: linked with Liella's `libc.a` and `liella-rcrt1.o` the program
has FEWER than 3 writable PT_LOADs (the guard worked; the start itself ran). The `-nostdlib` probe split into 3; with
libc.a's data the gap before a 16 KiB-aligned section is probably under a page somewhere. Next: `readelf -lW
conformance/build/s01_static_pie_multi_writable` on mei, then adjust the split (e.g. 64 KiB alignment, or put the
aligned objects after a page-sized object) until 3 RW PT_LOADs appear, and check it runs. `boot` unit tests,
clippy and static are green on head (check3 passed static before the conformance step).

## Not done (next run, in order)
1. RELRO across segments: `rt::start::program`/`relro_at` (static start, ld.so's program path) and
   `reloc::relro_range`/`protect_relro` (objects) protect one `[page_down(start), page_down(end))` range with ONE
   mprotect. A `PT_GNU_RELRO` spanning two segments with a gap fails (kernel-mapped: hole -> ENOMEM after the part
   before the hole) or (ld.so mapper) makes reservation gap pages readable. Design (keeps start-up cost): count
   writable PT_LOADs in `program()`'s existing pass; one writable -> one mprotect as now; more -> protect
   RELRO ∩ each PT_LOAD's pages. GNU ld 2.42 keeps RELRO inside one PT_LOAD (probes: 16 KiB-aligned `.data.rel.ro`,
   `.fini_array` first, an `INSERT AFTER .data.rel.ro` script: none split it), so the end-to-end test needs a
   header-patched program (as `crates/ld/tests/reloc.rs` `with_loads` does).
2. Dynamic program with 2 and 3 writable PT_LOADs under ld.so (both ways: `ld.so PROG` and PT_INTERP): add to
   `crates/ld/tests/reloc.rs` (corpus program built with the same `.data1`/orphan 16 KiB-aligned pointers) and a
   `-nostdlib` PIE without dependencies in `ldso_run.rs` (goes through `boot::image` + `relocate_self`).
3. Startup counters vs origin/next (cbench_startup, same host) and `liella::hot_text_products`.
4. The brief's "malformed image with a target in no segment is still refused" end-to-end (unit tests cover it).

## Jobs (mei, role s3)
See handoff/reports/DL-MULTI-WRITABLE.json `jobs`. check1 (rc 101) was the first unit-test run: a wrong test
expectation (the RELR run `[0x1018, 3]` writes 0x1018 before refusing 0x1020), fixed. check2 (rc 1): records
test references must be `file::fn` without the module path, fixed.

## Do not redo
- The GNU ld probes above (RELRO cannot be split by source attributes or an INSERT script with ld 2.42).
