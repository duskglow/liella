# DL-IMAGE-READER(-2): ld.so reads the kernel-started program from its memory image

**State:** branch `task/dl-image-reader-2` (continues `task/dl-image-reader`), role keke:~/w/n4. Decisions in force:
D-27.18 (amends D-27.17; extended in -2 with the load bias), D-27.17, L14-6; rulings #4229 (no /proc/self/exe), #4323
(TEXTREL programs run; aliasing removed at its source), #4341 (a dependency naming the program's own file loads
separately: accepted for now), #4342 (no-/proc proof on a host with unprivileged user namespaces), #4371 (bias without
PT_PHDR), #4372 (ld.rs plants run `cargo xtask so` in their command).

## Done (DL-IMAGE-READER)
* `liella_ld::elf` image mode (`Elf::from_image`, `Span`, `image_span_kind` Live/Copy/Unseen); `boundary/src/ld.rs`
  `image_view` builds the spans, copies before the first store, no `/proc/self/exe` and no program file opened.
  Details: D-27.18.

## Done (DL-IMAGE-READER-2)
* CI red cause (mei, `reloc_ldso_kernel_started_without_proc`): `unshare: failed to execute chroot: No such file or
  directory`. chroot(8) is in /usr/sbin, outside the CI job's PATH. The test now uses `unshare -r --root=<dir>` (no
  chroot binary). Passes on keke (unprivileged user namespaces work there); the parked `proc-self-exe-read-back`
  plant now runs.
* #4371: `liella_ld::start::bias` (pure): PT_PHDR's p_vaddr when present; without it, the ELF header at the start of
  the page holding AT_PHDR (magic, e_phoff == offset in that page, e_phentsize 56, e_phnum == AT_PHNUM) gives e_phoff,
  and the PT_LOAD whose file-backed part holds the table gives its link-time address; otherwise refused (never bias 0).
  ld.rs reads those 64 bytes with `read_image` (same page as the table). RELRO protection uses the same bias
  (`rt::start::relro_at`; `relro` delegates, static start unchanged), for `ld.so PROGRAM` too (it used PT_PHDR before).
* Test `reloc_ldso_program_without_pt_phdr` (s27_1_c_1): the corpus PIE with PT_PHDR patched to PT_NULL runs
  kernel-started and as `ld.so PROGRAM` (copied by cp(1) so the executed file is never open for writing in the test
  process: no ETXTBSY window).

## Results (keke)
* dl-image-reader-2-lint0 (inherited head 691af02af): clippy + static rc 0.
* dl-image-reader-2-t2: static rc 0, reloc 12/12 (the without-/proc and no-PT_PHDR tests included).
* Final head 837633ebe: dl-image-reader-2-t3 clippy + static rc 0, reloc 12/12. dl-image-reader-2-gate2 (affected
  --base origin/next) and dl-image-reader-2-plants2 (all four plants, /dev/shm/claude/DL-IMAGE-READER-2/plants-2)
  started detached at hand-back: their verdicts are the dispatcher's.
* dl-image-reader-2-gate (head 83faf4a7d, test-only diff to the final head): rc 0; 1874 passed, 0 failed;
  conformance 17/17; 0 SKIPPED lines.
* dl-image-reader-2-plants (head 83faf4a7d): proc-self-exe-read-back, bias-header-check-inverted,
  bias-zero-without-pt-phdr CAUGHT with their msg; relro-bias-from-pt-phdr SURVIVED: the corpus program has
  dependencies, so `link()` protects its RELRO and `run()`'s `relro_at` step was never reached. 837633ebe adds a
  dependency-free PIE without PT_PHDR to `reloc_ldso_program_without_pt_phdr`; plants2 reruns all four.
* dl-image-reader-2-counts (cbench_startup --compare --counters, head 83faf4a7d), Liella instructions: c 2146,
  setloc 68079, printf 71873, wctype 69109. These rows are static programs: they never enter ld.so, and nothing on
  this branch changes code a static start runs (`rt::start::program` unchanged; `relro`/`relro_at` are reached only
  from ld.so). Not moved by construction; no origin/next before-count was taken (NOT VERIFIED as a pair).
* Superseded: dl-image-reader-2-t1 (static red: the UNSAFE tag fell out of the audit's window after the SAFETY
  comment grew; codemap line refs stale; fixed in 83faf4a7d), dl-image-reader-2-plants (1 SURVIVED, above).

## Open / informational
* `crates/boundary/src/process.rs:53` and `runtime.rs:87` (libc's own start for a dynamic program) still take the bias
  from PT_PHDR via `rt::start::program` (0 without it): only matters for a dynamic PIE without PT_PHDR whose TLS/RELRO
  libc reads; not reached by this story's tests. Candidate follow-up: pass ld.so's bias, or use `start::bias` there.
* No startup row runs a dynamic program through Liella's ld.so (cbench_startup rows are static). A row for it would
  let counters guard this path.
* Miri: not practical (the access pattern crosses the process image and raw syscalls); the targeted reader test stands in.

## Don't redo
The linker refuses a TEXTREL against a library data symbol in a PIE (error 6): text-relocate program-local data. Do not
call chroot(8) from tests: use `unshare --root`.

## Next step
Land. Optional follow-ups: the libc-side bias without PT_PHDR (above) and a dynamic start-up count row.
