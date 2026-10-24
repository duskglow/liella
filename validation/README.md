# Validation against GNU coreutils, bash, sed and six more GNU packages

Plan §6, "Validation and optimization (before phase 14)", TEST thread: milestone 2 (first
runs), milestone 3 (fixes, second runs, kernel 5.10) and milestone 4 (as-is runs on the
overridable `libc.a`), and milestone 6 (grep, gawk, diffutils, tar, gzip, make). **This is a
static validation**: the programs are linked statically against Liella's `libc.a`. Dynamic
linking waits for phase 14 (D-22.10), so nothing here exercises `libc.so.6`, `ld.so`, `dlopen`
or `LD_PRELOAD`.


## Lab-only scripts and data

The `validation/*.sh` scripts (and `tools/probe/remote.sh`, `tools/probe/admin_stress.sh`, `tools/probe/probe`) drive the
project's own test hosts and are not needed to build or test Liella; their host names and addresses are the project's lab
and are left as they are. The host-keyed `.tsv` files (`validation/wl_thresholds.tsv`, `validation/wl_known_diffs.tsv`,
`bench/host_limits.tsv`, `bench/host_cycle_limits.tsv`, `bench/budgets.tsv`, `bench/perf-ledger.tsv`,
`bench/cycle_cpus.tsv`, `bench/parity-measure-meta.tsv`, `docs/libm-accuracy.tsv`) and `validation/results/` are measurements
from the project's lab hosts; the host names in them label the machine that produced each number.

## Method

* **Machine.** AMD EPYC 7551 (Zen 1), Ubuntu 24.04.5, Linux
  6.8.0-146, glibc 2.39-0ubuntu8.9 (the reference), gcc 13.3.0. The machine is shared (it is
  not the timing machine): every timing below carries the load average before and after.
* **Sources.** The current GNU releases from ftp.gnu.org, each signature checked with `gpgv`
  against `gnu-keyring.gpg` (from the same site):

  | Package | Version | SHA-256 | Signed by |
  |---|---|---|---|
  | coreutils | 9.12 | `a480198559733e9b3da999e90543ac6f888a2caa544d8d664c5a1f17e528e210` | Pádraig Brady |
  | sed | 4.10 | `b8e72182b2ec96a3574e2998c47b7aaa64cc20ce000d8e9ac313cc07cecf28c7` | Jim Meyering |
  | bash | 5.3, official patches 001–020 (each signed) | `0d5cd86965f869a26cf64f4b71be7b96f90a3ba8b3d74e27e8e9d9d5550f31ba` | Chet Ramey |
  | findutils (for `find` only, step 6) | 4.11.0 | `bfd19cb06cc71f3352d567e90284d8cdac02ac89774bbeadf0b533b0c11432fd` | James Youngman |
  | grep (milestone 6) | 3.12 | `2649b27c0e90e632eadcd757be06c6e9a4f48d941de51e7c0f83ff76408a07b9` | Jim Meyering |
  | gawk (milestone 6) | 5.4.1 | `07f6f7342b7febe4313fc2c2542ad93d64fe20ad8717200109f105a826f5fd37` | Arnold Robbins |
  | diffutils (milestone 6) | 3.12 | `7c8b7f9fc8609141fdea9cece85249d308624391ff61dedaf528fcb337727dfd` | Jim Meyering |
  | tar (milestone 6) | 1.35 | `4d62ff37342ec7aed748535323930c7cf94acf71c3591882b26a7ea50f3edc16` | Sergey Poznyakoff |
  | gzip (milestone 6) | 1.15 | `9aa0cc780dec156b8282844833b342ab7cb08c25d2cd9a1869cdd0df31deff48` | Jim Meyering |
  | make (milestone 6) | 4.4.1 | `dd16fb1d67bfab79a72f5e8390735c49e3e8e70b4945a15ab1f81ddb78658fb3` | Paul D. Smith |

  The milestone-6 tarballs (owner approval of 2026-10-05: exactly these six and their
  signatures) are fetched and checked by `validation/fetch.sh`; every size equalled the approved
  one and every signature was good.

* **Liella.** First runs: the products of commit 5dd6c9d (`cargo xtask so`), copied to
  `~/validation/products` so later builds do not change them under a run. Second runs
  (2026-10-04): the products of c0a8753, in `~/validation/products-m3`. Fourth runs
  (2026-10-05): the products of main 246ae7f (`libc.a` overridable, D-X.14), in
  `~/validation/products-m4`. Sixth runs (milestone 6, 2026-10-05): the products of
  test/validation 3535ebf (contains main 75c0f5c; no library change since), in
  `~/validation/products-m6`.
* **Builds** (`validation/suite.sh PACKAGE LIBC`): configure, `make -k`, `make -k check`, each
  package in a fresh tree, the three suites in parallel (`-j5` or `-j6` each). Configure options:
  coreutils `--without-selinux`, bash `--without-bash-malloc` (the C library's allocator),
  findutils `--without-selinux`; no development libraries for ACL, SELinux, capabilities or
  curses are installed, so both builds use the same feature set.
  * `glibc`: the host compiler, dynamically linked (as the distribution builds).
  * `liella`: `CC=scripts/liella-cc`, as is.
  * `liella-w` (first runs): the same with the labelled workarounds below, so the suites can
    run at all.
  * `liella-t1` (second runs): only the workarounds the coordinator kept, T1's
    `--allow-multiple-definition` and the libm stand-in (`modf`, T3).
  * `liella-t1r` (second runs): `liella-t1` with gnulib's regex for coreutils and findutils, to
    see their results past T5 (the regex fix is the optimization thread's).
  * `liella-m` (fourth runs, the headline): as is; only findutils gets the `modf` stand-in
    (T3, provisional).
  * `liella-mr` (fourth runs, diagnosis only): `liella-m` with gnulib's regex for coreutils and
    findutils (milestone 6: grep).
  * Milestone 6: `liella-m` as is for all six packages; gawk links the labelled maths stand-ins
    of `validation/shim/libm.c` (T3, provisional); make cannot link as is (T27) and is compared
    as the labelled pair `glibc-noload`/`liella-noload` (`--disable-load` under both).
    `glibc-sysregex` was built for grep, diffutils and tar, but their configure already accepts
    glibc's `re_compile_pattern` (`gl_cv_func_re_compile_pattern_working=yes` under both
    libraries), so their stock builds use the C library's regex and the variant equals the
    stock build (tar: 207 pass, 26 skip in both). gawk bundles its own regex; gzip and make use
    none.
* **The compiler driver** `scripts/liella-cc` compiles with the system's headers and links as
  the conformance suite does: `-static -nostdlib`, `liella-crt1.o`, the objects, `libc.a`,
  `-lgcc`. It drops `-lpthread -lrt -ldl -lutil -lanl -lresolv -lc` (their functions are in
  `libc.a`) and `-lm -lmvec` (Track M's libm is not a product yet). `-shared` objects (tests'
  `LD_PRELOAD` libraries) are left to the system compiler: a static program never loads them.
* **Workarounds** (each is a finding, see the triage table; the second runs use only the first
  and the `modf` stand-in):
  * `-Wl,--allow-multiple-definition` (T1): programs that define a function `libc.a` also
    defines link, the program's definition winning, as with glibc's one-object-per-function
    `libc.a`.
  * `validation/shim/missing.c`, not part of Liella (T2, T3): in the first runs thin stand-ins
    for `uname`, `gethostname`, `getdomainname`, their `_chk` forms, `sysinfo`, `getloadavg`
    (over Liella's `syscall`) and `modf`, written from the man pages; since milestone 3 only
    `modf` (Liella implements the rest, T2).
  * coreutils and findutils with `--with-included-regex` (gnulib's regex instead of Liella's,
    T5): Liella's regex breaks the coreutils test harness itself (`expr` computes test
    directory names), which hid every other result. sed keeps Liella's regex.
* **Clean room.** No source under the packages' `lib/`, `gnulib/` or `gnulib-tests/` was
  opened. Results come from the harness's summary lines on standard output, `config.log`
  (including the configure test programs it logs, re-run by `validation/conftest.sh`), the
  packages' own `tests/`/`testsuite/` scripts and logs, and black-box probes. Since the
  coordinator's ruling R4 (milestone 3) the gnulib-tests *output* files (`*.log`, `*.trs`) are
  read too: a quoted assertion only names the failing check, and the behaviour is then settled
  with an own probe (T18). To find a failing call's arguments the test binaries were also
  observed from outside: `strace` (system calls and the `execve` of the test program with its
  locale variables), `nm -u` on a test object (which C library functions it calls), and one
  `gdb` breakpoint on Liella's own `strncpy` printing the three argument registers (no
  backtrace, no source).
* **Hung tests.** A watchdog in `suite.sh` stops a package program running longer than 300 s
  (`*.hung`).
* **Results** are in `validation/results/` (summaries with load and timing, configure cache and
  substitution values, per-test results, the bash comparison, the traversal table, the timings
  and profile tops); the full logs stay in `~/validation/results/`.

## Results

Counts are automake's (coreutils, sed, findutils: the package's own tests, then its gnulib
tests). bash's suite has no counter: `validation/bash_results.py` compares each `run-*` test's
output under both libraries. Check times are wall-clock seconds of `make check` with the load
average (1 min) at its start and end; the machine was shared (the three or six suites of a run ran in
parallel, `-j4` each in the second runs), so they are indicative only.

Seventh runs (V-5 `PRE14-VALIDATION-RERUN`, 2026-10-23): **all ten packages as is on the merged
head, no stand-in**. Products built with the shipped profile (`LIELLA_FULL_PROFILE=1 cargo xtask
so`) from task/pre14-validation-rerun `ed6d4449` (origin/next `23fa3087` plus this run's
`scripts/liella-cc` change: `-lm` now links Liella's `libm.a`, a product since D-10.7; no shim
object on any link); `built-from` in each `results/v5/liella-*.summary`. Zen 1 host job
`pre14-validation-rerun-suites`, four suites at a time in one 7-core slot, load 12 to 93, so the
times are not comparable. glibc baselines: the earlier runs' (same host, same glibc
2.39-0ubuntu8.9), except bash, re-run in the same job environment (`pre14-validation-rerun-suites2`).
Counts are unique test lines (`sort -u`: a failing automake run prints some lines twice).

| Package | glibc (dynamic) | Liella as is (`liella`, V-5) |
|---|---|---|
| coreutils tests (761) | 606 pass, 155 skip | 600 pass, 159 skip, **2 fail**: `misc/stdbuf`, `misc/responsive` (T17, phase 14). `ptx/word-regex-loop` (T26) and `split/suffix-length` (T14) pass |
| coreutils gnulib tests (601) | 495 pass, 106 skip | 495 pass, 106 skip, 0 fail |
| sed (75 / 287) | 64 pass, 11 skip / 222 pass, 65 skip | **equal** |
| findutils (32 / 423) | 28 pass, 4 skip / 346 pass, 77 skip | **equal**, now without the `modf` stand-in (no longer provisional; T3, T23) |
| grep tests (128) | 92 pass, 34 skip, 2 xfail | 91 pass, 34 skip, 2 xpass (T31), **1 fail**: `stack-overflow` (T30, the kept depth bound). `ere` (T28) and `case-fold-titlecase` (T29) pass |
| grep gnulib tests (315) | 257 pass, 58 skip | equal |
| gawk | 588 pass, 6 fail (T32) | **does not link as is (T34, new)**: `sin`, `cos`, `exp`, `log`, `pow`, `atan2` undefined, configure added no `-lm` |
| diffutils (34 / 343) | 31 pass, 2 skip, 1 xfail / 275 pass, 68 skip | **equal** |
| tar (233) | 207 pass, 26 skip | **equal** |
| gzip (34) | 34 pass | **equal** |
| make (136) | 134 pass, 2 skip | does not link: `dlopen`, `dlsym`, `dlerror`, `dlclose` (T27, phase 14) |
| make `-noload` (136) | 132 pass, 4 skip | **equal** |
| bash `run-*` (86) | see below | see below |

No package chose a gnulib replacement under Liella that it did not choose under glibc (the
`replace` diffs are empty for all ten). bash, against glibc re-run in the same job environment (`results/v5/bash-compare.txt`): 81 of 86 `run-*` tests give identical output; `run-builtins` and `run-glob-bracket` differ (loadable builtins, T4); `run-execscript`, `run-glob-test` and `run-history` fail identically under both (T21); `run-printf` is now identical (T15 fixed). gawk with `LIBS=-lm` passed to configure (labelled diagnosis of T34, `pre14-validation-rerun-suites2`): 586 pass and the same 6 failures as glibc (T32), on Liella's own `libm.a`, so gawk's results are no longer provisional; `apiterm` and `filefuncs` not run (T33).
Summary: `results/v5/summary.txt`.

Sixth runs (milestone 6, 2026-10-05): **six more packages, as is**, Liella built from
test/validation 3535ebf (contains main 75c0f5c; `built-from` line in each
`results/m6/*.summary`); Zen 1; suites two at a time, load 1.6 to 4.8. The per-test lines of tar
(autotest), make (its Perl driver) and gawk (its own Makefile) are converted to automake's form
by `validation/results_other.py`; `validation/summarize.sh` prints the comparison
(`results/m6/m6-summary.txt`).

| Package | glibc (dynamic) | Liella as is (`liella-m`) | Check time glibc / Liella (s) |
|---|---|---|---|
| grep tests (128) | 92 pass, 34 skip, 2 xfail | 89 pass, 34 skip, **3 fail** (`ere`: T28; `case-fold-titlecase`: T29; `stack-overflow`: T30), **2 xpass** (`glibc-infloop`, `triple-backref`: T31) | 34.7 / 31.1 |
| grep gnulib tests (315) | 257 pass, 58 skip | 257 pass, 58 skip, 0 fail | — |
| grep with gnulib's regex (`liella-mr`, diagnosis) | — | 91 pass, 34 skip, 3 xfail, **0 fail**: all three failures are Liella's regex | — / 40.5 |
| gawk tests (594) | 588 pass, **6 fail** | 586 pass, **6 fail**: the same six, byte-identical diffs (T32) **(P)**; `apiterm` and `filefuncs` not run, no extensions without `dlopen` (T33) | 5.8 / 5.6 |
| diffutils tests (34) | 31 pass, 2 skip, 1 xfail | **31 pass, 2 skip, 1 xfail** | 25.3 / 20.6 |
| diffutils gnulib tests (343) | 275 pass, 68 skip | 275 pass, 68 skip, 0 fail | — |
| tar tests (233) | 207 pass, 26 skip | **207 pass, 26 skip, 0 fail** | 259.7 / 220.2 |
| gzip tests (34) | 34 pass | **34 pass** | 12.1 / 12.5 |
| make tests (136) | 134 pass, 2 skip | **does not link**: `dlopen`, `dlsym`, `dlerror`, `dlclose` (T27) | 58.1 / — |
| make, `--disable-load` under both (`-noload`) | 132 pass, 4 skip | **132 pass, 4 skip, 0 fail** | 57.4 / 58.1 |

diffutils, tar and gzip equal glibc as is, test for test; make equals glibc once both are built
without its `load` directive. Every failure is Liella's regex (grep: T28, T29, T30, for the
optimization thread) or the same under glibc (gawk, T32). No package chose a gnulib replacement
under Liella that it did not choose under glibc; under glibc, tar and gzip replace `fopen`,
`fclose`, `fseek` and `fseeko` (gzip also `fflush`), and grep and diffutils the rwlock (gnulib's
tests of glibc's behaviour, as in the first four packages). The outputs of the 13 profile
workloads are identical under both libraries; timings and profiles: `validation/profiles.md`,
"Milestone 6". The corpus harvest (item C) was not extended to these packages.

Fifth runs (milestone 5, item B, 2026-10-05): **as is after the correctness fixes**, built from
branch d2d4630 = main 75c0f5c (contains 12fdf19: T5, T6, T7, T8, T15) merged into
test/validation (`built-from` line in each `results/m5/keke-liella-m-*.summary`; Zen 1; check times at
load 30 to 40, so not comparable):

| Suite | glibc (dynamic) | Liella as is (`liella-m`) |
|---|---|---|
| coreutils tests (761) | 606 pass, 155 skip, 0 fail | 587 pass, 170 skip, **4 fail**: `misc/stdbuf`, `misc/responsive` (T17), `split/suffix-length` (T14), `ptx/word-regex-loop` (**T26**) |
| coreutils gnulib tests (603) | 497 pass, 106 skip | 497 pass, 106 skip, 0 fail |
| sed tests (75) | 64 pass, 11 skip | **64 pass, 11 skip, 0 fail** (`dc.sed`, `binary2.sed`, `binary3.sed` finish; check 29.5 s against glibc's 25.0 s) |
| sed gnulib tests (287) | 222 pass, 65 skip | 222 pass, 65 skip, 0 fail |
| findutils find tests (29) | 25 pass, 4 skip | 25 pass, 4 skip, 0 fail **(P)** |
| findutils gnulib tests (424) | 347 pass, 77 skip | 347 pass, 77 skip, 0 fail **(P: `test-modf`)** |

T5 and T6 are fixed as far as these suites see (the coreutils harness, `test-regex` in all three
packages, sed's runaways); T7 and T8 too: configure no longer replaces `memchr`, `strstr`,
`strcasestr` or the printf family (`results/m5/keke-liella-m-*.replace`). sed and findutils now equal
glibc as is. bash was not re-run (no regex or printf change reaches its six differences).

Fourth runs (milestone 4, main 246ae7f, 2026-10-05), **as is** (no
`--allow-multiple-definition`, no stand-in except `modf` for findutils):

| Suite | glibc (dynamic) | Liella as is (`liella-m`) | diagnosis only: with gnulib's regex (`liella-mr`) |
|---|---|---|---|
| coreutils tests (761) | 606 pass, 155 skip, 0 fail | 62 pass, 2 skip, **697 fail** (the harness's `expr`, T5) | 587 pass, 171 skip, **3 fail** (`misc/stdbuf`, `misc/responsive`: T17; `split/suffix-length`: T14) |
| coreutils gnulib tests (603) | 497 pass, 106 skip | 496 pass, 106 skip, **1 fail** (`test-regex`, T5/T6) | 497 pass, 106 skip, 0 fail |
| coreutils check time | 100.8 s (load 3.6 → 5.2) | 125.7 s (load 7.7 → 11.4) | 193.2 s (load 7.3 → 11.2) |
| sed tests (75) | 64 pass, 11 skip | 62 pass, 11 skip, **2 fail** (both hang, T6) | — |
| sed gnulib tests (287) | 222 pass, 65 skip | 221 pass, 65 skip, **1 fail** (`test-regex`) | — |
| sed check time | 25.0 s (load 2.8 → 3.0) | 659.8 s (load 4.6 → 4.1; 4 programs stopped by the watchdog, T6) | — |
| bash `run-*` tests (86) | 76 without a diff | 75 without a diff; **6 differ from glibc**: 2 need loadable builtins (T4), 1 `printf` (T15), 3 only in what differs per run or locale (T21) | — |
| bash check time | 121.6 s (load 2.9 → 5.0) | 130.7 s (load 4.6 → 7.7) | — |
| findutils find tests (29) | 25 pass, 4 skip | 25 pass, 4 skip, 0 fail **(P)** | 25 pass, 4 skip, 0 fail **(P)** |
| findutils gnulib tests (424) | 347 pass, 77 skip | 346 pass, 77 skip, **1 fail** (`test-regex`) **(P: `test-modf`)** | 347 pass, 77 skip, 0 fail **(P: `test-modf`)** |
| findutils check time | — | 54.9 s (load 5.3 → 7.7) | 70.6 s (load 3.1 → 7.3) |

**T1 is gone**: all four packages configure, build and link as is (no "multiple definition" in
any build log); a program's own `error`, `obstack_alloc_failed_handler`, `group_member` and
`glob_pattern_p` win over `libc.a`'s. The as-is results equal the second runs' T1-workaround
results test for test, except `install/stdin.sh` (skipped in the `liella-mr` run: "/dev/stdin
is not readable" in that run's environment, T21) and bash's `run-read` (identical output
again; its difference in the second runs came from the inherited standard input, T21).
configure chose the same gnulib replacements as in the second runs. What remains as is:
T5/T6 (regex: the coreutils harness, `test-regex` in all three packages, sed's two hung
tests), T14 and T17 (seen with gnulib's regex), T4 and T15 (bash), and T3 (find needs the
`modf` stand-in, provisional; see T23). Logs: `results/liella-m-*`, `results/liella-mr-*`,
`results/bash-compare-m4.txt`.

Second runs (milestone 3, products of c0a8753, 2026-10-04):

| Suite | glibc (dynamic) | Liella as is | Liella, T1 + libm only (`liella-t1`) | the same with gnulib's regex (`liella-t1r`) |
|---|---|---|---|---|
| coreutils tests (761) | 606 pass, 155 skip, 0 fail | does not build: 103 programs fail to link (T1) | 62 pass, 2 skip, **697 fail** (the harness's `expr`, T5) | 588 pass, 170 skip, **3 fail** (`misc/stdbuf`, `misc/responsive`: T17; `split/suffix-length`: T14) |
| coreutils gnulib tests (603) | 497 pass, 106 skip | (not reached) | 496 pass, 106 skip, **1 fail** (`test-regex`, T5/T6) | 497 pass, 106 skip, 0 fail |
| coreutils check time | 100.8 s (load 3.6 → 5.2) | — | 141.3 s (load 10.0 → 9.1; most tests fail at once) | 225.5 s (load 11.0 → 7.9) |
| sed tests (75) | 64 pass, 11 skip | `sed` does not link (`obstack_alloc_failed_handler`, T1) | 62 pass, 11 skip, **2 fail** (both hang, T6) | (sed keeps Liella's regex) |
| sed gnulib tests (287) | 222 pass, 65 skip | 221 pass, 65 skip, **1 fail** (`test-regex`) | 221 pass, 65 skip, **1 fail** (`test-regex`, T5/T6) | — |
| sed check time | 25.0 s (load 2.8 → 3.0) | — | 659 s (load 4.6 → 3.2; 4 programs stopped by the watchdog, T6) | — |
| bash `run-*` tests (86) | 76 without a diff | does not link: `group_member`, `glob_pattern_p` (T1) | 75 without a diff; **7 differ from glibc**: 2 need loadable builtins (T4), 1 `printf` (T15), 4 only in what differs per run, locale or standard input (T21) | — |
| bash check time | 121.6 s (load 2.9 → 5.0) | — | 130.5 s (load 5.7 → 9.5) | — |
| findutils find tests (29) | 25 pass, 4 skip | `find` does not link (`modf`, T3) | 25 pass, 4 skip, 0 fail **(P)** | 25 pass, 4 skip, 0 fail **(P)** |
| findutils gnulib tests (424) | 347 pass, 77 skip | (not reached) | 346 pass, 77 skip, **1 fail** (`test-regex`) **(P: `test-modf`)** | 347 pass, 77 skip, 0 fail **(P: `test-modf`)** |
| findutils check time | — | — | 55.5 s (load 6.1 → 9.6) | 83.4 s (load 9.3 → 12.1) |

**(P)** marks a provisional result (owner ruling, 2026-10-04): it rests on the `modf` stand-in
of `validation/shim/missing.c`, not on Liella, and is re-run when Track M's libm is a product
(see "Provisional results" below).

Before and after: the first runs needed the three workarounds (T1, the §24 and `modf`
stand-ins, gnulib's regex) and then failed 4 coreutils tests and 6 + 6 + 3 gnulib tests. With
the second runs' fixes (T2, T9–T13, T18, T19) and the same regex workaround (`liella-t1r`),
coreutils fails 3 tests, all of them environment or allocator findings (T14, T17), and every
gnulib test passes that passes under glibc; findutils equals glibc. Without gnulib's regex the
remaining failures are T5/T6 alone (`test-regex`, the coreutils harness, sed's two hung tests).
As is, every package is blocked only by T1 (and `find` by T3). The bash differences are
`results/bash-compare-m3.txt`; `run-read`, new in this list, gives byte-identical output under
both libraries when both read the same standard input (its `read -t` cases read the inherited
input).

First runs (milestone 2, products of 5dd6c9d, 2026-10-03):

| Suite | glibc (dynamic) | Liella as is | Liella with workarounds |
|---|---|---|---|
| coreutils tests (761) | 606 pass, 155 skip, 0 fail | does not build: 101 programs fail to link (T1), 3 need `uname` (T2) | 586 pass, 171 skip, **4 fail** |
| coreutils gnulib tests (603) | 497 pass, 106 skip | (not reached) | 491 pass, 106 skip, **6 fail** |
| coreutils check time | 100.8 s (load 3.6 → 5.2) | — | 192.0 s (load 22.0 → 9.5) |
| sed tests (75) | 64 pass, 11 skip | `sed` does not link (T1) | 62 pass, 11 skip, **2 fail** (both hang, T6) |
| sed gnulib tests (287) | 222 pass, 65 skip | 216 pass, 65 skip, 6 fail | 216 pass, 65 skip, **6 fail** |
| sed check time | 25.0 s (load 2.8 → 3.0) | — | 658 s (load 5.3 → 4.1; 4 programs stopped by the watchdog) |
| bash `run-*` tests (86) | 76 without a diff | does not compile: no `gethostname` (T2) | 75 without a diff; **6 differ from glibc**: 3 only in text that differs per run or locale, 2 need loadable builtins, 1 `printf` (T15) |
| bash check time | 121.6 s (load 2.9 → 5.0) | — | 127.4 s (load 5.2 → 7.5) |
| findutils find tests (29) | 25 pass, 4 skip | `find` does not link (`modf`, T3) | 25 pass, 4 skip, 0 fail **(P)** |
| findutils gnulib tests (424) | 347 pass, 77 skip | (not reached) | 344 pass, 77 skip, **3 fail** **(P: `test-modf`)** |

The 16 coreutils tests skipped only under Liella need `LD_PRELOAD`, a dynamically linked
helper, or memory limits a static program does not meet (T17, T14).

### gnulib replacements chosen by configure

Values from `config.log` (`validation/results/*.cache`, `*.replace`). Liella-specific
replacements point at Liella defects; glibc-specific ones are gnulib working around glibc.
The second runs (`liella-t1-*`) no longer replace `strndup`, `futimens`, `getgroups`, and find
`rpmatch`, `gethostname`, `uname`, `sysinfo`, `getloadavg` and `sethostname`.

| Function | glibc | Liella | Why (configure test re-run with `validation/conftest.sh`) |
|---|---|---|---|
| `memchr` (coreutils, sed, findutils) | kept | **replaced** | the configure test crashes: `memchr` reads past the match into an inaccessible page (T7) |
| `strstr`, `strcasestr` | kept | replaced | follows from `memchr` (their linear-time tests are not run once `memchr` is replaced) |
| `printf`, `snprintf`, `sprintf`, `vasprintf`, `vfprintf` (coreutils) | kept | **replaced** | `%.2ls` converts wide characters past the precision and fails (T8) |
| `strndup` (coreutils, findutils) | kept | kept (first runs: replaced) | `strndup(NULL, 0)` returned NULL; glibc returns `""` (T11, fixed) |
| `futimens` (coreutils) | kept | kept (first runs: replaced) | `futimens(AT_FDCWD, NULL)` did not fail with `EBADF` (T9, fixed) |
| `getgroups` (coreutils, findutils) | kept | kept (first runs: replaced) | `getgroups(-1, list)` aborted in the fortified form (T10, fixed) |
| `frexp`, `frexpl`, `ldexp`, `ldexpl` | kept | replaced | no libm (T3) |
| `dlopen`, `dl_iterate_phdr` | present | absent | the loader functions wait for phase 14 |
| `rpmatch`, `gethostname`, `uname`, `sysinfo`, `getloadavg`, `sethostname` | present | present (first runs: absent) | T2, fixed |
| `fclose`, `fflush`, `fopen`, `fseek`, `fseeko` | **replaced** | kept | glibc's `fflush`/`fclose` on an input stream (gnulib's POSIX test); Liella passes |
| `pthread_rwlock_init`, `pthread_rwlockattr_init` | **replaced** | kept | glibc's rwlock lets readers starve a waiting writer; Liella passes gnulib's waitqueue test |
| `re_compile_pattern` (sed, findutils) | **replaced** (gnulib regex) | kept | glibc fails gnulib's regex configure test, Liella passes it (yet see T5, T6) |
| `stime` | absent | present | Liella's `libc.a` offers a function glibc 2.39 keeps only for old binaries (T20) |

### Directory removed during a traversal (step 6; D-13.2, D-13.13)

`validation/traversal.sh` removes `top/b` (three files and a subdirectory) at an exact moment,
delaying one `getdents64` call with strace: before the first read of `top/b`, or after its
entries were read and before the end of the directory. Liella's `readdir` then reports
`ENOENT` (owner ruling, D-13.2); glibc ends the listing silently.
(`validation/results/traversal/traversal.tsv`.)

| Program | glibc | Liella |
|---|---|---|
| `ls -R` | exit 0, same listing | identical |
| `find` | exit 0 (read 1); read 2: `‘top/b/sub’: No such file or directory`, exit 1 | `‘top/b’: No such file or directory`, exit 1 (both moments; read 2 also reports `top/b/sub`) **(P)** |
| `du` | lists `top/b` with what it counted, exit 0 | **omits `top/b` silently**, exit 0 |
| `rm -r` | `cannot remove` for the vanished entries, exit 1 | the same, plus `traversal failed: top/b: No such file or directory` |

All four behave sensibly: none crashes, loops or misreports a directory that is still there.
`find` and `rm` say what happened; `du` drops the vanished directory instead of counting what
it saw. These follow from the owner's ruling D-13.2 and are reported, not changed.

**Phase 15 re-run (LG-0e, 2026-10-23):** both ledger items above were repeated on current
Liella, `validation/results/lg0e/README.md`. No gnulib replacement is now selected under
Liella only; glibc alone gets gnulib's `fflush`/`fclose`/`fopen`/`fseek`/`fseeko`, rwlock and
regex (differences rows added). The traversal table is unchanged, and `find` now links as is.

### Timing and profiles

`validation/profiles.md` (13 workloads; the optimization thread's input).

## Kernel 5.10 (an earlier machine)

The coordinator ran the whole test suite on an earlier machine (Ubuntu 24.04 userland, Linux 5.10.0,
main 9396053): 20 tests failed. Each was classified with glibc 2.39 *on an earlier machine* as the
reference (black-box probes of the same calls): (a) the test assumed a newer kernel, and glibc
gives the same answer as Liella, so the test now accepts the older kernel's documented answer
before the release that added the feature (`crates/core/tests/support/kernel.rs`, ⚑ KERN) and
still checks everything on newer kernels; or (b) Liella answered differently from glibc, a
Liella bug, fixed.

| Test | Feature (Linux release) | 5.10 answer (glibc = Liella unless said) | Class | Change |
|---|---|---|---|---|
| `aio_products::aio_equals_the_reference` | io_uring on 5.10 | io_uring refuses `RWF_NOWAIT` on pipes (`EOPNOTSUPP`), never completes socket reads, and gives `EFAULT` for an `INT_MAX`+1 count into a 4096-byte buffer; glibc (threads with `pread`) gives the system call's answers | b | D-11.11: with the ring, reads and writes on a descriptor that is not a regular file or block device run on worker threads; a ring `EFAULT` is retried on a worker (3bc4350) |
| the same | the probe itself | 5.10 refuses a count whose end passes the top of user space before shortening it; the probe's stack buffer was sometimes within 2 GiB of the top (glibc's `pread` too, 2 of 30 placements) | a (test) | static buffer in `aio_diff.c` |
| `s11_lowio_clauses::s11_7_c_4_3`, `s11_lowio_symbols::sym_epoll_pwait2`, `appendix_a::k_epoll_pwait2` | `epoll_pwait2` (5.11) | `ENOSYS` | a | kernel-aware |
| `s11_lowio_symbols::sym_close_range`, `appendix_a::k_close_range` | `CLOSE_RANGE_CLOEXEC` (5.11) | `EINVAL` | a | kernel-aware |
| `s11_lowio_clauses::s11_9_c_0`, `s11_9_g_0`, `s11_lowio_symbols::sym_sendfile`, `sym_sendfile64` | `sendfile` from a regular file into a pipe (later kernels) | `EINVAL` | a | kernel-aware (probed with one byte into an own pipe) |
| `s15_sockets_clauses::s15_1_c_9` | `SIOCATMARK` on a Unix socket (5.15) | `ENOTTY` | a | kernel-aware |
| `s00_conventions_clauses::s0_6_c0_8_2` | `AT_MINSIGSTKSZ` on x86 (5.14) | absent from the auxiliary vector | a | kernel-aware |
| `s24_system_clauses::s24_4_c_1_5`, `s24_system_symbols::sym_sysconf`, `sym___sysconf` | `_SC_MINSIGSTKSZ` without `AT_MINSIGSTKSZ` | glibc 1348, Liella 1332 (padding 48 instead of 64) | b | fixed in 2aa507f (448 + 64 + XSAVE size + 4) |
| `appendix_a::k_mount_setattr`, `s13_filesystem_symbols::sym_mount_setattr` | `mount_setattr` (5.12) | `ENOSYS` | a | kernel-aware |
| `appendix_a::k_lock_pi2` | `FUTEX_LOCK_PI2` (5.14) | `ENOSYS` | a | kernel-aware |
| `appendix_a::k_process_mrelease` | `process_mrelease` (5.15) | `ENOSYS` | a | kernel-aware |
| `appendix_a::k_fanotify_mark` | unprivileged fanotify (5.13) | `EPERM` | a | kernel-aware |

Re-runs with these changes (2026-10-04, HEAD c0a8753; an earlier machine is shared with Track M, whose
jobs raised its load to between 7 and 29 during the runs):

* an earlier machine (5.10), `cargo xtask test --no-fail-fast`: the plain pass passes. In the first
  attempt `aio_products` failed once on the *reference*: glibc's `aio_fsync` on a regular file
  was not complete after 10 s while the disk was busy (Liella's completed); alone it passed 3 of
  3. The export pass had 3 failures, all races under load, none specific to 5.10:
  `aio_products` (the reference glibc completed a sync before its write 49 times of 50),
  `process_products` (the order of a child's report line and the parent's `pidfd_spawn` line,
  which swaps under glibc too; once a `SigBlk` snapshot inside the spawn window), and
  `s23_time_clauses::s23_1_c_1_7` (a 30 ms spin got less than 20 ms of CPU). Re-run in the
  export configuration: `aio_products` and `s23_time_clauses` 3 of 3; `process_products` 2 of
  3, then 0 of 5 at load 29, the same line-order race in both directions.
* Zen 1 (6.8): the full gate (milestone-3 report).

The four suites on an earlier machine (milestone 5, 2026-10-05; Liella built from 17c73bb = main 246ae7f,
before the correctness fixes; glibc baseline built there too; same gcc 13.3.0 and glibc
2.39-0ubuntu8.9 as Zen 1, sources copied from Zen 1 and their signatures checked again): Liella's
per-test results equal Zen 1's fourth runs exactly (coreutils 2,063, sed 363, findutils 456 result
lines; bash's comparison identical; `results/m5/margarete/`). glibc's differ from Zen 1's only in coreutils'
`install/stdin.sh`, which passes on one machine and was skipped on the other ("/dev/stdin is not
readable", T21). No kernel-5.10 finding. (An earlier machine was loaded by Track M; sed's runaways took
the 600 s watchdog there.)

## Corpus harvest (milestone 5, item C)

The regex and printf-family calls the four suites make against glibc, recorded and replayed
against Liella. Tools: `validation/harvest/harvest.c` (an `LD_PRELOAD` library for the dynamic
glibc builds: it interposes `regcomp`, `regexec`, `re_compile_pattern`, `re_search`,
`re_match` and the printf family including the `__*_chk` forms, logs inputs and glibc's results
and passes the call on; only processes whose executable lies in the package's build tree log),
`validation/harvest/harvest_reduce.py` (deduplicated, self-contained corpus lines labelled with
the builds that made them), and two replayers that are differential tests
(`crates/core/tests/oracle/regex_corpus.c` with `regex_corpus_products.rs`,
`printf_corpus.c` with `printf_corpus_products.rs`).

Builds harvested (Zen 1, glibc 2.39-0ubuntu8.9, harvest tools of branch d2d4630; `suite.sh` with
`HARVEST_SO`, results `results/m5/glibc-harvest-*`, `results/m5/glibc-sysregex-harvest-*`):

* the stock glibc builds of coreutils, sed, bash, findutils (the validation baseline). Their
  configure rejects glibc's `re_compile_pattern` (`gl_cv_func_re_compile_pattern_working=no`)
  and links gnulib's regex into coreutils, sed and findutils, which the shim cannot see: these
  three made no regex call through glibc (bash's 93 `regcomp` calls did);
* `glibc-sysregex`: coreutils, sed and findutils configured with `--without-included-regex`, so
  their regex calls go through glibc. Under glibc's regex these builds fail gnulib's
  `test-regex` (all three) and one sed test that pass with gnulib's regex; that is glibc's.
  The stock builds stay the baseline; these exist for the harvest.

The logs (50,331 processes, 157 MB) stay in `~/validation/harvest/`; the full corpora
(19,334 regex lines over 437 patterns at most 200 executions each, 9,660 printf lines over
1,590 formats at most 50 calls each) in `~/validation/corpus-m5/`. The repository keeps
`validation/corpus/regex.corpus` (5,638 lines, at most 25 executions per pattern) and
`validation/corpus/printf.corpus` (all 9,660). 40 formats with C23's `%wN`/`%wfN` length
modifiers are logged but not replayed (the shim's parser does not take them).

Replay against Liella built from branch d2d4630 (main 75c0f5c, with the correctness fixes):

| Corpus | Lines | Liella = glibc | Disagreements (`validation/corpus/*.known`, for the optimization thread) |
|---|---|---|---|
| regex | 5,638 (full 19,334) | all but 2 (full: all but 2) | `a\{32768\}` above `RE_DUP_MAX`: glibc `REG_ESIZE`, Liella `REG_BADBR` (POSIX lists "number too large" under `REG_BADBR`); `^(.?)(.?).?\2\1$` on `ab`: glibc reports an empty match at 0, Liella no match, which is correct (a glibc defect) |
| printf | 9,660 | all but 4 | `%Le`/`%Lg` of an x87 pseudo-denormal (exponent field 0, integer bit set): glibc 8.405258e-4934, Liella 3.446156e-4932, the value the Intel SDM gives the encoding |

Fidelity: replayed under glibc, 5 of the 19,334 regex calls give a different answer from the one
recorded (state the replay does not rebuild); the replay compares glibc with Liella on the same
replayed calls, so they do not affect the comparison. The harvest found T26 indirectly: ptx's
calls replay identically, because the logger records a negative size as an empty subject.

## Next step (after milestone 6)

Milestone 6 is done (results above; triage T27–T33; the area's one-page note is
`liella-historic:handoff/test.md`). Next, in order:

0. **After V-5 (2026-10-23)**: the seventh runs above. Left: T34 (fix story, then gawk as is
   again); T17, T27, T33, T4 wait for phase 14; T30's balanced case is the kept difference.
1. **Re-run when other threads land** (done in V-5 for all ten packages) (no download needed; sources, builds and products are on
   Zen 1): `validation/run_remote.sh TAG PRODUCTS COMMIT PKG:LIBC...` with new products after the
   optimization thread's merge (T28–T30 should go: grep's `ere`, `case-fold-titlecase`,
   `stack-overflow`) and after Track M's libm (then drop `shim/libm.c` from `suite.sh`'s gawk
   case and re-run gawk, whose results stop being provisional; T23's `modf` likewise for
   findutils). Re-take `validation/profile6.sh` and `profile.sh` under `~/bin/timing` on the
   merged result.
2. Optionally extend the corpus harvest (`validation/harvest/`) to grep, diffutils and tar
   (their stock builds already call the C library's regex) and to gawk's printf calls; add new
   disagreements to `validation/corpus/*.known`.
3. Dynamic validation (T4, T17, T27, T33) waits for phase 14.

## Triage

Classes: **bug** (Liella defect), **spec** (spec error), **glibc** (glibc-specific behaviour a
program depends on), **env** (environment or scope of this validation), **open** (not yet
classified). Probes are in `~/validation/probes`.

| # | Finding | Evidence | Class | Status (milestone 3) |
|---|---|---|---|---|
| T1 | `libc.a` could not be overridden: a program that defines a function or object `libc.a` also defines (gnulib's `error`, `error_at_line`, `error_message_count`, …, `obstack_alloc_failed_handler`; bash's `group_member`, `glob_pattern_p`) fails to link with "multiple definition", because `libc.a` holds a few large objects, so pulling any symbol pulls all. glibc's `libc.a` has one object per function, so the program's definition wins. | coreutils: 103 programs; sed: `sed`; bash. | bug | **Fixed** by the LEDGER thread (D-X.14, main 246ae7f): every package builds as is in the fourth runs. |
| T2 | §24.5–§24.6 were not in `libc.a`: `uname`, `gethostname`, `getdomainname`, `sethostname`, `setdomainname`, `sysinfo`, `getloadavg`, the `_chk` forms, `rpmatch`, `cuserid`, `acct`, `quotactl`, `reboot`, `klogctl`, `vhangup`, … | bash did not compile; coreutils `uname`, `hostname`, `uptime` did not link. | bug | **Fixed** (2aa507f): §24 at 0 pending except the phase-15 compat stubs; `rpmatch` (D-7.9), `cuserid` (D-12.15). Names left for phases 14/15 below. |
| T3 | No libm product (Track M): `frexp`, `ldexp`, `modf`, … | `find` does not link; gnulib replaces `frexp`/`ldexp`. | env | Track M (its own thread). Every result that used the `modf` stand-in is provisional (P), listed under "Provisional results". |
| T4 | No `dlopen` in a static program. | bash `run-builtins`, `run-glob-bracket` (loadable builtins). | env | Phase 14. |
| T5 | **Regex subexpressions after a greedy atom**: `expr '././tests/misc/arch.sh' : '.*/\(.*\)$'` gives `./tests/misc/arch.sh` (glibc: `arch.sh`). | 697 of 761 coreutils tests fail (`init.sh` derives directory names with `expr`); `test-regex` in all three. | bug | **Fixed** (optimization thread, main 12fdf19): the coreutils harness and `test-regex` pass as is (fifth runs). |
| T6 | **Regex execution never finishes or is quadratic** (sed `dc.sed`, `binary2/3.sed`; sed `s///` 7–10× glibc). | sed `dc`, `binary` stopped by the watchdog. | bug | **Fixed** (12fdf19): sed `dc.sed`, `binary2/3.sed` finish; sed equals glibc as is (fifth runs). |
| T7 | **`memchr` reads past the match** into an inaccessible page. | configure `memchr works` crashes. | bug | **Fixed** (12fdf19): configure keeps `memchr`, `strstr`, `strcasestr` (fifth runs). |
| T8 | **`%.Nls` converts past the precision.** | configure `printf supports the 'ls' directive` exits 8. | bug | **Fixed** (12fdf19): configure keeps the printf family (fifth runs). |
| T9 | `futimens(AT_FDCWD, NULL)`: no `EBADF`. | configure `futimens works`. | bug | **Fixed** (4b1dff6; POSIX, futimens(3) and glibc agree). |
| T10 | `__getgroups_chk(-1, list, n)` aborted; glibc returns -1 `EINVAL`. | configure `getgroups handles negative size`. | bug | **Fixed** (4b1dff6). |
| T11 | `strndup(NULL, 0)` returned NULL; glibc returns `""`. | configure strndup test. | bug | **Fixed** (4b1dff6); generalised in T18. |
| T12 | **The name-service cache's region descriptor took the lowest free number**: with standard output closed, `id` wrote its output into the shared region. | coreutils `misc/io-errors`; strace. | bug (security) | **Fixed** (f64d64e): every descriptor the library keeps is moved to 3 or above (`F_DUPFD_CLOEXEC`); `conformance/c/s11_private_descriptors.c` closes 0–2 and runs every family. `misc/io-errors` passes. |
| T13 | A region named `liella-nscache-0`, owned by uid 1001, appeared in the host's `/dev/shm`. | `ls -l /dev/shm`; reproduced with `unshare --user --map-root-user id`. | bug (security) | **Fixed** (f64d64e): no shared region outside the initial user namespace (`REASON_SECURE`); the `nscache_check` setns test. |
| T14 | Under a small address-space limit: `ulimit -v` of 1–3000 KiB gives `SIGSEGV` before `main`. Analysis: the kernel fails `execve` of the 3.1 MB static image with `ENOMEM` and then kills the process (not Liella code). With 20 MB, every `malloc` fails because the heap reserves 8/16/32 MiB segments up front (`MAP_NORESERVE` still counts against `RLIMIT_AS`); glibc still runs. | coreutils `split/suffix-length`. | bug | Allocator (reservation granularity under `RLIMIT_AS`): INFORMATIONAL for the allocator's owner; see T22. |
| T15 | Precision beyond `INT_MAX` (`%.21474836470s`): Liella formats it; glibc fails with `EOVERFLOW`. | bash `run-printf`. | open | **Fixed** (12fdf19: `EOVERFLOW`, owner ruling R3). bash not re-run yet. |
| T16 | A failed `setlocale` leaves `errno` alone; glibc leaves `ENOENT`. | bash `cannot change locale` message. | glibc | None (cosmetic). |
| T17 | Static programs ignore `LD_PRELOAD`. | coreutils `misc/stdbuf`, `misc/responsive`. | env | Phase 14 (dynamic validation). |
| T18 | gnulib-tests failures (diagnosed from their logs, R4, and own probes): `strverscmp("01", "010")` positive; `strncpy(d, NULL, 0)` and other bounded calls with NULL and 0 aborted; `mbrtowc(&wc, NULL, n, ps)` stored into `wc`; `mbsinit` ignored the header's `__count`, which gnulib's replacement converters set; `pthread_mutex_trylock` (T19); `test-regex` (T5/T6). | `test-strverscmp`, `test-strncpy`, `test-strndup`, `test-mbrtowc-1/-3`, `test-mbrlen-3`, `test-mbrtoc32-3`, `test-mbsrtowcs-3`, `test-mbsrtoc32s-3`, `test-pthread-mutex-type`. | bug | **Fixed** (2276a7f: D-4.19; c0a8753: D-6.1); all pass in the second runs except `test-regex`. The probes also found glibc's lenient UTF-8 (lead bytes F5–FD, `0x110000`) and its state after `EILSEQ`: Liella follows §6.2, rows in `docs/differences-from-glibc.md`; the state after `EILSEQ` is RULING NEEDED R5. |
| T19 | `trylock` on an owned error-checking mutex returned `EDEADLK`. | probe; `test-pthread-mutex-type`. | bug | **Fixed** (7f30c9d; POSIX and glibc: `EBUSY`). |
| T20 | `stime` is linkable from `libc.a`; glibc 2.39 keeps it for old binaries only. | configure `ac_cv_func_stime`. | open | Recorded as phase-15 compat (default and compat versions in a static link); no change now. |
| T21 | bash `run-execscript`, `run-glob-test`, `run-history`, `run-read`: the same differences under both libraries (no terminal, inherited signal dispositions, no `zh_TW.big5` locale, the inherited standard input); only process numbers and T16 differ. | `results/bash-compare*.txt`; `read7.sub` byte-identical under both with the same input. Also coreutils `install/stdin.sh`, skipped once ("/dev/stdin is not readable"). Checked again (milestone 5, item A): `run-execscript`, `run-glob-test` and `run-history` run under the glibc and the Liella bash from one test directory in the same environment, without and with a terminal (`script`), fail the same way under both (22/19, 157/157, 83/62 diff lines), and their outputs are identical once process numbers are normalised, except T16's message in `run-glob-test` (`~/validation/probes/bash_same_env.sh`). | env | None: glibc fails them identically in the same environment. |
| T22 | **stdio needs the heap to print**: a stream's slot is allocated on first use (`stdio::new_slot`), so once the heap is exhausted nothing is printed, not even on `stderr`, `dprintf` fails too, and the program exits 0 silently. | T14's probes under `ulimit -v`. | bug | RULING NEEDED: assignment (stdio/printf owner); recommendation: give the three standard streams static slots and let `dprintf` format into a bounded stack buffer. Assigned to the LEDGER thread (with T14). |
| T23 | **Math names glibc keeps in `libc.a` itself are missing from Liella's**: `frexp`, `ldexp`, `modf`, `scalbn`, `finite`, `isinf`, `isnan`, their `__` and `f`/`l` forms, `__signbit`/`f`/`l`, and the variable `__fpu_control` (20 names, `cargo xtask symgap`). A static program that links only `libc.a` finds them there under glibc; this is why `find` needs the `modf` stand-in and gnulib replaces `frexp`/`ldexp`. | `results/symgap-m4.txt`. | bug | In neither my phase-14/15 list nor T3. Coordinator: Track M (track/libm 04bbd24) already exports these into `libc` as well as `libm`; they arrive with its merge, and then the `modf` stand-in goes. |
| T24 | `__nss_hostname_digits_dots` (§16, default version) is in glibc's `libc.a` and not in Liella's; the plan records it as a spec gap without a signature. | `symgap`. | spec | Pending its signature; not in my list. |
| T26 | **Fixed** (found fixed in code by PHASE-14-PLAN, 2026-10-23: `crates/boundary/src/regex.rs` gives -1 for a negative size; the coreutils re-run is V-5). Original finding: **`re_search`/`re_match` with a negative size return -2** (internal error); glibc returns -1 (no match). coreutils' ptx passes `limit - cursor` that can be negative and treats -2 as fatal ("error in regular expression matcher"). 180 cases (sizes -1 and -5 with every start and range) differ in `~/validation/probes/re_edge.c`; the other cases agree. | coreutils `ptx/word-regex-loop` (as is, milestone 5). | bug | For the optimization thread (regex). Recommendation: -1 for a negative size, as glibc. |
| T27 | **make enables its `load` directive from `<dlfcn.h>`'s declarations alone**: configure checks that `dlopen`, `dlsym` and `dlerror` are declared, finds no library containing `dlopen`, and keeps `MAKE_LOAD`; a static link has no `dlopen`, so `make` does not link. | `results/m6/liella-m-make.summary` (build rc=2; 11 undefined references from `src/load.c`). | env | Phase 14 (T4). With `--disable-load` under both libraries the suites are identical (132 pass, 4 skip). |
| T28 | **Fixed** (PRE14-REGEX-TRIAGE, 2026-10-23): `{}` is `REG_BADBR` under every syntax, `RE_INVALID_INTERVAL_ORD` included, as re-probed against glibc 2.39 (`spec/behaviour/regex-pre14-triage.md`; test `regex_pre14_triage`); the grep re-run is V-5. Original finding: **`a{}` compiles under the GNU syntaxes**: `re_compile_pattern("a{}")` with `RE_SYNTAX_EGREP` or `RE_SYNTAX_POSIX_EGREP` (also with `RE_INVALID_INTERVAL_ORD` or `RE_NO_EMPTY_RANGES`) succeeds under Liella; glibc fails it with "Invalid content of \{\}" (`REG_BADBR`). `regcomp(REG_EXTENDED)` gives `REG_BADBR` under both. | grep `ere` (Spencer test 38: `grep -E 'a{}'` expects status 2, gets 1); reproducer `validation/probes/re_gnu.c` (4 of 45 lines differ). | bug | **For the optimization thread (regex).** Recommendation: `REG_BADBR` for an empty interval under every syntax, as `regcomp` already gives. |
| T29 | **Fixed** (PRE14-REGEX-TRIAGE, 2026-10-23): re-probed, glibc's rule is "same `towupper`" (not the `towlower(towupper(c))` closure recommended below: the Kelvin and Angstrom signs and `İ` stay apart), and under `REG_ICASE` `[:upper:]`/`[:lower:]` take any cased character; Liella now does both (`spec/behaviour/regex-pre14-triage.md`). Ranges follow collation and were not changed. Original finding: **`REG_ICASE` matches only a character's own upper and lower case**: in en_US.UTF-8, `µ` (U+00B5) does not match `Μ`/`μ`, `ı` (U+0131) not `I`/`i`, `ſ` (U+017F) not `S`/`s`, and `i` matches `İ` (U+0130); glibc matches the first three groups both ways and keeps `i` and `İ` apart. grep's own DFA (over `towupper`/`towlower`) agrees with glibc's regex, so the case mappings are right and the regex's folding is not. | grep `case-fold-titlecase` (cases 0–3); reproducer `validation/probes/re_icase.c` (10 of 36 case lines differ). | bug | **For the optimization thread (regex).** Recommendation: fold through the case-mapping closure (two characters match when their `towlower(towupper(c))` agree, or either maps to the other), as grep's DFA does, then settle `i`/`İ` against glibc by probe. |
| T30 | **Error code fixed, bound kept** (PRE14-REGEX-TRIAGE, 2026-10-23): past the depth bound (256) an unbalanced pattern is now `REG_EPAREN` (`REG_EBRACK` with an open bracket), as glibc; a balanced pattern that deep stays `REG_ESPACE` where glibc compiles it (the kept difference: bounded parser depth). grep's `stack-overflow` test may still fail on the balanced case; V-5 re-runs it. Original finding: **1,000 or more nested `(` give `REG_ESPACE`**; glibc gives `REG_EPAREN` at 1,000 and 10,000 and overflows its stack at 100,000 (the probe crashes). grep's `stack-overflow` test accepts only a caught stack overflow or a clean result, so it fails against a regex that bounds its depth instead of recursing. | grep `stack-overflow` (`Memory exhausted` at every size); reproducer `validation/probes/re_icase.c`. | glibc (test) | INFORMATIONAL for the optimization thread: the bounded depth is the better behaviour and the test depends on glibc's recursion; only the error code for an unbalanced pattern differs (`REG_EPAREN` would be exact). No change to the depth limit recommended. |
| T31 | grep `glibc-infloop` and `triple-backref` are XFAIL under glibc (known glibc regex defects: a loop on `((()|a)|())*` with `-w`, and `(.?)(.?)(.?)\3\2\1`) and pass under Liella. | `results/m6/liella-m-grep.results` (XPASS). | glibc | None: Liella is correct; `triple-backref` is the defect behind the corpus's `^(.?)(.?).?\2\1$` disagreement (milestone 5). |
| T32 | gawk `arraysort2`, `delarpm2`, `elemnew2`, `elemnew4`, `matchuninitialized`, `typeof7` fail under glibc too, with byte-identical diffs (untyped values print as `0` where the expected output is empty). | `results/m6/*-gawk.results`; the diffs after "6 TESTS FAILED" in both check logs compared. | env | None: the package's own result here, independent of the C library. |
| T33 | gawk's extension tests (`apiterm`, `filefuncs`) are not run under Liella: no dynamic extensions without `dlopen`. | 594 tests listed under glibc, 592 under Liella. | env | Phase 14 (T4). |
| T34 | **`libc.a` defines `fmod`; glibc's libc does not** (glibc keeps only T23's names in `libc.a`). Autoconf's "library containing fmod" test then finds it without `-lm`, so configure adds no `-lm`, and a program using other maths functions fails to link. | gawk (V-5, as is): configure `checking for fmod` links without `-lm` (glibc: `-lm`); `sin`, `cos`, `exp`, `log`, `pow`, `atan2` undefined. With `LIBS=-lm` gawk equals glibc. `nm libc.a`: `W fmod` beside T23's `frexp`, `ldexp`, `modf`. | bug | **New (V-5)**: fix story. Recommendation: keep `fmod` (and any other math export outside T23's list) in `libm.a` only, as glibc's libc; then re-run gawk as is. |
| T25 | `cargo xtask symgap` counts 86 names as gaps up to phase 13 that the plan gives to phase 15: the 74 `_IO_*` libio internals (§12.13, F32; the plan counts 72) and the 12 profiling names (§28). It treats only §27, §29 and compatibility-only versions as later work. | `symgap` against the phase-15 scope. | tool | **Fixed** (milestone 5): `symgap` knows §28 and the 72 F32 libio internals (an explicit list in `xtask/src/symgap.rs`). The two extra names are `_IO_proc_open` and `_IO_proc_close`: D-12.15 gives them to `popen` (§18), not to F32, so they remain current gaps (owner: whoever owns §18's `popen` companions). The plan's "72 compat-only" was inexact: all 74 are default versions at GLIBC_2.2.5; corrected. On main 246ae7f + this change: 23 gaps up to phase 13 (T23's 20, the two `_IO_proc_*`, T24), 102 later (F32 72, §28 12, §27 13, §29 2, compat 3). |

### Provisional results (re-run when libm lands)

Owner ruling (2026-10-04): results that used a stand-in for a missing math function are
provisional. Until milestone 6 the only such stand-in was `modf` in `validation/shim/missing.c`, linked into the
`liella-w`, `liella-t1` and `liella-t1r` builds and the findutils builds of `liella-m` and
`liella-mr`. Its callers there (`nm -u` over every object
and archive of those builds) are findutils' `find` (`find/parser.o`, `find/libfindtools.a`)
and gnulib's `test-modf`; no other program refers to it. Provisional, therefore:

* findutils' find tests (29) and gnulib tests (424, through `test-modf`) in the first runs
  (`liella-w`), the second runs (`liella-t1`, `liella-t1r`) and the fourth runs (`liella-m`,
  `liella-mr`);
* the `find` column of the traversal table (`find` from the `liella-w` build);
* gawk's whole suite in the milestone-6 runs (`liella-m-gawk`) and the gawk rows of
  `validation/profiles.md`: gawk links the stand-ins of `validation/shim/libm.c` (`sin`, `cos`,
  `atan2`, `exp`, `log`, `log10`, `sqrt`, `pow`, `fmod`, `floor`, `ceil`, `modf`; x87 and SSE2
  instructions, not correctly rounded);
* findutils' configure result `gl_cv_func_modf_no_libm=yes` (`results/liella-*-findutils.cache`):
  the shim is on every link, so configure's link test found the stand-in.

gnulib's own `frexp`/`ldexp` replacements (chosen because `libc.a` has no libm, T3) are
gnulib code, not a stand-in of this validation; the programs using them are re-run with the
others when libm lands, to see the replacement go. The first runs' stand-ins for libc
functions (`uname`, `gethostname`, `getdomainname`, `sysinfo`, `getloadavg`) are gone: the
second runs use Liella's own (T2).

### Left for phases 14 and 15

`libc.a` still lacks these manifest names (`validation/results/libc-a-missing-manifest-symbols.txt`
lists the first runs' set); none is in a milestone-3 scope:

* phase 14: §27 `dl*` and `__tls_get_addr`; §22 `td_*` (thread debugging); the loader side of T4/T17.
* phase 15: §28 profiling; §29 RPC, NIS, STREAMS and `<regexp.h>`; §12 the `_IO_*` vtables; §2
  `sys_errlist`; §4 the `*_small` helpers; §16 `__nss_*` and the `res_gethostby*` compat
  names; §24's compat stubs (`bdflush`, `create_module`, `get_kernel_syms`, `query_module`,
  `nfsservctl`, `sstk`, `sysctl`, `__sysctl`, `uselib`); T20.

Reconciled with `cargo xtask symgap` on main 246ae7f (`results/symgap-m4.txt`: 107 gaps up to
phase 13, 18 names owned by phases 14 and 15): the 18 are §27, §29 and compatibility-only
names of this list (`td_*` are `libthread_db`'s, not `libc.a`'s); of the 107, 74 (`_IO_*`) and
12 (§28) are phase 15 here and in the plan (T25), and 21 are in neither list: 20 §10 names
(T23) and `__nss_hostname_digits_dots` (T24).

## The workload gate: deterministic counters (WL-EXPLAIN, 2026-10-10)

`wl_gate.sh` (see its header) fails only on deterministic counters, per owner rule; wall time is
measured and printed in the advisory `time` column and is never a verdict.

* **Counters** (`wl_run.sh`, per workload and variant): Liella user instructions
  (`perf stat -e instructions:u`, run and children), page faults, and `strace -c -f` call counts: total and
  the classes rw, open, fstat (`fstat`, `newfstatat`, `statx`), stat, dir, mem, proc, other.
* **Threshold** (`wl_report.py`): NEW fails a counter when it exceeds BASE by more than
  `max(0.5 %, 2 x A/A spread)` (`wl_thresholds.tsv`, per host, workload, metric; made by `wl_aa.py` from
  `wl_run.sh OUT kit kit@2 kit@3 kit@4 kit@5`, spread = (max-min)/mean) and by more than an absolute floor
  (3 calls, 8 page faults). A decrease never fails.
* **Output**: a diff against glibc FAILS unless `wl_known_diffs.tsv` has the workload with the same
  fingerprint (count of differing lines : sha1 of them). A changed fingerprint fails. First entry:
  ls-l-enUS (STRCOLL-PUNCT, POSIX wins).
* **Attribution** (`wl_attr.sh`, run by the gate for counter failures): `perf record -e instructions:u -c 1000000`
  on the BASE and NEW kits only (never glibc), top-5 growing and top-5 largest symbols.
* `results/*.replace` are regenerated from the live kit builds' `config.log` (Zen 1 `~/validation/build`).

### wl_count.sh: counters of a branch in one call (WL-COUNT, 2026-10-10)

`validation/wl_count.sh [--base REV] [--rev REV] <dir|rev> <workload>...` (see "Running the wl_ tools through jobrun" below) prints, per workload, instructions:u, instructions:k, page faults and syscalls (the gating number excludes futex and, for a workload that creates threads, its mmap/munmap/brk/mprotect/madvise calls: each on its own advisory row, WLCOUNT-FUTEX, WLCOUNT-MTMEM-2; wl_run.sh uses the same classifier, validation/wl_syscls.sh) for NEW,
BASE (default origin/main) and the host glibc, with new/glibc and new/base ratios and SAME/DIFF output. It builds only
the programs the workloads use (`WL_PROGS` in wl_prep.sh), all on the host it is started on; products, kits and per-variant rows are cached
under ~/wl/{products,kit,count}/<sha>, so a later call only measures the new sha (~4 min wall incl. a library build,
~8 min for two workloads of two programs when both shas are new). `COUNT_FRESH=1` ignores cached rows. The worktree must
be committed. Counts are deterministic (sd 0.00%): main against itself differs by < 0.001% in instructions:u.

### Running wl_percall, wl_count and wl_trace through jobrun (VALIDATION-JOBRUN, 2026-10-20)

The three tools run **on the host where they are started**: no ssh, no `vm_run.sh`, no `S`, no `--host`. Remote dispatch
is the caller's job; the queue is `jobrun`. From a developer's role directory (synced by `tools/sync_role.sh`):

    ~/bin/jobrun <label> count -- sh -c 'cd ~/w/<role> && bash validation/wl_percall.sh --spec validation/percall/locale.c --cases getenv .'
    ~/bin/jobrun <label> count -- sh -c 'cd ~/w/<role> && bash validation/wl_count.sh --base <full sha> . seq'
    ~/bin/jobrun <label> count -- sh -c 'cd ~/w/<role> && bash validation/wl_trace.sh . strcoll_short strcoll'

* **No git needed.** A role directory has no `.git`, only the marker `.synced`. The revision (the cache key under
  `~/wl/{src,products,percall,count}/<rev>`) is `--rev REV`, else the marker's `head=`, else `unknown` (a run of its own,
  never cached across runs, never an error). A tree the marker calls dirty (`tree=...-<hash>`) is refused unless `--rev`
  names it. The role tree itself is copied to `~/wl/src/<rev>` (not `target/`, `.git`, the marker files).
* **Base without git.** The base column needs the base's source or products on the host already:
  `--base <full sha>` with `~/wl/src/<sha>` or `~/wl/products/<sha>` present (any earlier run from a git checkout leaves
  them). wl_percall without `--base` drops the base column; wl_count needs `--base` (exit 2 otherwise). In a git worktree,
  or with a rev of the repository you run in, the old behaviour (merge-base with origin/next, `git archive`) is unchanged.
* `--nowait` is accepted and ignored (jobrun detaches). The tool's stdout is the result (the same tables as before);
  the full log is also `~/logs/<label>.log`, the exit status `~/logs/<label>.done`. `WLP_TAIL=0` prints the whole log.
* The other wl_ scripts (wl_gate, wl_alloctrace, wl_entries, wl_shapes) are unchanged and still dispatch to Zen 1 themselves.

## Accepted deltas (counter growth that follows the reference)

A workload-gate counter increase against an older main that the reference build shows too is recorded here, not fixed.

* **bash-loop `sys_open` +20% (10 -> 12 on the gate's count; WL-BASH-LOOP-SYSOPEN, 2026-10-24).** Six more
  `openat` calls, all ENOENT, all `bash.mo` lookups: `/usr/share/locale-langpack/{en_US.UTF-8,en_US.utf8,en_US,
  en.UTF-8,en.utf8,en}/LC_MESSAGES/bash.mo`, made after the six `/usr/local/share/locale/...` ones (strace -f on the Zen 1 host,
  LANG=en_US.UTF-8; next 17 openat vs old main 11). Cause: gettext's Ubuntu `locale-langpack` fall-back
  (DIFF-AUDIT-FOLLOW-GLIBC, #411, owner ruling 2026-10-22; docs/differences-from-glibc.md, D-7.5). The kit's glibc bash
  makes the same six calls, in the same order, so the opens match the reference. Nothing to fix. The same cause
  accounts for the small `sys_open` growth in ls-l-enUS (+1.62%) and pipe-mix (+6.25%): any program that
  starts with a non-C `LC_MESSAGES` and a bound text domain. Other counters of that gate run (sys_mem, pf, sys_total)
  are not explained by this entry.
