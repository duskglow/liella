# Conformance suite

`sh conformance/run.sh [name ...]` builds and runs each `conformance/c/<name>.c`: a self-checking C
program, linked statically against Liella's `libc.a` and started by Liella's runtime. The directives
(one `/* NAME: value */` comment line each) are documented at the top of `run.sh`.

## Dynamic-link mode: `LINK-HELPERS`, `DLOPEN-HELPERS`

A test that needs shared objects (the §27 dlfcn tests) declares them:

    /* LINK-HELPERS: dlh_handle1 dlh_handle2 */
    /* DLOPEN-HELPERS: dlf_c dlf_gone dlf_missingdep */
    /* HELPER-FLAGS: dlf_missingdep -ldlf_gone */

* Each name is a helper source `conformance/c/helpers/<name>.c` (a subdirectory, so the runner does
  not take it for a test), built as `lib<name>.so` (`-shared -fPIC`, then its `HELPER-FLAGS`, given
  in `conformance/c/helpers` so a version script there is found, with `-L` at the helper
  directory so `-l<earlier helper>` links one). Helpers are built in the order named, LINK-HELPERS
  first. A helper missing, failing to build or left unbuilt is a FAIL.
* `LINK-HELPERS` are linked into the program by path, in order, with `--no-as-needed` (a helper the
  program reaches only through `dlsym` is still `DT_NEEDED`); `DLOPEN-HELPERS` are only built. A
  linked helper should not set a soname (its `DT_NEEDED` entry would then be a name to search for).
* The program is built and run twice, and both runs must meet `EXPECT-EXIT`/`EXPECT-SIGNAL`/
  `EXPECT-STDERR`:
  * the reference: helpers and program built by the host compiler against the host C library;
  * Liella: the same sources against Liella's `libc.so.6` with Liella's `ld.so`
    (`target/liella/ld-linux-x86-64.so.2`) as the program interpreter. Liella's loader does not
    search for libraries yet, so the C library is named by path (a copy without `DT_SONAME`,
    through a `libc.so` script with the host's `libc_nonshared.a`, as the start files are the
    host's).
* Each run has `CONF_HELPERS` (and `LD_LIBRARY_PATH`) set to its own helper directory: a program
  opens helpers as `$CONF_HELPERS/lib<name>.so` and builds the expected texts from that path.
* `LINK-EXPORT-DYNAMIC: yes` links the program with `-rdynamic`.
* `PENDING-LIELLA: <story>`: Liella's run is expected to fail until that story lands (the dlfcn
  exports of phase 14). The reference run must pass; Liella's failing run prints
  `conformance: PENDING <test> on Liella until <story> ...` and counts (passed, and in the
  summary's pending count); Liella's run passing is a FAIL, so the directive and the clause test's
  pending mark are dropped the day the story lands. A pending program links with unresolved
  symbols allowed (what Liella does not export yet fails the run, not the link); a build failure
  is still a FAIL.
* Other link directives (`LINK-LIBM`, `LINK-UNWIND`, `LINK-CXX`, `LINK-NOW`, `LINK-STATIC-PIE`) and
  `RUN-SECURE`/`RUN-WRAP` do not apply in this mode.

Examples: `c/s27_dynlink_helper_dlsym.c`, `c/s27_dlsym_handles.c`, `c/s27_dlopen_failures.c`.
