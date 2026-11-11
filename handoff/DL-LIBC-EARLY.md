# DL-LIBC-EARLY: LIELLA_PRIVATE, `__liella_libc_early_init` (phase 14 DL-13, first half)

**State:** branch `task/dl-libc-early` (base origin/next 672acd9aa, which already holds DL-INIT-ORDER-3 and
DL-TLS-STATIC-2: no merge needed), role shiki:~/w/sh1. Split point taken (brief): "early init runs before
constructors" is done; **the export moves and rseq publishing are NOT done** (continuation).

**Decisions in force:** D-27.22 (new, this story), D-27.19/D-27.20 (init order, interpreter exports), D-27.18/.21
(shared bias). Plan rows: plan/06-phases/17-phase-14-dynamic-linking.md DL-13; plan/ph14/design.md §14; R14-12.

## What was done
* `liella_ld::start::private`: the `LIELLA_PRIVATE` names, the 64-byte build tag (`LIELLA-BUILD-TAG:` + identity),
  `StartInfo` (argc, argv, envp, auxv, stack_end; `repr(C)`), and `find`: the first loaded object (not ld.so)
  defining `__liella_libc_early_init@LIELLA_PRIVATE` must carry `__liella_build_tag@LIELLA_PRIVATE` (64 bytes,
  inside a PT_LOAD's file part) equal to ld.so's tag, else `<object>: LIELLA_PRIVATE build tag differs ...`, 127.
* ld.so (`boundary/src/ld.rs`): the check runs after the version check, before any relocation; `run_init` calls
  the early init with the initial stack before `.preinit_array`.
* libc (`runtime::early_init`, export in s01_startup.rs; tag static in data.rs): publishes auxv, environ,
  program names, `__libc_stack_end`, parses tunables (no diagnostics), `cpu::init_with`. `__libc_start_main` is
  byte-for-byte unchanged (its hot-text keys depend on closure numbering: a refactor renamed one, so early init
  has its own step-3 helper; DL-14 makes `start` skip steps 1-4 under Liella's ld.so).
* xtask so: `LIELLA_BUILD_TAG` = FNV-1a 64 over crates/, csys/, include/, Cargo.lock (sorted paths) for both
  products (role dirs are not git checkouts, so a commit id would always be "unversioned"); libc.so.6 gets a
  `LIELLA_PRIVATE` node (`LIBC_PRIVATE` list); the versions.tsv set check skips those names.
* Tests (crates/ld/tests/reloc.rs): `reloc_libc_early_init_before_preinit` (preinit sees getenv work, memcpy/memset
  NULL,0 return; kernel-started and `ld.so PROGRAM`), `reloc_libc_build_tag_mismatch_refused` (one tag byte
  flipped in a by-path libc copy: 127 + diagnostic); unit `private_tag_layout`. libc named by path (#4537).

## Results (shiki)
* clippy `dl-libc-early-clippy3` rc 0; `dl-libc-early-static3` (clippy + records check + static) rc 0 on the
  pre-final code (the final commit only moved exports and the xtask skip; static re-runs inside the gate's steps).
* proofs `dl-libc-early-proof4` rc 0: 3 passed (bias test + the two new), no SKIP.
* Started detached on head fe50fd01d (PENDING at hand-back): `dl-libc-early-gate` (affected --run),
  `dl-libc-early-plants` (handoff/DL-LIBC-EARLY-plants.txt: early-after-preinit, tag-check-skipped, both msg:),
  `dl-libc-early-startup` (~/work/DL-LIBC-EARLY/counts.sh: cbench_startup counters, head vs next = head minus
  this story's diff, release builds in /mnt/zram/claude/DL-LIBC-EARLY/counts-1). Expectation: static rows equal
  (start() unchanged, only new functions); the dynamic start-up row was not measured (no dynamic start-up bench
  row exists yet that I found).
* Superseded: clippy, clippy2 (lint fixes), static, static2 (records tests paths, export category), proof,
  proof2, proof3 (hot-text key, versions.tsv set check).

## Open / next step
1. Read the three jobs; a red plants/gate is fixed in a continuation.
2. Continuation (DL-13 second half): move `__libc_stack_end` and `__rseq_offset/size/flags` to ld.so's exports
   (versions.tsv places them in ld-linux-x86-64.so.2; libc.a keeps them), ld.so registers rseq and publishes the
   values; libc.so.6 then references them from ld.so (needs libc.so.6 linked against ld.so or the objects
   resolved through ld.so's scope). Also DL-PLACEHOLDERS #3633: malloc_get/set_state forwarders once wanted.
3. INFORMATIONAL: the tag env var makes liella-ld and its dependants rebuild when any product source changes
   (they mostly do anyway); hashing ~13 MB costs xtask so a fraction of a second.

Plants file: handoff/DL-LIBC-EARLY-plants.txt. Read: D-27.22, design §14.
