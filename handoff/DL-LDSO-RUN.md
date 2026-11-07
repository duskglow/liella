# DL-LDSO-RUN (phase 14 DL-6): ld.so reads auxv/env, decides secure mode, enters a dependency-free PIE

**State:** branch `task/dl-ldso-run` (stacked on DL-LDSO-PRODUCT-2's commits; base origin/next e0c6b146).
Code commits 8973010d (change) and the static-fix commit after it; then this handoff. Role mei:~/w/s3.
Decisions in force: D-27.14 (new, plan/07-decisions/s26-29.md), D-27.13 (DL-5), owner #2661 (secure = AT_SECURE
only; one UNSECURE list, two call sites), R14-2 (a), R14-8 (program entry primitive), R14-18.

## Done
* `liella_ld::start` (pure): `secure` (AT_SECURE only), `program` (refuses no/zero AT_PHDR or AT_ENTRY,
  AT_PHENT != 56, AT_PHNUM 0 or >= PN_XNUM), `rewrite` (AT_PHDR/PHNUM/PHENT/ENTRY/BASE for `ld.so PROG`),
  `ld_env` (LD_PRELOAD, LD_LIBRARY_PATH, LD_BIND_NOW, GLIBC_TUNABLES; first occurrence), `preload_names`,
  `needs_objects`. `liella_ld::map`: `plan`/`segments` for a PIE file (ET_DYN only, page-congruent sorted
  PT_LOADs, no W+X, span <= 2^46); DL-7 grows it into the general mapper.
* `Arch::features(&Auxv, &impl Machine) -> Features` (x86_64: CPUID/XGETBV; AT_HWCAP not read).
* `boundary/src/ld.rs`: after self-relocation and the provisional TP: auxv copied (<= 128 pairs, else
  refused), secure decided once, UNSECURE removed from envp and the auxv moved down to follow it, THEN the one
  `LD_*` read; tunables parsed with the secure filter; features from the seam; LD_PRELOAD names get the
  reference-format "cannot be preloaded (...): ignored." line (Liella's reason) and the program still runs.
  PT_INTERP start (bias from PT_PHDR) or `ld.so PROG ARGS` (file mapped read-only for the ELF reader, one
  aligned PROT_NONE reservation, MAP_FIXED segments, bss tail zeroed, argv shifted one word). Then DT_NEEDED
  refused, RELATIVE/RELR applied (DL-4 relocator), RELRO protected, `liella_machine_ld_enter` (new R14-8
  primitive in entry.S: %rsp, %rdx=0, %rbp=0, jmp). Failures: one line on fd 2, status 127.
* Tests `crates/ld/tests/ldso_run.rs` (7, all pass on mei; no skips): malformed auxv; rewrite/env parsing;
  features seam (source scan); 100 starts by PT_INTERP and 100 as `ld.so PROG` (half `setarch -R`), each
  checking relocation, AT_ENTRY, AT_PHDR, argv; refusals (non-ELF, DT_NEEDED, missing file) with 127;
  LD_PRELOAD/tunables read; set-group-ID child (supplementary group, no root): AT_SECURE=1, all 34 names
  gone, near-misses kept, no LD_* diagnostic. The DL-5 test `ldso_product_refuses_as_interpreter` is removed
  (superseded). `cargo xtask clippy` and `cargo xtask static` clean on mei.
* Plants: handoff/DL-LDSO-RUN-plants.txt (13, incl. the brief's three).

## Jobs (mei, started detached on the final code head; results not read before hand-back)
`dl-ldso-run-gate` (affected --run --base origin/next), `dl-ldso-run-plants`, `dl-ldso-run-counts`
(cbench_startup counters on this branch; starts after the gate). Re-attach: `~/bin/jobwait dl-ldso-run-gate
dl-ldso-run-plants dl-ldso-run-counts`; logs `~/logs/<label>.log`.
**NOT DONE:** origin/next cbench_startup counts on mei for the before/after comparison (the role is not a git
tree; needs an origin/next role sync). Expected no move: libc's start code (runtime.rs, rt::start) is
unchanged; only ld-feature code and the Arch trait changed. NOT VERIFIED until measured.

## Arch seam checklist: ticked
1 literals only in arch.rs/arch/mod.rs/entry.S/x86_64-only tests; 2 relocation via the DL-4 relocator
(RelocKind); 3 n/a (no TLS yet); 4 new primitive `liella_machine_ld_enter`, fixed sequence, header-documented,
declared once in boundary/src/arch/; 5 features only from `Arch::features` (test pins it); 6 n/a; 7 tests
`cfg(target_arch = "x86_64")` with the reason; 8 `features` added to the table; 9 nothing new runs before
relocation.

## Open / next
* RULING NEEDED: none new. (DL-5's unsafe-audit list ruling still open.)
* INFORMATIONAL: DL-5's open plant survivor `link-reloc-symbols` (handoff/DL-LDSO-PRODUCT.md) is not addressed here.
* Gaps for DL-7: ET_EXEC programs, AT_PAGESZ (4096 assumed), the mapper's corpus. Env values over 4096 bytes are
  read truncated by `c_string` (only the LD_PRELOAD diagnostic uses them now).
* Next: read the three jobs; run origin/next cbench_startup on mei; then DL-7.
