# Contributing

Thanks for helping. A libc is only as good as the corner cases it has been checked against.
The principles behind these rules are in [docs/PRINCIPLES.md](docs/PRINCIPLES.md).

## Project status

Liella is public as a work in progress, not an alpha or a release candidate (see the status
table in [README.md](README.md)). The maintainer
reviews pull requests and answers issues. Well-tested fixes, new probes and tests, parity
improvements and documentation are the most likely to be merged. Phases 14 and 15 (the dynamic
loader and binary-compatibility acceptance) are the largest open areas; talk to us in an issue
before starting one, because the design is recorded in
[plan/06-phases](plan/06-phases/00-overview.md) and in owner rulings.

## Clean-room design (read first)

Liella was written as a clean room: from a specification and from observing glibc's behaviour,
not from glibc's code. Licensing was never the main reason (Liella is LGPL-2.1-or-later, the same
licence as glibc). The reason is design: every decision in Liella should be its own, made for a
reason we can state, for current hardware and with current techniques, rather than inherited from
decades of another library's choices. The history and the record are in
[docs/clean-room.md](docs/clean-room.md) and [plan section 1.4](plan/01-ground-rules.md).

What that asks of every contributor, human or AI:

* **Do not build on glibc's implementation in any form.** Do not copy its code, tables, comments
  or structure, and do not write Liella code from memory of how glibc (or another C library)
  does something. Work from the specification and from observed behaviour.
* **glibc is a specification, not a foundation.** Its observable behaviour is what programs
  depend on, so Liella matches it by default ([docs/PRINCIPLES.md](docs/PRINCIPLES.md) section
  2). But nothing glibc does, now or in the future, is settled for Liella: any behaviour,
  interface or design choice is open to discussion and to a different implementation, as long as
  the change states what it gains.
* **Say where an idea came from.** If you have studied how glibc or another C library implements
  the area you are working on, mention it in your pull request, so reviewers can check that the
  design stands on the specification.

**Sources to work from:**

* [`glibc-functional-spec.md`](glibc-functional-spec.md) and `spec/`: the contract;
* the Linux man-pages, POSIX, the GNU C Library manual, and other official documentation that
  describes behaviour rather than implementation;
* the Linux kernel's UAPI and documentation, the x86-64 psABI, ELF gABI, IEEE 754, RFCs,
  the Unicode Character Database;
* **black-box probes** of the installed glibc: write a small C program, run it against glibc
  and against Liella, compare outputs (`tools/probe/`); what glibc does is the question, not
  how it does it.

The complete table is in [plan section 1.2](plan/01-ground-rules.md).

If the specification and probes are not enough to decide a behaviour, that is a spec gap: raise
it in an issue, and the decision is made and recorded for Liella's own reasons.

## Setup

Linux x86-64 or WSL, Rust stable (`rust-toolchain.toml`), `cc`, `ar`. The reference system and
every build command are in [docs/toolchain.md](docs/toolchain.md).

```bash
cargo xtask stubs     # regenerate test stubs and spec/clauses.tsv from the spec
cargo xtask so        # build libc.a, libc.so.6, liella-crt1.o into target/liella/
cargo xtask test      # the full test gate (long)
cargo xtask affected --list   # only the tests your change can reach (--tier direct, the default)
cargo xtask affected          # run them
cargo xtask clippy            # must be clean (workspace, all targets, -D warnings)
cargo xtask static            # static policy checks
```

`cargo xtask help` (or `cargo xtask` alone) lists the rest (`status`, `symgap`, `trace`, `unsafe-audit`, `purity`,
`bench`, `fuzz`, ...). The code map, [docs/codemap/README.md](docs/codemap/README.md), says
for each spec section which files hold its code, tests and decisions. For a symbol:
`grep -P '^strlen\t' docs/codemap/symbols.tsv`.

Before opening a pull request run the affected tests after your last code change. Say which
you ran. Tests that need a kernel feature skip or reduce on older kernels; say so if your
change touches one.

## How a change gets to `main`

The flow below is how the maintainers work today. Outside contributors open a pull request
against `next`; the maintainers run the lab steps (counts, the merge queue) for you.

1. **Branch** from `origin/next` (the integration branch), one story per branch.
2. **Direct-tier gate.** `cargo xtask clippy`, `cargo xtask static`, then
   `cargo xtask affected` (default `--tier direct`: the changed module, its one-hop users, the
   test targets naming either and the spec sections of those modules; anything the selector is
   unsure of escalates to the full run). Pass `--base origin/next` when the branch was cut from
   `next`. `--tier wide` and `--tier full` exist; the header of `xtask/src/affected.rs` has the
   rules. A developer proves their own change; whole-tree runs are the merge queue's job.
3. **Plants** (mutation checks). For a code change, deliberately break the new code in a
   scratch copy and show that the direct tests fail with the expected message; a plant that
   survives means a missing test, or is written up as equivalent with a reason.
4. **Counts** for performance work: `validation/wl_count.sh` (instructions, page faults and
   system calls of whole workloads, new versus base versus glibc, with an output check) and
   `validation/wl_percall.sh` (instructions per call of single functions). Counts are
   deterministic; wall time is advisory. See [validation/README.md](validation/README.md).
5. **Merge queue.** A green, pushed branch is queued. A dedicated CI host composes queued
   branches onto `next`, runs the FULL gate (`cargo xtask test` with the conformance programs),
   bisects a red train and lands the green branches on `next`.
6. **Promotion.** The release manager promotes `next` to `main`; release candidates get one
   timing run of old versus new `main` per host.

**Planned (owner ruling 2026-10-11, not implemented yet):** the direct tier will drop the
serialized `*_products` tests and the interposed re-run, and will map exported assembly labels
(and `crates/libc-so/hot-text.list`) to the tests that reach them, so a change to a `.S` kernel selects its
tests directly. The CI FULL gate stays the exhaustive run.

## Tests and how decisions are recorded

* Tests are generated from the spec. A function is complete when every edge case and failure
  clause of its spec entry has an active test. Add a regression test for each bug; it must
  fail without your fix.
* Decisions go in the decision register, `plan/07-decisions/`, as `D-<section>.<n>` rows in
  the part file for that section. Find one with `grep -rn "^| D-12.18 " plan/`. Local details
  you can decide yourself with a one-line row; trust boundaries, spec deviations, public
  interfaces and anything costly to unwind need a maintainer's ruling first.
* **The three-way rule** (plan section 1.4) applies when an implementation, test or probe
  disagrees with the spec. Compare the spec clause, a glibc probe and official documentation:
  if the spec is contradicted by both, correct the spec and `spec/clauses.tsv` with the
  evidence; if the spec agrees with the documentation and glibc differs, follow the spec and
  add a row to [docs/differences-from-glibc.md](docs/differences-from-glibc.md); in any other
  case change nothing and ask. The kernel's behaviour wins over all three.
* **Don't break the ABI without a very good reason.** Liella's userspace interface stays
  identical to glibc's unless there is a very good reason to differ, and an accepted difference
  comes with a compatibility option that restores glibc's behaviour
  ([docs/compat-flags.md](docs/compat-flags.md)), unless there is a very good reason not to
  offer one ([docs/PRINCIPLES.md](docs/PRINCIPLES.md) section 2).
* **Wall-clock bounds in tests scale with the machine load.** Never write `elapsed < N s` or
  `Duration::from_secs(N)` as a pass/fail limit: a gate that shares a loaded machine fails it
  (FL-13: a 120 s bound failed at load 20). Prefer an instruction or iteration budget. Where a clock
  is unavoidable, include `tests/support/timeout.rs` and write the idle-machine value:
  `timeout::assert_within("what", Duration::from_secs(N), t.elapsed())` after the work (the failure
  names the bound, the load scale and the measured time), `timeout::deadline(d)` / `timeout::limit(d)`
  for polling loops and `recv_timeout`. The scale is `clamp(1 + ceil(load / cpus), 2, 10)`;
  `LIELLA_TEST_TIMEOUT_SCALE=<n>` overrides it.
* `unsafe` is permitted only in `liella-boundary` and the C layer, within the documented
  categories, each use commented `// UNSAFE(<category>) SAFETY:`.

## Tested processors

Liella is tested on these classes of x86_64 processor. Each class covers an instruction-set level the
library selects code for at start-up, so every level has a machine that runs it.

| Processor class | Instruction-set level it covers |
|---|---|
| Intel Xeon Scalable 1st generation (Skylake-SP) | AVX-512 |
| AMD EPYC 1st generation (Zen 1) | AVX2 on an AMD core |
| Intel Xeon E5 v3 (Haswell-EP) | AVX2 |
| Intel Xeon E5 v4 (Broadwell-EP) | AVX2; used for timing and workload runs before 2026-10-20 |
| Intel Xeon E3 v2 (Ivy Bridge) | AVX without AVX2 (the fallback kernels) |
| Intel N100 (Alder Lake-N efficiency cores) | AVX2 without AVX-512 |

Instruction counters (`instr/call`) are measured on AMD Zen 1; timing comparisons run on the Ivy Bridge
and N100 machines (and, in the early tables of [docs/benchmarks.md](docs/benchmarks.md), on Haswell-EP and
Broadwell-EP). Figures from different processors are not comparable with each other: compare a figure only
with the glibc figure measured beside it on the same processor.

## Performance

Glibc parity is the upper bound: for each benchmarked row Liella's median time should be at or
below glibc's (ratio 1.00). A row above parity needs either a plan to reach it or measured
evidence that it cannot. Host limits in the benchmark files are regression guards, not
targets. Benchmarks and the method are in [docs/benchmarks.md](docs/benchmarks.md) and
`benchmarks/`; run them with `cargo xtask bench`. Timing needs a quiet machine; instruction
counts are steadier. Do not claim a speedup without the before and after numbers and the
machine.

## Conduct

Be professional, keep discussions technical, and keep politics out of the project entirely.
See [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).

## Using AI tools

AI-assisted contributions are welcome, and the clean-room design applies to them as to any other. You
remain responsible for everything you submit. See [AI_POLICY.md](AI_POLICY.md).

## Commits

Imperative subject line; a body that says why, not just what. State only what was done and
tested. Stage files by name, one commit per logical change. For substantial AI help, end the
message with a `Co-Authored-By: <model or tool>` trailer, as [AI_POLICY.md](AI_POLICY.md)
describes.

## Licence

Liella is licensed under LGPL-2.1-or-later ([LICENSE](LICENSE)). Contributions are accepted under
the same licence: by submitting a contribution you license it under LGPL-2.1-or-later.

## Reporting a problem

Include the Liella commit, the function and input, what glibc does (a probe's output is best),
and what Liella does. For security issues use [SECURITY.md](SECURITY.md), not a public issue.
