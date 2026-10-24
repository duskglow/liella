# Processes: exec, spawn, process descriptors, waiting, shell commands, groups and sessions

**Spec:** §18.3 (the exec family, `fexecve`, `execveat`), §18.4 (`posix_spawn`, `posix_spawnp`,
`pidfd_spawn`, `pidfd_spawnp`, the attribute and file-action objects with the `_np` actions),
§18.5 (`pidfd_open`, `pidfd_getfd`, `pidfd_send_signal`, `pidfd_getpid`), §18.6 (`wait`,
`waitpid`, `waitid`, `wait3`, `wait4` and the status macros), §18.7 (`system`, `popen`, `pclose`),
§18.8 (`getpgid`, `setpgid`, `getpgrp`, `setpgrp`, `getsid`, `setsid`, `daemon`), §18.9 (`prctl`,
`personality`, the `ptrace` return convention). §18.1-18.2 are context only. Public interfaces:
`<unistd.h>`, `<spawn.h>`, `<sys/pidfd.h>`, `<sys/wait.h>`, `<stdio.h>`, `<stdlib.h>`,
`<sys/prctl.h>`, `<sys/personality.h>`, `<sys/ptrace.h>`. Normative references (plan §1.2):
POSIX.1-2017 (`exec`, `posix_spawn` and the `posix_spawn_file_actions_*`/`posix_spawnattr_*`
functions, `wait`/`waitid`, `system`, `popen`/`pclose`, `setpgid`, `setsid`), the man pages
`exec(3)`, `posix_spawn(3)`, `system(3)`, `popen(3)`, `daemon(3)`, `pidfd_open(2)`,
`pidfd_getfd(2)`, `pidfd_send_signal(2)`, `wait(2)`, `execve(2)`, `execveat(2)`, `prctl(2)`,
`personality(2)`, `ptrace(2)`.

**Clauses:** `s18_3_c_0`, `s18_3_c_1`, `s18_3_c_2`, `s18_3_c_3`, `s18_3_c_4`, `s18_3_c_5`,
`s18_3_g_0`, `s18_3_f_0`, `s18_4_c_0`, `s18_4_c_1`, `s18_4_c_2`, `s18_4_c_2_1`, `s18_4_c_2_2`,
`s18_4_c_2_3`, `s18_4_c_2_4`, `s18_4_c_2_5`, `s18_4_c_2_6`, `s18_4_g_1`, `s18_4_g_2`, `s18_4_g_3`,
`s18_4_f_0`, `s18_5_c_1`, `s18_5_c_2`, `s18_5_c_3`, `s18_5_c_4`, `s18_5_f_0`, `s18_6_c_0`,
`s18_6_c_1`, `s18_6_c_2`, `s18_6_c_3`, `s18_6_c_4`, `s18_6_g_0`, `s18_6_f_0`, `s18_6_x_0`,
`s18_7_c_1`, `s18_7_c_2`, `s18_7_t_0`, `s18_8_c_1`, `s18_8_c_2`, `s18_8_c_3`, `s18_8_c_4`,
`s18_8_c_5`, `s18_9_c_1`, `s18_9_c_2`, `s18_9_c_5`.

**Probe:** `crates/core/tests/oracle/process_diff.c`, run by `process_run.sh PROGRAM SCRATCH`.
**Record:** `process.glibc.txt` (625 lines). It was made on the VM (Ubuntu 24.04, glibc
2.39-0ubuntu8.9, kernel 6.8) with `cc -O0 -o p process_diff.c` and
`env -i PATH=/usr/bin:/bin sh process_run.sh p SCRATCH`, twice, with different scratch
directories: the two outputs are byte-identical (and identical to a `-std=gnu17 -O1` build and to
a run under an ordinary login environment).

The harness has three phases. The first runs unprivileged and writes its fixture (scripts with and
without `#!`, a non-executable file, a directory named like a program, a symbolic-link loop, a
script whose interpreter does not exist, a mode-000 file, a FIFO) into the scratch directory. The
second (`root`) runs as root in a private mount namespace (`sudo -n unshare -m`), from a copy of
the program on a tmpfs mounted over `/mnt` inside the namespace (so that a child with real UID
65534 can execute it): only `POSIX_SPAWN_RESETIDS` and a real-time policy need it. The third
(`noshell`) runs in another private mount namespace with a non-executable file bind-mounted over
`/bin/sh`, to see `system`, `popen` and the `execvp` shell fallback when no shell can be executed.
The mounts vanish with the namespaces; nothing on the system is read or written.

## What the probe does

The program runs copies of itself as children (a first argument starting with `@` selects a helper
mode). The `@report` helper prints one `report:` line describing the process it runs in: blocked,
pending, ignored and caught signals (1-64), open descriptors 0-63, the working directory, whether
its process group and session are its own, the parent's, or another, whether real and effective
IDs agree (or the IDs themselves in the root phase), and its scheduling policy/priority. Every
scratch path is printed as `<dir>`; nothing prints a PID, a descriptor number that depends on the
environment, or a time. At start the probe closes descriptors above 2, resets every disposition,
clears its mask, sets `PATH=/usr/bin:/bin`, makes itself a child subreaper (so that orphans of its
children come back to it) and changes to the scratch directory. Every child has a parent-death
signal and an alarm, so a failing run leaves nothing behind.

* **Exec** (`== exec search`, `== exec limits`, `== fexecve, execveat`, `== exec keeps`): each case
  runs in a forked child whose output is captured (`label: | line`, then `label: <wait status>`;
  `returned errno=N` with exit 99 means the call returned). `execvp`/`execvpe`/`execlp` over
  PATH lists combining every failure kind before and after a good entry; the `ENOEXEC` shell
  fallback (what the shell gets as `$0` and arguments); the PATH of `execvpe`; PATH unset, empty,
  with empty, `.` and relative elements; names with a slash, empty, longer than `NAME_MAX`, a PATH
  element longer than `PATH_MAX`; `ETXTBSY`; every errno of the non-`p` forms; the `l` and `e`
  forms. `E2BIG` against `MAX_ARG_STRLEN` and against a quarter of `RLIMIT_STACK` (argv and
  environment), and `sysconf(_SC_ARG_MAX)` under several stack limits. `fexecve` on regular,
  `O_PATH`, close-on-exec, invalid, directory and script descriptors; `execveat` with each flag
  and descriptor kind. What `execv` keeps (mask, pending signal, ignored and caught dispositions,
  descriptors with and without close-on-exec, cwd, PID).
* **Spawn file actions** (`== spawn file actions`): the errors the `add*` functions return when
  an action is added; then one spawn per action that fails in the child (each line says whether a
  child was left to reap and whether a descriptor leaked); the order of actions (open/dup2/close,
  chdir before and after a relative open, chdir before a relative program path, closefrom before
  dup2); `adddup2(fd, fd)` on a close-on-exec descriptor; `closefrom`; `fchdir`; redirecting
  descriptor 1; reusing one action list.
* **Spawn exec failures** (`== spawn exec failures`): every exec error through `posix_spawn`, the
  `posix_spawnp` search (the same PATH cases as `execvp`), `pid == NULL`, spawning with `SIGCHLD`
  ignored.
* **Spawn attributes** (`== spawn attributes`): the defaults of a new attribute object and what
  each setter accepts; signal mask and dispositions in the child with and without `SETSIGMASK`
  and `SETSIGDEF`; `SETPGROUP` (0, an existing group, a missing group); `SETSID` alone and with
  `SETPGROUP`; `SETSCHEDULER`/`SETSCHEDPARAM` (including a caller running `SCHED_BATCH`);
  `USEVFORK`; `RESETIDS`; `SETCGROUP` errors. The window between clone and exec: the child is held
  in a file action (an open of a FIFO) while another thread signals the process group; whether the
  parent's handler ran in the child, and what the child gets after exec. 100 spawns from four
  threads at once.
* **`pidfd_spawn`, `pidfd_spawnp`** (`== pidfd_spawn`): the descriptor, `waitid(P_PIDFD)`,
  failures (is `*pidfd` written, is a descriptor left open), search, `SETSID` with a signal
  through the pidfd.
* **`addtcsetpgrp_np`** (`== tty actions`): in a child that is a session leader with a pty as its
  controlling terminal, with and without `SETPGROUP`, on a terminal that is not the controlling
  one, and on the master side.
* **Process descriptors** (`== pidfd`): `pidfd_open` on a running child, a zombie, a reaped child,
  the caller, a thread, missing PIDs and invalid flags; `poll`; `pidfd_getfd`;
  `pidfd_send_signal` with flags, bad signals, a `siginfo_t`; `pidfd_getpid` on non-pidfds;
  `PIDFD_NONBLOCK` with `waitid`.
* **Waiting** (`== wait`): the raw status and the macros for exit codes 0-300 and -1, for deaths
  by signal (non-dumpable children, so no core file is written whatever the system's core
  pattern), stop and continue; `waitid`'s `si_code`/`si_status` for the same; `WNOWAIT`,
  `WNOHANG`, `WUNTRACED`, `WCONTINUED`; invalid options and id types; process-group selection;
  `ECHILD` cases; `wait3`/`wait4` usage; `EINTR` with and without `SA_RESTART`; `SIGCHLD`
  ignored.
* **`system`** (`== system`): `system(NULL)`, statuses, the shell's `$0`, PATH and `SHELL`; the
  caller's and the shell's blocked and ignored signals while `system` runs (read from
  `/proc/PID/status` by the shell); signals sent to the caller during the call; `SIGCHLD`
  delivery after the call; a caller that already ignores `SIGINT`; `SIGCHLD` ignored; other
  children; two threads in `system` at once (the dispositions while one is still running).
* **`popen`/`pclose`** (`== popen`): read and write streams, statuses, close-on-exec with and
  without `e`, every mode string, the descriptors a popen child inherits while other popen
  streams are open, `pclose` of an `fopen` stream, `pclose` with `SIGCHLD` ignored, `popen` with
  descriptors 0 and 1 closed.
* **Groups and sessions** (`== groups`): `getpgid`, `getsid` (also for other sessions and missing
  PIDs); every `setpgid` error; `setsid`, `setpgrp` in children (as session and group leaders).
  `daemon(nochdir, noclose)` for the four combinations (the daemon reports through a pipe left
  open on descriptor 9), and whether the exiting parent flushes stdio or runs `atexit` handlers.
* **`prctl`, `personality`, `ptrace`**: the return conventions of a few options (the result as
  the return value, through a pointer, `EINVAL`), `personality` query and set, and the
  `PTRACE_PEEKDATA` return convention on the probe's own traced child.
* **`root` phase**: `RESETIDS` with real UID/GID 65534 and effective 0; `SCHED_FIFO`.
* **`noshell` phase**: `system(NULL)`, `system`, `popen`/`pclose`, the `execvp` fallback and
  `posix_spawn("/bin/sh")` with no executable shell.

## Observations the record shows directly

### Exec (§18.3)

* PATH search (`execvp ...` lines): `EACCES` (also a directory with the program's name) is
  remembered and the search continues; if nothing later succeeds the call fails with `EACCES`
  even when later entries were `ENOENT` (`EACCES then missing dir`, `missing dir then EACCES`).
  `ENOENT` (also from a `#!` interpreter that does not exist) and `ENOTDIR` (a PATH element that
  is a file) continue the search; alone they are the error (`file as PATH element only: 20`).
  `ELOOP`, `ETXTBSY`, `ENAMETOOLONG` (a name over `NAME_MAX`, or an element plus name over
  `PATH_MAX`) and `E2BIG` stop the search at once and are returned even when a later entry would
  succeed (`symlink loop then ok: 40`, `ETXTBSY then ok: 26`, `PATH element longer than PATH_MAX
  then ok: 36`, `E2BIG execvp: 7`).
* `ENOEXEC` fallback: the first PATH match that the kernel rejects with `ENOEXEC` is run with
  `/bin/sh`, and the search does not go on (`ENOEXEC first then ok` runs the no-`#!` file). The
  shell gets the found path (as built from the PATH element, or the name as given when it has a
  slash: `argv0=bin_plain/plain`) as `$0` and the caller's `argv[1..]` as its arguments; the
  caller's `argv[0]` is dropped, and an empty `argv` gives no arguments (`argc=0`). `execvpe`
  passes its `envp` to that shell (`X=from-envp`). The non-`p` forms (`execv`, `execve`,
  `execle`) and `fexecve` return `ENOEXEC` (8).
* `execvpe` searches the caller's `PATH`, not the one in `envp`; with the caller's `PATH` unset it
  uses the default and ignores `envp`'s (`ENOENT`).
* `PATH` unset: the default list finds `sh` and does not include the working directory
  (`cwd not searched: 2`). `PATH=""` searches the working directory, as do a leading, trailing
  or doubled `:`; a relative element is used as given (`argv0=bin_ok/prog`).
* A name with a slash is executed as given, relative to the working directory; an empty name is
  `ENOENT`. `execv` with a name without a slash runs it from the working directory (no search).
  `execv("")` is `ENOENT`. The non-`p` forms give the kernel's errors unchanged: `EACCES` (also
  a directory), `ENOENT`, `ENOTDIR`, `ELOOP`, `ENOENT` for a missing interpreter, `ETXTBSY`.
* `E2BIG`: one string of 131071 characters (131072 bytes with its NUL) passes, 131072 characters
  fail. Totals are limited to a quarter of `RLIMIT_STACK` for argv and the environment alike
  (200 KB passes and 300 KB fails under 1 MiB; 1.5 MB passes and 2.5 MB fails under 8 MiB), but
  never below 128 KiB (100 KB passes under a 256 KiB stack). `sysconf(_SC_ARG_MAX)` reports the
  same: the stack limit / 4, at least 131072, and 6291456 when the stack is unlimited.
* `fexecve` works on `O_RDONLY` and `O_PATH` descriptors and honours `envp`. A script through a
  descriptor sees `/dev/fd/N` as `$0`; when that descriptor is close-on-exec the call fails with
  `ENOENT`. Not executable or a directory: `EACCES`; a closed descriptor: `EBADF`; `fd == -1`:
  `EINVAL` (not `EBADF`).
* `execveat`: a relative path resolves against the directory descriptor (a script there sees
  `/dev/fd/N/prog` as `$0`); `""` without `AT_EMPTY_PATH` is `ENOENT`, with it the descriptor's
  file runs; `AT_SYMLINK_NOFOLLOW` on a link is `ELOOP`; an unknown flag is `EINVAL`; a bad
  descriptor is `EBADF` with a relative path and ignored with an absolute one.
* What exec keeps (`exec keeps`): the PID, the mask and a pending blocked signal, ignored
  dispositions, the working directory and descriptors without close-on-exec; caught signals become
  default and close-on-exec descriptors are closed.

### Spawn (§18.4)

* When an action is added: a negative descriptor, or one at or above the descriptor limit
  (100000, `INT_MAX`), is `EBADF` for `addopen`, `addclose`, `adddup2` (either side),
  `addclosefrom` and `addtcsetpgrp_np`. `addfchdir_np(-1)` and `addchdir_np("")` are accepted;
  they fail in the child (`EBADF`, `ENOENT`).
* **Child errors are the return value, for every failing action** (`fail ...` lines): `addopen`
  (`ENOENT`, `EISDIR`, `EACCES`, `EEXIST`), `adddup2` from a closed descriptor (`EBADF`, also with
  `fd == newfd`), `addchdir` (`ENOENT`, `ENOTDIR`), `addfchdir` (`EBADF`, `ENOTDIR`),
  `addtcsetpgrp_np` (`ENOTTY` on a non-terminal, `EBADF`). In every case no child is left to reap
  and no descriptor leaks. Actions before the failing one have taken effect (`created1 exists:
  yes`), also when the exec itself fails.
* `addclose` of a descriptor that is not open is not an error: the spawn succeeds (`fail addclose
  not open: r=0`), and closing one descriptor twice is accepted.
* Actions apply in order: a relative `addopen` after `addchdir` resolves in the new directory, one
  before it in the old one (`ENOENT`); a relative `addchdir` resolves against the caller's
  directory. The program path itself is resolved after the actions: a relative path runs from the
  directory set by `addchdir`, and `posix_spawnp` with `PATH=.` finds it there (`argv0=./prog`).
  `closefrom` then `dup2` of a descriptor it closed is `EBADF`.
* `adddup2(fd, fd)` clears close-on-exec in the child only (`parent fd 7 still cloexec: yes`).
  `addclosefrom(3)` leaves 0-2; `addclosefrom(8)` keeps 7; an `addopen` after `addclosefrom`
  works. An action list can be used for several spawns.
* Exec failures through `posix_spawn`: `ENOENT` (also an empty path and a missing interpreter),
  `EACCES` (also a directory), `ENOEXEC` (8: no shell fallback), `ELOOP`, `ENOTDIR`, `E2BIG`. All
  without a child left over.
* `posix_spawnp` search: as `execvp` for `EACCES` (continue, report `EACCES` last), `ENOENT`,
  `ENOTDIR` and a missing interpreter (continue), `ELOOP` (stop); the caller's `PATH`, not
  `envp`'s; the default when unset (no working directory); `""` and empty elements search the
  working directory. Unlike `execvp`, `ENOEXEC` is returned (8) and stops the search (`spawnp no
  #! line first then ok: r=8`).
* `pid` may be `NULL`. With `SIGCHLD` ignored, errors are still returned, the child inherits the
  ignored disposition (`ignored=[17]`) and is reaped automatically (`waitpid` then gives
  `ECHILD`).
* Attribute defaults: flags 0, pgroup 0, empty `sigmask` and `sigdefault`, policy 0, priority 0,
  cgroup 0. `setflags` accepts any combination of the nine defined bits (0x1ff) and refuses others
  with `EINVAL`, leaving the flags unchanged. `setpgroup(-5)`, `setschedparam` with priority 1000,
  `setcgroup_np(-1)` and sets containing `SIGKILL` are accepted without checks.
  `setschedpolicy` accepts `SCHED_OTHER`, `SCHED_FIFO` and `SCHED_RR` only (`SCHED_BATCH`,
  `SCHED_IDLE`, 6 and -1 are `EINVAL`).
* Signals in the child: without `SETSIGMASK` it has the caller's mask; ignored dispositions are
  inherited and caught ones are default. `SETSIGMASK` replaces the mask; `SETSIGDEF` makes the
  listed ignored signals default (a full set, including `SIGKILL`/`SIGSTOP`, is accepted and
  clears every ignored one). A full `SETSIGMASK` set gives a mask of everything except 9, 19, 32
  and 33.
* Between clone and exec (`window`): a signal sent to the process group while the child is held
  in a file action runs the parent's handler only in the parent (`handler_in_other_process=0`).
  The child gets the signal after exec, with its default action (killed by 12); with `SETSIGMASK
  {12}` it is still pending after exec (`pending=[12]`).
* `SETPGROUP 0` makes the child a group leader; `SETPGROUP` with another child's group joins it;
  a group that does not exist is `EPERM`. `SETSID` makes the child leader of a new session and
  group. `SETSID|SETPGROUP` (pgroup 0 or the caller's group) is `EPERM`.
* Scheduling: `SETSCHEDULER` with `SCHED_OTHER`/0 works; `SCHED_OTHER` with priority 5 and
  `SCHED_FIFO` with priority 0 are `EINVAL`; `SCHED_FIFO`/`SCHED_RR` with priority 1 are `EPERM`
  unprivileged and work as root (`sched=1/1`). `SETSCHEDPARAM` alone keeps the caller's policy (a
  caller running `SCHED_BATCH` gives a `SCHED_BATCH` child), and a priority the policy does not
  allow is `EINVAL`. `SETSCHEDULER` with `SCHED_OTHER` takes a `SCHED_BATCH` caller's child back to
  `SCHED_OTHER`.
* `USEVFORK` changes nothing. `RESETIDS` changes nothing when real and effective IDs agree; as
  root with real 65534 and effective 0, the child keeps effective 0 without it and gets 65534
  (user and group) with it.
* `SETCGROUP`: descriptor -1 is `EINVAL`; a directory that is not a cgroup, or a closed
  descriptor, is `EBADF`.
* 100 concurrent spawns from four threads all succeed.
* `pidfd_spawn`: the pidfd is the lowest free descriptor and close-on-exec; `waitid(P_PIDFD)`
  reaps through it and its `si_pid` equals `pidfd_getpid`. On failure the error is returned and
  `*pidfd` is not written. `pidfd_spawnp` searches as `posix_spawnp`.
* `addtcsetpgrp_np` (`tty:` lines): with `SETPGROUP 0` the child's new group is the terminal's
  foreground group when it runs (and stays so after it exits); without `SETPGROUP` the child is
  in the caller's group, which is foreground. A terminal that is not the controlling terminal is
  `ENOTTY`; the master side of the controlling pty is accepted.

### Process descriptors (§18.5)

* `pidfd_open` gives a close-on-exec descriptor; `poll` is 0 while the process runs and `POLLIN`
  once it has exited, before it is reaped. Flags other than `PIDFD_NONBLOCK`, PID 0, PID -1 and a
  thread that is not a group leader are `EINVAL`; a missing PID is `ESRCH`. With
  `PIDFD_NONBLOCK` the descriptor is non-blocking and `waitid(P_PIDFD, WEXITED)` on a running
  child is `EAGAIN` (with `WNOHANG` it returns 0 and `si_pid` 0).
* `pidfd_getpid` gives the PID for a running process and a zombie, and -1 with `ESRCH` once the
  process is reaped; a descriptor that is not a pidfd, -1 or a closed one is `EBADF`.
* `pidfd_send_signal`: signal 0 tests existence; a zombie still accepts signals (0); a reaped
  process is `ESRCH`; flags or signal 99 are `EINVAL`; a `siginfo_t` with `SI_USER` sent to
  another process is `EPERM`, with `SI_QUEUE` it is delivered; to the caller with the signal
  blocked it becomes pending. A non-pidfd is `EBADF`.
* `pidfd_getfd` duplicates the target's descriptor (same file, close-on-exec set); a descriptor
  the target does not have, or a bad pidfd, is `EBADF`; flags are `EINVAL`.
* `waitid(P_PIDFD)` on a reaped process is `ECHILD`, on a non-pidfd `EBADF`.

### Waiting (§18.6)

* Raw status: exit code `c` gives `(c & 0xff) << 8` (`_exit(256)` 0x0, `_exit(300)` 0x2c00,
  `_exit(-1)` 0xff00); death by signal `s` gives `s` (core bit clear for a non-dumpable process),
  including the real-time signals 34 and 64; stopped by 19 gives 0x137f; continued gives 0xffff.
  The macros decode each.
* `waitid`: `si_signo` `SIGCHLD`, `si_pid` and `si_uid` of the child; `CLD_EXITED` (1) with only
  the low 8 bits of the exit code in `si_status` (300 gives 44, 0x12345 gives 69: see the
  section below), `CLD_KILLED` (2) with the signal, `CLD_STOPPED` (5) with 19, `CLD_CONTINUED`
  (6) with 18.
* `WNOWAIT` leaves the child waitable; a stop is reported once; without `WUNTRACED` a stopped
  child gives 0 under `WNOHANG`. `WNOHANG` on a running child: `waitpid` returns 0; `waitid`
  returns 0 with `si_pid` and `si_signo` zero (the caller's buffer was pre-filled with 0xff).
* `EINVAL`: unknown `waitpid` option bits, `waitid` with no state option or with `WNOHANG` alone,
  an unknown id type.
* Selection: `waitpid(0)` and `waitid(P_PGID, 0)` mean the caller's group; `-pgid` and `P_PGID`
  another group; `P_ALL` ignores the id. `ECHILD` for no children, a PID that is not a child,
  a group with no children, for `wait`, `waitid` (also with `WNOHANG`) and `wait4`. After a child
  is reaped, `kill(pid, 0)` is `ESRCH`.
* `wait4` reports the child's CPU time; `wait3` fills `ru_maxrss`; a NULL usage pointer is
  accepted; a NULL status pointer is accepted.
* A caught signal without `SA_RESTART` interrupts `waitpid` and `waitid` with `EINTR`; with
  `SA_RESTART` `waitpid` resumes and returns the child.
* With `SIGCHLD` ignored, children are reaped by the system and `waitpid`/`wait` give `ECHILD`.

### `system`, `popen`, `pclose` (§18.7)

* `system(NULL)` is 1. Statuses are wait statuses (`exit 3` 0x300, `""` 0, a shell killed by
  `SIGTERM` 0xf, a missing command 127). The shell is `/bin/sh` by absolute path whatever `PATH`
  and `SHELL` say; it sees `$0 = sh` and no positional parameters.
* While `system` runs, the caller (as the shell reads `/proc/$PPID/status`) has `SIGCHLD` blocked
  (`SigBlk: 0000000000010000`) and `SIGINT`/`SIGQUIT` ignored (`SigIgn: 0000000000000006`);
  the shell has nothing blocked and nothing ignored. `SIGINT` and `SIGQUIT` sent to the caller
  during the call are discarded (`int=0 quit=0`); a `SIGCHLD` is delivered once after the call
  returns (`chld=1`); the caller's handlers and mask are back afterwards.
* A caller that already ignores `SIGINT` and blocks `SIGUSR1`: the shell has `SIGINT` ignored and
  `SIGUSR1` blocked, and `SIGINT` is still ignored afterwards.
* With `SIGCHLD` ignored, `system` returns -1 with `ECHILD`. `system` does not reap other
  children.
* Two threads in `system` at once: when the shorter call returns, `SIGINT` and `SIGQUIT` stay
  ignored while the longer one runs, and the handlers come back after both. The main thread's
  mask does not have `SIGCHLD` while another thread is in `system`.
* `popen` `"r"` and `"w"` work and `pclose` returns the wait status (`exit 4`, killed by 15).
  The descriptor is close-on-exec with `e` (`"re"`, `"we"`), not without it; `"er"` is accepted
  as a read stream. The mode strings `""`, `"x"`, `"rw"`, `"wr"`, `"r+"`, `"w+"`, `"rb"`, `"wb"`, `"ee"`, `"rx"`,
  `"rew"`, `"a"` give NULL with `EINVAL`. The command runs as `sh -c` with `$0 = sh`.
* A popen child does not inherit the pipes of the caller's other popen streams (it has 0, 1, 2
  only), but does inherit the caller's other descriptors (7). With descriptors 0 and 1 closed,
  `popen("r")` returns a stream on descriptor 0 and `popen("w")` one on descriptor 1, and both
  work.
* `pclose` with `SIGCHLD` ignored is -1 with `ECHILD`.
* With no executable shell (`noshell:` lines): `system(NULL)` is 0, `system(cmd)` returns status
  127 (0x7f00), `popen` returns NULL with `EACCES`, the `execvp` fallback fails with `EACCES` (the
  error of executing the shell), and `posix_spawn("/bin/sh")` returns `EACCES`.

### Groups and sessions (§18.8)

* `getpgid(0)` is `getpgrp()`; `getsid` and `getpgid` work for processes in other sessions
  (PID 1); a missing PID is `ESRCH`.
* `setpgid`: a child can be moved to its own group, back, and with `pgid == 0`. `pgid < 0` or
  `pid < 0` is `EINVAL`; a group that does not exist is `EPERM`; a process that is not a child
  (PID 1) or does not exist is `ESRCH`; a child that has executed a program is `EACCES`; a child
  that is a session leader, or in another session, is `EPERM`.
* `setsid` in a child returns its PID and makes it session and group leader; again it is `EPERM`;
  `setsid` in a group leader is `EPERM`. A session leader cannot change its group (`setpgid(0,0)`,
  `setpgrp`, joining another session's group: `EPERM`). `setpgrp` in an ordinary child makes it a
  group leader, and `setpgid(0, parent's group)` in the same session moves it back.
* `daemon(nochdir, noclose)`: the caller's process exits with status 0; the daemon is a new
  process (reparented to the subreaper), leader of a new session and group, returns 0 with `errno`
  untouched, has `/` as its directory unless `nochdir`, and descriptors 0-2 on `/dev/null`
  (opened read-write) unless `noclose`. Other descriptors stay open.

### `prctl`, `personality`, `ptrace` (§18.9)

* `prctl` returns query results as the return value (`PR_GET_DUMPABLE` 1, then 0 after
  `PR_SET_DUMPABLE 0`; `PR_GET_NO_NEW_PRIVS` 1 once set) or through the pointer argument
  (`PR_GET_PDEATHSIG` gives 10). `PR_SET_NAME` truncates to 15 bytes. Invalid values, an unknown
  option and a nonzero unused argument of `PR_SET_NO_NEW_PRIVS` are `EINVAL`. The subreaper
  attribute is not inherited by a forked child.
* `personality(0xffffffff)` returns the current persona without changing it; setting returns the
  previous one (0, then 0x40000, then 8); any value, including 0x7fffffff, is accepted.
* `ptrace(PTRACE_PEEKDATA)` returns the word itself: a word of all ones gives -1 with `errno`
  0; a bad address gives -1 with `EIO`, as does a misaligned `PTRACE_PEEKUSER`.

## Observations that look like historical artifacts

The implementer may decide these differently (and record the difference).

* `pidfd_spawn` leaves one pidfd open in the caller when it fails, in a file action or at exec
  (`extra_open_fds=1`, `of which pidfds: 1`), although `*pidfd` is not written and the caller has
  no way to find it. `posix_spawn` failures leak nothing. (The `pidfd_spawnp` failure case was
  not counted.)
* `execvp` runs a no-`#!` file with the shell, but `posix_spawnp` returns `ENOEXEC` for the same
  file and stops searching (the man page describes the shell fallback only for exec).
* The set of errors that stop the `execvp` search (`ELOOP`, `ETXTBSY`, `ENAMETOOLONG`, `E2BIG`)
  versus those that continue it (`ENOENT`, `ENOTDIR`, `EACCES`, remembered) is historical
  practice (exec(3) VERSIONS); `ELOOP` stopping while `ENOTDIR` continues has no functional
  reason.
* `pclose` of a stream that `popen` did not return (`pclose of an fopen stream: 0 errno=0`)
  succeeds and reports 0 instead of failing (the stream is not used afterwards).
* `popen` accepts `"er"` but refuses `"ee"`, `"rx"` and `"rew"`: the mode string is checked in a
  way that depends on the order of its letters.
* `addfchdir_np(-1)` and `addchdir_np("")` are accepted when added while every other invalid
  descriptor is refused at that point; `setpgroup`, `setschedparam`, `setcgroup_np` do no
  checking; `setschedpolicy` refuses `SCHED_BATCH` and `SCHED_IDLE`, which the kernel supports,
  so a spawn can only select them by inheriting them. The default cgroup descriptor is 0, a
  valid descriptor number.
* `SETSID` together with `SETPGROUP` always fails with `EPERM`, because the new session leader
  cannot then change its group.
* `fexecve(-1, ...)` is `EINVAL` while a closed descriptor is `EBADF`.
* `system(NULL)` without an executable shell returns 0 and leaves `errno` at `EACCES`;
  `system(cmd)` returns status 127 and also leaves `errno` set although the call succeeded.
* `system` with `SIGCHLD` ignored fails with `ECHILD` after running the command (POSIX allows
  this; it is a consequence of the system reaping the child).
* `daemon`'s exiting parent neither flushes stdio buffers nor runs `atexit` handlers (`pipe got
  "XY"`, `atexit handlers ...: ""`): unflushed output is written once, by the daemon.
* `addtcsetpgrp_np` on the master side of the controlling pty succeeds (the kernel applies it to
  the terminal); the foreground group stays the child's group after the child exits.

## What the record does not settle

* §18.6's ⚑ note says `waitid` exposes the full 32-bit exit value in `si_status`; on this kernel
  `si_status` carries only the low 8 bits (`_exit(300)` gives 44). The kernel decides this, so
  the spec note is a candidate for `spec/ERRATA.md`; the record settles what the reference shows
  on Linux 6.8.
* The successful `POSIX_SPAWN_SETCGROUP` path: it needs a writable cgroup, and the probe does not
  create cgroups on the test machine. Only its errors are recorded.
* `RESETIDS` with set-user-ID or set-group-ID executables, and with only the group IDs differing.
* Whether the child's mask holds the implementation's reserved signals 32 and 33: they never
  appear in a reported set (the reference's `sigprocmask` does not report them), so the record
  cannot say.
* Signals arriving in the clone-to-exec window other than through a blocking file action, and
  whether caught dispositions are reset before or after the actions run.
* The exact `E2BIG` boundaries for totals (the per-pointer accounting); the record uses wide
  margins.
* Cancellation points (§18.6 "Cancellation: yes", `system`, `pclose`) and `system`'s behaviour
  when a caller thread is cancelled.
* `fexecve` on kernels without `execveat` (the `/proc/self/fd` path the spec mentions): the test
  kernel has `execveat`.
* `daemon` reacquiring a controlling terminal (no second fork) and the rest of job control:
  the terminals document (§19).
* `unshare`, `setns`, `arch_prctl` and the rest of `prctl`/`ptrace` are thin pass-throughs and are
  not probed; `PR_SET_PDEATHSIG` delivery is not probed beyond its get/set.
* The `sysconf(_SC_ARG_MAX)` line for an unlimited stack depends on the hard limit of the
  environment (unlimited on the test machine; the probe prints a different line otherwise).
