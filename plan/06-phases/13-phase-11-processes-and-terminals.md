<!-- Part of the Liella implementation plan; index and reading guide: implementation-plan.md -->

### Phase 11: Processes and terminals

* **Scope:**
  * §18.3–18.9, exec, spawn (and pidfd spawn), pidfd, wait, system/popen, groups and
    sessions, daemon, prctl and friends;
  * §19, terminals and ptys (34 symbols);
  * §17.5, getpass.
* **Symbols:** about 120.
* **Clauses:** about 85.
* **Needs:** P8 (`popen` returns a stream), P6 (fork, spawn, signal reset), P10 (`getlogin`/`getpass` use the passwd database and utmp).
* **Test surface:**
  * The spawn child-error reporting guarantee (spec §18.4), for every file action failing.
  * Signal reset in the child.
  * `execvp` search rules (`EACCES` continuation, the `ENOEXEC` shell fallback).
  * `E2BIG` limits.
  * Wait-status encoding.
  * `system` signal handling with concurrent callers.
  * A pty pair round trip, and `TIOCGPTPEER`.
  * `tcsetattr` partial-application semantics, and arbitrary baud rates internally (D-19.1).
* **Exit:** in-scope clauses and symbols green.
* **Delivered** (exit met, with the deviations listed):
  * **Method (§1.4):** behaviour documents first (`spec/behaviour/process.md`, `terminals.md`, with
    their recorded outputs; privileged cases in private mount namespaces with their own devpts,
    `/proc` and fixture `/etc`), then two fresh sessions implemented the parts from the spec, POSIX,
    the man pages and those documents. `getpass` (§17.5) and `login_tty` came with phase 10.
  * **Processes (§18.3–18.9; D-18.2 to D-18.7, D-24.10):** one exec search; one spawn path
    (`clone3(CLONE_VFORK|CLONE_CLEAR_SIGHAND)` without `CLONE_VM`, the child's error in a shared
    word, every `_np` action and attribute, `pidfd_spawn`); the waits and the status encoding;
    `system` with reference-counted SIGINT/SIGQUIT across concurrent callers and cancellation;
    `popen` with a locked stream list (both locks join fork's order, with a regression test);
    groups, sessions, `daemon`; `prctl`/`ptrace`/`arch_prctl` pass-throughs; `_SC_ARG_MAX` capped
    at 6 MiB.
  * **Terminals (§19; D-19.1 to D-19.4):** `termios2` with integer rates inside and the glibc
    struct at the boundary; `tcsetattr`'s partial application as observed; line and job control;
    `ttyname` through `/proc/self/fd` with validation; ptys with `TIOCGPTPEER`; `forkpty`;
    the ttyent parser.
  * **Tests:** §18: 57 clauses and 77 symbols specified; §19: 25 clauses and 34 symbols (plus
    `sym_revoke`). Differential tests against the host glibc: `process_products` (13 lines, 6
    decisions, 1 ADDED) and `terminals_products` (21 KNOWN, 4 ADDED, 2 DROPPED), each with its
    reason; conformance programs `s18_exec` and `s19_terminals` (variadic exec, `daemon`, `forkpty`,
    cancellation). `process_products` links an interim shim for §13's `chdir`/`fchdir`/`getcwd`/
    `__realpath_chk` (phase 12) into the Liella build only; it goes when those exist.
    `cargo xtask test`: plain, export+selftest, conformance; clippy clean.
  * **Budgets:** `benchmarks/progress.md` entry 9 (`cbench_process`, 15 sanity rows; 61 budgets met
    in three runs). Spawn rows are kernel- and shell-bound (1.1–1.2× glibc); `vfork` is 3× by
    D-18.1's copy-on-write child; the terminal rows are at parity or faster. The P9 review before
    phase 12 fixed `ttyname_r` (4,699 → 3,945 ns, glibc 3,910) and `fork` in a single-threaded
    process (no library locks while the caller is alone, P5); the `CLONE_VM` spawn child is
    deferred as architectural.
  * **Also fixed in this phase:** `s19_4_c_3` raced with other tests on `ptsname`'s process-wide
    buffer (the tests now use `ptsname_r` or hold one lock); the process probe's stopped-and-
    continued case let the child's exit overtake the continued report under either library (the
    child now waits on a pipe; the recorded glibc output is unchanged); the resolver probe read
    its servers' query log after a fixed 50 ms sleep and could miss a line under load (phase 10's
    `dns_products`; it now synchronises with each server through a marker query; output
    unchanged).
  * **Deviations and open items for the owner:**
    * Spec over reference and cleaner choices, recorded in `docs/differences-from-glibc.md`
      (D-18.3, D-18.5, D-19.1 to D-19.4).
    * **Erratum candidate:** §18.6's ⚑ note says `waitid` exposes the full 32-bit exit value; the
      kernel gives 8 bits (`_exit(300)` → 44), and Liella passes on what it gives.
    * **For the owner:** `vfork` is 3× glibc because D-18.1 makes its child copy-on-write; the
      spawn family copies page tables per spawn until the `CLONE_VM` child (D-18.3's target).
    * Untested for lack of privilege or hardware: successful `SETCGROUP`, `setns`/`unshare` into
      real namespaces, the `clone3` `ENOSYS` fallback, real serial lines, `EINTR` in `tcdrain`,
      exhausting the system's ptys.
    * Compat-only versions wait for phase 15: `posix_spawn`/`posix_spawnp@GLIBC_2.2.5`, the 2.2.5
      `openpty`/`forkpty`/`login_tty`; `_IO_proc_open`/`_IO_proc_close` need glibc's internal
      `FILE` layout.
