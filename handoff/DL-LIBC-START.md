# DL-LIBC-START: the dynamic path of `__libc_start_main` (phase 14 DL-14)

**State:** branch `task/dl-libc-start-3` (DL-LIBC-START-3, on -2's head d833923bd merged with origin/next 4e3941bd7:
LAYOUT-DATA-CLUSTERS-4B/4C and DL-LIBC-EARLY-2B landed). Role keke:~/w/n3. Head: see the report json.

**Decisions in force:** D-27.24 (records/decisions/D-27/D-27.24.toml), D-27.22, D-27.19/.20, D-10.5, D-X.22,
STARTUP-FAULTS-RESTORE (`.data1`), STARTUP-DATA-WINDOWS (D-X.50). Plan rows: plan/06-phases/17-phase-14-dynamic-linking.md
DL-14; plan/ph14/design.md §14.

## Done
- DL-LIBC-START(-1): dynamic `__libc_start_main` (crates/boundary/src/runtime.rs `start_under_liella_ld`, `EARLY_DONE`,
  `PROGRAM_INIT`), ld.so passes the program's init hooks (crates/boundary/src/ld.rs), `reloc_libc_start_hello_world`
  (crates/ld/tests/reloc.rs), `conformance_dynamic` + #814 lift (fpu_control_*), plants handoff/DL-LIBC-START-plants.txt.
- -2: `cargo xtask native` step 5b (native_dynamic under Liella's ld.so, xtask/src/native_examples.rs).
- -3:
  1. Inherited red fixed: `cargo xtask static` hottext refused the -2 report json naming the regen build flag.
  2. Merged origin/next. xtask/src/coldbss.rs conflicted: next fixed the same split + ThinLTO name gap (#4716,
     `split_stem`); next's file taken whole, -2's duplicate stripping and its plant `state-part-and-thinlto` dropped.
  3. STARTUP-DATA-WINDOWS no longer blocks: with 4B/4C the regen builds pass (release: 15 written pages, budget 21).
  4. hot-text lists regenerated in both profiles (tools/hottext/regen.sh . --build --allow-drop; Zen 1 host). Fast
     inlining moved with the new branch in `start`: `rt::start::program` and 3 catalog/time generics inlined at
     their traced sites (dropped); `thread::list_insert`, `catalog::textdomain` (curated extras, already absent in
     release) now match no build: tools/hottext/prune.py dropped them and their hot-text.absent lines.
     `liella_machine_fill_erms` was kept by hand: this host's dispatch does not pick it, but it is not gone (an ERMS
     host takes it); regen.sh drops a key that one CPU's trace misses (IMPROVEMENT).
  5. Dynamic conformance subset: `conformance_dynamic` applies RUN-ARGS / RUN-ENV / EXPECT-EXIT and refuses other
     directives; `s1_1_c_0_dynamic_start_conformance` (crates/boundary/tests/s01_startup_symbols.rs) runs s01_startup,
     s01_exit_order, s01_exit_status, s01_quick_exit, s01_cxa_finalize, s01_thread_dtors, s04_strings_smoke,
     s12_printf, s12_stdio_front, s23_timefmt on glibc and on Liella under Liella's ld.so.

## Jobs (keke)
step0 rc1 (static hottext, fixed), step1 rc3 (hot-text keys, regen needed), step2 rc1 (regen refused drops), step3
rc1 (2 curated keys, pruned), step4 rc0 (release regen). FINAL: `dl-libc-start-3-gate` (clippy, xtask clippy, static,
so, hot_text_products, reloc_libc_start, the dynamic subset, fpu_control, xtask native, affected --base origin/next)
and `dl-libc-start-3-plants2` (3 plants); results in the report json / hand-back.

Correction to item 4: dropping `rt::start::program` was wrong (it still has a section; my nm check grepped the
hash-less key) and failed hot_text_products; it and `intl::record_for` were restored (55f53ff6e). FINAL gate is
`dl-libc-start-3-gate2`: clippy, static, so, hot_text_products PASS; reloc_libc_start_hello_world FAILS in its
ORACLE step: keke's host C library prints `preinit setenv lost`, the test hard-codes `kept` (written on mei). Liella's
variants did not run; the subset, fpu_control, native and affected did not run (script stops at the first red).

## Next step
1. Fix reloc_libc_start_hello_world's host dependence (crates/ld/tests/reloc.rs ~891): find by a black-box probe
   why the host library loses a `.preinit_array` setenv on keke and keeps it on mei (version? setenv into a static
   environ copy?), decide what Liella must print (spec §1.1/§1.3), and compare Liella to that, not to one host.
   Then rerun ~/work/DL-LIBC-START-3/step5.sh on keke. A dynamic-subset program failing on Liella is a real gap: fix it.
2. cbench_startup side by side with origin/next (NOT RUN: no next product tree on keke; needs a second role or
   `cargo xtask bench --ab-from <next tree> --counters cbench_startup`): static start must not change.
3. D-10.5's body still says the dynamic run is a skip: an `amends` record once the subset is green.
