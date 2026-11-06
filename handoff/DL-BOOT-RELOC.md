# DL-BOOT-RELOC (phase 14 DL-4): self-relocator, first used by libc.a's static-PIE start

**State:** branch `task/dl-boot-reloc-2` (stacks on `task/dl-boot-reloc`, base origin/next `349b94d0`), HEAD after
this handoff's commit. Role shiki:~/w/sh2. INCOMPLETE (budget): static PIEs now start; the gate's
last run failed at `static` K5 (fixed in `3bf2bbcf`, not rerun); 2 plants survive; static counts not measured.

## Done (DL-BOOT-RELOC-2)
* Crash cause fixed: the anchor was a weak hidden `_DYNAMIC`, which gcc reaches through a GOT slot that is
  unrelocated in a static PIE. Now `csys/src/adapter/image_anchor.c` declares `__ehdr_start` hidden and NOT
  weak (one `R_X86_64_PC32` lea, seen in `readelf -r libc.a`), helper `liella_link_image_header`
  (prototype in liella_csys.h). `bootstrap::static_pie_header()` relocates only when `e_type == ET_DYN`
  (`boot::position_independent`); bias = header address minus `Image::header` (p_vaddr of the PT_LOAD at
  file offset 0); PT_DYNAMIC = bias + p_vaddr. `liella_link_dynamic` removed.
* Build check: `cargo xtask so` runs `boot_anchor_pc_relative` (so.rs): the member defining the anchor
  must reference `__ehdr_start` and have no GOT-typed relocation. Unit test
  `boot_anchor_reference_is_pc_relative`; plant `weak-anchor` (weak declaration) CAUGHT.
* Unit tests: `dynamic_and_headers` (header search), `position_independent_header`.
* Pre-existing clippy failures fixed: `len % ent` -> `checked_rem`; three-op unsafe block -> `table()` helper.
* D-27.10 rule (1) rewritten; design §0a checklist item 9 (pre-relocation references PC-relative, not weak).
* Job shiki:dl-boot-reloc-2-t1 (head 7d776e68): `so` ok, ld unit 4/4, xtask anchor test ok, conformance
  `s01_static_pie` (RELR) and `s01_static_pie_rela` (RELA) PASS: each does the 100 starts (50 random,
  50 ADDR_NO_RANDOMIZE). It ended rc 1 only on the clippy lint above.

## Exact next steps (in order)
1. Rerun the gate on HEAD: `~/bin/jobrun <label> gate -- sh -c ". ~/.cargo/env; ~/bin/gate 16G sh
   ~/work/DL-BOOT-RELOC-2/gate.sh"` (clippy, static, `affected --run --base origin/next`). clippy was clean
   in the foreground on ebb71c51; static failed only on K5 (fixed in 3bf2bbcf).
2. Plants (run dl-boot-reloc-2-plants-2 on ebb71c51: 22 plants, 15 CAUGHT, 2 SURVIVED, 5 WARN-NO-MSG):
   * `relr-span` SURVIVED: the unit test never sets bitmap bit 62 far enough to tell 63*8 from 64*8 spans;
     add a RELR case with two bitmap words (the second word's base depends on the span).
   * `tls-rebase` SURVIVED: `delta = 0` passed the RELA conformance program; find out why the TLS image
     copy still works (does `start::program` already give the right TLS address, or does the program's TLS
     check not read the image?). Fix the test or remove dead code. Do not loosen the plant.
   * WARN-NO-MSG on relr-skip-word, no-relocate, pie-type-run, irelative-skip, irelative-bias: the
     conformance runner's failure text is not `FAIL s01_static_pie...`; read one log under
     /mnt/zram/claude/DL-BOOT-RELOC-2/plants-2/logs/ and fix the `msg:` regex.
3. Static non-PIE counts (INVIOLABLE): NOT MEASURED. `validation/wl_count.sh --base 349b94d0... . seq du`
   on shiki fails with `wl_prep: no link command for seq` (the workload kit tree is missing on that host;
   base source is primed there). Run it on a host with the kit (the script says it measures on the Zen 1
   host). Expected change on the non-PIE path: one direct call to the anchor (lea, ret), a 2-byte load and a
   compare, in place of the old call + test: a few instructions per process start.

## Arch seam (§0a checklist)
1 ticked (the new xtask check matches GOT-typed relocation names generically); 2 ticked; 3 n/a; 4 ticked (no
asm added: the anchor is C); 5-6 n/a; 7 ticked (unit tests use `X86_64`); 8 n/a; 9 (new) ticked.

## Not this story (follow-up the coordinator cuts)
* PT_PHDR load offset: `start::read_only` (D-12.4 `%n`) and static `dl_iterate_phdr`/`_dl_find_object` take the
  bias from PT_PHDR; a static PIE has none, so they see bias 0.

Decisions: D-27.10. Plan rows: design §0a (item 9), §1; phase-14 item 8; R14-1. Plants: handoff/DL-BOOT-RELOC-plants.txt.
