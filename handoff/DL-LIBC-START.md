# DL-LIBC-START: the dynamic path of `__libc_start_main` (phase 14 DL-14)

**State:** branch `task/dl-libc-start-2` (DL-LIBC-START-2; on `task/dl-libc-start`'s head e521e6e3e, itself on
`task/dl-libc-early-2` 47fa75c1d: DL-LIBC-EARLY-2 had NOT landed on next on 11-11, so no merge with next was made).
Role mei:~/w/s3. INCOMPLETE: `cargo xtask so` is not green yet on this branch (below); budget checkpoint.

**Decisions in force:** D-27.24 (records/decisions/D-27/D-27.24.toml), D-27.22, D-27.19/.20, D-10.5, D-X.22 (cold
`.bss`), STARTUP-FAULTS-RESTORE (start-up state `.data1`), STARTUP-DATA-WINDOWS (D-X.50).
Plan rows: plan/06-phases/17-phase-14-dynamic-linking.md DL-14; plan/ph14/design.md §14.

## Done (DL-LIBC-START, -1 run)
Dynamic `__libc_start_main` path (crates/boundary/src/runtime.rs `start_under_liella_ld`, `EARLY_DONE`,
`PROGRAM_INIT`), ld.so passes the program's init hooks (crates/boundary/src/ld.rs), tests
`reloc_libc_start_hello_world` (crates/ld/tests/reloc.rs) and `conformance_dynamic` (fpu_control_*; #814 skip lifted),
plants handoff/DL-LIBC-START-plants.txt.

## Done (DL-LIBC-START-2)
1. **`xtask so` start-up state check (`SYSINFO_EHDR (0 sections)`)** diagnosed: in the `fast` profile LLVM split the
   static (`.0`) AND ThinLTO promoted it (`.llvm.<hash>`): section `.data.<sym>.0.llvm.<hash>`. The check stripped one
   suffix or the other, never both. Fixed in xtask/src/coldbss.rs (`section_key`: ThinLTO suffix, then part number);
   the check is as strict as before (one section per key). SYSINFO_EHDR stays a declared start-up static: the
   STATIC start writes it too, so "keep it off the static path" did not apply. Unit test extended; plant
   `state-part-and-thinlto` added. Same parser is on next (unchanged there).
2. **hot-text.list**: 4 `extra` keys (thread::list_insert, recheck/time_compat select, catalog variants, textdomain)
   no longer match a function section of the fast libc.a (codegen-unit split moved with runtime.rs). Remedy is
   `tools/hottext/regen.sh . --build` in both profiles (the list is generated; never hand-edit).
3. **STARTUP-DATA-WINDOWS** (fast) is the BLOCKER: 21 written start-up pages > budget 20, in regen's
   `xtask so --hottext-regen` build (so the hot-text list is not regenerated either). Tried and REVERTED (6b3234716):
   `EARLY_DONE`/`PROGRAM_INIT` renamed `.lbss` -> still 21 pages (FILL_MIN, global_locale::C, __environ, RESERVE_MEM,
   HEAP, EXIT_HANDLERS each alone on a page) and COLD-BSS-LBSS broke. The new statics are not the cause. Not yet known:
   whether the base 47fa75c1d (or origin/next) is at 20 or 21 in this role (layout jitter, ruling #4347, applies
   only to changes that touch no start-up code; this one does).
4. `cargo xtask native` step 5b (xtask/src/native_examples.rs): native_dynamic linked against a by-path libc copy with
   Liella's ld.so, run, output must equal the static one's with `linkage dynamic`. Compiles; NOT RUN (no products).

## Jobs (mei)
dl-libc-start-2-step0 rc1 (inherited failure reproduced), -step1 rc1 (state check fixed; hot-text keys), -regen
(stopped by me: script ended in `git diff`, role is not a git tree), -regen2 rc1 (21>20), -step2 rc1 (clippy: doc
list indentation, fixed), -step3 rc1 (21>20 with the .lbss move, + COLD-BSS-LBSS; reverted). Clippy and static
passed in step3 on 5db1cf448; the revert restores the coldbss.rs of 07fce60b5 (its unit test passed in step1).

## Next step
1. Measure the fast-profile STARTUP-DATA-WINDOWS page count of the base 47fa75c1d in the same role
   (`cargo xtask so`, fast) and of this head; if the base is 20, find which start-up-written object moved
   (nm of both libc.a, the BROKEN lines) and make the new code fit (ALWAYS FORWARD); never raise the budget.
   If DL-LIBC-EARLY-2 has landed, merge origin/next first (allowed) and measure on the merge.
2. Then `bash tools/hottext/regen.sh . --build` and `LIELLA_FULL_PROFILE=1 bash tools/hottext/regen.sh . --build`
   in the role (script ~/work/DL-LIBC-START-2/step2.sh on mei does lints, regen, so, the dynamic tests, `xtask native`);
   copy crates/libc-so/hot-text.list back to the worktree and commit it.
3. Plants: `cargo xtask plants handoff/DL-LIBC-START-plants.txt --dir /mnt/zram/claude/DL-LIBC-START-3/plants-1`;
   `affected --run --base origin/next`; hot_text_products; cbench_startup counters head vs next (static unchanged).
4. Wider dynamic conformance subset (only fpu_control_* now); D-10.5's body still says the dynamic run is a skip:
   correct it with an `amends` record once the test is green.
