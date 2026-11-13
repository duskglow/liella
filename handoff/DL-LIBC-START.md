# DL-LIBC-START: the dynamic path of `__libc_start_main` (phase 14 DL-14)

**State:** branch `task/dl-libc-start-8` (DL-LIBC-START-8: -7's head merged with next b31418181, code head
8c95bc90c). Role kanon:~/w/kn1. Report: handoff/DL-LIBC-START-8-report.md; json handoff/reports/DL-LIBC-START-8.json.
Plants: handoff/DL-LIBC-START-8-plants.txt (3 on the merge sites + -7's 3). Earlier: -6/-7 reports and plants.

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

## Do not redo
-4's host probes; -5's cause hunt for the link/oracle failures; -6's two binding probes; -8's merge and regen (the
hot-text.list is next's minus three dead keys; do not re-add the five generic keys without pinning them by count).

## Next step
1. If dl-libc-start-8-gate (FULL) or dl-libc-start-8-plants on kanon is red, fix that first (failing suite only).
2. Land. Then RULING (-8 report): regen.sh vs the check_pinned static check.
3. DL-LIBC-NONSHARED (#4807, accepted) ships Liella's own libc_nonshared.a + libc.so script.
4. s23_timefmt check 75 fails on the reference: a stdio/time story finds whether the program or the spec is wrong.
5. DL-14 threads: a test that a dynamic program's new thread registers rseq at the loader's offset (covers
   `StartInfo.rseq_size`).
