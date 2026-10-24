<!-- Part of the Liella implementation plan; index and reading guide: implementation-plan.md -->

## 4. Test methodology

### 4.1 Traceability: every clause, symbol, primitive and layout has a test ID

`cargo xtask stubs` parses the spec (and `spec/abi/`) and generates **pending** tests
(ignored, and failing if forced) with stable IDs:

| Suite | Location | Source | Count |
|---|---|---|---|
| Clause tests: internal contract | `crates/core/tests/sNN_*_clauses.rs`, `crates/math/tests/s10_math_clauses.rs` | Contract, Inputs, Guarantees, Failure, Threads, Cancellation and Notes fields, bullets and table rows of spec §1–§29 | 1,203 |
| Clause tests: C conventions | `crates/boundary/tests/s00_conventions_clauses.rs` | Spec C0.1–C0.8 | 22 |
| Translation tests | `crates/boundary/tests/sNN_*_symbols.rs` | Appendix B: one per exported name (2,922) and one per `libm` family (184 families covering 1,021 names; six one-member families with truncated names were merged into their real families on 2026-10-04, Track M) | 3,106 |
| Kernel primitive tests | `crates/boundary/tests/appendix_a_primitives.rs` | Appendix A.1 and A.2, exercised through `CSys` and therefore through the C layer | 281 |
| ABI data tests | `crates/abi/tests/appendix_c_layouts.rs` | Appendix C: one per type layout (161) and per static initializer (10) | 171 |
| **Total pending** | | | **4,789** |
| Specified already | `crates/boundary/tests/csys_link.rs` | The C layer builds, links and is callable from safe Rust | 1 |

Clause IDs encode the location, for example `s4_1_c_2` for §4.1, Contract, bullet 2.
`spec/clauses.tsv`, `spec/symbols.tsv` and `spec/primitives.tsv` map IDs to text.
Regenerating never overwrites a file that already contains specified tests; instead it checks
that every ID is still present.

**Commentary items** (decided 2026-10-05): a spec item that is design commentary rather than
behaviour a program can observe carries the explicit marker `⚑ COMMENTARY:` in the spec (§0.3),
followed on the same line by the testable behaviour it implies and the clause that covers it
(`covered by` and the clause ID) or by `nothing testable` and why. Its ID stays in the suite as
`commentary!(...)` (no test, not pending), so it remains traceable. `cargo xtask stubs` checks
the mark's form (a cited clause must exist) and that the item is a `commentary!` item, and prints
the count and the IDs at every gate; `cargo xtask status` counts them in their own column, and
the schedule check finds them by the marker in `spec/clauses.tsv`'s text. Marked so far: 2
(`s4_n_5`, `s4_n_6`).

**Clause traceability** (owner item 6, 2026-10-04): a spec correction must yield its affected
set by a search for the clause's ID. Every other test (differential, engine, runtime, native
interface) names the clause IDs it checks in a `// Clauses:` comment as the first line INSIDE the function body (never above `#[test]`: the trace check reads from
the `#[test]` line down, so a comment above it attaches to the previous test; `cargo xtask trace` reports it as an error with
file:line), or says
`// Trace: no spec clause (...)` with the decision it comes from, or `no clause ID` where the
spec states the behaviour but `spec/clauses.tsv` has no row for the text. Every row of
`docs/differences-from-glibc.md` (its `Clauses` column) and every behaviour document (its
`**Clauses:**` paragraph) names the clauses it bears on. `cargo xtask trace` checks all of this
and that every clause ID named exists in `spec/clauses.tsv`, so a renumbered clause shows as a
stale reference; `stubs` and `test` run it.

### 4.2 Test kinds

| Kind | What it proves | Where | From phase |
|---|---|---|---|
| **Internal unit** | The clean contract of `liella`/`liella-math`, including all edge and failure clauses, against `Faulty<CSys>` where the kernel is involved. | `crates/{core,math}/tests` | 2 |
| **Translation** | Each export converts correctly: argument mapping, return convention, errno behaviour, `_r`/static-slot/data-symbol semantics. Rust tests call the `extern "C"` functions directly, without symbol export. | `crates/boundary/tests` | 2 |
| **Kernel primitives** | Each `Sys`/`Machine` method against the real kernel, through the C layer: success cases, reachable errnos, and argument layout. | `crates/boundary/tests` | 1 |
| **ABI data** | `size_of`/`align_of`/`offset_of` of every `liella_abi::layout` type and every initializer's bytes against Appendix C. | `crates/abi/tests` | 1 |
| **Runtime primitives** | Heap, sync, thread-block and signal-table invariants against `Faulty<CSys>`. | `crates/rt/tests` | 3 |
| **Policy** | `purity`, `unsafe-audit`, `csys-audit` on every `cargo xtask test`. | `xtask` | 0 |
| **C conformance** | Real C programs, compiled against system headers and statically linked to `libc.a`. They exercise the calling convention, header-inline protocols (Appendix C.9) and data symbols. | `conformance/c` | 4 |
| **Property and exhaustive** | All `f32` inputs for unary math; all bytes and code points for ctype and UTF; round-trips for number parsing and printing. | alongside the unit tests | 2 |
| **Fault injection** | `EINTR`, short I/O, `ENOMEM` at every allocation, descriptor exhaustion (X5). | alongside the unit tests | 3 |
| **Concurrency stress** | Locks, the condvar ordering guarantee, `fork` in threaded programs, cancellation. | `crates/core/tests` | 6 |
| **Binary compatibility** | Unmodified distro binaries against our `ld.so` + `libc.so.6`. | release gate | 14–15 |

Oracles come from the spec. Numeric reference values come from a test-only exact arithmetic
module written in this project. Where the spec leaves behaviour open, black-box observation of
the installed glibc (probes and differential tests comparing outputs) settles it, and each
deliberate difference is recorded (owner ruling 2026-10-02, §1.4; until then this paragraph
said behaviour was never defined by comparison, while phases 7 and 8 already used such tests).
glibc's source is never an oracle.

### 4.3 Lifecycle and gates

```
pending ──(test written from spec; fails)──▶ specified ──(code)──▶ implemented ──(all I/F clauses active)──▶ complete
```

`cargo xtask status` reports pending versus specified per section and suite. The **phase exit
gate** requires all of the following:

* every clause, symbol and layout in scope is specified and passing;
* `purity`, `unsafe-audit` and `csys-audit` are clean;
* the unsafe site count per category is reviewed against the previous phase;
* the clippy lints of S7 are at `deny` for the crates touched;
* benchmark budgets are met;
* `cargo xtask status --schedule` flags nothing the phase owns, and every item the phase
  leaves pending is moved in `spec/schedule.tsv` to a later owner or a `DEFERRAL`, each with
  its plan reference.

**The schedule check** (2026-10-05). `spec/schedule.tsv` assigns every pending test to the
phase, track or stage of §6 that owns it: by spec section, by status group, and finer where the
plan splits a section or moves an item (clause IDs, symbol names, and `compat-only`, the names
Appendix C.3 gives no default version), with the plan reference of each row. It also records the
schedule: each owner's order and state (phases 0–13 done, the validation and optimization stage
current, Track M from phase 2, started 2026-10-05, joining at phase 8). A row may be a `DEFERRAL`,
which names an owner ruling or a decision row and its reason, or `UNASSIGNED` where the plan names
no owner. `cargo xtask status` prints a per-owner summary after its table, and `cargo xtask status
--schedule` lists every pending test under its owner and fails when pending work belongs to a
phase that is done, to a track that has not started or whose join phase is done, or to nobody; it
also fails when a selector names nothing in the manifests, or a status group or spec section has
no row. In `cargo xtask test` the schedule is a report and does not fail the gate, until the
owners of the flagged work are settled.

### 4.4 Environment

Linux x86_64 (WSL2 works) with a C compiler (`cc`, `ar`) for the C layer. `scripts/dev.sh`
keeps build output on a Linux filesystem. Network tests use loopback and network namespaces
only.

Privileged success paths run unprivileged, as root of a fresh user namespace in a forked
child (`selftest::in_child` plus `unshare(CLONE_NEWUSER | ...)`). Where a test needs a full
subordinate ID mapping (`setgroups`, switching between mapped IDs) it uses the setuid helpers
`newuidmap`/`newgidmap` (package `uidmap`) with the user's `/etc/subuid` and `/etc/subgid`
ranges. Destructive primitives (`reboot`, module loading, `swapon`, `acct`, ...) are tested on
error paths only. Phase 1 needed no `#[ignore = "privileged"]` test.

Kernel tests run as threads of one process, which has two hazards, both handled in the suites:
* **A forked child copies every open descriptor.** Checks that depend on the *last* reference
  being closed (pipe end of file, `POLLHUP`, `EPIPE`, `ECONNREFUSED` on a datagram pair) run
  inside a single-threaded forked child, where no concurrent fork can hold a copy.
* **Descriptor numbers are reused at once.** "Closed descriptor" checks run in children too, or
  use numbers beyond any table (999,999). A test never closes a number twice.
* **Forked children of a multithreaded process must not allocate.** `raw::fork` runs no
  `atfork` handlers, so another thread may hold the allocator's lock at the fork. Child bodies
  use stack buffers, statics and `KPathBuf` only.

### 4.5 Fuzzing and crash triage

Built 2026-10-05 (fuzzing infrastructure, before phase 14; the campaigns run in the
optimization-then-fuzzing thread, §6 "Validation and optimization").

**Engine: libFuzzer through `cargo fuzz`**, with a dated nightly (`fuzz/rust-toolchain.toml`)
installed beside the pinned stable toolchain. Why: it is coverage-guided and in-process (the
parsers are small and fast, so millions of inputs per minute), the instrumentation is rustc's
own (no compiler wrapper), and it brings what triage needs: crash minimisation (`tmin`), corpus
minimisation (`cmin`), time-outs, a memory limit and fork mode. What it detects matches what a
safe-Rust library can still get wrong: panics (with debug assertions on, so integer overflow
panics as in the test builds), time-outs (unbounded work) and memory blow-ups. AFL++ was not
chosen (a separate installation and a fork per input, slower on parsers this small), nor an
in-repository mutation driver (no coverage feedback, so blind beyond the seeds). AddressSanitizer
is off by default (the harnesses run safe Rust over Liella's internal API; `--asan` turns it on).

**Isolation.** `fuzz/` is a workspace of its own, excluded from the root workspace: only it
depends on `libfuzzer-sys` and uses the nightly; the library crates and the products keep zero
dependencies and the pinned toolchain. Each harness calls Liella's own code (internal API) on
bytes in memory: no system file, no command, no network, never the host C library.

**Harnesses** (`fuzz/fuzz_targets/<target>.rs`; each file's header states its input format):

| Target | Parser | Checks beyond "no panic, no hang" |
|---|---|---|
| `nsswitch` | `nsswitch.conf`, `host.conf` `multi`, the action engine | encoded form round trip |
| `resolv_conf` | `resolv.conf`, `LOCALDOMAIN`, `RES_OPTIONS`, `HOSTALIASES`, the search rules | a server always |
| `hosts` | `hosts` lines, lookups by name and address | the cache encoding round trip |
| `netdb_files` | `services`, `protocols`/`rpc`, `networks`, `aliases`, `netgroup` | |
| `passwd`, `group`, `shadow` | the user and group databases, group lists | `put*ent` output reads back; each GID once |
| `nscache` | the name-service cache region (D-16.10): raw and patched tables | an untouched table returns what was stored |
| `dns` | DNS responses: messages, names, the host answer reader, TTLs, text form | found hosts carry addresses of the type |
| `regex` | compilation under any syntax bits, the serialised program, matching | matches and groups within the subject |
| `printf`, `scanf` | formats with arguments, byte and wide | the returned count is the output; pushback of the unit read |
| `locale` | locale archive, category files, alias file, then their use (ctype, collation, `strftime`, brackets) | |
| `charmap` | gzip inflation, the charmap format, conversions through it | |
| `iconv` | charset names and suffixes, the built-in codecs in pieces | UTF-8 → UTF-32 → UTF-8 round trip |
| `glob`, `wordexp` | patterns over a simulated file system and shell (commands never run) | |
| `digits_dots` | the numeric host parsers `__nss_hostname_digits_dots` is to layer on (ruling 2026-10-05) | `inet_aton` agrees with `inet_pton`; text round trips |

Locale data and charmaps are `'static` in the library (D-7.7, D-6.8), so those harnesses leak a
copy of the input, within a budget per process (`liella_fuzz::LEAK_BUDGET`, 1 GiB); their runs
are cut into short chunks.

**Seeds and corpora.** `fuzz/seeds/<target>` holds small seeds (at most 160 per target, about
60 KiB in all), regenerated by `fuzz/harvest_seeds.py` from the repository's behaviour records
(`patterns`, `scanf`), the probes' format strings and fixture texts
(`crates/core/tests/oracle/*.c`), and inputs built by the script (DNS messages, synthetic locale
and charmap files). Campaign corpora and crash artifacts live outside the repository, in
`~/liella-fuzz-corpus/<target>/{corpus,artifacts}` on the fuzzing machine
(`LIELLA_FUZZ_CORPUS` overrides); `harvest_seeds.py --system ~/liella-fuzz-corpus` adds seeds
made from the installed locale files and charmaps there, never in the repository.

**Running.** `cargo xtask fuzz <target> --time <minutes>` (or `--all`), under the machine queue
(`~/bin/shared bash -lc 'cd ~/liella-fuzz && cargo xtask fuzz regex --time 60'`). One invocation
is refused above 140 minutes (the queue's cap is 150); a campaign is repeated invocations, each
resuming from the corpus, and inside an invocation each target runs in chunks (10 to 30 minutes)
of fresh processes. `--jobs <n>` uses libFuzzer's fork mode on `n` cores. `cargo xtask fuzz
--list` names the targets.

**Smoke test** (Zen 1, 2026-10-05, 3 minutes per target under the queue, release with debug
assertions, no sanitizer): every target ran clean after triage. Findings: `printf` ran out of
memory on `%8888…8$f` (a position far beyond `NL_ARGMAX`); that is the library bug fixed
independently as D-12.18 on `ledger/items` (the harness skips such positions until this branch
has the fix, and the inputs are pinned seeds); two harness bugs, fixed in the harnesses: `scanf`'s
simulated pointer for a huge position overflowed, and `digits_dots` assumed `inet_net_pton` never
answers more than 32 bits (the reference answers 40 for `1.2.3.4.5` with room, probed).

**Crash triage** (a crash, a time-out, a memory-limit report or a failed check):
1. *Reproduce*: `cargo xtask fuzz <target> --repro <artifact>` (the artifact path is printed;
   it is under `~/liella-fuzz-corpus/<target>/artifacts`).
2. *Minimise*: `cargo xtask fuzz <target> --tmin <artifact>`.
3. *Classify*: a library bug (the panic or the unbounded work is in Liella's code); a harness bug
   (the harness's own check or simulation is wrong: fix the harness, say so in the report); or
   behaviour the contract allows (work proportional to a documented bound): the harness then
   bounds that input, with a comment naming the bound. A fix that changes observable behaviour
   follows the three-way rule (§1.4) or goes to the owner.
4. *Regression test*: in the crate's tests, under the clause or decision ID the code
   implements (its `// Clauses:` or `// Trace:` line, §4.1), built from the minimised input; it
   must fail before the fix.
5. *Fix*, then: the regression test and the area's tests (three runs), `--repro` on the
   artifact, and the minimised input added to `fuzz/seeds/<target>`.
6. *Report*: the commit, the target, the input (or its description) and the cause.

---
