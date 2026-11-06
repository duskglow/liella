# DL-BOOT-RELOC (phase 14 DL-4): self-relocator, first used by libc.a's static-PIE start

**State:** branch `task/dl-boot-reloc` (from origin/next `349b94d0`), WIP. Role mei:~/w/s1. INCOMPLETE: the
code, tests, plants file and D-27.10 are written; the two static-PIE conformance programs link but crash
(SIGSEGV) for the reason below. No gate and no plants run yet (pointless on a failing head).

## Done
* `crates/ld/src/boot.rs` (`liella_ld::boot`, pure): `image` (one PT_DYNAMIC, exactly one writable PT_LOAD),
  `tables` (DT_RELA/JMPREL/RELR; refuses DT_TEXTREL, bad RELAENT/RELRENT/PLTREL, partial tables, tables
  overlapping the writable segment, no DT_NULL), `relocate` (pass 1: `RelocKind::Relative` of RELA then
  JMPREL via `Arch::classify`, then RELR; Irelative/None wait; others refused), `irelative` (pass 2 list),
  `Segment::store` (bounds-checked). No panics. 3 unit tests pass (job mei:dl-boot-reloc-t3).
* `crates/boundary/src/bootstrap.rs` (category *slice*; added to the slice placement list in
  xtask/src/policy.rs, no new category): `relocate_self(phdr, phnum, dynamic)`, `irelative(&Relocated)`.
* `runtime.rs`: `initial_thread` calls the relocator when `liella_link_dynamic() != 0` (non-PIE path
  unchanged apart from that test replacing `bias != 0`), rebases TLS image and RELRO by the real bias (a GNU
  ld static PIE has NO PT_PHDR, so the old bias was 0 and the old refusal never fired: it crashed instead);
  pass 2 where `apply_irelative` runs. Pass-1 failure: one fixed write to fd 2, `exit_group(127)`.
* `xtask/src/so.rs` `shared_personality_ref`: libc.a could not be linked `-static-pie` at all
  (the product crate's LTO object keeps `DW.ref.rust_eh_personality` LOCAL in its COMDAT group, so the
  compiler_builtins members' references are undefined; non-PIE resolves them to 0 silently). Made weak
  hidden after `weaken_archive` (which rebuilds that object). With it the programs link.
* `conformance/run.sh` directive `LINK-STATIC-PIE: rela|relr`; `conformance/c/s01_static_pie.c` (RELR) and
  `s01_static_pie_rela.c` (RELA): relocated tables, TLS-image pointer, constructor, IFUNC called and taken
  by address, 100 starts (50 random, 50 ADDR_NO_RANDOMIZE).
* D-27.10 written (plan/07-decisions/s26-29.md); phase-4 deviation 2 marked closed.
* Plants file: `handoff/DL-BOOT-RELOC-plants.txt` (16 plants incl. "skip one RELR word", bounds, ordering).

## The open defect (exact next step)
strace at a fixed base: `arch_prctl(ARCH_GET_FS)` then SIGSEGV at si_addr=0x94e00 = the LINK-time
`_DYNAMIC`. `liella_link_dynamic()` in csys/src/adapter/runtime.c reads a weak hidden `_DYNAMIC` that gcc
-fPIC reaches through a GOT slot, and the GOT slot is itself unrelocated. (The same applies to every weak
`liella_link_*` helper: none may be used before pass 1.) Fix options, in order of preference:
1. Find the bias with no symbol at all: walk the headers at AT_PHDR for the PT_LOAD with p_offset 0
   and use AT_PHDR - (its p_vaddr + e_phoff)... needs e_phoff; or require PT_DYNAMIC and take the
   dynamic address from a NON-weak hidden `_DYNAMIC` in a separate C member pulled only by PIE links.
2. Make the helper's reference PC-relative: declare `_DYNAMIC` hidden and non-weak inside a member only
   static-PIE starts pull in, keeping non-PIE links free of it (non-PIE ld does not define `_DYNAMIC`).
3. A one-instruction `lea _DYNAMIC(%rip)` (weak hidden) as a machine primitive in `machine/*.S`
   (K2-clean: no logic), declared once; check a non-PIE static link resolves it to 0.
Then: rerun `sh ~/work/DL-BOOT-RELOC/t1.sh` (so, unit, 4 conformance, clippy, static), gate
(`cargo xtask affected --run --base origin/next`), plants (`cargo xtask plants handoff/DL-BOOT-RELOC-plants.txt
--dir /mnt/zram/claude/DL-BOOT-RELOC/plants-1 --timeout 600`), and the static workload count before/after
(INVIOLABLE static gate; not measured yet).

## Arch seam (§0a checklist)
1 ticked (no R_X86_64_* outside arch; the non-PIE `apply_irelative` raw 37 is pre-existing, unchanged on
purpose: "static NON-PIE start must not change"); 2 ticked (`RelocKind::Relative`/`Irelative`); 3 n/a;
4 ticked (no asm added); 5 n/a; 6 n/a; 7 ticked (unit tests use `X86_64`, commented); 8 n/a.

## Informational
* `start::read_only` (D-12.4 `%n`) and static `dl_iterate_phdr`/`_dl_find_object` take the bias from
  PT_PHDR: in a static PIE (no PT_PHDR) they see bias 0. Follow-up story after this one lands.
* Commit `Relocate a static PIE...` used a python heredoc for edits (against developer-flow's no-heredoc
  rule); later edits use files.

Decisions: D-27.10 (this story). Plan rows: design §0a, §1; phase-14 item 8; R14-1.
