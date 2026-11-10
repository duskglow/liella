# DL-TLS-STATIC (phase 14 DL-12): static TLS of the initially loaded objects, final thread block

**State:** branch `task/dl-tls-static` (cut from origin/next f1f5358d5), role mei:~/w/s2. Decisions in force: D-27.20
(new, this story), R14-4, R14-8, D-27.17, D-27.11, D-4.15, D-22.10, D-5.11 safety note. Full report: this file.

## What is in
* `liella_ld::arch`: TLS rows of the table (`TLS_VARIANT`, `TCB_SIZE`, `TCB_ALIGN`, `SELF_SLOT`, `DTV_SLOT`,
  `STACK_GUARD_SLOT`, `POINTER_GUARD_SLOT`, `DTP_BIAS`, `TP_BIAS`, `apply_tls`, `TlsOperands`, `TlsVariant`).
  The three test doubles (elf_reader.rs, elf_tables.rs) got variant-I values.
* `liella_ld::tls` (pure): `layout` (variant II and I, Drepper §3.4, keeps `p_vaddr mod p_align`, 64 KiB `SURPLUS`
  below the last block), `fill_dtv`, `lookup` (generation + slot-count check), `image_in_load`.
* `liella_ld::reloc`: `Obj.tls`; DTPMOD64 / DTPOFF64 / TPOFF64 applied (no symbol = own module); `STT_TLS` values not
  biased; `RelocError::TlsModule` (object without PT_TLS); TLSDESC still refused (message changed).
* `boundary/src/ld.rs`: canary + pointer guard from AT_RANDOM into the provisional block; `install_tls` maps the
  area (surplus, blocks, thread block with FLAG_OWN/self/dtv/guards, dtv), copies `.tdata` after relocation, sets
  TP; used on all three start paths (with dependencies, program alone with/without PT_DYNAMIC). An object naming
  the interpreter's soname gets the running interpreter (file read for its tables, not relocated again).
  `__tls_get_addr` exported (refuses a block without FLAG_OWN, then generation/slot checks; else exit 127).
* `xtask so`: the ld.so version script is generated from `spec/abi/versions.tsv` for `LD_EXPORTS` (`__tls_get_addr`
  at GLIBC_2.3). `tb.rs`: `Dtv` field marker.
* Tests `crates/ld/tests/tls.rs`: layout/surplus/alignment, TLS arithmetic, dtv lookup + generation, and the
  LE/IE/GD/LD matrix (program + liba + libb; alignments 4, 16, 32, 64; tdata/tbss; GD vs IE agree on one address;
  canary low byte 0, guard non-zero, `%fs:0x10` = TP), as `ld.so PROG` and via PT_INTERP. readelf asserts the
  corpus has DTPMOD64/DTPOFF64/TPOFF64.

## Owner safety requirement (D-5.11 / TB-FOREIGN-SAFETY)
TB-FOREIGN-SAFETY (task/tb-foreign-safety..-4) is NOT in origin/next, so it is not merged. This story writes only
the block it mapped itself, and `__tls_get_addr` checks FLAG_OWN of the calling block before reading the dtv word.
Accesses that will need the process-level ownership test once libc runs under another loader: DL-13's
`__liella_tls_init`/`__liella_tls_release` (writes into a libc-made block) and any libc read of `ThreadBlock::dtv`.

## RULING NEEDED
1. The interpreter as an object reads its own FILE (program's PT_INTERP path, or /proc/self/exe when started as a
   program) to get its tables. Same family as #4229 (no /proc/self/exe re-read). Recommend: fold into DL-RELOC-4's
   image-mode reader (the interpreter's own image is read-only to the relocator, so it is the easy case).
2. Start of a program WITHOUT dependencies under ld.so now costs one mmap + one arch_prctl more (it used to run on
   the provisional block in ld.so's .bss, which made LE TLS in such programs write into ld.so's data). Recommend keep.

## Proof / jobs (mei, role s2)
* dl-tls-static-lint0 (first clippy: 2 arithmetic lints, fixed), dl-tls-static-t1 (reloc message test), t2
  (clippy 0, so 0, tests tls 4/4, reloc 8/8, elf_reader, elf_tables, ldso_run all green), dl-tls-static-static (rc 0).
* dl-tls-static-gate2 (affected --base origin/next), dl-tls-static-plants (6 plants), dl-tls-static-counts
  (cbench_startup): results in the section below. A first `dl-tls-static-gate` was started undetached by mistake
  and ended with kill-label (not a result).
* cbench_startup's programs are static Liella builds that never enter ld.so; libc code is unchanged (tb.rs adds a
  marker type only). The run is this branch only, not a side-by-side with next: NOT VERIFIED numerically vs next.

## Arch seam: ticked
1 literals only in arch.rs and x86_64-tagged tests; 2 RelocKind only; 3 variant, TCB size/align and all slots from
the table (const asserts tie them to `ThreadBlock`); 4 no new asm; 5, 6, 9 n/a; 7 tests cfg x86_64; 8 rows added.

## Not done / next step
Variant I is implemented but has no host test (A-4). TLSDESC (R14-8 primitives), threads (`__liella_tls_layout/
init/release`, DL-13), dlopen'd TLS and the slow path (DL-15/16), `cbench_tls` rows (M14-6). D-27.20 was written by
hand in the records' shape (no cargo on the agent host); `cargo xtask static` accepted it.

## Results
