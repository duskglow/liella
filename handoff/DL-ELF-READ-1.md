# DL-ELF-READ-1 (phase 14 DL-2, IP-24 part 1): the loader's ELF reader

**State:** branch `task/dl-elf-read-1` (from origin/next `eef6b2dd`); HEAD = the commit that adds this file (see the
report json). Role keke:~/w/n2.

## What was done

* `crates/ld/src/arch.rs` (new): `Arch` trait (associated constants `NAME`, `E_MACHINE`, `EI_CLASS`, `EI_DATA`),
  `X86_64` (EM 62, ELFCLASS64, LSB), `pub type Native = X86_64` under `cfg(target_arch = "x86_64")`.
* `crates/ld/src/elf.rs`: `Elf::parse::<A: Arch>(&[u8])` (header + program header table), `program_headers`,
  `segment_bytes`, `bytes_at` (vaddr -> file bytes through `PT_LOAD`), `interp`, `dynamic` (`Dynamic`: entries through
  the first `DT_NULL`, string table, `string`, `needed`), `notes` (`Notes` iterator) and `Note::properties`
  (`NT_GNU_PROPERTY_TYPE_0`). No I/O, no allocation, no panic; typed `ElfError`; byte order taken from `A::EI_DATA`.
* Error rules: decision row **D-NEW** in `plan/07-decisions/s26-29.md` (placeholder: the coordinator reserves the id).
* The `cfg(target_arch)` seam in `crates/ld/src/arch.rs` carries an `ARCH(x86_64):` tag (arch-audit). Adding the
  file to the audit's ALLOWED table (as design §0a suggests) touches xtask and escalates the gate to FULL; left to A-0.
* Tests: `crates/ld/tests/elf_reader.rs` (35 tests: synthetic image, every prefix length, each error rule, overlaps,
  overflows, huge counts, wrong class/data/machine, a big-endian and a 32-bit-class test `Arch`);
  `crates/ld/tests/elf_readelf_parity.rs` (readelf -h/-l/-d/-n parity over a corpus built with the host `cc`:
  static, PIE with RELR and both hash styles, non-PIE with `DT_RPATH`, shared with soname + `DT_RUNPATH`, shared
  without notes; `-n` reads an `objcopy --strip-section-headers` copy so readelf reports `PT_NOTE` segments).
* Plants: `handoff/DL-ELF-READ-1-plants.txt` (41 plants: 38 with `msg:`, 3 `equivalent:` double guards with reasons).

## Arch seam (§0a checklist)

arch seam: ticked, with two exceptions:
1. `Native` is left undefined on non-x86_64 targets instead of a `compile_error!`: `cargo xtask arch-check` (A-2)
   type-checks `liella-ld` for aarch64 with all targets, and a `compile_error!` would break it. The first use of
   `Native` on another target still fails to compile.
2. (item 7) `elf_reader.rs` runs on `X86_64` plus two test arches; `valid_image_reads_back` asserts the machine as
   the literal 62 (an x86_64 fact) instead of `X86_64::E_MACHINE`. Cosmetic; fix when DL-3 touches the file.
Items 2-6 do not apply (no relocations, TLS, assembly, features or paths here). Item 8: the identity row is in
`liella_ld::arch`.

## Open / next

* RULING NEEDED: none. D-NEW needs its id (`ops reserve`, then `tools/assign_placeholder.sh`).
* dlerror texts for `ElfError` wait for the DL-0b probes (design §2 "exact texts from M14-0 probes").
* Next step: DL-3 `DL-ELF-READ-2` (hash tables, versym/verdef/verneed, RELA/RELR iterators, the `elf` fuzz
  target) builds on `Elf::bytes_at` and `Dynamic`. Read plan/06-phases/17 item 7, design §2, D-NEW.
* Not to redo: the readelf output formats were probed on the host (binutils 2.42); `readelf -n` on a file with
  section headers lists note *sections* (including non-allocated `.note.stapsdt` in static programs), hence the
  stripped copy.
