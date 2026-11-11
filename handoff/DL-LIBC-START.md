# DL-LIBC-START: the dynamic path of `__libc_start_main` (phase 14 DL-14)

**State:** branch `task/dl-libc-start-4` (DL-LIBC-START-4, on -3's head 6c6198db6; merge base with next 07a4db6ac).
Code head d347d9318; role mei:~/w/s1. Full report: handoff/DL-LIBC-START-4-report.md; json handoff/reports/DL-LIBC-START-4.json.

**Decisions in force:** D-27.24 (corrected in -4, records/decisions/D-27/D-27.24.toml), D-27.22, D-27.19/.20, D-27.11,
D-10.5, D-X.22, STARTUP-FAULTS-RESTORE (`.data1`), STARTUP-DATA-WINDOWS (D-X.50). Plan rows:
plan/06-phases/17-phase-14-dynamic-linking.md DL-14; plan/ph14/design.md §14.

## Done
- -1: dynamic `__libc_start_main` (crates/boundary/src/runtime.rs `start_under_liella_ld`, `EARLY_DONE`,
  `PROGRAM_INIT`), ld.so passes init hooks, `reloc_libc_start_hello_world`, plants handoff/DL-LIBC-START-plants.txt.
- -2: `cargo xtask native` step 5b. -3: merged next, hot-text lists regenerated, dynamic conformance subset
  `s1_1_c_0_dynamic_start_conformance` written (never ran green; see Next step).
- -4 (see the report): (1) the -3 red was NOT host-dependence: glibc 2.39 prints `preinit setenv lost` on mei AND keke
  (black-box probes); the test's hard-coded `kept` was wrong. (2) Probed reference behaviour, now Liella's: a dynamic
  program's `.preinit_array` setenv/unsetenv/`program_invocation_name` change is undone before constructors, a
  constructor's setenv stays, an old crt1's `fini` is never called. `start_under_liella_ld` re-publishes environ and
  names before the program's init and no longer registers `fini` (static start unchanged). (3) The test states its
  expected outputs (no host oracle; a NOTE when the host differs). (4) ld.so gap found and fixed: a program's COPY
  definition (versym = a verneed index) made libc's lookup fail ("malformed symbol tables") for any PIE using
  stdout/stderr/environ directly; crates/ld/src/symbols.rs `def_version_name`.

## Do not redo
Host-library probes (report lists them); the hot-text regen (-3); the coldbss merge (-3).

## Next step
1. `s1_1_c_0_dynamic_start_conformance` fails at LINK: s01_startup references `__libc_stack_end`, which belongs to the
   dynamic loader in the ABI (spec/abi/versions.tsv), and the subset links only Liella's libc.so.6. Fix the subset's
   link (crates/boundary/tests/products/mod.rs ~313, `conformance_dynamic`) to also link Liella's ld.so (check it
   exports `__libc_stack_end`@GLIBC_2.2.5; if not, that is the gap to fix). Then rerun ~/work/DL-LIBC-START-4/gate.sh
   on mei (steps: clippy, static, so, hot_text_products, reloc_libc_start, subset, fpu_control, native, affected).
2. cbench_startup vs next: ~/work/DL-LIBC-START-4/counts.sh base|head (base = copy of ~/wl/src/07a4db6ac in
   ~/work/DL-LIBC-START-4/next-base, already copied). `--counters-only cbench_startup` was refused there ("unknown
   argument"): read xtask/src/bench.rs ~1505 for how --counters-only takes its bench, fix the script, run base then head.
3. D-10.5's body still says the dynamic run is a skip: an `amends` record once the subset is green.
