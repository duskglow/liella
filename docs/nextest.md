# Handoff: TOOL-NEXTEST (the gate's tests under cargo-nextest)

**State (2026-10-07).** Branch `tool/nextest` from main a7bc952, pushed; HEAD = the commit that
adds this note. Commits: 7353020 (s23 test fix; ALSO carries `.config/nextest.toml` and
`xtask/src/nextest.rs` by a staging mistake, said in 340c3f9's message), c61989f (run.sh lock),
340c3f9 (xtask wiring + affected summary), then this note.

## What changed
* `cargo xtask test`: both workspace passes run `cargo nextest run --workspace [--features ..]`
  then `cargo test --workspace --doc [--features ..]` (nextest skips doctests), when `cargo
  nextest --version` works and `LIELLA_NO_NEXTEST` is unset; otherwise `cargo test --workspace`
  exactly as before (vm510 guest has no nextest: unchanged there). `xtask/src/nextest.rs`.
* `cargo xtask nextest <args>` = `cargo nextest run <args>` + the summary lines; `affected --run`
  runs its per-crate tests through it (FULL escalation goes through `xtask test` anyway).
* Summary lines: after nextest's own report, one `test result: ok|FAILED. P passed; F failed;
  S ignored; ... finished in Ts (nextest)` and `---- <binary> <test> stdout ----` per failed /
  signalled / timed-out test. `~/bin/test-summary` and affected's `summary()` parse it unchanged.
  No `Summary` line (nextest died) -> `FAILED ... no summary line`.
* Coordinator addendum: affected's `summary()` appends the first `--> file:line:col` after each
  kept error line (`error: ... [file:line:col]`). The same change for `~/bin/test-summary` is in
  `$S/work/new/test-summary` (NOT installed; tested on a sample log) - coordinator installs.
* `.config/nextest.toml` (default profile): test-threads `num-cpus` (affinity-aware; `cargo xtask` passes
  CPUs x `TEST_THREADS_PER_CPU`, GATE-TEST-THREADS), retries 0, fail-fast false,
  status-level pass (every test named in the log), slow-timeout 180 s x 10 (a hang ends as TIMEOUT
  after 30 min instead of eating the 150-min cap).
* `conformance/run.sh`: flock per program (`build/<name>.lock`), fd closed for the program.

## Serial groups and why (decided by reading the tests)
* `privileged` (max-threads 1): the 13 `*_products` binaries that use `sudo -n` (aio, catalogs,
  dns, fstree, mntent, netdb, nscache, patterns, posixipc, process, syslog, terminals, users) and
  the two that run in their own user/PID/mount namespaces (admin, noproc). dns_products' own lock
  says load stretches its timed waits. One at a time = how cargo test ran them.
* Cross-process conformance runner: NOT a group but a per-program flock in run.sh. The ~25 test
  files guard run.sh with an in-process `RUNNER` mutex, which does nothing across nextest's
  processes, and many programs are run from several tests (s01_argp x7): their exe/logs/secure copy
  are per name. A group would have serialised whole binaries.
* Not grouped, checked: the other ~40 `static Mutex` locks in tests guard process-wide state
  (env, locale, logger, getopt, pools) - per process under nextest, so moot; scratch dirs carry
  the pid or the test tag; `>=` elapsed asserts get more true under load; regex_subpatterns' 120 s
  bound and s07's 800 ms check guard themselves; rlimits are per process; s24/s25 namespace tests
  re-exec `current_exe` (works under nextest).

## Results on Zen 1 (role h, same tree; `~/bin/gate 12G ... cargo xtask test`, whole command)
| run | runner | wall (whole `xtask test`) | result | peak |
|---|---|---|---|---|
| nx-run1 | nextest, before the s23 fix | 668 s, stopped | 4652 passed, 1 SIGTERM (hang, see Findings) | 9.8G |
| nx-base2 | `LIELLA_NO_NEXTEST=1` (cargo test) | **2935 s** | 340 result lines ok; 9334 "passed" (=9330 + 4 child lines, below); 1396 ignored; conformance 82/0 | 9.8G |
| nx-run2 | nextest | 1131 s (cold-ish) | 9330 passed, 0 failed, 1396 ignored; conformance 82/0 | 12G, oom_kill=0 |
| nx-base | nextest (env lost, see Findings) | 940 s | same, 0 failed | 11G |
| nx-run3 | nextest | 961 s | same, 0 failed | 11G |
| nx-run4 | nextest | 991 s | same, 0 failed | 11G |

**Saved per Zen 1 gate: ~33 min (2935 s -> ~960 s warm, -67%).** Each nextest pass ~400-430 s.
Same tests: per binary the pass counts match libtest's `--list` (checked s04/s09, the only
differing binaries, and the other ~150 by count); ignored 1396 both (697 skipped per nextest pass
+ 2 doc). 4 extra cargo-test "passes" are re-exec children's own result lines (Findings).
FL-5 (nscache window) did not show in 4 green nextest runs.
Longest tests (critical path): special_cr long_double_bessel 207-211 s, aio_products 163-165 s,
liella fnmatch direct-vs-general 148 s. Not yet known which chain sets the ~400 s pass.

## Findings
* FINDING (fixed, 7353020): `sym_clock_nanosleep` (boundary s23_time_symbols) slept on
  CLOCK_PROCESS_CPUTIME_ID and only returned because other tests' threads in the same process
  burned CPU; alone (nextest) it hung at 0% CPU (nx-run1: killed at 324 s, SIGTERM). Red = nx-run1
  on the unfixed tree; green = every later run. A spinning thread now drives the clock.
* `~/bin/gate` runs the command via `sudo systemd-run`: the caller's environment is DROPPED.
  `LIELLA_NO_NEXTEST=1 ~/bin/gate ...` silently ran nextest (my first "baseline", nx-base);
  put the variable inside the command: `~/bin/gate 12G bash -lc 'cd .. && LIELLA_NO_NEXTEST=1 cargo xtask test'`.
* cargo test's own counts are inflated by tests that re-exec their binary with inherited stdout
  (the child prints its own `test result: ok. 1 passed; 23 filtered out`): +4 per gate (s04, s09
  clauses, both passes). nextest captures the child, so its 9330 is the true count.
* Nested queue: `~/bin/gate` takes a `~/bin/shared` slot itself; inside a vm_run (already a slot)
  it worked with free slots but can self-deadlock on a full queue. vm_gate.sh avoids it; my
  measurement script did not.

vm_affected default (whole branch, escalated FULL: stubs + clippy + xtask test under nextest):
green - 9330 passed, 0 failed, 1396 ignored, conformance 82/0, static checks ok.

## OUT-OF-THE-BOX / RESOURCES / SATURATION
* Proposal (no ruling, tooling): start the longest tests first (nextest override `priority`, or a
  list kept from the PASS durations) and/or split special_cr's long-double Bessel test (207 s) and
  the 148 s fnmatch agreement test into shards: a pass could drop from ~410 s toward ~250 s
  (another ~5 min per gate). Measure which chain sets the pass first.
* Considered: letting `privileged` run 2 at a time (private namespaces should not collide) - not
  done: unproven isolation of the sudo tests, and aio_products alone is 164 s. Worth a look.
* Considered: sharing build between the two passes - no: features differ, binaries differ.
* Resources: up to 24 test processes instead of ~1-2 busy; peak 11-12G under the 12G gate limit
  (page cache included, oom_kill=0 in all runs; the cargo-test run peaked 9.8G). Nothing per
  process/thread in the library.
* SATURATION: none (not a library row).

## Next step
Coordinator: merge; install `$S/work/new/test-summary`; the 5.10 gate is unaffected (no nextest
there). Optional: install nextest on the continuous-integration machine for its gate-like runs (owner approval needed).
