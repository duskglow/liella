# Conformance suite

`sh conformance/run.sh [name ...]` builds and runs each `conformance/c/<name>.c`: a self-checking C
program, linked statically against Liella's `libc.a` and started by Liella's runtime. The directives
(one `/* NAME: value */` comment line each) are documented at the top of `run.sh`.

A program that passed is not run again for the same thing (D-X.38): `conformance/build/<name>.pass` records
the hash of the built program, of the process state it inherits (environment, rlimits, umask, blocked and
ignored signals, descriptors, credentials, directory) and its link key (the products' hash among them). A
later call with the same stamp counts it as passed and prints `conformance: N not rerun (...)`; a failed
run records nothing. `LIELLA_CONFORMANCE_RERUN=1` (or removing the `.pass` files) forces a rerun. The runner does not
pass the test runner's `NEXTEST_*` and `CARGO_*` variables to the programs.

## Dynamic-link mode: `LINK-HELPERS`

A test that needs shared objects (the §27 dlfcn tests) declares them:

    /* LINK-HELPERS: dlh_add other_helper */

* Each name is a helper source `conformance/c/helpers/<name>.c` (a subdirectory, so the runner does
  not take it for a test). The runner builds it as `conformance/build/<test>.helpers/lib<name>.so`
  (`-shared -fPIC`).
* The test program is linked dynamically against the helpers (`-l<name>`, with `--no-as-needed`, so a
  helper the program only reaches through `dlsym` is still a `DT_NEEDED` entry) and run with
  `LD_LIBRARY_PATH` set to that directory (appended to `RUN-ENV` when that is given).
* Until Liella has its own `ld.so` the mode runs against the reference C library only. A pass
  prints `SKIPPED <test> for Liella (...)` and counts as passed; a failure on the reference is a
  FAIL. A missing or non-building helper is a FAIL.
* Other link directives (`LINK-LIBM`, `LINK-UNWIND`, `LINK-CXX`, `LINK-NOW`) do not apply in this mode.

Example: `c/s27_dynlink_helper_dlsym.c` with `c/helpers/dlh_add.c`.
