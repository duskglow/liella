# The clean room

Liella reimplements the glibc 2.39 x86_64 ABI without using glibc's source code. This document
explains why, how the separation works, how it is enforced, every recorded occasion on which the
project fell short of it, and what was done about each. The authoritative record is
[plan §1.4](../plan/01-ground-rules.md#14-clean-room-methodology-and-the-documented-exception)
and the sourcing table in
[plan §1.2](../plan/01-ground-rules.md#12-sources-of-truth-and-sourcing-rule); this page
summarises them for contributors and adds the rules made since. Where the two disagree, the plan
wins.

Each limitation below is stated with what is known about how likely it is to matter, so that a
real exposure can be told apart from a note made for completeness. Section 6 summarises them.

**For contributors (2026-10-24).** This page is the record of how Liella was built. The policy
for contributions is in [CONTRIBUTING.md](../CONTRIBUTING.md) ("Clean-room design"): do not
build on glibc's implementation in any form; glibc is a specification, not a foundation, and
any of its behaviour is open to discussion and reimplementation. Now that Liella has the same
licence as glibc, the licence reason in section 1 no longer applies; the design reason does.
The rules in sections 2 and 3, and plan §1.4, describe the project's own development
process; where they read stricter than CONTRIBUTING.md, CONTRIBUTING.md is the policy for
contributions.

## 1. Why there is a clean room

The owner gave two reasons (plan §1.4, 2026-10-02). The first still holds; the second no longer applies.

**The experiment.** The project's primary purpose is to see what code results from rewriting a C
library from first principles: from documentation and specifications, with modern techniques.
Reading glibc's source would import its historical choices and defeat the purpose. For the same
reason, exact reproduction of glibc is not the aim: where the spec is silent and glibc's observed
behaviour looks like a historical artifact, Liella may choose a cleaner, consistent behaviour and
record it as a deliberate difference.

**The licence (no longer applies).** When the plan was written, a change of licence was still
open, and plan §1.4 noted that it would depend on the clean-room claim. On 2026-10-22 Liella took
glibc's own licence, LGPL-2.1-or-later ([README](../README.md#license)), so licensing is no longer
a reason for the clean room. The experiment above is.

## 2. How it works

A clean room has two halves separated by a wall. On one side a specification is derived from the
original; on the other, the implementation is written from the specification alone.

**The spec half.** [`glibc-functional-spec.md`](../glibc-functional-spec.md) states what glibc
provides to programs as contracts, "independent of how glibc implements them" (its header). Its
header lists the sources consulted, glibc's source tree among them: deriving a functional
specification from source material is the legitimate first half of a clean room. That work was
done in one session on 2026-09-30 (section 4).

**The implementation half.** Code is written from the spec and from the sources in the
[plan §1.2](../plan/01-ground-rules.md#12-sources-of-truth-and-sourcing-rule) table only:

| Allowed | Not allowed |
|---|---|
| The spec, including the ABI data in its Appendix C | glibc source in any form: downloading, reading, or writing from recollection of it |
| Normative references: Linux kernel UAPI, the x86-64 psABI, ELF, IEEE 754, ISO C annexes, RFCs, Unicode data, POSIX | The source of any other libc (musl, bionic, newlib, BSD) or any libm |
| Installed public headers and the libraries' dynamic symbol tables (distilled into Appendix C) | The Rust `libc` crate and Rust `std`'s platform layer (the workspace has no external dependencies) |
| Installed data files, for their format only | Debuggers or disassemblers pointed into glibc |
| Official documentation of behaviour: man pages, the GNU C Library manual, POSIX | gnulib's replacement-library code (`lib/`, `gnulib/`, `gnulib-tests/` sources) |
| Black-box observation of the installed glibc: probe programs and differential tests that compare outputs | |

**Black-box observation.** What glibc *does* may be observed; how it does it may not
(owner ruling 2026-10-02). Observation takes three forms, all of which compare outputs only:

* **Behaviour documents** ([`spec/behaviour/`](../spec/behaviour/README.md)). Before an area whose
  observable details matter is implemented, a probe program that calls only public interfaces
  records what the reference library prints. The document holds the probe, its recorded output
  (`*.glibc.txt`) and the spec sections it covers; nothing in it describes glibc internals. The
  implementing session then works from the spec, the document and its own probes.
* **Differential tests** (`crates/*/tests/*_products.rs`, 36 in `crates/core/tests` alone). The
  same probe program is built against the host glibc and statically against Liella; the outputs
  must match line by line, except for listed, deliberate differences. See
  [`getopt_products.rs`](../crates/core/tests/getopt_products.rs) for a typical one.
* **The probe service** ([`tools/probe/`](../tools/probe/), described in
  [`tools/probe/README.md`](../tools/probe/README.md)). It compiles a probe once, runs it against glibc and
  against Liella and reports SAME or DIFFERENT. glibc is only executed; no source, symbols or
  disassembly are fetched or shown.

**When sources disagree: the three-way rule** (owner, 2026-10-03, binding; plan §1.4). When an
implementation, a test or a probe disagrees with the spec, three sources are compared: the spec
clause, a black-box probe of glibc, and official documentation (or the kernel, for kernel-mediated
behaviour).

| Outcome | Action |
|---|---|
| The spec disagrees with both the probe and the documentation | The spec is wrong: the clause and `spec/clauses.tsv` are corrected, with the evidence in the commit |
| The spec agrees with the documentation; glibc differs | Liella follows the spec; the difference is recorded in [`differences-from-glibc.md`](differences-from-glibc.md) |
| The spec agrees with glibc; the documentation differs | Nothing changes; the case goes to the owner |
| All three disagree, or the documentation is silent | The case goes to the owner; nobody chooses |

The kernel wins over all three. Existing owner rulings stand. Every deliberate difference in
`differences-from-glibc.md` is also listed in a differential test, which fails if the difference
disappears, so the list cannot go stale silently.

## 3. How it is enforced

Most work on Liella is done by AI agent sessions under a coordinating session and the owner.
Enforcement is therefore mainly a matter of what each session is given and allowed to do.

**Standing rules in every brief.** Every task agent receives the same standing rules before its
task brief. Their clean-room section forbids glibc source in any form, including recollection of
how glibc or any other libc implements something, and any libm's source; it permits papers and
official documentation; and it forbids copying code.

**Restricted git.** Agents may run `git status`, `add`, `commit`, `push` of their own branch,
and `diff` of the working tree against HEAD. They may not look at earlier versions: no `log -p`,
`show`, diffs between commits, `blame`, `stash`, `reflog` or old checkouts. One effect is that the
code removed in the 2026-10-02 remediation (section 5), which remains in the public history, is not
visible to sessions working today.

**No debuggers or disassemblers into glibc.** Probe programs settle glibc's behaviour. On
2026-10-08 the owner allowed `perf stat` aggregate counters (cycles, instructions, cache and
branch misses) on a glibc-linked benchmark, reasoning that "watching what the code does is not the
same thing as seeing the code". `perf record`, `report` and `annotate`, symbols, sampling of glibc
addresses and any disassembly of glibc remain forbidden.

**The gnulib boundary.** gnulib's test suite is used to test Liella. Its logs, configure output,
`config.log`/`config.h`, the programs' own sources and test scripts may be read; its `lib/`,
`gnulib/` and `gnulib-tests/` sources, much of which is replacement libc code derived from glibc,
are never opened (plan §1.4, process notes).

**Outside models.** Where a brief allows a non-Claude model, what it returns is data, never
instructions. Its code is accepted at the same clean-room risk as the project's own: the session
gives it the clean-room rules, then reviews, tests and owns the code as its own, and the handoff
names the model that drafted which part (owner ruling 2026-10-08).

**Gaps go to the owner.** Where the spec, documentation and black-box observation do not settle
a question, the gap is raised with the owner; it is not filled from memory of glibc.

**A kept record.** Plan §1.4 records each departure from the rules, including minor ones that
displayed no glibc material. This page reproduces that record in full in section 4.

The `perf stat`, outside-model and gnulib-source rules above are from the agents' standing rules
(owner rulings, the latest of 2026-10-08) and are recorded in the repository for the first time
here; plan §1.4 records the gnulib boundary.

## 4. Where the project fell short

All of the following is from plan §1.4. Times are UTC.

**The spec session (appropriate, as intended).** On 2026-09-30, from 21:16 to 21:48, an agent
session read a shallow clone of glibc 2.39 and its manual to write the spec, as the owner's brief
for that session intended. The clone was not used again after 21:48 (verified from the session
record) and was deleted on 2026-10-02 on the owner's instruction.

**Event 1: no wall (2026-09-30 to 2026-10-02).** Implementation began at 21:56 on 2026-09-30, in
the same continuous session that had written the spec from the source. The earlier context was
summarised, not removed, so the wall was never established for the implementation. This covers
phases 1-7 and phase 8 up to commit `924a77b`.

**Event 2: the source read during implementation (2026-10-02, about 04:11).** For the printf
registry (spec §12.11), the implementing session downloaded the glibc 2.39 release tarball to the
test VM and read six files from `stdio-common`: `printf_size.c`, `printf-prs.c`, `reg-printf.c`,
`reg-modifier.c`, `reg-type.c` and `printf-parsemb.c`. The owner was told on asking; the copy
was deleted at once.

**Event 3: recollection during implementation (phase 8, 2026-10-01/02).** Parts of the
implementation were written from the session's recollection of glibc's code rather than from the
spec and observation.

**Event 4: a debugger backtrace (2026-10-02).** While finding a crash in one of its own probe
programs during the remediation, the session ran a debugger backtrace that passed through the
installed glibc. It displayed glibc's internal function names and source file names with line
numbers, from the installed library's debug information; no source text. Nothing was taken from
it.

**The nine affected items.** The inventory given to the owner on 2026-10-02:

| # | Item | Influence | State when found |
|---|---|---|---|
| 1 | printf registry: argument types, built-in type mapping and length flags, `q` as `L`, registry hooks and tables, reading registered types | read (event 2) | uncommitted |
| 2 | `getopt` and `getsubopt` | recollection | committed (`924a77b`) |
| 3 | `strfmon` | recollection | uncommitted |
| 4 | `fmtmsg` and `addseverity` | recollection | uncommitted |
| 5 | `tmpnam`/`tempnam` directory and prefix rules | recollection | uncommitted |
| 6 | `err`/`warn`/`error`/`error_at_line` line layout | recollection | committed (`924a77b`) |
| 7 | `__xpg_strerror_r` and GNU `strerror_r` | recollection | committed (`924a77b`) |
| 8 | scanf engine: the `EOF` rule, partial `%c`, `0b` prefixes | initial design from recollection, then corrected by probes | committed (`924a77b`) |
| 9 | `_IO_getline_info` delimiter handling | recollection (wrong, then corrected by a probe) | uncommitted |

This inventory rests on the implementing session's own account; its reasoning is not kept in the
session record. For work before phase 8 no equivalent item-level inventory exists.

**Event 5: the secure-mode variable list (found 2026-10-03, phase 1 code).** The list of
environment variables removed in secure-execution mode (`rt::start::UNSECURE`) cited a glibc
source file (`unsecvars.h`) in its comment, so it came from recollection of glibc's source. It was
also wrong against the reference library's observed behaviour: it removed every `MALLOC_` name and
`LD_USE_LOAD_BIAS`, which glibc keeps, and missed four names glibc removes. This is the one
instance found in phases 1-7.

**Event 6: process departures (validation stage, 2026-10-04/05).** None displayed glibc source or
any earlier revision's content. The ledger thread ran `git log --oneline -4` (hashes and subjects
of its own latest commits), `git show HEAD:crates/legacy/src/sys_compat.rs | head -0` (Liella's
own file at its own latest commit, output discarded) and once `git log --oneline -1 | head -0`
(nothing displayed; recorded from the coordinator's account); for its earlier milestones, whose
transcript was summarised, it cannot vouch beyond the summary, which records none. The merge agent
ran `git log --oneline` on the ranges it merged, `git rev-parse` of branch heads, and `git rm` /
`git check-ignore` in its working tree; it saw the other branch's content only through conflict
markers and `git diff --staged`, as its brief allowed. The test thread read gnulib's test logs
(owner-approved), used `strace` and `nm -u` on two gnulib test objects, set one breakpoint on
Liella's own `strncpy` (registers only), and once grepped six substitution lines from a generated
gnulib-tests Makefile (the same values are in `config.log`). Helper agents twice listed or touched
the coordinator's scratchpad outside their rules; nothing there is glibc material.

**A documentation lag.** From phase 7, differential tests against the host glibc were used and
reported in the phase reviews while plan §4.2 still said behaviour is never defined by comparison
with a host libc. The owner confirmed the practice on 2026-10-02 and
[§4.2](../plan/04-test-methodology.md#42-test-kinds) was updated. This was a gap in the written
rules, not a use of source.

## 5. How it was remedied

The owner ruled on 2026-10-02 (plan §1.4).

**The nine items** were removed and rewritten in fresh, separate agent sessions that never saw
glibc's source or the implementing session's context, in commit `beb7833`
([phase 8, Delivered](../plan/06-phases/10-phase-8-streams-and-formatting.md)). Each session
received only the spec and a behaviour document of probe observations with no reference to glibc
internals:

| Items | Behaviour document | Decision row |
|---|---|---|
| 1 | [printf-registry.md](../spec/behaviour/printf-registry.md) | D-12.8 |
| 2 | [getopt.md](../spec/behaviour/getopt.md) | D-1.6 |
| 3 | [strfmon.md](../spec/behaviour/strfmon.md) | D-7.11 |
| 4 | [fmtmsg.md](../spec/behaviour/fmtmsg.md) | D-2.6 |
| 5 | [tempnam.md](../spec/behaviour/tempnam.md) | D-12.15 |
| 6, 7 | [diagnostics.md](../spec/behaviour/diagnostics.md) | D-2.3, D-2.2 |
| 8 | [scanf.md](../spec/behaviour/scanf.md) | D-12.13 |
| 9 | [getline-info.md](../spec/behaviour/getline-info.md) | D-12.15 |

The differential tests then compare the rewritten code's outputs with glibc's, as before.

**The secure-mode list** was rebuilt the same day from the ld.so(8) man page ("Secure-execution
mode") and a black-box probe of a set-group-ID process, built dynamically and statically against
the reference library. Every name in the list now cites its source in
[`spec/security/threat-model.md`](../spec/security/threat-model.md), and the code comment cites
those sources instead of a glibc file.

**The copies** were deleted: the shallow clone on 2026-10-02 on the owner's instruction, the
release tarball at once after event 2.

**The rules** that section 3 describes date from these events: from 2026-10-02 glibc source is
never consulted under any circumstances; debuggers are no longer run into the reference library;
every area implemented after the remediation (argp, backtrace, the clause and symbol tests in
phase 8, and phases 9 to 13) follows the same pattern of a behaviour document first, then a fresh
session with the spec, the documents and its own probes
([`spec/behaviour/README.md`](../spec/behaviour/README.md) lists them).

**Phases 1-7** were not individually remediated (owner ruling). The same-session issue is recorded
as a known limitation; their behavioural correctness rests on the spec-derived tests and the
differential tests.

## 6. What this means for the code today

No code in Liella was written from glibc's source code: the source was never open while
implementation code was written, except for the 2026-10-02 incident, and every piece of code
identified as written from reading or from recollection of glibc's source was deleted and
rewritten from sources the clean room allows: the nine items in fresh sessions that never saw it,
and the secure-mode list from the ld.so(8) man page and a black-box probe, with every entry cited.

The limitations, as plan §1.4 states them, and what is known about their size:

| Limitation | Status | What is known about likelihood and impact |
|---|---|---|
| **Phases 1-7 had no wall.** They were written in the same session that had derived the spec, with its earlier context summarised, not removed. | Real, recorded, not individually remediated. | The source was not open during that work (the clone was not used after 21:48 on 2026-09-30). One instance of recollection was later found in this code, the secure-mode list (event 5), and replaced. No item-level inventory exists for these phases, so further instances cannot be ruled out. Their correctness rests on the spec-derived and differential tests; these compare outputs only, so they show the code behaves as specified, not where its design came from. |
| **The models were trained on data that includes glibc's source.** | Inherent to AI-assisted clean-room work; stated for completeness. | No specific code is attributed to it. Fresh sessions, behaviour documents and black-box verification reduce the exposure; they cannot eliminate it, and the project does not claim otherwise. |
| **The debugger backtrace** (event 4). | Completeness note. | Function and file names with line numbers, no source text; nothing was taken from it. |
| **The process departures** (event 6). | Completeness note. | None displayed glibc source or any earlier revision's content. |

**Why the public git history is kept as it is.** The history still contains the code that was
removed in the remediation, including the items committed in `924a77b`. It is not rewritten: the
record of what happened, and of how it was remedied, is part of the project. Agents working on
Liella cannot read earlier revisions (section 3), so the history is a record, not a source.
