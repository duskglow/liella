# DL-LIBC-START: the dynamic path of `__libc_start_main` (phase 14 DL-14)

**State:** branch `task/dl-libc-start-10` (-9's head f9200d008 + merge of origin/next 5e5ec2a57 + one fix; code
head 84fb8d645). Role keke:~/w/n3. Report: handoff/DL-LIBC-START-10-report.md; json
handoff/reports/DL-LIBC-START-10.json. Plants: handoff/DL-LIBC-START-10-plants.txt. Earlier: -6..-9 reports and plants.

**Decisions in force:** D-27.24, D-27.22, D-27.20 (ld.so exports; R14-12 (a) placement), D-27.19, D-27.11 (amended
by D-27.27), D-27.17, D-27.26 (DL-LIBC-EARLY: rseq, loader exports), D-10.5 (amended by D-10.100), D-X.10, D-X.22. Plan rows:
plan/06-phases/17-phase-14-dynamic-linking.md DL-14; plan/ph14/design.md §14; R14-12 in plan/ph14/open-questions.md.

## Done
- -1..-4: dynamic `__libc_start_main` (runtime.rs `start_under_liella_ld`), init hooks, hello-world test, xtask native
  5b, `.preinit_array` undo and no old-crt1 `fini` (probe-settled), ld.so COPY-definition version fix.
- -5: ld.so defines and exports `__libc_stack_end@@GLIBC_2.2.5`; the dynamic subset (`conformance_dynamic`) links
  through a `libc.so` script with the HOST's `libc_nonshared.a`; s01_thread_dtors passes `&__dso_handle`;
  s12_stdio_front and s23_timefmt are static-only (they fail on the reference).
- -6: the dynamic `__fpu_control` hook works. Cause: libc.so.6 reads `__fpu_control` through a `GLOB_DAT` naming
  `GLIBC_2.2.5` (no local binding: readelf shows the relocation, DEFAULT visibility), and `liella_ld::symbols`
  refused a base (index 1) definition for any named version, so libc bound its own. Two black-box probes (glibc 2.39,
  spec/behaviour/dynlink.md "Symbol binding", new row) show a versioned reference, the library's own or a program's
  needed one, binding an unversioned definition earlier in the scope. New `Want::Reference` (from `reference_want`)
  accepts the exact version, else a base definition; `Want::Version` (dlvsym, start-up handshake) is unchanged.

- -7: native_dynamic has its own expected output (static list minus the static-only `kernel types` step, D-X.10).
- -8: merged next (DL-LIBC-EARLY-6, STRTO-FENV-FLAGS-3): `StartInfo` carries both the program's init hooks and
  `rseq_size`; ld.so defines `__libc_stack_end` once (mangled under feature `runtime`) and stores it with the rseq
  objects; hot lists regenerated per profile, three dead keys dropped, five new unpinned generic keys kept out
  (details and RULING in the -8 report).
- -9: -8's gate failed only hot_text_products `init_written_startup_objects_are_in_the_data1_cluster`:
  `runtime::EARLY_DONE` (1 byte, this branch's dynamic-path flag, in startup-data.keys since -8's regen) was not in the
  hand-written crates/libc-so/startup-state.list, so it stayed in `.bss.<name>`. Declared in the every-start tier;
  all 23 hot_text_products tests pass. Ruling #5171's five keys were already absent from hot-text.list/.unpinned.
- -10: merged origin/next 5e5ec2a57. hot-text.list, .unpinned, .absent and probe-text.keys are next's unchanged
  (the branch's edits there were -8's dead-key drops and regen lines for functions it does not touch; next has since
  moved those keys itself). The branch's start-up keys (startup-data.keys, startup-state.list: EARLY_DONE) kept.
  After the merge the fast build's first mktime after localtime_r took 1 fault (next: 0). A page-fault trace of both
  builds showed that the branch emitted `TscGate::fresh` (time zone instantiation) out of line, in cold text, where
  next inlines it. Fixed with `#[inline(always)]` (crates/legacy/src/recheck.rs). Fast: 23/23.

## Do not redo
-4's host probes; -5's cause hunt for the link/oracle failures; -6's two binding probes; -8's merge and regen;
-10's merge and the mktime fault trace (cause above). Do not re-add the five generic #5171 keys.

## Next step
1. If dl-libc-start-10-gate2 (keke) is red, fix that first (failing suite only).
2. Land. (Ruling #5171 resolved: merge.py skips refused keys, HOTLIST-TRAIN-REGEN-3.)
3. DL-LIBC-NONSHARED (#4807, accepted) ships Liella's own libc_nonshared.a + libc.so script.
4. s23_timefmt check 75 fails on the reference: a stdio/time story finds whether the program or the spec is wrong.
5. DL-14 threads: a test that a dynamic program's new thread registers rseq at the loader's offset (covers
   `StartInfo.rseq_size`).
6. `runtime::PROGRAM_INIT` (24 bytes) is written by `early_init` on every start under Liella's ld.so but is not
   start-up state of a static start (the trace is static): declare it when a dynamic start-up fault trace exists.
