# DL-BOOT-RELOC (phase 14 DL-4): self-relocator, first used by libc.a's static-PIE start

**State:** branch `task/dl-boot-reloc-4` (continues dl-boot-reloc-3; base origin/next), code head `d3ba674b`
(handoff commit on top). Role mei:~/w/s1. Jobs: mei `dl-boot-reloc-4-lint0` (clippy + static on the inherited head
05aa096d, rc 0) and `dl-boot-reloc-4-gate` on d3ba674b (clippy, static, `affected --run --base origin/next`; the
tool escalates to FULL because entry.S defines `_start`): rc 0, 32 suites ok, 11128 tests passed, 0 failed,
1200 ignored; conformance 124 passed, 0 failed (s01_static_pie and s01_static_pie_rela included); peak 16G
(page cache), oom_kill=0. No plants this run: the merge changed none of this story's
code (only a status line in so.rs), per the coordinator's note.

## Done (DL-BOOT-RELOC-4)
* CI conflict: merged origin/next (be49b4cd). so.rs: kept DL-PLACEHOLDERS' placeholder build (placeholders.rs,
  check_no_private) and this story's liella-rcrt1.o product and GOT check whole; the status line now names
  liella-crt1.o, liella-rcrt1.o, liella-gcrt1.o, libresolv placeholder. s26-29.md: D-27.10 (this story) and D-27.11
  (DL-SYMBOLS) both kept; D-27.9 (placeholders) and D-27.10 each appear once.
* Ruling #3670 applied: docs/toolchain.md section 3.3 documents liella-rcrt1.o (build line, what it does, PC-relative
  check), the section 2 table lists it, section 4 gives the static-PIE link line (d3ba674b).
* Note: docs/toolchain.md 3.3 still describes the old libpthread-only placeholders; DL-PLACEHOLDERS (D-27.9) changed
  them. Not this story's text; left for that story's docs.

## Done (DL-BOOT-RELOC-3)
* Cause of the last red (dl-boot-reloc-2-t1 rc 1): clippy `arithmetic_side_effects` in liella-ld, already fixed
  in ebb71c51; clippy + static clean on the inherited head (job dl-boot-reloc-3-lint, rc 0).
* **Link-time selection, no runtime test on the non-PIE start** (coordinator item 1): `entry.S` assembled with
  `-DLIELLA_STATIC_PIE` is the new `target/liella/liella-rcrt1.o`; its `_start` does `leaq main(%rip)` and
  `call liella_start_main_static_pie` (PC-relative; `cargo xtask so` refuses a GOT-typed relocation in it, unit
  test `static_pie_start_object_is_pc_relative`). That entry (`internal_entry!`, local in libc.so.6) runs
  `runtime::start::<true>`; `__libc_start_main` runs `start::<false>`, which compiles neither the ET_DYN test nor
  the relocation step nor the pass-2 IRELATIVE branch and keeps origin/next's `prog.bias != 0` refusal (message now
  names liella-rcrt1.o). The conformance runner links LINK-STATIC-PIE programs with liella-rcrt1.o. D-27.10 updated.
  Static non-PIE instruction delta vs origin/next: zero by construction (same code as origin/next in that
  instantiation); NOT MEASURED with perf stat (budget).
* `relr-span` (item 2): the RELR unit test sets a bit in a second consecutive bitmap word (63- vs 64-word span).
* `tls-rebase` (item 2), cause: the conformance program's `static __thread` variables were never written, so gcc
  folded them; the linked static PIE had NO PT_TLS (readelf), and the RELRO half of the delta only feeds an
  mprotect whose failure is ignored by design. Fix: external-linkage `__thread` variables (79821fcf).
* WARN-NO-MSG (item 3): msg patterns now `conformance FAILED: s01_static_pie(_rela|:)`. Run plants-1 (head
  5246efe1): 24 plants, 23 CAUGHT, tls-rebase SURVIVED (cause above), 0 WARN.
* New plants: `rcrt-check-got` (so.rs check), `pie-entry-plain` (rcrt1's entry runs start::<false>).

## Open / next
1. If gate-2 / plants-2 are red: fix, run only the failing suite.
2. Done in DL-BOOT-RELOC-4 (docs/toolchain.md); README not changed.
3. Optional: a conformance check that RELRO is read-only in a static PIE (the RELRO half of the bias delta has no
   test of its own; the tls-rebase plant is caught through the TLS half).
4. PT_PHDR load offset: already cut as STATIC-PIE-LOAD-OFFSET; not here.

## Arch seam (§0a checklist)
1 ticked (GOT check matches type names generically); 2 ticked; 3 n/a; 4 entry.S variant: x86_64 asm, the
aarch64 start will need its own rcrt1 variant (same -D switch); 5-6 n/a; 7 ticked; 8 n/a; 9 ticked (rcrt1 refs
PC-relative, checked).

Decisions: D-27.10. Plan rows: design §0a (item 9), §1; phase-14 item 8; R14-1. Plants: handoff/DL-BOOT-RELOC-plants.txt.
