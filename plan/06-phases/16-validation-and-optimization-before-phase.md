<!-- Part of the Liella implementation plan; index and reading guide: implementation-plan.md -->

### Validation and optimization (before phase 14)

Owner instructions (2026-10-04). Outputs: (1) a project benchmarked and as close to glibc parity
as P10 allows, every change measured; (2) a project validated against GNU coreutils, bash and
sed, with failures triaged. Two threads run in parallel, each with its own agent, branch
(`test/validation`, `opt/performance`), working directory and VM directories, merged through the
repository remote:

* **Machines.** Zen 1 (2 × AMD EPYC 7551, 16 vCPUs, two NUMA nodes): validation, fuzzing, and
  anything needing two nodes. Haswell-EP (Xeon E5-2697 v3, Haswell-EP, 16 vCPUs, alone on its host):
  timing benchmarks. Any work may run on either, but never anything on a machine while another
  agent is timing there: every remote command runs under `flock -s ~/machine.lock`, timing runs
  under `flock -x ~/machine.lock`, with a line in `~/CLAIMS`. Baselines are taken on the machine
  of the later numbers. Liella is installed only inside a prefix or chroot.
* **TEST thread.** First the owner's small items (a second seccomp check via
  `/proc/self/status`; the AIO fallback test through the injected-failure kernel; caller stacks
  serialised on every notification path). Then each suite against glibc (baseline), then
  statically against `libc.a`: GNU coreutils, bash, sed (a static validation: dynamic linking waits
  for phase 14, D-22.10, and the report says so). Records which gnulib replacements configure
  selects for each library, and checks `ls`, `find`, `du`, `rm -r` when a directory is removed
  during traversal (D-13.13). Triage: Liella bug or spec error (fixed under the three-way rule);
  glibc-specific behaviour a program depends on (an owner ruling when it would change a
  default); environment. Timing and profiles recorded during validation feed the optimization
  thread.
* **OPTIMIZATION-THEN-FUZZING thread.** P10, on Haswell-EP; the pre-optimization numbers first; the
  first run also checks the cache-geometry path on Intel CPUID (D-24.7). Then, rebased onto the
  TEST thread's fixes, fuzzing on Zen 1 of every parser of untrusted input (`nsswitch.conf`,
  `resolv.conf`, the hosts and passwd files, the shared cache region, DNS responses, regex
  compilation, printf and scanf formats, locale and iconv data, glob and wordexp patterns,
  `__nss_hostname_digits_dots`'s names and buffer sizes (owner ruling 2026-10-05)):
  corpus size, crashes and fixes reported; after the last fix the benchmarks are re-run so the
  final table matches the shipped code. This is the early part of the security pass. The
  infrastructure (one harness per parser, the engine, seeds, `cargo xtask fuzz`, the triage
  procedure) is §4.5, built and smoke-tested beforehand by a separate thread (2026-10-05).
* **Gate before phase 14.** Both threads merged; validation and benchmarks re-run on the merged
  result; report (validation with triage, the before/after table and the "could not improve"
  list, owner items); pause.

A short final optimization pass follows phase 15 (loader, startup and compat-dispatch costs).

#### State on 2026-10-23 and the stories that close this stage (PHASE-14-PLAN)

Checked in the tree at `42e682a3` (origin/next), not assumed:

| Item | State | Evidence |
|---|---|---|
| Owner item: second seccomp check | **done** | `boundary/src/aio.rs` reads `Seccomp:` from `/proc/thread-self/status` before `PR_GET_SECCOMP` |
| Owner item: AIO fallback through the injected-failure kernel | **done** | `core/tests/aio_engine.rs` (`Faulty` through `selftest::aio_choose_engine`) |
| Owner item: caller-supplied stacks serialised on every notification path (timers, message queues, AIO) | **done** (V-1, 2026-10-23) | all three paths start a caller-stack notification through `boundary/src/stacks.rs` (`launch`: one thread at a time on an overlapping stack); `core/tests/sigev_thread.rs` checks an in-use count and a stack-canary word on each: `timer_notification_attributes` (two timers, one stack), `mq_reregistering_shares_stack`, `aio_completions_share_stack` and `lio_completions_share_stack` (io_uring and workers; the list notification on the same stack); a plant that drops the wait in `launch` fails them |
| Static validation (coreutils, bash, sed + six GNU packages) | **done**, triage partly stale | `validation/README.md`. T26 is fixed in code (`boundary/src/regex.rs`: a negative size gives -1) but its row still says open; T28, T29, T30 (regex) show no fix in code or tests; T17, T27, T33 wait for phase 14 |
| Fuzz harnesses for every listed parser | **done** | 20 targets in `fuzz/fuzz_targets/` (nsswitch, resolv.conf, hosts, passwd, group, shadow, nscache, dns, regex, regex_dfa, printf, scanf, locale, charmap, iconv, glob, wordexp, digits_dots, netdb_files, scanners) |
| Fuzz campaign report (corpus sizes, crashes, fixes) | **not found** in the repository | no report under `docs/`, `validation/`, `handoff/` |
| Benchmarks near parity | **open** (continuous) | `docs/benchmarks.md` at `5cc49dc`: Haswell-EP 55 faster / 14 parity / 92 slower, median 1.14; its §5 says a fresh full bench of current `main` is needed |
| Gate report (validation re-run on the merged result, before/after table, "could not improve" list) | **not written** | — |
| Saturation register | separate story SATURATION-REGISTER | owner reviews it before phase-14 implementation starts (standing gate) |

Stories (ids `PRE14-*`; size S/M = one 50-call run; "before P14" = must land before the first
phase-14 *implementation* story, "overlap" = may run alongside phase 14):

1. **V-1 `PRE14-OWNER-ITEMS`** (S, no dependencies, before P14). Confirm the caller-supplied-stack
   rule on all three notification paths (`SIGEV_THREAD` with `pthread_attr_setstack` in the
   `sigevent` for `timer_create`, `mq_notify`, `aio_*`/`lio_listio`): two notifications in flight
   must never run on the one caller stack at once. Proof: one test per path that fires two
   notifications sharing an attribute with a caller stack and checks they do not overlap (a
   stack-canary word plus an in-use flag); a plant that drops the serialisation must fail.
   Fixes only if a path is missing; records the three owner items as done in this file.
   **Done 2026-10-23:** no path was missing; the tests and the plant are named in the table
   above, and the three owner items (seccomp check, AIO fallback, caller stacks) are done.
2. **V-2 `PRE14-REGEX-TRIAGE`** (M, no dependencies, overlap). T28 (`a{}` under the GNU syntaxes),
   T29 (`REG_ICASE` case-mapping closure), T30 (error code at deep nesting; keep the bounded
   depth, which is the gain). Re-probe each against glibc first; follow glibc unless the
   difference gains something (T30 keeps its differences row). Proof: probes in
   `spec/behaviour/`, clause tests, plants on the new branches, the grep suite's three failures
   gone; the T26/T28-T30 rows of `validation/README.md` updated. Watch: regex is a HOT area
   (benchmark rows), so the change needs a `wl_percall` count of the regex rows.
3. **V-3 `PRE14-FUZZ-CAMPAIGN`** (M, after V-2 for the regex targets, before P14; long jobs start
   detached). Run every target for a fixed time each (one shared budget, e.g. 30 min per target,
   in RAM on sumire), record corpus size, coverage and crashes per target in
   `docs/fuzzing.md` (new); a crash becomes its own fix story. Security-relevant: any crash that
   may be a weakness goes to SECURITY-PRIVATE in the hand-back only.
4. **V-4 `PRE14-BENCH-RERUN`** (M, timing booking, before P14). A fresh full bench of the merged
   head on Haswell-EP (and Broadwell-EP), `docs/benchmarks.md` rewritten with the before/after
   table against `5cc49dc` and the "could not improve" list (each row with its reason or its
   open story). Needs the timing booking of the brief's flow (30-60 min wait).
5. **V-5 `PRE14-VALIDATION-RERUN`** (M, after V-1 and V-2, before P14; detached suite jobs). The nine
   packages statically against the merged head's `libc.a`; results and triage rows refreshed;
   any new failure becomes a fix story.
   **Done 2026-10-23** (`validation/README.md`, "Seventh runs"): all ten packages as is, no
   stand-in (`liella-cc` links Liella's `libm.a`); everything equals glibc except phase-14 items
   (T4, T17, T27, T33), grep `stack-overflow` (T30, kept bound) and one new failure, **T34**
   (`libc.a` exports `fmod`, so gawk's configure drops `-lm`): fix story `PRE14-LIBC-FMOD`
   (proposed).
6. **V-6 `PRE14-GATE-REPORT`** (S, after V-1 to V-5 and SATURATION-REGISTER, before P14). The gate
   report the stage asks for (validation with triage, before/after table, "could not improve"
   list, owner items), one page in `handoff/`, then the pause for the owner.

**Order and overlap.** V-1, V-2 and V-4 can start in parallel now. V-3 follows V-2 (regex), V-5
follows V-1 and V-2, V-6 closes. Phase-14 *planning and probe* stories (the no-code stories
DL-0a to DL-0d in `17-phase-14-dynamic-linking.md`) may run alongside the whole stage; phase-14
*implementation* (DL-1 onwards) waits for V-6 and for the owner's review of the saturation
register. The ordinary optimization stories continue throughout: the stage gate is the report,
not parity.
