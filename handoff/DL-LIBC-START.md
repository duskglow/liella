# DL-LIBC-START: the dynamic path of `__libc_start_main` (phase 14 DL-14)

**State:** branch `task/dl-libc-start-6` (DL-LIBC-START-6, on -5's head d45e62e70; merge base with next 07a4db6ac).
Role mei:~/w/s1. Report: handoff/DL-LIBC-START-6-report.md; json handoff/reports/DL-LIBC-START-6.json.
Plants: handoff/DL-LIBC-START-6-plants.txt (the four new plants plus -5's three).

**Decisions in force:** D-27.24, D-27.22, D-27.20 (ld.so exports; R14-12 (a) placement), D-27.19, D-27.11 (amended
by records/pending/DL-LIBC-START-6-1.toml, id to be assigned), D-27.17, D-10.5, D-X.22. Plan rows:
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

## Do not redo
-4's host probes; the -3 hot-text regen and coldbss merge; -5's cause hunt for the link/oracle failures; -6's two
binding probes (sources in the -6 report).

## Next step
1. If the gate/plants/counts named in the -6 report are red, fix that first.
2. Coordinator: assign the id of records/pending/DL-LIBC-START-6-1.toml (amends D-27.11), and the D-10.5 amends
   record (its body still says the dynamic `__fpu_control` run waits for Liella's ld; it now runs and passes).
3. RULING #4807 (Liella's own `libc_nonshared.a` + installed `libc.so` script): a product story once ruled.
4. s23_timefmt check 75 fails on the reference: a stdio/time story finds whether the program or the spec is wrong.
