# Probe service

Black-box probes of the reference C library (see `docs/clean-room.md`). This page joins the
former working notes on the probe service and its admin-namespace reference.

## Probe service (task PR-8, branch process/probesvc)

State: done. `tools/probe/probe` (python3 stdlib) is installed on Zen 1 as `~/bin/probe`; `tools/probe/remote.sh`
runs it from the Windows host. Self-test passes on Zen 1 (7 checks). Not part of `cargo xtask test`.

### For agents (copy 3-4 lines into thread_common)
    tools/probe/remote.sh my_probe.c [--liella '~/w/b'] [--args a b] [--stdin F] [--env K=V] [--timeout S]
      -> compiles once, runs vs glibc and vs Liella (static, conformance/run.sh link line), prints SAME/DIFFERENT
         with exit/signal/stdout/stderr and cache HIT/miss. Exit 0 SAME, 1 DIFFERENT, 2 error. Repeats are free.
    tools/probe/remote.sh --find <words>    search past probes (title = first comment line of the source, functions called)
    tools/probe/remote.sh --show <hash>     a stored result;  --record <hash>  ready-to-paste spec/behaviour block (glibc side)
    Put `--args` last. Default tree is ~/w/nightly; pass your own role tree (`--liella '~/w/b'`; quote the ~).

### How it works (decisions, DECIDE OR ASK)
- Cache `~/probe-cache/<key>/` (machine state, not committed). key = sha256(source, flags, args, stdin, env, timeout).
  `glibc.json` keyed by that alone; `liella-<products hash>.json` additionally by sha256 of liella-crt1.o, libc.a,
  libm.a, libpthread.a, so a new main reruns only the Liella side; compiled binaries cached by (source, flags).
  `index.tsv`: hash, date, title, functions, verdict, Liella commit (`.published`, else git HEAD, else `products-<hash8>`).
- Flags (both sides): `-std=gnu17 -O1 -fno-builtin -fstack-protector-strong` (+ `--cflags`). glibc: dynamic, `-lm -lpthread`.
  Liella: the run.sh line (`-static -nostdlib -I include crt1 probe.c libc.a -lgcc`); only if that does not link, it retries
  with libm.a and libpthread.a. A compile/link failure is a result (status compile-error/link-error, text in stderr).
- Products are built with `cargo xtask so` only if missing or any crates/csys/include/Cargo file is newer than libc.a.
  ~/w/nightly has NO target/liella yet: the first probe against it runs `cargo xtask so` (slow, once).
- Safety: refuses root; cache misses re-exec under `~/bin/shared` (hits skip the queue); each run has a timeout (default
  20 s, kills the process group), RLIMIT_AS 2 GiB (`--mem`), no core, 64 MiB max file; cwd and HOME/TMPDIR are a scratch dir
  `~/probe-tmp/<key16>/wd` (same path both sides, so printed paths compare equal), removed afterwards; env is only
  PATH, LC_ALL=C, LANG=C plus `--env`; argv[0] is `./probe`. Nothing in /tmp, /dev/shm, /dev/mqueue.
- Verdict compares status (exit code / signal / timeout), stdout and stderr bytes. Streams stored up to 1 MiB, shown 2000 bytes.
- Clean room: glibc is only executed as a black box; no source, disassembly or symbols are fetched or shown.

### Open / not done
- A `--liella` path with `~` must be quoted so Zen 1, not the Windows shell, expands it (remote.sh passes `~/..` through).
- Programs that print the stack/ASLR values differ run to run; use `--no-cache` sparingly (it also reruns glibc).
- Next step: coordinator copies the usage lines into thread_common; optionally pre-build ~/w/nightly products.
Full report: this file; record: liella-historic:handoff/reports/PR-8.json.

Affected run: `cargo xtask affected --files ... --run` on Zen 1 ~/w/b escalated to FULL (tools/ is an unknown path): 9257 passed, 0 failed, 80 conformance, 2813 s; summary reported "340 suites (339 ok, 1 failed)" with 0 failed tests, exit ok (not investigated). Suggest affected.rs treat tools/ as policy-only.

## ADMIN-REFERENCE-NS: state

Branch task/admin-reference-ns. Result: NOT REPRODUCED. 4800 loaded runs of `admin_equals_the_reference` on an idle Zen 1 machine, all three
modes (plain, user namespace, PID namespace) executed in every run: 0 failures, failure rate 0/4800 per mode. No product or test change.
Only change: tools/probe/admin_stress.sh drops the harmless "rm: Is a directory" stderr noise of its cold-batch step.

### Run (Zen 1, role r1, scratch in /dev/shm/ans, removed afterwards)
- Idle check before the load: `~/bin/slots-status` "running 0", no other jobrun scope, load average 0.17, no other story on the host.
- Zen 1: kernel.apparmor_restrict_unprivileged_userns=0, `unshare -U --map-root-user` and `unshare -Ufp --map-root-user` work, dmesg_restrict=1.
- Smoke (1 run, 0.34 s): passes, no "namespace runs skipped" line.
- Load inside jobrun's 7 cores: 12 CPU burners + 3 dd burners (512 MB files in /dev/shm) + 12 concurrent test processes, deadline 1500 s.
- Phase A: 3000 warm runs, fails=0, skipped=0. Phase B: 150 batches x 12 = 1800 runs, compiled oracles deleted before each batch (12 processes
  race to build them under the flock), fails=0, skipped=0. Total N=4800 per mode, 0 failures, 0 skipped, longest run 1.22 s (watchdog 60 s).
  Wall 199 s (script finished well inside the 30-minute bound).
- Plain mode is not separable in the test (one test runs all three modes), so the plain figure is the same N=4800 run, 0/4800. the second machine's
  earlier plain-only run was also 0/4800.

### Ranked candidates from ADMIN-REFERENCE-INVESTIGATE-2
1. Kernel stall in a whole-machine syscall under a real-disk dirty backlog: NOT TESTABLE under the owner's rule (stress loops run in RAM,
   never on an SSD role). The only known instance (Q_SYNC) is already fixed in flakes6. Left open, no evidence for a live cause.
2. `cc` ENOSPC on a full /tmp: infrastructure; needs a full disk, not exercised (RAM scratch, 56 GB free).
3. klogctl(READ_ALL) differing between runs: ruled out on Zen 1 too (dmesg_restrict=1; EPERM from both sides).
4. Watchdog 60 s too tight: only a consequence of (1); max run was 1.22 s under load, no near-miss.
No deterministic regression test is possible (no mechanism found); changing the test without one is not allowed.

### RULING NEEDED
- none. Recommendation: close ADMIN-REFERENCE as not reproduced; reopen only on a new CI failure log (then read out/FAIL-*.log for
  "stalled in step" and for "No space left").

### Jobs (Zen 1 ~/logs/jobs.tsv)
ans-build (cargo xtask so), ans-build2 (test binary), ans-one (single smoke), ans-stress (N=4800, rc 0). No gate, plants, counts or
percall: no code under test changed (a comment/stderr redirect in a probe script only).
