# DL-IMAGE-READER: ld.so reads the kernel-started program from its memory image

**State:** branch `task/dl-image-reader` (stacks on DL-RELOC-3), role kanon:~/w/kn1. Decisions in force: D-27.18
(new, amends D-27.17), D-27.17, L14-6; rulings #4229 (no /proc/self/exe), #4323 (TEXTREL programs run; remove the
aliasing at its source).

## Done
* `liella_ld::elf`: image mode. `Elf::from_image::<A>(phdrs, spans)` (header synthesized: ET_DYN, the table's phnum;
  file-offset overlap checks skipped), `Span { vaddr, bytes }`, `Elf::image_span_kind(ph, dynamic) -> SpanKind
  {Live, Copy, Unseen}`. `segment_bytes`, `bytes_at`, `bytes_from` look ranges up in the spans (still bounded by one
  PT_LOAD's file-backed part).
* Rule (D-27.18): Live (sliced in place) only when readable, not PF_W, the program not text-relocated and no changing
  segment overlaps it. COPIED before the first relocation store, into anonymous mappings: the phdr table and
  PT_DYNAMIC always; the whole file-backed part of a changing PT_LOAD holding a table (DT_STRTAB, SYMTAB, HASH,
  GNU_HASH, VERSYM, VERDEF, VERNEED, RELA, JMPREL, RELR): in practice only a TEXTREL program. Copies (not raw reads):
  the reader keeps its slice API, a copy cannot alias, the cost lands only on rare layouts.
* `boundary/src/ld.rs`: `image_view` builds the spans (`read_image` copies, `live_segment` the one in-place slice,
  `scratch` the mappings, unmapped at the end of `link`); `/proc/self/exe` and the program's file are no longer opened
  (both start paths). The program's name in messages: its path (`ld.so PROGRAM`) or argv[0]. `inside()` unchanged.
* Tests (crates/ld/tests/reloc.rs): `reloc_image_reader_slices_only_unchanging_segments` (no spec clause: a changing
  segment never Live; the image reader reads exactly what the file reader reads, GOT program and TEXTREL program),
  `reloc_ldso_kernel_started_without_proc` (s27_1_c_1: chroot without /proc via `unshare -r`),
  `reloc_ldso_textrel_program_text_rx_before_it_runs` (s27_1_c_1, L14-6: kernel-started and `ld.so PROGRAM`).

## Results (kanon)
* dl-image-reader-t5 (head before the last test fix): clippy + static rc 0; reloc 9 passed, the TEXTREL program
  failed to link (fixed in b670024: text word now points at program data); the existing kernel-started multi-object
  tests pass through the image reader.
* Final head jobs: dl-image-reader-gate (affected --base origin/next), dl-image-reader-plants (3 plants): see the
  hand-back for their verdicts.
* NOT VERIFIED: the chroot-without-/proc run. kanon refuses `unshare -r` (writing /proc/self/uid_map: Operation not
  permitted), so the test SKIPs with that reason, and the `proc-self-exe-read-back` plant is parked (commented in the
  plants file). Run both where unprivileged user namespaces work.
* NOT RUN: startup counters. Budget; DL-RELOC-3 found cbench_startup's rows are static programs that never enter
  ld.so, so no existing row reaches this code.
* Miri: not run (the access pattern crosses the process image and raw syscalls); the targeted reader test stands in.

## Open / RULING NEEDED
* A dependency naming the program's own file is now loaded as a separate object (no (dev, ino) for the program
  without its file). Recommend: accept until a case needs it; `ld.so PROGRAM` could fstat its path cheaply.
* The program may have at most 64 PT_LOADs (LOADS_MAX, as for libraries). Recommend: keep.

## Next step
Run `reloc_ldso_kernel_started_without_proc` and the parked plant on a host that allows `unshare -r`; add a startup
row that runs a dynamic program through Liella's ld.so and count it.

## Don't redo
The linker refuses a TEXTREL against a library data symbol in a PIE (error 6): text-relocate program-local data.
