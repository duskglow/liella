# Liella

Liella is a clean-room implementation of the GNU C library (glibc) 2.39 ABI for x86-64 Linux,
written in Rust. The goal is a library that exports glibc's symbols with glibc's calling
conventions, data layouts and symbol versions (3,943 names; see
[plan section 1.1](plan/01-ground-rules.md)), so that programs built against glibc can run
against it. It is written from a functional specification, [`glibc-functional-spec.md`](glibc-functional-spec.md),
and checked by black-box comparison with an installed glibc. It is also an experiment: what
does the code look like when it is rewritten from documentation, with modern techniques?

## Status: public work in progress

Liella is public as a work in progress. It is not an alpha or a release candidate, and nothing
here should be used in production. In particular:

* **It cannot run ordinary programs yet.** The dynamic loader (`ld-linux`, phase 14) is not
  written, so dynamically linked programs, which are almost every program on a Linux system,
  have not been run against Liella. Programs run only when they are linked statically
  against Liella's `libc.a`.
* **It cannot replace glibc on a system.** Running unmodified glibc-linked binaries is the
  project's aim (phase 15) and has not been demonstrated. Do not install it as a system library.
* **It has not had a security audit.** That is planned after phase 15.
* **It is under active development.** Interfaces, behaviour, file layout and the build change
  without notice, and there are no release branches or compatibility promises between commits.

Its full development history is kept in a separate historical repository; references of the
form `liella-historic:<path>` point into it.

| Area | State | Where it is recorded |
|---|---|---|
| Phases 1 to 13 (kernel boundary through remaining services: strings, locale, math, stdio, file system, sockets, name services, processes, signals, threads, time, IPC, ...) | Implemented on `main` and covered by the test gate, except in libm, where the complex `_Float128` functions (18 names, Track M milestone 5) are not yet exported; see [docs/spec-symbol-gaps.md](docs/spec-symbol-gaps.md) (`cargo xtask symcov`) for every name not yet defined. Every change lands on `main` only after the full test gate (unit and integration tests, conformance programs, static checks and an instruction-count check) passes in CI. | [plan/06-phases](plan/06-phases/00-overview.md), [docs/spec-symbol-gaps.md](docs/spec-symbol-gaps.md) |
| Phase 14, the dynamic loader (`ld-linux`, `dlopen`, TLS descriptors, IFUNC, ...) | **Not done.** No dynamically linked program has yet been run against Liella's `libc.so.6`; such programs are linked and checked, not executed. | [docs/toolchain.md section 8](docs/toolchain.md), [plan phase 14](plan/06-phases/17-phase-14-dynamic-linking.md) |
| Phase 15, legacy tier and binary-compatibility acceptance | **Not done.** Running unmodified glibc-linked binaries is the project's aim and has not been demonstrated. | [plan phase 15](plan/06-phases/18-phase-15-legacy-tier-and.md) |
| Security pass | Planned after phase 15. The library has not had a security audit. | [plan](plan/06-phases/19-security-pass-after-phase-15.md) |
| Rust `std` programs | Not supported yet; Rust programs are `#![no_std]`. | [docs/toolchain.md section 8](docs/toolchain.md) |
| Performance | Measured against glibc with a micro-benchmark suite (parity per row is the target; some rows are still above it) and with a workload set of real programs (coreutils, gawk, sed, grep, tar, gzip, ...) built statically against Liella. **Preliminary:** on 2026-10-24 (the tree of this release, one lab host), each of the 30 workloads ran no more user-space instructions as Liella static builds than with the host's glibc 2.39; the geometric mean ratio was 0.821 (below 1.00 means fewer instructions). Their outputs match glibc's, except one documented difference: `ls -l` in `en_US.UTF-8` orders some punctuation differently, because Liella follows the POSIX collation rules there (decision D-4.42). This is one host and one counter, not a release benchmark, and per-call micro-benchmarks still show rows slower than glibc, notably in libm. Merge decisions use deterministic counters (instructions, page faults, system calls), never wall time. | [benchmarks/progress.md](benchmarks/progress.md), [docs/benchmarks.md](docs/benchmarks.md), [validation/README.md](validation/README.md) (workload gate) |
| Platforms | Linux x86-64 only. Building on Windows is not supported (use WSL or Linux). | [docs/toolchain.md](docs/toolchain.md) |

For exact numbers, run `cargo xtask status` (specified versus implemented tests) and
`cargo xtask symgap` (exported symbols versus the glibc symbol list). Do not read the table
above as a symbol-coverage figure; it is a phase list.

Liella's userspace interface is meant to stay identical to glibc's: it differs only where there
is a very good reason, each difference is listed with its reason in
[docs/differences-from-glibc.md](docs/differences-from-glibc.md), and each comes with a
compatibility option to restore glibc's behaviour unless there is a very good reason not to
([docs/compat-flags.md](docs/compat-flags.md); most options are registered designs, not yet
implemented).

## What is in the repository

* `glibc-functional-spec.md` and `spec/`: the contract, and the ABI data in Appendix C. It is the
  source of truth.
* `csys/`: the C syscall layer, system-call wrappers and the few machine primitives that need
  assembly. No logic.
* `crates/`: the Rust library. Every crate is `#![forbid(unsafe_code)]` except `liella-boundary`,
  which permits `unsafe` only in documented, audited categories (`cargo xtask unsafe-audit`).
* `crates/boundary/src/posix`: the translation layer between C conventions and the Rust library.
* `implementation-plan.md` and `plan/`: architecture, phases and the decision register.

## Quick start

Linux x86-64 (or WSL), Rust stable, `cc` and `ar`. From the repository root:

```bash
cargo xtask stubs             # regenerate the test stubs from the spec
cargo xtask so                # build libc.a, libc.so.6 and liella-crt1.o into target/liella/
cargo xtask test              # the full test gate (long; see docs/toolchain.md)
cargo xtask affected --list   # the tests a change can reach (direct tier by default)
```

`scripts/dev.sh xtask test` runs the same through a helper script. Every build parameter is in
[docs/toolchain.md](docs/toolchain.md); the ways a program can use Liella are in
[docs/interfaces.md](docs/interfaces.md). To work on the code, start with
[CONTRIBUTING.md](CONTRIBUTING.md) and the code map in [docs/codemap/README.md](docs/codemap/README.md).

## Clean-room record

Liella is developed as a clean room on a best-effort basis, and it was not a perfect one. The
specification was derived from glibc's source and documentation in a session that then went on
to begin the implementation, so no wall separated the two halves for the early phases. On
2026-10-02 the implementing session also read six glibc source files for the printf
registry and, in phase 8, wrote parts of the code from recollection. Nine items were
identified, removed and rewritten in separate sessions from the specification and black-box
observations; phases 1 to 7 were not individually remediated. Since 2026-10-02 glibc source
is not consulted in any form. The full account, the inventory and the remediation are in
[docs/clean-room.md](docs/clean-room.md) and
[plan section 1.4](plan/01-ground-rules.md). The full development history is kept, unscrubbed,
in the historical repository (see Status): the record of what happened is part of the project.
The project makes no claim of purity beyond what that record supports.

## Documentation

* [docs/architecture.md](docs/architecture.md): how the pieces fit together
* [docs/clean-room.md](docs/clean-room.md): the clean-room rules and record
* [docs/benchmarks.md](docs/benchmarks.md): how performance is measured against glibc
* [docs/differences-from-glibc.md](docs/differences-from-glibc.md): deliberate differences
* [docs/interfaces.md](docs/interfaces.md): the four ways into Liella
* [docs/toolchain.md](docs/toolchain.md): build parameters and commands
* [docs/PRINCIPLES.md](docs/PRINCIPLES.md): the design principles and why
* [CONTRIBUTING.md](CONTRIBUTING.md), [AI_POLICY.md](AI_POLICY.md), [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md), [SECURITY.md](SECURITY.md)

## AI use

Liella was written with Claude Code under the direction of a human maintainer. See
[AI_POLICY.md](AI_POLICY.md).

## License

Liella is licensed under the GNU Lesser General Public License, version 2.1 or (at your option)
any later version (SPDX `LGPL-2.1-or-later`), the same licence as glibc. The full text is in
[LICENSE](LICENSE).

Contributions are accepted under the same licence: by submitting a contribution you license it
under LGPL-2.1-or-later.
