# Smoke results: Debian programs built against Liella (AQOURS-SMOKE, 2026-11-13)

Harness: [README.md](README.md). Debian trixie source packages, built on a Zen 1 build host (Ubuntu 24.04,
gcc 13, glibc 2.39 as the reference C library) twice: against the host's C library ("reference") and
against Liella (release-profile products of this branch's base). Each program's own test suite ran on both
builds. Every Liella build passed `check-elf.sh` (all objects on Liella's loader and libc; no fallback).

Counts are test-suite totals: pass / fail / skip (/ xfail where the suite has them). "Differs" lists the
tests whose result on Liella is not the reference's.

| Program (source package) | Built | Suite | Reference | Liella | Differs on Liella |
|---|---|---|---|---|---|
| zlib | both | `make test` (one suite) | pass | pass | none |
| xz-utils | both | `make check`, 19 tests | 19 / 0 / 0 | 19 / 0 / 0 | none |
| sqlite3 | both (no load-extension, b) | fuzzcheck, 46388 cases | 0 errors | 0 errors | none |
| grep | both | `make check` incl. gnulib-tests, 353 | 282 / 0 / 69 / 2 xfail | 281 / 3 / 69 / 0 xfail | `stack-overflow` (F5), `glibc-infloop` and `triple-backref` XPASS (F6) |
| sed | both | `make check`, 260 | 213 / 0 / 47 | 213 / 0 / 47 | none |
| diffutils | both | `make check`, 240 | 202 / 0 / 37 / 1 xfail | 202 / 0 / 37 / 1 xfail | none |
| findutils | both | `make check`, 387 | 313 / 0 / 74 | 313 / 0 / 74 | none (the DejaGnu part needs `runtest`, absent on both) |
| coreutils | both | `make check` incl. gnulib-tests, 1185 | 963 / 0 / 222 | 952 / 2 / 231 | `ls/removed-directory` FAIL (F1); `misc/stdbuf` FAIL and 7 tests SKIP for LD_PRELOAD (F7); `tail/inotify-race`, `inotify-race2` SKIP for gdb (F8) |
| gawk | interpreter (no extensions, b) | `make check` in `test/`, 529 | 529 / 0 / 0 | 529 / 0 / 0 | none |
| make (make-dfsg) | no (c, F11) | - | - | - | - |
| bash | both | `make tests`, 83 `run-*` sections | 83 (the reference's own output) | 82 same, 1 differs | `run-glob-test`: the setlocale message (F3); `run-execscript` differs only in a process id |
| perl | miniperl (b) | `make minitest-notty`: 362 scripts, 468895 tests | all successful | all successful | none |
| python3.13 | built-in modules (b) | regrtest sample of 35 files: 4772 tests, 458 skipped | 34 OK, `test_posix` fails | 34 OK, `test_posix` fails | none (the same file fails on both) |
| cppunit (C++: iostream, exceptions) | library + test program (b) | `make check` in `examples/`, 3 | 2 / 0 / 0 / 1 xfail | 2 / 0 / 0 / 1 xfail | none |
| lua5.4 | not built (c, F10) | - | - | - | - |
| a program that dlopens plugins | not possible yet (b, F9) | - | - | - | - |

Programs not reached in this run: a C++ package with threads (cppunit has none; Python's `test_threading`
and `test_thread` cover the C thread functions), and a fuller Python run.

## Findings

Classes: (a) Liella differs from the reference (each needs a ruling: [RULING NEEDED in the hand-back]),
(b) not implemented yet (the loader story is named), (c) harness or build issue. Reproducers are in `repro/`
and run against both C libraries as each file's comment says.

| # | Class | Seen in | What | Reproducer / item |
|---|---|---|---|---|
| F1 | (a) | coreutils `tests/ls/removed-directory` | `readdir` on a stream whose directory was removed after `opendir`: Liella returns NULL with `errno` `ENOENT`; the reference returns NULL with `errno` 0 (end of stream), as `spec/behaviour/fstree.md` ("Removed directory") records. `ls` in a removed directory then fails with "reading directory '.': No such file or directory". | `repro/readdir_removed.c` |
| F2 | (a) | probe after grep's results | `regcomp` of an ERE with 256 or more nested groups: Liella returns `REG_ESPACE` (`MAX_DEPTH` 256 in `crates/core/src/pattern/regex/parse.rs`); the reference compiles and matches depths 256 to 1000. No row in `docs/differences-from-glibc.md` records the limit. | `repro/regcomp_nesting.c` |
| F3 | (a) | bash `run-glob-test` | `setlocale` that fails (locale not installed) leaves `errno` unchanged (0 in the probe); the reference sets `ENOENT`. bash appends `strerror(errno)` to its warning when `errno` is set, so its message differs. POSIX defines no `setlocale` errors; Liella's `newlocale` sets `ENOENT` for an unknown name. | `repro/setlocale_errno.c` |
| F4 | (b) | sqlite3, cppunit (`DllPlugInTester`), Python, gawk and Lua extensions, perl's DynaLoader | Liella's `libc.so.6` exports `dlopen`, `dlerror` and `dl_iterate_phdr` but not `dlsym`, `dlvsym` (DL-16), `dlclose` (DL-17), `dladdr`, `dlinfo` (DL-18): a program that references them does not link. Built here without those parts. | link errors in the build logs |
| F5 | test expectation | grep `tests/stack-overflow` | The test feeds `grep -E` patterns of 10^4 to 10^7 unmatched `(`. Liella's `regcomp` reports `REG_EPAREN` ("Unmatched ( or \\(") at every depth, which is the POSIX result for an unmatched parenthesis, and grep exits 2, an outcome the test does not accept. Not a Liella defect; the test's assumption. | - |
| F6 | informational | grep `glibc-infloop`, `triple-backref` | Two tests marked expected-to-fail (XFAIL) on the reference pass on Liella (XPASS, which automake counts as a failure). Liella's regex gives the results the tests want. | - |
| F7 | (b) | coreutils `misc/stdbuf` and 7 tests that preload a helper (`csplit-io-err`, `dd/nocache_fail`, `df/no-mtab-status`, `df/skip-duplicates`, `ls/getxattr-speedup`, `rm/rm-readdir-fail`, `rm/r-root`) | Liella's loader ignores `LD_PRELOAD` ("loading objects is not available in this build"): DL-21 (its validation item T17 is `stdbuf`). | - |
| F8 | (b) | coreutils `tail/inotify-race`, `inotify-race2` | gdb cannot set a breakpoint in a program started on Liella's loader (the tests skip): the debugger interface `_r_debug` / `_dl_debug_state` is DL-23. Not investigated further. | - |
| F9 | (b) | the dlopen program of the brief | No program can load a plugin and call into it without `dlsym` (F4). Also every shared library a program needs is found only by soname at start-up, which Liella's loader cannot do yet (DL-8's search is not wired into the start-up path): the harness links libraries statically, and Liella's `libm.so.6` (which names `libc.so.6` by soname) is replaced by `libm.a`. | - |
| F10 | (c) | lua5.4 | Debian's build of lua5.4 uses libtool, which the host does not have; the source package has no test suite. Needs the chroot (below). | - |
| F11 | (c) | make-dfsg | Debian removes `doc/` and regenerates the autotools files; without autotools the shipped files still want `doc/make.1`, and building the `make` target alone needs `lib/libgnu.a` first. Recipe to fix (build `lib/` first), then rerun. | - |
| F12 | (c) | all | The build host has neither `mmdebstrap` nor `newuidmap` (`uidmap`), so no Debian chroot: the harness builds Debian's sources with the host toolchain and without Debian's build dependencies (no autotools, libtool, cmake, Tcl or zlib headers). | - |

## How to rerun

```sh
LIELLA_FULL_PROFILE=1 cargo xtask so
validation/smoke/smoke.sh --work /tmp/smoke --jobs 24          # every program, about 45 minutes
validation/smoke/smoke.sh --work /tmp/smoke coreutils bash     # some
```
