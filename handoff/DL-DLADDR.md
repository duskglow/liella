# DL-DLADDR (DL-18): handoff (run 4, DL-DLADDR-4)

**State.** Branch `task/dl-dladdr-4` (all of `task/dl-dladdr-3`, plus a merge of origin/next b60c73ac6 and the fixes
below). HEAD: see handoff/reports/DL-DLADDR-4.json. Role keke:~/w/n1. dladdr, dladdr1 and dlinfo are exported by
Liella's libc (GLIBC_2.34, compat GLIBC_2.2.5 / GLIBC_2.3.3) and answered in `ld.so`
(`crates/boundary/src/ld/dlopen/dladdr.rs`). Work done and verified in runs 1-3; run 4 only merged next and wrote the
reserved ids.

**Run 4 commits.**
- `dabcd9b8b` merge origin/next (DL-16c DL-DLOPEN-CONCURRENT, DL-16d DL-DLOPEN-TLS-SURPLUS). Conflicts were all
  additive and keep both sides: `StartInfo` has next's `dl_iterate`, `dl_find_object`, `dl_fork`, then `dladdr`,
  `dlinfo` (ld.rs fills, runtime.rs stores); next's dl_iterate_phdr differences row, then the TLS_DATA row; next's
  `== readers (DL-16c)` probe record, then `== dladdr-dlinfo`.
- `a1cb371fc` dlinfo's `RTLD_DI_TLS_MODID`/`TLS_DATA` come from the published module table (`THREAD_MODS` through
  `ld::initial_place`), the table every thread's static area is filled from and `dl_iterate_phdr`'s `tls_of` reads;
  a dlopen appends to it before publication, so a published object always has its entry.
- `e2d5ea80e` reserved ids: pending DL-DLADDR-2-1..5 -> D-27.45 (dladdr object/symbol), D-27.46 (lock-free readers),
  D-27.47 (dlinfo requests/failures), D-27.48 (SERINFO sizing), D-27.49 (TLS_DATA); DL-DLADDR-2-6 -> COMPAT-29
  (decisions D-27.49, D-27.39). Series lines dropped; citations in the records, the differences row and dladdr.rs
  name the ids.
- `2c16e89e1` the merged `read_section` (dlopen.rs) used the old single lock (`lock`/`unlock`/`OWNER`), gone in
  DL-16c: a first reader now builds the state under `STATE` alone (lock order kept, as fork's prepare), and refuses
  when the calling thread holds `STATE` (signal handler during the first build).
- `0461f75a5` s27_dladdr checks dladdr as the process's first loader call (no test reached that path before); plants
  file `handoff/DL-DLADDR-4-plants.txt` (2 plants on this run's changed sites).

**Results.** See handoff/DL-DLADDR-4-report.md (jobs, rc, counts).

**Decisions in force.** D-27.1, D-27.39, D-27.45..D-27.49, COMPAT-29 (registered, not implemented).

**Probe-settled.** spec/behaviour/dynlink.md "dladdr and dlinfo"; `== dladdr-dlinfo` in dynlink.glibc.txt.

**Open / INFORMATIONAL (not in this story's scope).**
- Two read paths now exist: `ld::readers` (DL-16c: its own table `TABLE`/`PUBLISHED`, for dl_iterate_phdr and
  _dl_find_object) and `ld::dlopen::read_section`/`View` (the chain, for dlsym's RTLD_NEXT, dladdr, dlinfo). Both
  are empty sections today; DL-17 (dlclose, reader epochs) must give both the same epoch, or fold one into the other.
- The vDSO is not in the loader's chain: `dladdr` of a vDSO address returns 0 (the reference finds `linux-vdso.so.1`);
  `dl_iterate_phdr` (DL-16c) lists it apart.
- `RTLD_DI_SERINFO` on the program: the reference leaves out the program's own `DT_RUNPATH` (probe `/prog/rp`);
  Liella's likely lists it. Not checked. Follow-up: probe DT_RPATH vs DT_RUNPATH programs, align `serinfo` for i == 0.
- `RTLD_DI_ORIGIN` of the program and of ld.so is refused; the reference's answer is not recorded.

**Do not redo.** The probe and its record; compat-gen regeneration; the conformance programs; the nine plants of run 3
(handoff/DL-DLADDR-3-plants-a/b.txt, all CAUGHT); the id assignment.

**Exact next step.** Land. Optional follow-up story: the SERINFO-of-the-program probe above.

**Plan rows.** plan/ph14/design.md §8, §10; plan/ph14/dl-16-dlopen.md §4; D-27.1, D-27.30, D-27.39, D-27.45..49.
