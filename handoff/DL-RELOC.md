# DL-RELOC (phase 14 DL-10): eager relocation — DL-RELOC-2 state

**State:** branch `task/dl-reloc-2` (DL-RELOC's work plus this run, origin/next merged once for the codemap),
role mei:~/w/s2. Decisions in force: D-27.17 (written this run; amends D-27.15 for empty PT_LOADs), D-27.11,
D-27.15, R14-13, R14-8, R14-18, L14-4, L14-6, L14-8, L14-12; rulings #4229, #4231 (below), #4230 (COMPAT-19
is a later story, not this one).

## Done in DL-RELOC-2
* Why dl-reloc-t4 was red (3 ld.so tests), all fixed at the cause:
  1. binutils 2.42 emits an EMPTY `PT_LOAD` (p_memsz 0) for `-z notext` objects; the mapper refused it
     ("malformed loadable segments"). Now skipped in `map::plan` / `map::segments` / reloc `text_segments`
     (gABI allows p_memsz 0). The map.rs malformed-corpus case "zero-length" was removed (it is valid input).
  2. A program that exports nothing has a GNU hash with all buckets empty: its count is `symoffset`, only a
     lower bound, and `Symtab::get` refused the JUMP_SLOT's symbol index ("malformed relocation tables").
     `Dynamic::symtab` now raises the limit over every RELA/JMPREL symbol in that case only (`symtab_count`).
  3. The toolchain links `--as-needed` by default: the exec-stack test's unreferenced libx.so had no
     DT_NEEDED. The test passes `-Wl,--no-as-needed`.
* #4231: dependencies dedup by (st_dev, st_ino) of the opened file (`open_file` fstat, `ids[]` in `link`);
  test `reloc_ldso_one_object_by_two_paths` (symlink; counts libb.so's executable mappings).
* static: ARCH(x86_64) tag on tests/reloc.rs; `cargo xtask codemap` regenerated after the merge.
* D-27.17 record; plants file handoff/DL-RELOC-2-plants.txt (9 plants).

## Jobs (mei)
dl-reloc-2-check1 (superseded: two-paths test parsed maps wrongly in C; others green), dl-reloc-2-gate,
dl-reloc-2-plants: see handoff/reports/DL-RELOC-2.json for their verdicts.

## Not done / next step (DL-RELOC-3)
1. Ruling #4229: read a PT_INTERP-started program from its memory image (AT_PHDR/AT_PHNUM/AT_ENTRY, the
   segments the kernel mapped) instead of `/proc/self/exe`; then drop the refusal without /proc. Design: the
   ELF reader translates vaddr->file offset via PT_LOAD (elf.rs ~558/574) and parse checks file ranges, so it
   needs an image mode (identity vaddr translation bounded by p_memsz, headers at AT_PHDR) rather than a
   faked file slice. Only `link()` in boundary/src/ld.rs uses the program's file.
2. docs/differences-from-glibc.md executable-stack row: drop "not yet implemented" (check its wording).
3. cbench_startup counters vs origin/next (brief item 4) were not run in this run.
4. Arch seam checklist (from DL-RELOC): 1 ok, 2 ok, 3 n/a, 4 no new asm, 5 n/a, 6 ok, 7 tests cfg x86_64 (now
   tagged), 8 PROPERTIES + apply in the table, 9 n/a.

## Don't redo
The three t4 causes above; dedup; codemap; the D-27.17 record (amend it, do not add another for the same).
