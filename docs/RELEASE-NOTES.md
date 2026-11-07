# Liella: release notes

## Second pre-release, 2026-11-07

This is the second public pre-release: the `next` branch promoted to `main` around 2026-11-07. Like
the first, it is a public work in progress, not an alpha or a release candidate, and it should not
be used in production. It is tagged `snapshot-2026-11-07`: a snapshot of the development tree, not a
version.

Every figure below says where it comes from and when it was measured. "The measured tree" is `next`
at commit `136b5246` (2026-11-07), the tree of this pre-release; timing figures name the tree they were
measured on. Work still on its way is named "in progress", without figures. Ratios are Liella/glibc 2.39 on the same
machine: below 1.00 means Liella takes less, above 1.00 more. A timing verdict is "faster" below
0.95, "slower" above 1.05 and "parity" between.

### Summary

- **Dynamic linking (phase 14) has started, and dynamically linked programs still do not run.**
  Liella now builds an `ld.so` that relocates itself and starts a program that needs no shared
  object. It does not load shared objects yet.
- **Static PIE** programs (`-static-pie`) are supported.
- **`libm`**: `exp` and `log` have a new quick first phase, the x87 `sinl` and `cosl` take about
  half of glibc's cycles, and the complex `_Float128` functions (milestone 5) are done. Most `libm`
  rows are still slower than glibc's, `long double` and complex ones by far.
- **Strings, memory and locale**: most timed rows are at parity with glibc or faster on the three
  classes of processor timed.
- **Symbols**: 557 of the 4,305 names of the glibc 2.39 ABI are not defined yet (716 on 2026-10-24).

### Landed late in this run

These landed on `next` on 2026-11-07, after the timing sweeps below were measured:

- **`expf`, `exp2f`**: a one-pass `float` path had left a dependency between consecutive calls
  (a conversion into a register it did not clear first). Fixed: booked timing on the N100, `expf`
  3.11 to 1.66 and `exp2f` 3.13 to 1.67 (Liella/glibc, before/after the fix); on Ivy Bridge `expf`
  2.51 to 1.24.
- **Quick first phases** for `exp2`, `exp10`, `log2`, `log10`, `log1p`, `sin`, `cos`, `tan` and
  `pow`. In progress: `expm1`, the inverse hyperbolic functions. No sweep figures for these yet.
- **Wide-character classification** in the built-in `C` and `C.UTF-8` locales reads one two-stage
  table of class words, and `towctrans` recognises the `toupper`/`tolower` descriptors directly.
- **Start-up page faults**: a statically linked coreutils `printf` (`printf '%s %d %x\n' a 1 1`,
  address randomisation off, one warm-up and five identical runs) takes 61 page faults on the
  measured tree and 62 on the 2026-10-24 tree, on the same host as the test figures below. Run-time
  start-up functions are kept in the hot text group again, and a build check now fails when a hot-text
  key matches no function.

### Tests and symbols on the measured tree

**Tests** (`cargo xtask status` on the measured tree, 2026-11-07, Intel Xeon Platinum 8151 (Skylake-SP),
Linux 6.8): 5,484 tests are specified and 577 of them are still pending (stubs for behaviour not yet
implemented): 85 belong to phase 14, 316 to phase 15, 173 to the mathematics track and 3 are
listed deferrals.

**Symbols** (`cargo xtask so && cargo xtask symcov` on the measured tree, 2026-11-07, the same
host): the glibc 2.39 ABI table (`spec/abi/versions.tsv`) has 4,305 names (4,600 version rows), and
557 of those names are not defined by any Liella product yet:

| Class | Names | Planned in |
|---|---:|---|
| dynamic linking: `ld.so` 33, the `dl*` functions of `libc.so.6` 11, `libthread_db` 40, `libc_malloc_debug` 26 | 110 | Phase 14 |
| compatibility-only versions in libraries Liella ships (`libc.so.6` 111, `libm.so.6` 37, `libresolv.so.2` 11) | 159 | Phase 15 |
| legacy tier, default version (`__libc_sa_len`, `bindresvport`) | 2 | Phase 15 |
| `GLIBC_PRIVATE` (internal to glibc's own libraries: `libc.so.6` 278, `libnsl.so.1` 6, `libresolv.so.2` 2) | 286 | out of scope |

Against the 2026-10-24 table: the 18 complex `_Float128` names are defined (`libm` milestone 5 is
closed); `libnsl.so.1` is now built (below), so its 121 public names are defined and its 6
`GLIBC_PRIVATE` names moved to the out-of-scope row; 17 ONC RPC names of `libc.so.6` are now
exported (below).

### Dynamic linking (phase 14): what `ld.so` does today

`cargo xtask so` now also builds `target/liella/ld-linux-x86-64.so.2`. Today it:

- **relocates itself** before it reads any word a relocation writes. The build checks this: it
  follows every function `ld.so`'s entry reaches before the relocation pass and refuses the product
  if one of them reads a relocated word (D-27.13);
- **is a product of its own**: it links, passes the build's link checks, and answers `--version`
  and `--help` (its own text; [`docs/differences-from-glibc.md`](differences-from-glibc.md));
- **starts a position-independent program that needs no shared object, both ways**: as the
  program's interpreter (`PT_INTERP`), and as `ld.so PROGRAM ARGS`. It applies the program's
  relative relocations (RELA and RELR) and its read-only-after-relocation segment, then enters it.
  The tests start such a program 100 times each way, half of them with address randomisation off
  (D-27.14);
- **clears the environment in secure mode**: it copies the auxiliary vector, decides secure mode
  once from `AT_SECURE`, removes the variables on its unsecure list in secure mode, and only then
  reads its own `LD_*` settings and tunables (D-27.14).

What it does not do yet: load shared objects (a program that names one in `DT_NEEDED` is refused,
with exit status 127), so `libc.so.6` itself is not loaded and **dynamically linked programs still
do not run against Liella**; `dlopen`, `dlsym` and the other `dl*` functions; lazy binding; TLS for
modules; `LD_PRELOAD`, `LD_LIBRARY_PATH` and `LD_AUDIT`; `libthread_db`; `libc_malloc_debug`.
The phase's story list is
[`plan/06-phases/17-phase-14-dynamic-linking.md`](../plan/06-phases/17-phase-14-dynamic-linking.md).

### Static PIE

Programs can be linked against `libc.a` as static position-independent executables, with the start
file `liella-rcrt1.o` (D-27.10). The start code relocates the program with the same self-relocator
as `ld.so`, then records the load bias once (D-27.12). `dl_iterate_phdr`, `_dl_find_object`,
`backtrace`, `backtrace_symbols` and the `%n` read-only check use that bias. One conformance
program starts a C++ static PIE at 100 random load addresses and checks these interfaces and an
exception thrown through three frames; another checks the `backtrace` addresses and text.

### `libm`

Since 2026-10-24:

- **Quick first phase for `exp` and `log`** (D-10.51; D-10.54 for `exp`, D-10.60 for `log`): a short
  computation that returns the correctly rounded result when its error test decides it, and
  otherwise hands the argument to the earlier fast path, in both builds (with and without FMA). Time ratios from the sweep
  tool on idle timing hosts, 2026-11-07, on the code that landed:

  | Function | Ivy Bridge (no FMA) | N100 | Before (11-07 sweep, Ivy Bridge / N100) |
  |---|---:|---:|---:|
  | `exp` | 1.89 | 2.17 | 2.90 / 3.35 |
  | `log` | 1.94 | 2.11 | 5.18 / 5.88 |

  The Ivy Bridge `exp` figure had a large round-to-round spread (0.68); read it as approximate.
  Both functions remain about twice as slow as glibc's on these two classes.
- **x87 `sinl`, `cosl` and `sincosl`**: a new tier in integer arithmetic (D-10.52, D-10.56). Cycles
  per call against glibc (`perf stat`, three interleaved rounds, idle hosts, 2026-11-07): Ivy Bridge
  `sinl` 0.48, `cosl` 0.48, `sincosl` 0.74; N100 `sinl` 0.55, `cosl` 0.56, `sincosl` 0.81. Large
  arguments (10^10, 10^300) measure the same. The 11-07 sweep, before this tier, timed `sinl` and
  `cosl` at 112 (Ivy Bridge) and 88 (N100).
- **`float` `expf` and `exp2f`**: a single pass in `double` arithmetic. Not yet timed on the timing
  hosts.
- **Fast tiers that landed on 2026-11-05 and 11-06**: `double` `sin`, `cos`, `tan` and `sincos`;
  FMA builds of `exp2`, `log2`, `log10` and `pow`; `expm1`, the hyperbolic functions and `cbrt`;
  inverse trigonometric functions and `hypot`; Bessel, gamma and error functions; `float`
  hyperbolic functions. Examples from the 11-07 sweep against the 11-05 snapshot on Ivy Bridge:
  `sin` 4.04 to 2.22, `cos` 3.92 to 2.13, `tan` 4.96 to 2.83, `sinhf` 11.8 to 0.66, `tgammaf`
  1,270 to 0.58.
- **Milestone 5**: the 18 complex `_Float128` functions are implemented.
- **In progress**: quick phases for `exp2`, `exp10`, `expm1`, `log2`, `log10`, `log1p`, `sin`, `cos`,
  `tan`, `pow` and `cexp`.

**Where `libm` stands** (timing sweep of every `libm` row, `next` at `66b8d3ba`, 2026-11-07, Intel
Xeon E3-1265L v2 (Ivy Bridge) and Intel N100; [`results/timing-sweep-libm-1107-table.md`](../results/timing-sweep-libm-1107-table.md)).
This sweep ran before the `exp`, `log`, `sinl`/`cosl` and `expf`/`exp2f` changes above. Of 166
rows, by time ratio:

| Time ratio | Ivy Bridge | N100 |
|---|---:|---:|
| 1.05 or less (faster or parity) | 6 | 6 |
| over 1.05 to 2 | 23 | 22 |
| over 2 to 5 | 39 | 49 |
| over 5 to 20 | 52 | 43 |
| over 20 to 100 | 9 | 18 |
| over 100 | 37 | 28 |
| median | 6.98 | 5.83 |

Median by precision (Ivy Bridge / N100): `float` 2.27 / 2.66, `double` 3.62 / 3.52, `_Float128`
10.4 / 7.5, `long double` 115 / 84. The complex functions have a median of 274 / 239. The rows
over 100 are mostly `long double` and complex functions.

### Strings, memory and locale

Timing sweep 1106E (`next` at `34c98f61`, 2026-11-06, every non-`libm` row; Ivy Bridge (Xeon
E3-1265L v2) and N100; [`results/timing-sweep-1106e-table.md`](../results/timing-sweep-1106e-table.md)):

| Rows | Ivy Bridge faster / parity / slower, median | N100 faster / parity / slower, median |
|---|---|---|
| all 407 judged rows | 252 / 99 / 56 | 313 / 74 / 20 |
| string and memory (`cbench_memref`, 117) | 62 / 33 / 22, 0.94 | 89 / 24 / 4, 0.73 |
| locale (`cbench_locale`, 33) | 19 / 9 / 5, 0.88 | 24 / 4 / 5, 0.80 |
| hot paths (`cbench_hot`, 58) | 32 / 17 / 9, 0.92 | 49 / 9 / 0, 0.67 |

The first sweep on an AMD Zen 3 (Ryzen 5 5500 class, `next` at `66b8d3ba`, 2026-11-07;
[`results/timing-sweep-zen3-1107-table.md`](../results/timing-sweep-zen3-1107-table.md)) judged 376
non-`libm` rows: 274 faster, 64 parity, 38 slower, median 0.75.

The work behind them: AVX2 and SSE2 kernels and short-length entries for `memcmp`, `__memcmpeq`,
`strcmp`, `strncmp`, `strcasecmp`, `memset`, `memchr`, `rawmemchr`, `strstr` and the copy
functions; `nl_langinfo` answered from a table per locale view; faster `wctype` lookups; lower
first-call cost for `setlocale`, `strcoll`, the time-zone load and `bindtextdomain`; directory walks
(`nftw`, `readdir`) and `glob` sorting. The per-row tables are in
[`docs/benchmarks.md`](benchmarks.md) and `results/`.

The workload count of real programs (the 2026-10-24 figure below) was not repeated on the measured
tree.

### Changes users may notice

- **`libnsl.so.1`** is built now, as a placeholder whose NIS/YP and NIS+ functions fail (121
  compatibility-only names, so only programs already linked against it bind them; D-29.6).
- **The ONC RPC client, port-mapper calls and server** are failing stubs; 17 of these names that
  were not exported before are exported now, at their compatibility versions (D-29.5).
- **Static C++ programs** that throw an exception without using threads run.
- **stdio**: a read function entered with the stream's error indicator already set leaves `errno`
  unchanged.

Deliberate differences from glibc are listed with their reasons in
[`docs/differences-from-glibc.md`](differences-from-glibc.md).

### For contributors: tests and CI

Every change must now prove its tests with plants (deliberately broken copies of the changed code)
run by `cargo xtask plants` ([`docs/plants.md`](plants.md)); `cargo xtask affected` selects the
tests a change can reach; the timing sweep tool (`xtask sweep`, [`docs/benchmarks.md`](benchmarks.md)
section 6) times every benchmark row against glibc with interleaved rounds on idle hosts.

### What does not work yet

- **Dynamic linking (phase 14)**: dynamically linked programs do not run (above).
- **Legacy tier and binary compatibility (phase 15)**: running unmodified glibc-linked binaries has
  not been demonstrated; 159 compatibility-only symbol versions and two legacy default symbols are
  not defined
  ([`plan/06-phases/18-phase-15-legacy-tier-and.md`](../plan/06-phases/18-phase-15-legacy-tier-and.md)).
- **Rust `std` programs** are not supported yet; Rust programs are `#![no_std]`
  ([`docs/toolchain.md`](toolchain.md) section 8).
- **Security audit**: the library has not had one; it is planned after phase 15.
- **Platforms**: Linux x86-64 only.

### Processors

The measured tree's status and symbol figures come from an AMD EPYC 7551 (Zen 1) with Linux 6.8.
Timing figures come from Intel Xeon E3 v2 (Ivy Bridge), Intel N100 and AMD Zen 3 hosts, as named
with each table. Tests run on the processor classes listed in
[`CONTRIBUTING.md`](../CONTRIBUTING.md) ("Tested processors").

## Previous release: soft release, 2026-10-24

This is a soft release: the current tree, published in a fresh repository. It is a public work
in progress, not an alpha or a release candidate, and it should not be used in production.

Released 2026-10-24 as the first commit of this repository, cut from the development tree; that
commit's message names the development commit it was cut from. It had no tag when it was published;
on 2026-11-07 it was tagged `snapshot-2026-10-24`. Every
figure below says where it comes from and when it was measured.

### What Liella is

Liella is a clean-room implementation of the GNU C library ABI, glibc 2.39, for x86-64 Linux,
written in Rust. It aims to export glibc's symbols with glibc's calling conventions, data
layouts and symbol versions, so that programs built against glibc's headers can use it. The
contract is [`glibc-functional-spec.md`](../glibc-functional-spec.md); the ABI data is in
`spec/abi/`; the design, phases and decision register are in
[`implementation-plan.md`](../implementation-plan.md) and `plan/`.

Licence: LGPL-2.1-or-later, the same as glibc ([`LICENSE`](../LICENSE)).

The clean-room record, including where it fell short in the early phases and what was
rewritten, is in [`docs/clean-room.md`](clean-room.md) and the README's "Clean-room record".

### What works

Phases 1 to 13 of the plan are implemented: the kernel boundary, strings, locale, math, stdio,
the file system, sockets, name services, processes, signals, threads, time and IPC. Programs
are linked **statically** against Liella's `libc.a` (`cargo xtask so` builds `libc.a`,
`libc.so.6` and `liella-crt1.o` into `target/liella/`). See [`README.md`](../README.md) and
[`docs/interfaces.md`](interfaces.md).

**Tests** (`cargo xtask status` on the release tree, 2026-10-24, AMD Zen 1): 5,020 tests are
specified and 591 of them are still pending (stubs for behaviour not yet implemented). Most of
the pending tests belong to phases 14 and 15 (below).

**Symbols** (`cargo xtask so && cargo xtask symcov` on the release tree, 2026-10-24, AMD Zen 1;
the per-name list is [`docs/spec-symbol-gaps.md`](spec-symbol-gaps.md)): the glibc 2.39 ABI table
(`spec/abi/versions.tsv`) has 4,305 names (4,600 version rows), and 716 of those names are not
defined by any Liella product yet:

| Class | Names | Planned in |
|---|---:|---|
| dynamic linking / `ld.so`, `libthread_db`, `libc_malloc_debug`, placeholder libraries | 113 | Phase 14 |
| compatibility-only versions in libraries Liella ships (`libc.so.6` 128, `libm.so.6` 37, `libresolv.so.2` 11) | 176 | Phase 15 |
| `libnsl.so.1`, which Liella does not replace yet | 127 | Phase 15 |
| legacy tier, default version (`__libc_sa_len`, `bindresvport`) | 2 | Phase 15 |
| `libm`: the complex `_Float128` functions | 18 | Track M milestone 5 |
| `GLIBC_PRIVATE` (internal to glibc's own libraries) | 280 | out of scope |

So "phases 1 to 13 implemented" has one stated exception: the 18 complex `_Float128` names of
`libm` milestone 5. Their `float`, `double`, `long double` and real `_Float128` counterparts, and
the other complex functions, are implemented.

### What does not work yet

- **Dynamic linking (phase 14): not done.** There is no working `ld.so` yet. No dynamically
  linked program has been run against Liella's `libc.so.6`; such programs are linked and
  checked, not executed. `LD_PRELOAD`, `LD_LIBRARY_PATH`, `LD_AUDIT`, `dlopen` and
  `libthread_db` are not available. The phase has a story list
  ([`plan/06-phases/17-phase-14-dynamic-linking.md`](../plan/06-phases/17-phase-14-dynamic-linking.md));
  by the plan's own record, its design (`plan/ph14/design.md`) is proposed with no open
  question ruled yet, and `crates/ld` and `crates/thread-db` are skeletons.
- **Legacy tier and binary compatibility (phase 15): not done.** Running unmodified
  glibc-linked binaries is the project's aim and has not been demonstrated. Not done yet: the
  NIS client library `libnsl.so.1` (§29.2), 176 compatibility-only symbol versions and two
  legacy default symbols (the symbol table above). The binary-compatibility corpus does not
  exist yet
  ([`plan/06-phases/18-phase-15-legacy-tier-and.md`](../plan/06-phases/18-phase-15-legacy-tier-and.md)).
- **`libm` milestone 5**: the 18 complex `_Float128` names above.
- **Rust `std` programs** are not supported yet; Rust programs are `#![no_std]`
  ([`docs/toolchain.md`](toolchain.md) section 8).
- **Security audit**: the library has not had one. A security pass is planned after phase 15
  ([`plan/06-phases/19-security-pass-after-phase-15.md`](../plan/06-phases/19-security-pass-after-phase-15.md)).
- **Platforms**: Linux x86-64 only. Building on Windows is not supported (use WSL or Linux).

### How it was validated

- **Differential tests against glibc.** Liella's behaviour is checked by black-box comparison
  with an installed glibc 2.39: oracle programs run on the host glibc and their results are
  compared with Liella's. The specified tests and their state are counted by
  `cargo xtask status` (above). Every change lands only after the full gate passes in CI. The
  landing gate on the release tree (2026-10-24, Intel Xeon Scalable 1st generation, Linux 6.8)
  ran 11,236 tests with 0 failures and 1,198 ignored, and 114 of 114
  conformance programs.
- **Real programs.** A workload set of real programs (coreutils, gawk, sed, grep, tar, gzip,
  ...) is built statically against Liella, and its output is compared with the same programs
  on glibc ([`validation/README.md`](../validation/README.md)). The workload gate
  (`validation/wl_gate.sh`) fails on deterministic counters (user instructions, page faults,
  system calls), never on wall time. **Preliminary** (2026-10-24, the release tree, AMD Zen 1):
  each of the 30 workloads ran no more user-space instructions as a Liella static build than
  with the host's glibc 2.39; the geometric mean ratio was 0.821. Outputs match glibc's except
  one documented difference (`ls -l` in `en_US.UTF-8`, POSIX punctuation collation, D-4.42).
  This is one host and one counter, not a release benchmark.
- **Fuzzing.** 20 libFuzzer targets cover the parsers of untrusted input (name-service files,
  DNS messages, regex, printf/scanf formats, locale and charmap sources, iconv, glob, wordexp
  and others; [`docs/fuzzing.md`](fuzzing.md)). The latest campaign (2026-10-23, AMD Zen 1)
  ran every target for up to 30 minutes, about 823 million executions in all: 16 targets ran
  the full time with no finding; `regex`, `regex_dfa`, `glob` and `locale` stopped early on
  slow inputs (time-outs), which are open work.
- **Plants (mutation checks).** Changes are checked by planting deliberate faults in the
  changed code and confirming the direct tests catch them, before the change lands. The tree
  does not keep a running total.
- **Kernels and CPUs.** Linux 6.8 on every test host, on the processor classes listed in
  [`CONTRIBUTING.md`](../CONTRIBUTING.md) ("Tested processors"): Intel Xeon Scalable 1st
  generation (AVX-512), AMD Zen 1, Intel Haswell-EP and Broadwell-EP (AVX2), Intel Ivy Bridge
  (AVX without AVX2) and Intel N100.

### Deliberate differences from glibc and compatibility flags

Where Liella deliberately behaves differently from glibc, the difference and the reason for it
(correctness, performance, memory or security) are listed in
[`docs/differences-from-glibc.md`](differences-from-glibc.md), by area (behaviour,
mathematics, character sets).

Behaviour that a user could notice is listed first, in the section
[Deliberate user-facing incompatibilities](differences-from-glibc.md#deliberate-user-facing-incompatibilities).

[`docs/compat-flags.md`](compat-flags.md) registers the differences that a program might
depend on, and how a flag would restore glibc's behaviour (COMPAT-1 to COMPAT-12). Most are
registered designs, not implemented flags; COMPAT-12 (UDP RPC replies from another address) is
implemented as the tunable `liella.compat.rpc_reply_any_source`. COMPAT-3 (the
binding of WEAK exports) is resolved for `libc.so.6` and still registered for `libc.a`.

### Reporting a security issue

Please report privately, not in a public issue or pull request. Use GitHub private
vulnerability reporting on this repository. See [`SECURITY.md`](../SECURITY.md) for what to
include and what to expect.

### Contributing

See [`CONTRIBUTING.md`](../CONTRIBUTING.md), the code map in
[`docs/codemap/README.md`](codemap/README.md), [`AI_POLICY.md`](../AI_POLICY.md) (Liella was
written with Claude Code under the direction of a human maintainer) and
[`CODE_OF_CONDUCT.md`](../CODE_OF_CONDUCT.md). Contributions are accepted under
LGPL-2.1-or-later. Please do not paste glibc source into a security report (`SECURITY.md`).

### History

The project's full development history is kept in a separate historical repository.
References of the form `liella-historic:<path>` in this tree point into it.
