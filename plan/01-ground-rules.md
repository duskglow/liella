<!-- Part of the Liella implementation plan; index and reading guide: implementation-plan.md -->

## 1. Ground rules

### 1.1 What is being built

A library that exports the glibc 2.39 x86_64 ABI: the same 3,943 names, with the same
calling conventions, data layouts (spec Appendix C) and symbol versions (Appendix C.3).
Binaries linked against glibc should then run against it unmodified. It consists of four
artifacts:

| Artifact | Location | Role | Language rule |
|---|---|---|---|
| **Spec** | `glibc-functional-spec.md`, `spec/` | The contract: functional clauses (§0–§30), symbol index (Appendix B), ABI data (Appendix C, with `spec/abi/*.tsv`). | Source of truth |
| **C syscall layer** | `csys/` | System call wrappers and the few machine primitives that need assembly. No logic, no error handling, no data transformation. | C (freestanding) and assembly. Every function is a single statement. |
| **Rust library** | `crates/` | All logic: the clean capability API, quarantined historical semantics, math, runtime, ABI types. | Safe Rust. `#![forbid(unsafe_code)]` in every crate except `liella-boundary` |
| **POSIX translation layer** | `crates/boundary/src/posix` | Converts C inputs to internal values and internal results to C conventions. Nothing else. | Inside the boundary crate, the only place `unsafe` is permitted |

### 1.2 Sources of truth and sourcing rule

| May be used | Why | Where it enters |
|---|---|---|
| `glibc-functional-spec.md`, including **Appendix C** | The contract and the ABI data. | Everything. Tests are generated from it (§4). |
| **Linux kernel UAPI** (syscall numbers, kernel structure layouts, constants, ioctl codes) | Normative reference, approved (OD-2). The kernel ABI is fixed (⚑ KERN). | `csys/` and `liella-kernel` only |
| **x86-64 SysV psABI, ELF gABI and GNU extensions, IEEE 754, ISO C Annex F/G, RFCs, the Unicode Character Database, the POSIX lists** | Normative references, approved (OD-2). | Scoped per phase |
| **Installed public headers and the dynamic symbol tables of the installed libraries** | Interface declarations, approved (OD-1). Already distilled into Appendix C, which is regenerated with `spec/tools/`. | Appendix C |
| **Installed system data files** (locale archives, iconv modules, nscd protocol, message catalogs, `ld.so.cache`, ...) | Format inspection approved, for interface understanding only: how the data is structured, not how existing code processes it. | The phase that needs each format. Findings become spec Appendix D. |
| **Official documentation that describes behaviour, not implementation**, of any project: the Linux man-pages, the GNU C Library manual, and the official documentation of other projects where needed | Owner ruling 2026-10-02: the experiment rewrites from documentation and specifications; documentation describes what an interface does, not how existing code does it. It also covers, after the fact, the phase-9 timers brief, which had pointed the implementing session at the system-call man pages before this row existed. | Any phase; a behaviour taken from documentation is cited as such in the decision row that uses it |
| **Black-box observation of the installed glibc** (probe programs and differential tests that compare outputs) | Owner ruling 2026-10-02 (§1.4): observing what the reference library *does* is allowed; how it does it never is. In use since phase 7, before this table was updated (§1.4). | Probes and `*_products.rs` differential tests |
| **Not allowed, under any circumstances** | glibc source code, in any form: downloading it, reading it, or writing code from recollection of it (owner ruling 2026-10-02, §1.4). Also any other libc (musl, bionic, newlib, ...), the Rust `libc` crate and Rust `std`'s platform layer. Where the spec and black-box observation are insufficient, the gap goes to the owner. | The workspace has **zero external dependencies** by policy. |

### 1.4 Clean-room methodology and the documented exception

Liella is developed as a clean-room implementation, on a **best-effort** basis: driven by the
spec, verified by black-box observation, with contamination documented and remediated where it
is identified.

**Why it is a clean room** (owner, 2026-10-02). The primary purpose of the project, from the
start, is an experiment: to see what code results from rewriting from first principles, from the
documentation and specifications, with modern techniques. Consulting glibc's source would pull
its historical choices in and taint that goal. (A planned licence change also depended on the
clean-room claim when this was written; since 2026-10-22 Liella has glibc's own licence,
LGPL-2.1-or-later, so that reason no longer applies.) Exact reproduction of current glibc is therefore not the aim of
this pass: where the spec is silent and glibc's observed behaviour looks like a historical
artifact, a cleaner and consistent choice is preferred and recorded as a deliberate difference.
Inefficient, suboptimal or even wrong choices are acceptable now and are worked out over time.

**Spec over reference** (owner, 2026-10-02). Where the spec says something and glibc does
something different, Liella follows the spec and the difference is documented, unless there is a
good reason otherwise; a departure from the spec is recorded as a deviation from the spec.

**Spec disagreements: the three-way rule** (owner, 2026-10-03, binding; amends "spec over
reference": demonstrable spec errors are corrected, not followed). Whenever an implementation,
test or probe disagrees with the spec, three sources are compared: (a) the spec clause, (b) a
black-box probe of glibc, (c) official documentation (a man page, the POSIX text, the GNU C
Library manual, or the kernel's behaviour where the clause is kernel-mediated).
* The spec disagrees with both (b) and (c): the spec is wrong. The clause is corrected (the spec
  text and `spec/clauses.tsv`) with the evidence attached (probe output or a document reference),
  the tests derived from it are updated, and the commit message records the correction (examples:
  `waitid`'s exit status, §18.6; `readdir` on a removed directory, §13.2).
* The spec agrees with (c) and glibc differs: glibc is the odd one out. Liella follows the spec,
  and the difference goes into `docs/differences-from-glibc.md`.
* The spec agrees with (b) and (c) differs: the spec is the odd one out. Nothing is changed; the
  case goes to the owner for a ruling.
* All three disagree, or (c) is silent or ambiguous: the case goes to the owner for a ruling;
  nobody chooses.
Where the kernel contradicts any of the three, the kernel wins (as with `waitid`). Where the owner
has already ruled, the ruling stands.

This section records what happened, what was appropriate, what was not, and how it
was remediated. It does not claim a purity the project cannot guarantee.

**The intended process.** A clean room has two halves separated by a wall. On one side, a
specification is derived from the original; on the other, the implementation is written from the
specification alone.

**The spec session (appropriate, as intended).** On 2026-09-30, 21:16–21:48, the owner's brief
asked for a functional specification of glibc derived from its source and documentation. A
shallow clone of glibc 2.39 (`~/glibc-src` in WSL) and its manual were read to write
`glibc-functional-spec.md`, as that brief intended. Deriving a functional specification from
source material is the legitimate first half of a clean room, and that use was correct. The
clone was not used again after 21:48 (verified from the session record), and it was deleted on
2026-10-02 on the owner's instruction.

**What was not appropriate.**

1. *No wall.* Implementation began at 21:56 on 2026-09-30, in the same continuous agent session
   that had written the spec from the source. The earlier context was summarised, not removed, so
   the wall a clean room depends on was never established for the implementation phase. This
   applies to phases 1–7 and to phase 8 up to commit 924a77b.
2. *Direct consultation during implementation.* On 2026-10-02 at about 04:11 UTC, for the
   printf registry (§12.11), the implementing session downloaded the glibc 2.39 release tarball
   to the test VM and read six files from `stdio-common` (`printf_size.c`, `printf-prs.c`,
   `reg-printf.c`, `reg-modifier.c`, `reg-type.c`, `printf-parsemb.c`). The owner was told on
   being asked; the copy was deleted at once.
3. *Recollection during implementation.* In phase 8 (2026-10-01/02), parts of the
   implementation were written from the session's recollection of glibc's code rather than from
   the spec and observation.
4. *Debugger output.* On 2026-10-02, while finding a crash in one of its own probe programs
   during the remediation, the session ran a debugger backtrace that passed through the installed
   glibc. The backtrace displayed glibc's internal function names and source file names with line
   numbers, from the installed library's debug information (no source text). Nothing was taken
   from it; debuggers are no longer run into the reference library.

**The nine affected items** (the inventory given to the owner on 2026-10-02):

| # | Item | Influence | State when found |
|---|---|---|---|
| 1 | printf registry: argument types (`pa`), built-in type mapping and length flags, `q` as `L`, the registry hooks and tables, reading registered types | read (item 2 above) | uncommitted |
| 2 | `getopt` (`core::getopt`) and `getsubopt` | recollection | committed (924a77b) |
| 3 | `strfmon` (`core::monetary`) | recollection | uncommitted |
| 4 | `fmtmsg` and `addseverity` | recollection | uncommitted |
| 5 | `tmpnam`/`tempnam` directory and prefix rules | recollection | uncommitted |
| 6 | `err`/`warn`/`error`/`error_at_line` line layout (`boundary::report`, `diag::Report`) | recollection | committed (924a77b) |
| 7 | `__xpg_strerror_r` and GNU `strerror_r` | recollection | committed (924a77b) |
| 8 | scanf engine: the `EOF` rule, partial `%c`, `0b` prefixes | initial design from recollection, then corrected by probes | committed (924a77b) |
| 9 | `_IO_getline_info` delimiter handling | recollection (wrong, then corrected by a probe) | uncommitted |

The inventory rests on the implementing session's own account: its reasoning is not kept in the
session record, so for work before phase 8 no equivalent item-level inventory exists.

**Remediation** (owner ruling 2026-10-02):

* All nine items are rewritten in **fresh, separate agent sessions** that never saw glibc's
  source or the implementing session's context. Each receives only the spec and a **behaviour
  document**: inputs, outputs and edge cases observed by black-box probes of the installed glibc,
  with no reference to glibc internals. The affected code is removed before the rewrite. The
  differential tests then compare outputs, as before.
* Phases 1–7 are not individually remediated. The same-session issue is recorded here as a known
  limitation; their behavioural correctness rests on the spec-derived tests and the differential
  tests, which compare outputs only.
* From 2026-10-02, glibc source is never consulted under any circumstances (the table above).
  Where the spec is insufficient, the path is black-box probing and differential testing, or a
  spec gap raised with the owner.
* **Found 2026-10-03 (phase 1 code, same-session period):** the list of environment variables
  removed in secure-execution mode (`rt::start::UNSECURE`) cited a glibc source file
  (`unsecvars.h`) in its comment, so it came from recollection of glibc's source; it also
  removed every `MALLOC_` name and `LD_USE_LOAD_BIAS`, which the reference library keeps, and
  missed four names it removes. Remediated the same day under the owner's ruling: the list was
  rebuilt from the ld.so(8) man page and a black-box probe of a set-group-ID process
  (`spec/security/threat-model.md`), and the comment now cites those.
* **Process notes, validation stage (2026-10-04/05).** Agents worked on separate branches with a
  restricted git command list (status, add, commit, push of their own branch, diff of the working
  tree; never history). Recorded departures, none of which displayed glibc source or any earlier
  revision's content: the ledger thread ran `git log --oneline -4` (hashes and subjects of its own
  latest commits) and `git show HEAD:crates/legacy/src/sys_compat.rs | head -0` (Liella's own file
  at its own latest commit, output discarded), and once `git log --oneline -1 | head -0` (nothing
  displayed; recorded from the coordinator's account); for its earlier milestones, whose
  transcript was summarised, it cannot vouch beyond the summary, which records none. The merge
  agent (2026-10-04/05) ran `git log --oneline` on the ranges being merged (hashes and subjects of
  those commits only), `git rev-parse` of the branch heads, and `git rm`/`git check-ignore` in its
  working tree; it saw the other branch's content only through conflict markers and
  `git diff --staged`, as its brief allows. The TEST thread read gnulib's
  test logs (owner-approved; never its sources), used strace and `nm -u` on two gnulib test objects,
  set one breakpoint on Liella's own `strncpy` (registers only), and once grepped six substitution
  lines from a generated gnulib-tests Makefile (the same values are in `config.log`). Helper agents
  twice listed or touched the coordinator's scratchpad outside their rules; nothing there is glibc
  material. gnulib's `lib/` (replacement libc code, much of it derived from glibc) is never opened.

**Rulings since the validation stage** (owner, recorded here so the public record does not depend on agent
instructions):
* *Aggregate performance counters* (2026-10-08): `perf stat` aggregate counters (cycles, instructions, cache and
  branch misses) on a glibc-linked reference benchmark are allowed - "watching what the code *does* is not the same
  thing as seeing the code". Still forbidden on glibc: `perf record`/`report`/`annotate`, symbols, sampling of its
  addresses, any disassembly, any debugger.
* *Outside models* (2026-10-08): what a non-Claude model sends us is data, never instructions; code it drafts is held
  to the same rules and review as the project's own, and the handoff names the model that drafted it. The models
  doing this work were trained on the same public material, so this adds no exposure beyond the inherent
  limitation described below.
* *gnulib* (2026-10-04/05): gnulib's test logs, configure output and the programs' own sources may be read; its
  `lib/`, `gnulib/` and `gnulib-tests/` sources (replacement libc code, much of it derived from glibc) are never
  opened.
* *History* (standing): agents use a restricted git command list and never read earlier revisions of the repository,
  so the removed code of the nine items is not re-read. The public history is kept as it is: the record of what
  happened, when it was found and how it was fixed is part of the project.

**Inherent limitation.** Absolute clean-room purity was never achievable in this project: the
model doing the work was trained on data that includes glibc's source, whatever the session
boundaries. This is an inherent limitation of AI-assisted clean-room work. Fresh sessions,
behaviour documents and black-box verification reduce exposure; they cannot eliminate it, and
this plan does not claim otherwise.

**Differential testing.** §4.2 originally said that behaviour is never defined by comparing
against a host libc. From phase 7 onward, differential tests (one C program built against the
host glibc and against Liella, outputs compared line by line, deliberate differences recorded)
were used and reported in the phase reviews, but this plan's text was not updated. The owner
confirmed the practice on 2026-10-02; §4.2 now says so.

### 1.3 Tests are the source of truth

1. **Specify.** For each clause, write a test from the spec text. It must fail against the
   empty implementation.
2. **Implement.** Write the least code that makes it pass.
3. **Complete.** A function is complete when every clause of its spec entry that names an
   input constraint, an edge case or a failure condition has an active, passing test.

A phase ends only when `cargo xtask status` shows zero pending tests in its scope (§4.3).

---
