# DL-ELF-READ-2 (phase 14 DL-3, IP-24 part 2): hash tables, versions, RELA/RELR iterators

**State:** branch `task/dl-elf-read-2` (from origin/next `9424819f`); code head `e98962b8`, then this handoff
commit. Role keke:~/w/n3. Status: INCOMPLETE at the context checkpoint (see "Remaining").

## Done
* `crates/ld/src/arch.rs`: `RelocKind` (design §0a list; `TextRel { width, pc_relative, signed }`, where
  `signed` separates `R_X86_64_32` from `32S`, which the design's `TextRel(width, pc_relative)` could not)
  and `Arch::classify` (no default); `X86_64::classify` from the psABI numbers; unknown = `Unsupported(n)`.
* `crates/ld/src/elf.rs` part 2: `Dynamic::{sysv_hash, gnu_hash, symbol_count, versym, verdef, verneed,
  rela::<A>, jmprel::<A>, relr}`, `Elf::bytes_from`; types `SysvHash/SysvChain`, `GnuHash/GnuChain/GnuEntry`,
  `Versym/VersionIndex`, `Verdefs/Verdef/Verdauxes`, `Verneeds/Verneed/Vernauxes`, `Relas/Rela`, `Relrs`.
  Error rules: **D-27.8** (plan/07-decisions/s26-29.md).
* Tests: `crates/ld/tests/elf_tables.rs` (16: read-back, every prefix, byte corruption, each D-27.8 error);
  `elf_readelf_parity.rs` + `readelf_r_parity`, `readelf_v_and_dyn_syms_parity` over 3 new members
  (versioned + RELR + both hashes, versioned SysV-only, versioned GNU-only + `-z now`; TLS relocs included).
  Host binutils 2.42 supports `-z pack-relative-relocs`. All liella-ld tests green on keke (job
  dl-elf-read-2-t3: 6 + 35 + 16 passed). `elf_reader.rs`: test arches get `classify`; literal 62 fixed.
* Plants: `handoff/DL-ELF-READ-2-plants.txt` (31, all with `msg:`), job keke:dl-elf-read-2-plants.
* Probe-settled: a non-PIE executable with only a GNU hash and no exported symbols has all buckets
  empty, so the GNU table cannot give the dynamic symbol count (readelf 3, symoffset 1). Hence
  `symbol_count` prefers SysV `nchain`, and `versym` entries are bounded by the mapping, not by the count.

## arch seam (§0a checklist)
ticked: item 1 (no R_X86_64/EM literal outside `arch`; the parity test is x86_64-only and says so),
item 2 (iterators yield `RelocKind` only; `r_type` kept for diagnostics), item 7 (`rela_kinds_come_from_the_arch`
runs a second test `Arch`), item 8 (row filled in `arch.rs`). Items 3-6 do not apply.

## Remaining (exact next steps)
1. Read jobs keke:dl-elf-read-2-gate (clippy, static, affected) and keke:dl-elf-read-2-plants; fix any red.
2. The `elf` fuzz target: `fuzz/fuzz_targets/elf.rs` (parse::<X86_64>, then walk everything in elf_tables'
   `walk_dyn` style without unwraps, chains for a few hashes), `[[bin]] elf` + `liella-ld` dependency in
   `fuzz/Cargo.toml`, `"elf"` settings in `xtask/src/fuzz.rs` (max_len 1<<16; touching xtask may widen the
   gate), seeds `fuzz/seeds/elf/` (small `-nostdlib` shared objects built on the host), a row in
   docs/fuzzing.md; then a short `cargo xtask fuzz elf --time 5` run in RAM, zero crashes.
## Rulings
None needed. INFORMATIONAL: `TextRel` gained `signed` (above); the design table may want the same shape.
