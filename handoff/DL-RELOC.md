# DL-RELOC (phase 14 DL-10): eager relocation — INCOMPLETE (context checkpoint)

**State:** branch `task/dl-reloc` (stacked on DL-MAP 908f61a17), role mei:~/w/s2. All commits WIP.
Decisions in force: D-27.11 (#3655), D-27.15, R14-13, R14-8, R14-18, L14-6, L14-8, L14-12. D-27.17 is reserved
for this story and NOT written yet.

## Done (code, clippy clean on mei, job dl-reloc-clippy2)
* `arch.rs`: `Arch::apply(kind, Operands{s,a,b,p}) -> Field{value,width}` (x86-64 psABI calculations; default
  `NoArithmetic` for test doubles), `narrow` (32-bit signed/unsigned fit), `Arch::PROPERTIES` (x86 property types,
  parsed and ignored per L14-12; numbers from the psABI, NOT VERIFIED against a document copy).
* `reloc.rs` (pure): `relocate_all` (reverse load order), per object RELR, RELA, JMPREL; IRELATIVE and IFUNC-bound
  references deferred to a second pass; COPY (program only, scope without the program, copies min(sizes));
  TEXTREL: read-only segments RW (never X) for pass 1, restored before resolvers run; TLS kinds refused (DL-12);
  every store checked inside a writable PT_LOAD; `check_versions` (D-27.11: no VERDEF -> `NoVersions`, missing ->
  `Missing`, weak needs skipped); `relro_range`/`protect_relro`; `known_properties`; `UniqueTable` (no allocator).
* `map.rs`: `MapError::ExecutableStack`, `executable_stack()` (R14-13) in `plan`.
* `boundary/src/ld.rs`: `run` refuses an exec-stack program; with DT_NEEDED it calls `link`: loads dependencies
  BY PATH only (a name without `/` is refused until DL-8), dedup by name (NOT by dev/ino yet, L14-4), version check
  (NoVersions exit 127, Missing exit 1 as the probe), relocate, RELRO, unmap the file views. Program file in
  PT_INTERP mode comes from `/proc/self/exe` (open question below). `ProcImage` uses UNSAFE(slice) only.
* `crates/ld/tests/reloc.rs`: 7 tests (arithmetic; model corpus with ordering/W+X/RELRO checks; version check in the
  model; ld.so: multi-object run both modes, unversioned stub 127, exec stack refused, RELRO write faults).

## Not done / next step
1. Read job `mei:dl-reloc-t4` (`~/bin/test-summary ~/logs/dl-reloc-t4.log`): run t3 failed only because the corpus
   linked libraries before the source (fixed in 15d27c850); expect further fixes (COPY may not be emitted for PIE
   on this toolchain; then adjust the model expectations).
2. D-27.17 record (`records/decisions/D-27/D-27.17.toml`): the decisions above (by-path only, /proc/self/exe,
   COPY size min, weak verneed skipped, Missing exit 1, TEXTREL deferred store refused, TLS refused).
3. Update docs/differences-from-glibc.md row (executable stack: drop "not yet implemented"); COMPAT-19 is
   registered but NOT implemented (RULING NEEDED: implement in DL-10-2 or defer).
4. Plants file `handoff/DL-RELOC-plants.txt` via `cargo xtask plants`: Relative `wrapping_add(a)` -> drop addend;
   Abs64 S+A -> S; reverse order -> forward (`(0..n).rev()`); deferral off (`Irelative if !deferred => Ok(true)` ->
   apply now); `protect_relro` call removed in link; `check_versions` call removed; each with msg:.
5. handback-check at 58d0b6421 was red on static: arch-audit (1 untagged x86_64 seam, likely the
   `#![cfg(target_arch = "x86_64")]` in tests/reloc.rs: add an `// ARCH(x86_64): why` tag, docs/arch-seams.md) and
   codemap out of date (`cargo xtask codemap`). Gate: `cargo xtask static`, `cargo xtask affected --run --base origin/next`; cbench_startup counters vs next.
6. Arch seam checklist: 1 ok (R_X86_64 only in arch.rs), 2 ok (matches RelocKind), 3 n/a, 4 no new asm, 5 n/a,
   6 ok, 7 tests cfg x86_64, 8 PROPERTIES + apply added to table, 9 n/a — ticked but unreviewed.

## Open questions
* RULING NEEDED: PT_INTERP-started programs with dependencies read their file via `/proc/self/exe` (the reader is
  file based). Recommend: a memory-image reader mode in DL-10-2 instead; refuse without /proc meanwhile.
* Duplicate detection by name only (L14-4 wants dev/ino): add fstat identity next.
