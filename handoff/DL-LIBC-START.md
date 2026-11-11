# DL-LIBC-START: the dynamic path of `__libc_start_main` (phase 14 DL-14)

**State:** branch `task/dl-libc-start-5` (DL-LIBC-START-5, on -4's head 758adfda5; merge base with next 07a4db6ac).
Code head 8a228f3db; role mei:~/w/s1. Report: handoff/DL-LIBC-START-5-report.md; json handoff/reports/DL-LIBC-START-5.json.
Plants: handoff/DL-LIBC-START-5-plants.txt.

**Decisions in force:** D-27.24, D-27.22, D-27.20 (ld.so exports; R14-12 (a) placement), D-27.19, D-27.11, D-10.5,
D-X.22. Plan rows: plan/06-phases/17-phase-14-dynamic-linking.md DL-14; plan/ph14/design.md §14; R14-12 in
plan/ph14/open-questions.md.

## Done
- -1..-4: dynamic `__libc_start_main` (runtime.rs `start_under_liella_ld`), init hooks, hello-world test, xtask native
  5b, `.preinit_array` undo and no old-crt1 `fini` (probe-settled), ld.so COPY-definition version fix. See the -4 report.
- -5: (1) ld.so now defines and exports `__libc_stack_end@@GLIBC_2.2.5` (crates/boundary/src/ld.rs, stored from the
  initial stack at the top of `link()` before any relocation; xtask/src/so.rs `LD_EXPORTS`). It is unmangled only
  without the `runtime` feature, so test builds never interpose on the host loader's. (2) The dynamic subset
  (`conformance_dynamic`, crates/boundary/tests/products/mod.rs) links through a `libc.so` linker script:
  `GROUP ( libc-by-path libc_nonshared.a AS_NEEDED ( ld.so ) )`, the installed script's shape. `libc_nonshared.a` is
  the HOST's (Liella ships none; `atexit`/`at_quick_exit` are not in libc.so.6's ABI). (3) s01_thread_dtors passes
  `&__dso_handle` (weak; NULL in Liella's static link, which has no crtbegin), not NULL: with NULL the program
  crashed on the reference itself. (4) s12_stdio_front and s23_timefmt left the subset: both fail on the reference
  (exit 71: Liella's recorded wide-stream EINVAL; exit 75: unexplained), and the subset's contract is both-ways.

## Do not redo
-4's host probes; the -3 hot-text regen and coldbss merge; the cause hunt for the four link/oracle failures above.

## Next step
1. If the gate/plants/counts named in the report are red, fix that first (labels in the report).
2. RULING (see report): a Liella `libc_nonshared.a` product plus an installed `libc.so` script; then the subset uses
   Liella's archive instead of the host's.
3. s23_timefmt check 75 fails on the reference: find whether the program or Liella is wrong (a stdio/time story).
4. D-10.5's body still says the dynamic `__fpu_control` run waits for Liella's ld; it now runs (sym___fpu_control
   via `conformance_dynamic`, plant fpu-control-dynamic-hook-dropped): an `amends` record (needs an id from the
   coordinator).
