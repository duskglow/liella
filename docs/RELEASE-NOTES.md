# Liella: release notes

This is a soft release: the current tree, published in a fresh repository. It is a public work
in progress, not an alpha or a release candidate, and it should not be used in production.

Released 2026-10-24 as the first commit of this repository, cut from the development tree; that
commit's message names the development commit it was cut from. There is no version tag. Every
figure below says where it comes from and when it was measured.

## What Liella is

Liella is a clean-room implementation of the GNU C library ABI, glibc 2.39, for x86-64 Linux,
written in Rust. It aims to export glibc's symbols with glibc's calling conventions, data
layouts and symbol versions, so that programs built against glibc's headers can use it. The
contract is [`glibc-functional-spec.md`](../glibc-functional-spec.md); the ABI data is in
`spec/abi/`; the design, phases and decision register are in
[`implementation-plan.md`](../implementation-plan.md) and `plan/`.

Licence: LGPL-2.1-or-later, the same as glibc ([`LICENSE`](../LICENSE)).

The clean-room record, including where it fell short in the early phases and what was
rewritten, is in [`docs/clean-room.md`](clean-room.md) and the README's "Clean-room record".

## What works

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

## What does not work yet

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

## How it was validated

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

## Deliberate differences from glibc and compatibility flags

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

## Reporting a security issue

Please report privately, not in a public issue or pull request. Use GitHub private
vulnerability reporting on this repository. See [`SECURITY.md`](../SECURITY.md) for what to
include and what to expect.

## Contributing

See [`CONTRIBUTING.md`](../CONTRIBUTING.md), the code map in
[`docs/codemap/README.md`](codemap/README.md), [`AI_POLICY.md`](../AI_POLICY.md) (Liella was
written with Claude Code under the direction of a human maintainer) and
[`CODE_OF_CONDUCT.md`](../CODE_OF_CONDUCT.md). Contributions are accepted under
LGPL-2.1-or-later. Please do not paste glibc source into a security report (`SECURITY.md`).

## History

The project's full development history is kept in a separate historical repository.
References of the form `liella-historic:<path>` in this tree point into it.
