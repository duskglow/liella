# Design principles

These are the assumptions and principles Liella is built under, with the reasoning behind
them. [CONTRIBUTING.md](../CONTRIBUTING.md) says *how* to contribute; this file says *why*
the project makes the choices it does. Individual decisions are recorded in the decision
register, [`plan/07-decisions/`](../plan/07-decisions/), as `D-<section>.<n>` rows; this file
is the frame they are made in.

When a principle here conflicts with a decision row, the row is the specific ruling and wins
for its case; raise the conflict in an issue.

## 1. Clean room

Liella is written from a specification and from observing glibc's behaviour, not from glibc's
(or any other C library's) implementation. Its purpose was never mainly licensing (Liella has
glibc's licence): it keeps every design decision Liella's own, made for a reason we can state,
rather than inherited. See CONTRIBUTING.md ("Clean-room design") and
[docs/clean-room.md](clean-room.md).

**glibc is a specification, not a foundation.** Liella does not build on glibc's implementation
in any form. glibc's behaviour is what Liella is measured against (section 2), but nothing glibc
does, now or in the future, is settled for Liella: any of it is open to discussion and to a
different implementation that states its gain.

When the specification and black-box probes are not enough to decide a behaviour, that is a
spec gap to raise and decide on Liella's own reasons.

## 2. Compatibility with glibc

**Don't break the ABI without a very good reason.** Programs are written against glibc's
interface and observable behaviour, so Liella's userspace interface (symbols, versions, data
layouts, calling conventions and behaviour, edge cases included) is kept identical to glibc's
unless there is a very good reason to differ. It is not quite the kernel's "never break
userspace", but the bar is high: a difference has to buy something real, such as correctness, a
documented standard's behaviour, security, or a substantial gain in performance or memory, and
deviating for its own sake is never enough.

**Every accepted difference is documented and can be undone.** It gets a row in
[docs/differences-from-glibc.md](differences-from-glibc.md) that states what Liella does and
what it gains, in neutral words (describe Liella's behaviour; never characterise glibc's). It
also gets a compatibility option in [docs/compat-flags.md](compat-flags.md) that restores
glibc's behaviour for a program that depends on it, unless, again, there is a very good reason
not to offer one; that reason is recorded in the row. An option costs nothing when it is not
used. Today most options are registered designs that are not yet implemented.

**Where glibc is inconsistent with itself, Liella picks one rule.** If glibc answers the same
edge case differently in different functions or modes, Liella applies one consistent rule
everywhere, tests it with a table of cases, and records the difference factually.

**The three-way rule** decides disagreements between the specification, a glibc probe and the
official documentation; the kernel's behaviour wins over all three (CONTRIBUTING.md, plan
section 1.4).

**Extensions** beyond glibc are welcome only when the default stays glibc's behaviour (and
costs nothing when unused), the behaviour is documented, and it is opt-in. A new public
interface (a flag, function or variable) needs a maintainer's ruling.

**Protocol-era design choices are differences, not secrets.** Where glibc keeps a behaviour
that was a reasonable design decision in a more trusting era (for example accepting an RPC
reply from any address), Liella may choose the stricter behaviour by default, document the
reason plainly, and offer the old behaviour behind an explicit opt-in.

## 3. Safety, and what a C library is for

**libc is a helper library, not a security boundary.** Liella enforces safety in its *own*
code, and it prevents programs from doing things that cause wider harm. A program shares
responsibility for its own memory: Liella cannot, and does not try to, protect a program from
corrupting itself on every call.

In practice:

* **Keep:** the safety of Liella's own code (its internal invariants, no undefined behaviour
  of its own making, bounds on what it allocates and parses); checks that stop a program from
  affecting others (privileges, other processes, files it does not own); careful parsing of
  untrusted input (DNS answers, locale and configuration files).
* **Keep cheap hardening where libc data steers control flow or memory management:** indirect
  calls through `FILE` or cookie pointers, allocator metadata, saved `atexit`/`setjmp`
  pointers. Attackers target libc's structures to turn a program's bug into control of the
  process, so cheap checks there protect every program.
* **Keep what a caller asked for:** the `_chk` (fortify) entry points check because the
  program opted in.
* **Move off hot paths:** checks whose only effect is catching a program's own benign
  corruption, such as a scribbled `FILE` structure. They belong at a boundary (a buffer
  refill, a lock) or on a slow path, with the decision row saying what is no longer checked
  per call.

**There is always a trade-off.** The line is not always drawn at perfect safety at the
expense of speed. Each check is weighed on what it actually protects against what it costs.
A hole in Liella itself is never traded away.

`unsafe` Rust is confined to the boundary crate and the C layer, in documented categories,
each use carrying a `// UNSAFE(<category>) SAFETY:` comment that states the invariant it
relies on.

## 4. Mathematics

* **Correct rounding where it is claimed is non-negotiable.** Every step of an accuracy claim
  is proven (error bounds restated term by term) and checked by tests and sweeps. Where a
  function claims correct rounding in hard cases, it uses a proven retry at higher precision
  (Ziv's strategy) rather than a heuristic.
* **At the edges, prove what can be proven and document the rest.** 100% certainty is not
  always possible (for example, no published worst case exists for binary128). Liella proves
  every step, names the one residual assumption in the decision row, counts how often that
  path is reached, and moves on.
* **libm honours the caller's floating-point environment,** including flush-to-zero and
  denormals-are-zero modes: it is the caller's environment, and the specification's
  behaviour.
* **glibc's speed is still the bar** for the common path; correctly rounded fast paths are
  expected to come close to it.

## 5. Performance

* **glibc parity is the upper bound per row, and never a ceiling.** A row slower than glibc
  needs a plan to reach parity or measured evidence that it cannot. Assume there is something
  glibc missed; a claim that a row is "at the limit" needs a first-principles bound and a
  record of what was tried.
* **Measure before changing anything.** A suspected cause is a guess until measured. Record
  negative results too: a change that did not help is evidence.
* **Decisions use deterministic counters** (instructions, page faults, system calls); wall
  time is advisory and is never a pass/fail test limit (it fails under load).
* **Keep only changes that make no measured row slower,** or state each trade-off with its
  numbers.
* **Modern hardware first.** Optimise for current processors; older instruction-set levels
  must stay correct and reasonable, not first.
* **Hot calls first.** Effort goes where real programs spend their time.
* **The failure path is cheap to set up and lazy.** Rare failure paths initialise lazily,
  never at start-up or on the hot path; their performance still matters, but diagnostics
  weigh more there.
* **Test in stressful environments,** not only clean ones: large environments, long `PATH`s,
  many file descriptors, big name databases. Code should touch only the data it needs and
  reject early.

## 6. Testing

* **There are no flakes.** An intermittent failure is a defect with an unknown cause, not
  noise. Re-running measures; it never decides. Explain it, then fix it.
* **A test must prove something.** Tests are derived from the specification and name the
  clause they cover. A test that cannot fail proves nothing: plants (deliberately broken
  copies of the code) show each test catches what it is meant to. A test may skip only when
  a capability is genuinely absent on the host, and must say so; a tool that exists but fails
  makes the test fail.
* **No file-system dependence.** Nothing depends on directory order, inode numbers, timestamp
  granularity or one file system's quirks. Sort or compare as sets; where a capability itself
  differs between file systems, document it in
  [docs/testing-filesystems.md](testing-filesystems.md).
* **Every bug gets a regression test** that fails without the fix.

## 7. Security reports

**Problems in Liella:** report them privately, as [SECURITY.md](../SECURITY.md) describes.

**Problems you find in glibc while working on Liella** are glibc's to fix, not Liella's. How far
you investigate, and whether you report, is your own decision. If you report, report directly to
the glibc maintainers through their security process (<https://sourceware.org/glibc/security.html>,
as SECURITY.md says).

What happens on Liella's side depends on whether Liella already has the affected code:

* **Functionality Liella does not have yet:** when you implement it, make sure Liella does not
  repeat the weakness. Word that work neutrally: the commit message, the test names, comments
  and any differences row describe Liella's behaviour only. Do not mention glibc, the weakness,
  or why the change is security-relevant.
* **Functionality Liella already has:** if Liella shares the weakness, it is a Liella
  vulnerability, and it goes through Liella's own security reporting (SECURITY.md) **however
  minor it looks**. Do not fix it in a public pull request first: report it privately, and the
  maintainers coordinate the fix and its timing, with glibc's process where a report is pending
  there.

Either way, **do not draw attention to the hole in glibc until glibc's fix is public**, especially
while a report to glibc's security process is pending, and do not disclose it in a review, a
changelog or the release notes.
