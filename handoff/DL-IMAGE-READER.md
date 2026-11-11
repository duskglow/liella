# DL-IMAGE-READER(-2, -3): ld.so reads the kernel-started program from its memory image

**State:** branch `task/dl-image-reader-3` (continues `task/dl-image-reader-2`, merged with origin/next), role
mei:~/w/s3. Decisions in force: D-27.18 (amends D-27.17; load bias in -2; span list in -3), D-27.17, D-27.15 (no
PT_LOAD count bound, landed on next), L14-6; rulings #4229 (no /proc/self/exe), #4323 (TEXTREL programs run; aliasing
removed at its source), #4341 (a dependency naming the program's own file loads separately: accepted for now), #4342
(no-/proc proof on a host with unprivileged user namespaces), #4371 (bias without PT_PHDR), #4372 (ld.rs plants run
`cargo xtask so` in their command).

## Done (DL-IMAGE-READER, -2)
* `liella_ld::elf` image mode (`Elf::from_image`, `Span`, `image_span_kind` Live/Copy/Unseen); `boundary/src/ld.rs`
  `image_view` builds the spans, copies before the first store, no `/proc/self/exe` and no program file opened.
* -2: the no-/proc test uses `unshare -r --root` (no chroot(8)); `liella_ld::start::bias` finds the bias without
  PT_PHDR from the ELF header in the page holding AT_PHDR (#4371); RELRO at that bias (`rt::start::relro_at`).
  Tests `reloc_ldso_kernel_started_without_proc`, `reloc_ldso_program_without_pt_phdr`. Details: D-27.18.

## Done (DL-IMAGE-READER-3)
* CI conflict: merged origin/next (8eb22e081). start.rs: next had landed the same `relro_at` (same signature and
  body) with another doc comment; the comment now names both uses. docs/codemap/README.md regenerated (1e6059eab).
* Semantic conflict the merge exposed (the merged head did not compile): next removed `map::LOADS_MAX` (D-27.15: any
  number of PT_LOADs; `map_proc_self_maps_of_loaded_programs` runs a 300-PT_LOAD PIE as `ld.so PROGRAM`). `image_view`
  used a fixed `[Span; LOADS_MAX + 1]` and refused > 64 PT_LOADs. Now (c965b0c22) the span list lives in the scratch
  mapping that already holds the copied header table, `e_phnum` spans (one per PT_LOAD plus the PT_DYNAMIC copy;
  PT_DYNAMIC is a required non-PT_LOAD header), so no system call is added; `span_slots` writes every slot before
  forming the slice (two `UNSAFE(slice)` blocks). D-27.18's text corrected to match.

## Results (mei)
* dl-image-reader-3-lint0 (inherited head a6b589869): clippy + static rc 0.
* dl-image-reader-3-t2 (head c965b0c22, product code identical to the final head): codemap, clippy, static rc 0;
  `so`; map 5/5, reloc 12/12.
* dl-image-reader-3-gate (head 1e6059eab): rc 0; 1884 passed, 0 failed; conformance 17/17; 0 SKIPPED lines.
* dl-image-reader-3-plants (head 1e6059eab, killed after 5 verdicts): the four -2 plants CAUGHT with their msg;
  span-list-capped-at-64 SURVIVED: map.rs's 300-PT_LOAD program is `-nostdlib`, so ld.so never runs `link()` for it.
  Fixed by the new test `reloc_ldso_program_with_many_loads` (ec78516a2, c5fe52548): the corpus PIE (with
  dependencies) rewritten to 300 PT_LOADs, run as `ld.so PROGRAM`. Kernel-started is impossible at that count: the
  kernel refuses (ENOEXEC) a header table over one page (73 headers), seen in dl-image-reader-3-t4.
* FINAL HEAD (c5fe52548 + this handoff), started detached at hand-back, verdicts are the dispatcher's:
  dl-image-reader-3-gate2 (clippy, static, affected --base origin/next) and dl-image-reader-3-plants3 (all six plants,
  /mnt/zram/claude/DL-IMAGE-READER-3/plants-3; the span-list plants target the new test). The new test itself has not
  yet passed in a finished job (gate2 runs it): NOT VERIFIED until gate2 ends.
* Counts: none run. cbench_startup rows are static programs that never enter ld.so; this run changes only ld.so's
  `link` path (no system call added: the span list shares an existing mapping).
* Superseded: dl-image-reader-3-codemap (rc 128: a `git status` step in the job; the role is not a git checkout),
  dl-image-reader-3-codemap2 (rc 1: merged head did not compile, LOADS_MAX), dl-image-reader-3-t1 (rc 1: clippy
  multiple_unsafe_ops_per_block; split into two blocks).

## Open / informational
* `crates/boundary/src/process.rs` and `runtime.rs` (libc's own start for a dynamic program) still take the bias from
  PT_PHDR via `rt::start::program` (0 without it): matters only for a dynamic PIE without PT_PHDR whose TLS/RELRO libc
  reads. Candidate follow-up: pass ld.so's bias, or use `start::bias` there.
* No startup row runs a dynamic program through Liella's ld.so (cbench_startup rows are static).
* The 300-PT_LOAD test runs `ld.so PROGRAM` only; the kernel-started path shares `image_view`, untested at that count.
* Miri: not practical (the access pattern crosses the process image and raw syscalls); the targeted tests stand in.

## Don't redo
The linker refuses a TEXTREL against a library data symbol in a PIE (error 6): text-relocate program-local data. Do not
call chroot(8) from tests: use `unshare --root`. Do not reintroduce a PT_LOAD cap in the image reader (D-27.15).

## Next step
Land. Optional follow-ups: the libc-side bias without PT_PHDR (above) and a dynamic start-up count row.
