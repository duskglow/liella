# DL-MAP (phase 14 DL-7): the general object mapper

**State:** branch `task/dl-map` (from origin/next b14f639cc). Commits: fcfb18c8b (the change), b5c7234a2 (test clause
IDs, corpus accepts the reader's own refusals), then this handoff. Role mei:~/w/s3.
Decisions in force: D-27.15 (new, `records/decisions/D-27/D-27.15.toml`), D-27.14 (DL-6), D-27.13 (DL-5), R14-2 (a),
R14-8, R14-18; coordinator ruling 11-10 on the unsafe-audit file list (no new category used here: `SysMapper::store`
is `UNSAFE(slice)` in `boundary/src/ld.rs`, an existing category in an admitted file).

## Done
* `liella_ld::map` is the general mapper (pure; system calls through the new `Mapper` trait):
  `plan` validates before anything is mapped: ET_DYN or ET_EXEC; at most `LOADS_MAX` = 64 PT_LOADs
  (`TooManySegments`); per PT_LOAD non-empty, filesz <= memsz, offset ≡ vaddr mod page, p_align 0/1/power of two
  <= 2^46, file bytes inside the file, no W+X; sorted, no shared page; span <= 2^46; ET_EXEC below `Arch::USER_END`;
  PT_GNU_RELRO inside the span. `load`: one PROT_NONE reservation (ET_DYN: over-reserved by align - page and
  trimmed so the bias is a multiple of the largest p_align, also when the first segment is not at 0; ET_EXEC:
  `MAP_FIXED_NOREPLACE` at its address, `Placement` when taken or moved); each segment MAP_FIXED inside with exactly
  its p_flags; .bss tail zeroed (writable) or, for a read-only segment with .bss, the shared page rebuilt as zero
  pages + copied bytes then set read-only (never W+X); gaps stay PROT_NONE; any failed call unmaps the reservation.
  `Loaded::relro` recorded, `Loaded::protect_relro` for DL-10, `Loaded::unmap`.
* Page size: `Arch::page_size(&Auxv)` (AT_PAGESZ if a power of two >= `Arch::BASE_PAGE`, else BASE_PAGE);
  `Arch::BASE_PAGE` (4096) and `Arch::USER_END` (2^47) are the only such literals. `boundary/src/ld.rs` passes the
  page to the mapper and to the RELRO protection; `PAGE`/`PAGE_U64` are gone. `SysMapper` implements `Mapper`.
* ET_EXEC programs: `ld.so PROG` maps them at their address; a program without PT_DYNAMIC is entered without
  relocation (it has none); by PT_INTERP too (tested).
* Messages: `ld.so: <path>: <reason>` (the DL-6 messages printed `ld.so: ` twice), status 127.
* Fuzz: the existing `elf` target never called `map`; new target `ld_map` (planner at 4/16/64 KiB pages and
  `load` against a call-checking mapper; seeds = copy of the elf seeds; docs/fuzzing.md row). `cargo check --bin
  ld_map` in fuzz/ ok (job dl-map-fuzzcheck); not run under libFuzzer in this story.
* Tests `crates/ld/tests/map.rs` (5): model address space (every call inside the reservation, never W+X, stores
  only to writable pages) over PIE, shared object, static ET_EXEC, 2 MiB p_align, 16 KiB pages, read-only .bss;
  each call failing in turn leaves nothing mapped; Placement leaves the occupant alone; `Arch::page_size`; the
  corpus (overlapping, huge memsz, huge align, align not a power of two, congruence broken, zero-length,
  filesz > memsz, out-of-file, W+X, RELRO outside, ET_REL, 65 PT_LOADs; 64 accepted as a control; every 7th
  truncation) refused by the planner with nothing mapped and by the built ld.so (status 127, one line, no signal;
  plus ET_EXEC at address 0); `/proc/self/maps` of programs ld.so maps (PIE, 2 MiB aligned, ET_EXEC at 0x400000,
  4 runs each; ET_EXEC also by PT_INTERP): text r-xp, rodata r--p, data and .bss rw-p, .bss all zero, object range
  covered without a hole, no W+X, an inaccessible guard between 2 MiB-separated segments.

## Results (mei)
See handoff/DL-MAP-report.md (jobs, plants, counts).

## Arch seam checklist (ticked)
1 page literal only in arch.rs (`BASE_PAGE`), user-space end only in arch.rs (`USER_END`); `SPAN_MAX` (2^46) and
`LOADS_MAX` in map.rs are arch-neutral bounds; 2 n/a (no relocation); 3 n/a; 4 no new machine primitive; 5 features
unchanged; 6 n/a; 7 map.rs tests cfg x86_64 with the reason (system linker objects, x86-64 syscalls); 8 table rows
added: page size, user end; 9 nothing new before relocation (the mapper runs after ld.so's own relocation).

## Open / next
* RULING NEEDED: `LOADS_MAX` = 64 was not probed against the reference (an object with more PT_LOADs would load
  there and not here if the reference has no such bound). Recommend: probe with a 65-PT_LOAD object in DL-MAP-2;
  if the reference loads it, raise the bound (no differences row); keep D-27.15 meanwhile.
* Not covered by a test object: a movable object whose first segment is not at 0 with p_align above the page (the
  bias formula handles it; the model and maps tests use objects starting at 0). The MAP_FIXED_NOREPLACE
  "kernel placed it elsewhere" path is model-tested only for the taken case.
* `map::MapError::NotLoadable` is reached only if the reader admits another e_type (it refuses ET_REL itself).
* Next: DL-8 maps dependencies by path through `map::load` (boundary `load` returns `Program` today; a variant
  returning `Loaded` for shared objects), DL-10 calls `Loaded::protect_relro` after relocation.
