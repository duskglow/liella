# Smoke: real programs built from Debian sources against Liella

`validation/smoke/smoke.sh` builds programs from their Debian source packages (suite trixie) twice, once with
the host's compiler and C library (the reference) and once against Liella, runs each program's own test
suite on both builds, and records the counts. Results of the last run: [RESULTS.md](RESULTS.md).

```sh
LIELLA_FULL_PROFILE=1 cargo xtask so                 # Liella's products, release profile, in target/liella
validation/smoke/smoke.sh selftest                   # the harness's own check (no download, seconds)
validation/smoke/smoke.sh --work DIR [--jobs N] [PROGRAM...]   # default: every program, in order
validation/smoke/smoke.sh --list                     # the program names
```

`--products DIR` reads Liella's products elsewhere (default `target/liella`), `--timeout SECS` is the hang
guard per build and per test suite (default 5400). Results go to `DIR/results.tsv` (one row per program and
side: built, check, total/pass/fail/skip/xfail/error, status) and `DIR/results/<program>.<side>.*` (build and
test logs, `.count.failed`: the failing tests; `<program>.liella-only.failed`: the tests that fail on
Liella and pass on the reference). Exit status 1 means a Liella build produced an object that is not linked
against Liella (below); test failures are results, not errors.

## How a program is built against Liella

* **Sources**: only from deb.debian.org, through a private apt configuration under `DIR/apt` (`deb-src` lines
  only). Its trust anchor is the `debian-archive-keyring` package fetched over https from deb.debian.org; apt
  checks the suite's signature and each file's hash. `apt-get source` applies Debian's patches. Nothing is
  installed and nothing needs root.
* **Compiler**: `liella-cc` / `liella-c++` (this directory) as `CC` / `CXX`. Compiling is the system compiler
  with the system's headers. Linking is the dynamic link line of `conformance/run.sh`: Liella's loader as
  the program interpreter, Liella's `libc.so.6` linked by path (a copy without `DT_SONAME`, because the
  loader does not search for libraries by soname yet), `libm` from Liella's `libm.a` (Liella's `libm.so.6`
  names `libc.so.6` by soname), `-lpthread`, `-ldl`, `-lrt` and the like resolved to Liella's archives, and
  for C++ libstdc++ and the unwinder linked statically.
* **Check**: after each Liella build, `check-elf.sh` reads every ELF program and shared object in the build
  tree. A program must have Liella's loader as `PT_INTERP` and name Liella's libc; nothing may name the
  system's `libc.so.6`, `libm.so.6`, `libpthread.so.0` or loader by soname; a static program is rejected.
  A build that falls back to the system's C library therefore fails the run instead of passing on the wrong
  library. `smoke.sh selftest` proves each rule with a negative control, and the plants in
  `handoff/AQOURS-SMOKE-plants.txt` break each rule and the wrapper.
* **Build**: the upstream build system in a fresh copy of the unpacked source, with options that turn off
  optional libraries the host has no headers for and NLS. Autotools files are never regenerated
  (`AUTOCONF=true` and the like in `MAKEFLAGS`): the host has no autotools. Per-program options and the
  reason for each are in `programs.sh`.

## Limits of this harness today

* **Host mode, not a chroot.** The brief asked for an unprivileged Debian trixie chroot (`mmdebstrap
  --mode=unshare`), which gives the full Debian build environment (`apt-get build-dep`, `dpkg-buildpackage`).
  The build host this ran on has neither `mmdebstrap` nor `newuidmap` (package `uidmap`), so the harness
  builds on the host instead: Debian's sources with the host's toolchain (gcc 13, glibc 2.39, the reference
  ABI). Programs whose builds need tools or libraries the host lacks (libtool, autotools, cmake, Tcl headers,
  zlib headers) are built in a reduced form or not at all; RESULTS.md says which.
* **Libraries are linked statically** (`--disable-shared`): a program that needs another shared library by
  soname cannot start on Liella's loader until the start-time library search lands; `check-elf.sh` lists such
  objects as `needs-search` rather than failing them.
* **No `dlsym`/`dlclose` in Liella's libc.so.6 yet** (DL-16b, DL-17): programs whose builds reference them are
  built without their plugin loaders (sqlite3 without extension loading, Python with built-in modules, perl
  as miniperl).
