# DL-LIBC-EARLY: LIELLA_PRIVATE, early init, the export moves and rseq publishing (phase 14 DL-13)

**State:** branch `task/dl-libc-early-6` (= -5's head d4d21d28f: -4's head 02086ac72 plus a merge of origin/next
ca8aa835a, which has LAYOUT-DATA-CLUSTERS-4C), merge 16c1e78c4, code unchanged since c914ae2c9, role shiki:~/w/sh2.
-6: start-up counts settled (user-mode medians level with next, kernel medians +0.13%, noise); FULL gate and plants
pending at hand-back (dl-libc-early-6-gate, dl-libc-early-6-plants on shiki). DL-13 is complete
in code: build tag check and early init (-1/-2), export moves and rseq publishing (-3). -4 went red in CI on
STARTUP-DATA-WINDOWS (layout jitter, the branch adds no start-up read-only data); on the merged head that contract
holds (1 window, budget 1), hot_text_products passes, start-up user instructions are level with next or below.
Plants file: handoff/DL-LIBC-EARLY-4-plants.txt.
Reports: handoff/DL-LIBC-EARLY-6-report.md, handoff/DL-LIBC-EARLY-5-report.md (-4, -3, -2 too).

**Decisions in force:** D-27.26 (this story: placement, initial rseq area, publish before relocation),
D-27.22 (private interface; `StartInfo` gained `rseq_size`), D-27.19/.20 (init order, interpreter exports),
D-27.18/.21. Plan rows: plan/06-phases/17-phase-14-dynamic-linking.md DL-13; plan/ph14/design.md §14; R14-12.

## What exists
* ld.so (`boundary/src/ld.rs`, feature `ld`) defines and exports `__libc_stack_end@@GLIBC_2.2.5`,
  `__rseq_offset/size/flags@@GLIBC_2.35` (xtask `LD_EXPORTS`, versions from versions.tsv). In `link()` it lays
  the initial area out with `INITIAL_TAIL` (thread block + room to `RSEQ_OFFSET + 32`), maps it (`map_area`)
  BEFORE relocation, registers rseq at tp + `RSEQ_OFFSET` unless `glibc.pthread.rseq=0` (`RSEQ_WANTED`, from the
  tunables ld.so parses), stores `__libc_stack_end = sp`, and passes the size in `StartInfo.rseq_size`.
  The program-alone path (`run()`, no DT_NEEDED) is unchanged (TCB_SIZE tail, no rseq).
* libc: `early_init` → `thread::set_loader_rseq`; `entry()` (new threads) registers when
  `RSEQ_OWNED || LOADER_RSEQ`; the initial thread's call passes `RSEQ_OWNED` (static start code unchanged).
  libc.so.6 keeps its own four objects local (libc.a's), never names `liella_ld` (affected test green).
* Tests: crates/ld/tests/reloc.rs `reloc_loader_objects_published` (PIE with copy relocations, kernel-started and
  `ld.so PROGRAM`, rseq on and off; re-registering the area at tp+offset must give EBUSY); products
  `loader_owned` now also requires ld.so's export at the versions.tsv version (s22 `sym___rseq_*`).

## Do not redo
* libc-side code never names `liella_ld::` (D-27.22 note). Do not link libc.so.6 against ld.so: libc does not
  need the loader's objects (it learns of the registration through StartInfo).

## Open / next step
* Threads in dynamic programs are still forbidden (D-22.10) until DL-14; then check a new thread's rseq area
  is registered at the same offset (LOADER_RSEQ path) with a thread test.
* `__libc_stack_end` products test (s01 `sym___libc_stack_end`) still checks only from C; the reloc test covers
  the ld.so value.
