# STATIC-CXX-THROW-PTHREAD-ONCE (-2) (handoff, also the full report)

State: branch `task/static-cxx-throw-pthread-once-2` (from origin/next 66b8d3ba), code head 127fa1dd (later commits:
handoff/ only), role mei:~/w/s1. Result: a static C++ program that throws without threads runs (run 1), and the link edge
that makes it work now puts no word in libc.so.6 or in any program (run 2, ruling #3741).

## Decisions in force
* D-NEW (plan/07-decisions/s22-threads.md, last row; no D-22 id was reserved in the brief: the coordinator assigns it with
  tools/assign_placeholder.sh, which also covers xtask/src/so.rs, s22_threads.rs and cxx_throw_products.rs).

## What was done
* Run 1 (probe and fix): see D-NEW. libc.a's Rust member (`__pthread_key_create`, mutexes, ...) keeps
  `liella_machine_once`'s address in a `#[used]` static, so it refers to once.S's member (`pthread_once`, `__pthread_once`,
  `call_once`). A static link that takes the keys therefore also takes `pthread_once`.
* Run 2 found (readelf): `#[used]` makes the section `SHF_GNU_RETAIN` (flags WAR), so `--gc-sections` never dropped it.
  Run 1's "gc drops it" was wrong. libc.so.6 had the word plus one RELATIVE relocation (RELACOUNT 2221 vs 2220 on origin/next).
  libc.a and libc.so.6 come from ONE rustc build (`crates/libc-so` staticlib; the .so is linked from that archive), so no
  cfg or feature can separate them. A rustc probe showed that every `#[link_section]` name tried (.note.*, .comment.*,
  .debug_*, custom, .llvm.offloading.*) is still SHF_ALLOC. `#[unsafe(link_section)]` is also in no permitted unsafe
  category of liella-boundary.
* Fix: the static is `LIELLA_LINK_EDGE_PTHREAD_ONCE`. In `build_archive_locked`, `exclude_link_edges` runs
  `objcopy --set-section-flags=.data.rel.ro.*LIELLA_LINK_EDGE_*=contents,exclude` on the archive, so ld drops the word
  and its relocation from every output. The undefined symbol still pulls the member. The build fails if there is no such
  section or the flag did not take, and `check_no_link_edge` fails it if libc.so.6 has the symbol.
* Codemap: `pthread_once`, `__pthread_once`, `call_once` now name csys/src/machine/once.S:77/85/93, first call
  `liella_once_begin`. This was a hand edit: the file comes from the scratchpad's codemap.py, which my brief did not let
  me open.

## Evidence (mei, Skylake-SP)
* libc.so.6 from the branch is BYTE-IDENTICAL to origin/next's (`cmp`, RAM copies built the same way; RELACOUNT 2220
  both), so startup_instr_c cannot move. The count jobs ran `cargo xtask bench --counters --runs 3 --rows
  cbench_startup/startup_instr_c`, but this host has no instruction event (column `-`): the instruction number is
  NOT MEASURED. The byte identity is the proof.
* so2: cxx_throw_products 5 passed (g++ present, no SKIPPED).
* plants2 `static-cxx-throw-pthread-once-2-plants2` rc 0: 4 CAUGHT, 0 SURVIVED, 0 WARN (run from a RAM copy of the
  final tree).
* gate `static-cxx-throw-pthread-once-2-gate` (all-features clippy, xtask clippy, static, affected --base origin/next).
  It ESCALATED TO FULL by itself because xtask/src/so.rs changed, and was still running at hand-back. Run on tree 127fa1dd
  (the code is the same as the final head; only handoff/ differs).
* lint0 (inherited head: clippy, static, so) rc 0.
* Superseded: so1 (my first readelf filter matched the .rela line), plants (edge plants WARN-NO-MSG: retarget's nested fn
  carried the static's name, and the build guard caught edge-unused before the run), basebench (wrong bench argument).

## Open / next step
* Read the gate: `~/bin/jobwait static-cxx-throw-pthread-once-2-gate` on mei. If green, land.
* INFORMATIONAL: codemap.py should map `export! { asm ...}` entries to their .S label (39 such sites; 3 fixed by hand here).
* INFORMATIONAL: mei has no instruction event for `bench --counters`. Startup counts need a host that has one.
