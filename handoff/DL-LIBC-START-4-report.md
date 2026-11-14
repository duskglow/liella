# DL-LIBC-START-4 report

Branch `task/dl-libc-start-4`, code head d347d9318 (handoff commit on top). Role mei:~/w/s1. INCOMPLETE (budget).

## Commits
- 415f22f2f Undo .preinit_array environ changes in the dynamic start; drop old fini (runtime.rs, reloc.rs test,
  D-27.24 corrected, plan/ph14/design.md row, plants file).
- d347d9318 Bind to a program's COPY definition by its required version (crates/ld/src/symbols.rs).

## Why -3's gate went red (rc 5)
Not host dependence. Black-box probes on mei and keke (both Ubuntu glibc 2.39-0ubuntu8.9, gcc 13.3) print
`preinit setenv lost` for the test's exact program in every environment tried (plain, `env -i`, 200 variables, via
ld.so). The test's hard-coded `kept` never matched the reference.

## Probe-settled reference behaviour (glibc 2.39, dynamic PIE, kernel-started and as `ld.so PROGRAM`)
- `.preinit_array`: `setenv` lost, `unsetenv` undone, `program_invocation_name` change undone before constructors run.
- Constructor (`.init_array`) `setenv` reaches `main`.
- Old-style `_start` passing `init`/`fini`: `init` runs; `fini` is never called.
- Static program: all `.preinit_array` changes are kept (Liella's static start is unchanged).
Liella now matches all of these. Not calling an old crt1's `fini` also avoids running a pre-2.34 program's
`.fini_array` twice (once by `rtld_fini`, once by `__libc_csu_fini`). The test states both expected outputs
(`HELLO_OUT`, `HELLO_OLD_OUT`); the host library's run only prints a NOTE when it differs.

## ld.so gap found and fixed
A PIE using a libc data object directly gets a COPY relocation. The program's definition has a versym index that
names a DT_VERNEED entry. `Object::lookup` matched definition versions only against DT_VERDEF, so libc's own
reference failed with UnknownVersion ("malformed symbol tables"). That would affect every PIE using
stdout/stderr/environ directly under Liella's ld.so. `def_version_name` now falls back to the requirements.

## Jobs (mei)
- dl-libc-start-4-step0 rc 0: clippy + static on the inherited head.
- dl-libc-start-4-gate rc 5 (SUPERSEDED: hello world hit the ld.so COPY gap). dl-libc-start-4-plants rc 2
  (SUPERSEDED: baseline failed for the same reason).
- dl-libc-start-4-diag1: diagnostic (only the hello world failed in the whole reloc suite).
- FINAL head d347d9318: dl-libc-start-4-gate2 rc 6. Passed: clippy, xtask clippy, static, so, hot_text_products,
  reloc_libc_start. Failed: s1_1_c_0_dynamic_start_conformance at LINK (`__libc_stack_end` undefined: the subset
  links only Liella's libc.so.6; the symbol is the loader's). fpu_control, native and affected did not run.
- dl-libc-start-4-plants2 (4 plants) still running at hand-back; republish-removed CAUGHT so far.
- dl-libc-start-4-counts-base rc 1 (SUPERSEDED: `--counters-only cbench_startup` refused, "unknown argument").

## INFORMATIONAL
- The -3 subset (`conformance_dynamic`) never linked a program that uses `__libc_stack_end`; it was written but
  never ran green.
- No cbench_startup numbers yet. The static start code is unchanged except that one argument fewer is passed on its
  dynamic branch.

## Refusals
None.

## Next step
See handoff/DL-LIBC-START.md "Next step" (subset link fix, counts, D-10.5 amends).
