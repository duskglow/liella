# POSIX asynchronous I/O

**Spec:** §11.11 (`aio_read`, `aio_write`, `aio_fsync`, `aio_error`, `aio_return`, `aio_suspend`,
`aio_cancel`, `lio_listio`, `aio_init` and the `64` forms), with decision row D-11.8 of
implementation-plan.md (Liella runs POSIX AIO on io_uring where available and permitted, with a
worker pool as the fallback, and keeps completion order per descriptor and priority). The record
is therefore about the observable contract: results, error codes and where they are reported,
ordering, which notifications arrive, in which thread and with which `si_code`. It says nothing
about the reference's helper threads except where their existence shows through a public
interface (fork, `RLIMIT_NPROC`), and those lines are marked as such. Public interfaces: `<aio.h>`,
`<signal.h>`, `<pthread.h>`, `<unistd.h>`. Normative references (plan §1.2): POSIX.1-2017
(`aio_read`, `aio_write`, `aio_fsync`, `aio_error`, `aio_return`, `aio_suspend`, `aio_cancel`,
`lio_listio`, and the AIO sentences of `close`, `fork` and the `exec` functions), the man pages
`aio(7)`, `aio_read(3)`, `aio_write(3)`, `aio_fsync(3)`, `aio_error(3)`, `aio_return(3)`,
`aio_suspend(3)`, `aio_cancel(3)`, `lio_listio(3)`, `aio_init(3)`, `sigevent(3type)`, and the
glibc manual (Asynchronous I/O and its five subsections).

**Clauses:** `s11_11_c_0`, `s11_11_g_0`, `s11_11_f_0`.

**Probe:** `crates/core/tests/oracle/aio_diff.c`, run by `aio_run.sh PROGRAM SCRATCH`.
**Record:** `aio.glibc.txt` (460 lines). It was made on the VM (Ubuntu 24.04, glibc
2.39-0ubuntu8.9, kernel 6.8.0-146-generic, 16 CPUs, scratch on ext4) with
`cc -std=gnu17 -O1 -o p aio_diff.c` and `env -i PATH=/usr/bin:/bin sh aio_run.sh p SCRATCH`, as
the ordinary user `claude` (the record's first line confirms `running as root: no`). On
2026-10-04 it was recorded **twice under background load** (eight `while :; do :; done` loops at
nice 10, load average rising to 6.6 during the runs) with scratch directories of different names
and lengths (`~/aioprobe/run-a`, `~/aioprobe/run-second-longer-name`), then once more under the
same load from an `-O0` build, and once more on the idle machine. All four outputs are
byte-identical (SHA-256 `20138f36...92bdd42`). A run takes about 30 seconds.
The probe undefines `_FORTIFY_SOURCE` (on by default in Ubuntu's compiler at `-O1`), so the plain
functions are called and not their `__*_chk` forms. It needs no privileges and no environment;
nothing in it depends on the locale.

## What the probe does

Every section runs in its own forked child with its own `alarm`, so each starts with no AIO state
in the process, a crash ends only that section, and the section's end line says how the child
ended (`-- end NAME: exit 0` or `killed by SIG...`). A crash is therefore part of the record where
it happens. The whole run has a `timeout` in `aio_run.sh`, which also creates and removes the
scratch directory; the probe removes its own files and prints `cleanup: 0 entries left in <dir>`.

Nothing prints a descriptor number, an address, a PID, a time or a path. Every wait is either for
a definite event (polling `aio_error` until it is no longer `EINPROGRESS`, at most 10 s, or
`sigtimedwait` with a bound) or a fixed pause before checking something that cannot change in the
meantime. A request is held **in progress** deterministically by making it a read from an empty
pipe or socket, or a write into a full pipe (the pipe is set to 65536 bytes and the probe waits
until `FIONREAD` shows it full); it completes only when the probe feeds or drains the pipe. Where
the probe needs a request to have *started* (the cancel cases), it pauses 400 ms after submitting
it (`settle()`); see "Timing" below. Each submission line shows `submit=` (the call's result and,
on failure, `errno`), then the final `error=` (`aio_error`) and `return=` (`aio_return`), in that
order. `errno` is printed only for calls that returned -1 (`errno=-` otherwise): on success it is
unspecified.

* **Constants** (`== constants`): the `AIO_*`, `LIO_*`, `SIGEV_*` and `SI_ASYNCIO` values, the
  sizes and field offsets of `struct aiocb`, `struct aiocb64` and `struct aioinit`, the limit
  macros and the `sysconf` values.
* **aio_init** (three sections, each before any other AIO call in its process): small values
  (threads 1, num 1, idle time 0), every field -1, and a second call after requests completed;
  after each, two reads, a write, a two-entry `LIO_WAIT` list and a `SIGEV_THREAD` notification.
* **aio_read on a regular file** of 1000 bytes: offsets inside, straddling, at and past the end,
  `n=0`; the file offset afterwards; `aio_lio_opcode` set to `LIO_WRITE`, `LIO_NOP`, 99; offsets
  -1, `LLONG_MIN`, `LLONG_MAX`, `LLONG_MAX-5`; `aio_nbytes` `SSIZE_MAX+1`, `SIZE_MAX`, `SSIZE_MAX`
  and `INT_MAX+1` (the buffer is 4096 bytes); `aio_reqprio` -1, 0, 1, 20, 21, `INT_MAX`,
  `INT_MIN`; a write-only, a closed, -1, `INT_MAX`, a directory and an `O_PATH` descriptor;
  `aio_read64`.
* **aio_write on a regular file**: a write, a write past the end (hole), `n=0`, the file offset;
  an `O_APPEND` descriptor with offsets 0 and 3; invalid offsets, `n=SSIZE_MAX+1`, `aio_reqprio`
  21 and -1, `aio_lio_opcode=LIO_READ`, read-only and -1 descriptors; `RLIMIT_FSIZE` 1000 with a
  write past it and one straddling it (`SIGXFSZ` ignored); `aio_write64`. Two sections repeat the
  write past the limit with `SIGXFSZ` at its default disposition and blocked in the caller. The
  probe lowers the soft limit only around the request (it would also apply to its own output when
  that is a regular file).
* **Pipes**: offsets 0, 12345, 777 and -1 on a pipe; a read on an empty pipe (held, then fed); an
  empty `O_NONBLOCK` pipe; a 100000-byte write into an empty and into a full `O_NONBLOCK` pipe of
  65536 bytes; a blocking 100000-byte write held while the pipe is full, then drained; end of file;
  `EPIPE` with `SIGPIPE` ignored. Two sections repeat the `EPIPE` write with `SIGPIPE` at its
  default disposition and blocked in the caller.
* **Sockets**: on an `AF_UNIX` stream pair, a write with offset 99, a read with offset 5, a read
  held until the peer sends, end of file after `shutdown`, `EPIPE`, `aio_fsync`; on a datagram
  pair, a 4-byte read of a 10-byte datagram and the next read.
* **aio_fsync**: `O_SYNC`, `O_DSYNC` and invalid operations (0, `O_RDWR`, `O_SYNC|O_APPEND`,
  `O_DSYNC|O_RDWR`, -1, and `O_RSYNC`, which equals `O_SYNC` on Linux); the other fields set to
  garbage; read-only, -1 (with a valid and an invalid operation), closed, `/dev/null` and pipe
  descriptors; a sync on a pipe while a read on that descriptor is held; a write followed at once
  by a sync, 50 times; `SIGEV_SIGNAL`; `aio_fsync64`.
* **aio_error, aio_return**: a zeroed, never-submitted `aiocb`; a held request (both functions
  before completion, then after; `aio_return` twice; `aio_error` after `aio_return`); reuse of an
  `aiocb`; a failing request; the `64` forms on a held request and `aio_error` on the same block.
* **aio_suspend**: a held and a completed request with timeouts 0, 150 ms and none; lists with
  NULL entries, only NULL entries, `nent` 0 and -1; a completed request already `aio_return`ed; a
  zeroed `aiocb`; a `SIGUSR1` handler without and with `SA_RESTART`; `SIGUSR1` blocked; a
  completion while waiting; invalid timeouts (`tv_nsec` -1 and 1000000000, `tv_sec` -1 and
  `LONG_MAX`), each in its own section with a 10 s alarm; `aio_suspend64`.
* **aio_cancel**: -1 and closed descriptors; a descriptor with no requests; a completed request
  (before and after `aio_return`); a descriptor that does not match the request; never-submitted
  `aiocb`s. Then on one pipe: a read held in the kernel (A), two reads queued behind it (B, C,
  each with a `SIGEV_SIGNAL` notification), cancelled one by one and with NULL; a write held with
  the pipe full; a read held on a socket; `aio_cancel64`.
* **lio_listio**: invalid modes; `nent` 0; NULL and `LIO_NOP` entries (a NOP on fd -1); mixed
  reads and writes; an unknown opcode; lists with one failing entry (read on a write-only
  descriptor, read and write on fd -1, `aio_reqprio` 99, offset -1) in both modes; `LIO_NOWAIT`
  with `SIGEV_SIGNAL` (held until a pipe is fed), with a per-entry signal as well, with zero
  entries and with only NULL/NOP entries, with `SIGEV_THREAD`, and with no `sigevent`; `LIO_WAIT`
  with a list `sigevent` and a per-entry one; 5000 one-byte writes; `lio_listio64`. Separate
  sections: lists of 1024, 65536 and 1048576 NULL entries or copies of one NOP `aiocb` (the
  section prints the stack limit, 8 MiB); `LIO_WAIT` interrupted by a `SIGUSR1` handler without
  and with `SA_RESTART`; `nent` -1 and `INT_MIN` in both modes.
* **Notifications**: `SIGEV_NONE`; a zeroed `aio_sigevent` (which is `SIGEV_SIGNAL` with signal
  0); `SIGEV_SIGNAL` with an RT signal and a value, with `sival_ptr`, with `SIGUSR2`, for a failing
  request; whether the signal is pending for the process or for one thread (two threads both
  blocking it); an `SA_SIGINFO` handler unblocked only in the submitting thread; `SIGEV_THREAD`
  (the thread, detach state, signal mask, value, completion state at the call), with attributes
  (4 MiB stack, joinable), for a failing request, three at once. Separate sections: signal 65 and
  -1, `sigev_notify` 99, `SIGEV_THREAD_ID`, `SIGEV_THREAD` with a NULL function.
* **Ordering**: 100 rounds each of eight one-byte writes to an `O_APPEND` file, eight one-byte
  writes to offset 0 of a regular file, four overlapping writes (8, 6, 4, 2 bytes) to offset 0,
  eight one-byte writes to a pipe; 20 rounds of four one-byte reads queued on an empty pipe fed
  one byte at a time; a read of `/dev/zero` while a pipe read is held.
* **fork**: with a request held in the parent (and one complete but not yet returned), the child
  queries, waits for, cancels and submits; and a fork right after a completion with nothing
  pending.
* **exec**: with a read held on one pipe and a 100000-byte write held on another (both with
  `SIGEV_SIGNAL`, the signal blocked), the process execs itself (`/proc/self/exe ... afterexec`,
  an internal mode) and the new image inspects both pipes and the pending signals.
* **close**: a read held (A) and one queued behind it (B) on a pipe's read end, then `close` of
  that descriptor, then data.
* **`*64` interfaces**: a write, `aio_suspend64`, `lio_listio64`, `aio_cancel64`, `aio_fsync64`
  at an offset of 5 GiB, and a read there through `struct aiocb`.
* **RLIMIT_NPROC 1** (the user has other processes, so no thread can be created): every
  submitting function.

## Observations the record shows directly

### Constants and layout

* `AIO_CANCELED` 0, `AIO_NOTCANCELED` 1, `AIO_ALLDONE` 2; `LIO_READ` 0, `LIO_WRITE` 1, `LIO_NOP`
  2, `LIO_WAIT` 0, `LIO_NOWAIT` 1; `SIGEV_SIGNAL` 0, `SIGEV_NONE` 1, `SIGEV_THREAD` 2;
  `SI_ASYNCIO` -4.
* `struct aiocb` and `struct aiocb64` are both 168 bytes with identical offsets (`aio_fildes` 0,
  `aio_lio_opcode` 4, `aio_reqprio` 8, `aio_buf` 16, `aio_nbytes` 24, `aio_sigevent` 32,
  `aio_offset` 128); `struct aioinit` is 32 bytes (`aio_threads` 0, `aio_num` 4, `aio_idle_time`
  24). The non-64 and 64 functions accept each other's blocks (`*64` section, `aio_error on the
  same block through the non-64 function`).
* `AIO_PRIO_DELTA_MAX` is 20 (and `sysconf` says 20); `AIO_LISTIO_MAX` and `AIO_MAX` are not
  defined and `sysconf` returns -1 for both without setting `errno` (no limit).
  `_POSIX_ASYNCHRONOUS_IO` and `_POSIX_PRIORITIZED_IO` are 200809, also through `sysconf`.

### aio_init

* `aio_init` returns nothing and changes no result in the record: with small values, with every
  field -1, and when called a second time after requests completed, reads, writes, lists and
  `SIGEV_THREAD` notifications behave as without it. Its only effect is on concurrency; see
  "Outside the record".

### aio_read, aio_write (§11.11 Contract)

* Transfers happen at `aio_offset` on regular files: a short read at the end of the file returns
  the bytes there (offset 950, `n=100`: 50), reads at or past the end return 0, `n=0` returns 0,
  and nothing beyond the returned count is written to the buffer. A write past the end leaves a
  hole of zeros; `n=0` past the end does not extend the file. The file offset is unchanged by
  either (10 and 0 before, the same after); POSIX leaves it unspecified.
* On an `O_APPEND` descriptor writes go to the end of the file whatever `aio_offset` is.
* `aio_lio_opcode` is ignored by `aio_read` and `aio_write` (`LIO_WRITE`, `LIO_NOP` and 99 still
  read; `LIO_READ` still writes).
* **Where errors are reported.** Only an invalid `aio_reqprio` is refused at submission (-1,
  `EINVAL`): -1, 21, `INT_MAX` and `INT_MIN` fail; 0, 1 and 20 (`AIO_PRIO_DELTA_MAX`) are
  accepted. Everything else is accepted (`submit=0`) and reported through `aio_error`, with
  `aio_return` -1 and `errno` left untouched by `aio_return`:
  * offsets -1, `LLONG_MIN`, `LLONG_MAX`, `LLONG_MAX-5` (read) and `LLONG_MAX-2` (write):
    `EINVAL`;
  * `aio_nbytes` `SSIZE_MAX+1`, `SIZE_MAX`, `SSIZE_MAX`: `EFAULT` (the count cannot describe a
    buffer), not `EINVAL`; `INT_MAX+1` is accepted and reads the whole 1000-byte file;
  * a descriptor that is write-only (read), read-only (write), closed, -1, `INT_MAX` or `O_PATH`:
    `EBADF`; a directory: `EISDIR`;
  * a write past `RLIMIT_FSIZE`: `EFBIG`; a write that straddles it is partial (5 of 10 bytes).
* `SIGXFSZ` and `SIGPIPE` do not reach the application: a write past `RLIMIT_FSIZE`, or to a pipe
  without a reader, fails with `EFBIG` or `EPIPE` and the process survives with the signal at its
  **default** disposition (which would end the process for the same synchronous write), and the
  signal is not pending in the submitting thread when it is blocked there (`SIGXFSZ default`,
  `SIGXFSZ blocked`, `SIGPIPE default`, `SIGPIPE blocked`). (The stream-socket `EPIPE` case runs
  with `SIGPIPE` ignored.)
* **Pipes and sockets** (`== pipes`, `== sockets`): `aio_offset` is ignored (12345, 777, 99, 5
  behave as 0), but a **negative** offset is still `EINVAL` on a pipe, for reads and writes. A read
  of 100 bytes returns what is there (10); a read on an empty pipe or socket stays `EINPROGRESS`
  until data arrives and then returns it. With `O_NONBLOCK`: an empty pipe gives `EAGAIN`; a
  100000-byte write into an empty 65536-byte pipe is partial (65536); into a full one `EAGAIN`. A
  blocking 100000-byte write stays `EINPROGRESS` while the pipe is full and returns 100000 once the
  reader drains it (no partial result). End of file reads 0. A datagram read of 4 bytes takes the
  first 4 bytes of a 10-byte datagram and discards the rest; the next read gets the next
  datagram.

### aio_fsync

* `O_SYNC` and `O_DSYNC` (and `O_RSYNC`, the same value as `O_SYNC`) complete with 0/0. Any other
  operation is refused at submission with `EINVAL`: 0, `O_RDWR`, -1, and `O_SYNC` or `O_DSYNC`
  combined with another flag. The operation is checked before the descriptor (fd -1 with
  operation 0 is `EINVAL`).
* A -1 or closed descriptor is refused at submission with `EBADF` (unlike `aio_read`/`aio_write`,
  which report `EBADF` later). A read-only descriptor of a regular file succeeds. `/dev/null`, a
  pipe and a stream socket are accepted and fail with `EINVAL` through `aio_error`.
* `aio_offset`, `aio_nbytes`, `aio_reqprio` and `aio_lio_opcode` are ignored (garbage in all four
  still syncs).
* The sync covers the requests queued before it on the descriptor: on a pipe with a read held, the
  sync stays `EINPROGRESS` until the read completes and then fails with `EINVAL`. A write
  submitted just before a sync is complete when the sync is (50/50).
* `SIGEV_SIGNAL` works for a sync (`si_code` `SI_ASYNCIO`, the value).

### aio_error, aio_return

* A held request: `aio_error` is `EINPROGRESS`. `aio_return` before completion returns -1 and
  leaves `errno` untouched, and does not consume anything: `aio_error` is still `EINPROGRESS`, and
  after completion `aio_return` returns the count (4).
* After completion `aio_return` can be called again and returns the same value (4), and
  `aio_error` after `aio_return` still returns the final status (0): the `EINVAL` that POSIX
  allows ("may fail") never occurs.
* A zeroed `aiocb` that was never submitted: `aio_error` 0, `aio_return` 0 (POSIX: undefined).
* A failed request: `aio_error` is the error, `aio_return` -1, `errno` not set by `aio_return`.
* An `aiocb` is reusable for a new request once the previous one is complete.

### aio_suspend

* A held request with a zero timeout or 150 ms: -1 `EAGAIN`, after at least the timeout. A
  completed request in the list: 0 at once, with or without NULL entries and held requests beside
  it. Completion of a waited-for request ends the wait with 0.
* **A list in which nothing can complete still returns 0**: only NULL entries (timeout 0, 100 ms,
  or none), `nent` 0, a request already `aio_return`ed, a zeroed `aiocb`. `nent` -1 is `EINVAL`.
* A signal handler ends the wait with `EINTR`, **also with `SA_RESTART`** (the call is not
  restarted); a blocked signal does not end it.
* Invalid timeouts are not `EINVAL`: `tv_nsec` -1 and `tv_sec` -1 give `EAGAIN` at once;
  `tv_nsec` 1000000000 gives `EAGAIN` after about one second (not within 1 s, and within the 3 s
  alarm); `{LONG_MAX, 0}` gives `EAGAIN` at once, before a signal due 200 ms later (the huge
  timeout acts as already expired).

### aio_cancel

* `fd` -1 or closed: -1 `EBADF`, also with an `aiocb` given. A descriptor without outstanding
  requests, a completed request (before and after `aio_return`), and a never-submitted `aiocb`
  naming the descriptor: `AIO_ALLDONE`; a completed request keeps its status and return value.
* A request whose `aio_fildes` differs from `fd` (completed, held, or never submitted): -1
  `EINVAL` (POSIX: unspecified results).
* **A request that has started cannot be cancelled**: the held pipe read, the held pipe write and
  the held socket read give `AIO_NOTCANCELED` and stay `EINPROGRESS`; each then completes normally
  when fed or drained (5 bytes, 100000 bytes, 5 bytes) and sends its notification.
* **Requests queued behind it on the same descriptor can**: `aio_cancel(fd, &B)` gives
  `AIO_CANCELED`, B's `aio_error` `ECANCELED` and `aio_return` -1, and B's `SIGEV_SIGNAL`
  notification arrives (`si_code` `SI_ASYNCIO`, B's value); cancelling B again gives
  `AIO_ALLDONE`. `aio_cancel(fd, NULL)` with A started and C queued gives `AIO_NOTCANCELED`, C
  becomes `ECANCELED` and is notified, A stays `EINPROGRESS`; exactly one notification per
  cancelled request.

### lio_listio

* Mode other than `LIO_WAIT`/`LIO_NOWAIT` (5, -1, 7 for `lio_listio64`): -1 `EINVAL`. `nent` 0
  and lists of only NULL entries: 0. NULL entries and `LIO_NOP` entries are skipped (a NOP naming
  fd -1 is not an error); a NOP entry's `aio_error` afterwards is 0.
* `LIO_WAIT` returns 0 when every entry succeeded, all entries being complete at the return. If
  any entry fails it returns -1 `EIO` after **all** entries completed, the others normally: a read
  on a write-only descriptor (`EBADF`), fd -1 (`EBADF`), offset -1 (`EINVAL`), `aio_reqprio` 99
  (`EINVAL`), an unknown opcode 99 (`EINVAL`).
* `LIO_NOWAIT` returns 0 when the entries were queued, even if one later fails (`EBADF`,
  `EINVAL` for the offset); **except an invalid `aio_reqprio`, which makes `LIO_NOWAIT` return -1
  `EINVAL` while the other entries are still performed** (`[0] 0/2 ... [2] 0/4`).
* Notification of the list (`LIO_NOWAIT`): one `SIGEV_SIGNAL` (`si_code` `SI_ASYNCIO`, the list's
  value) after **every** entry is complete, not before (none within 400 ms while a pipe read was
  held), and no second one. A per-entry `aio_sigevent` is delivered as well (both signals). With
  `SIGEV_THREAD` one call, in a thread other than the caller's, with every entry complete. With
  zero entries, or only NULL and NOP entries, the list's notification (signal or thread) still
  comes. A NULL `sigevent` is accepted.
* `LIO_WAIT` ignores the list's `sigevent` (no signal) but each entry's own notification is
  delivered.
* 5000 one-byte writes in one `LIO_WAIT` list all succeed.
* **Long lists**: 1024 and 65536 entries (NULL, or one NOP `aiocb` repeated) return 0; **1048576
  entries kill the process with `SIGSEGV`** instead of failing with `EINVAL`.
* **Negative `nent`**: -1 returns 0 in both modes (an empty list); `INT_MIN` kills the process
  with `SIGSEGV` in both modes.
* **`LIO_WAIT` and signals**: a handler without `SA_RESTART` ends `LIO_WAIT` with -1 `EINTR` while
  an entry is still held (the other entry is complete); **when that remaining entry later
  completes, the process is killed by `SIGSEGV`** (`-- end lio_listio interrupted: killed by
  SIGSEGV`, the line before shows the process alive until the pipe is fed). With `SA_RESTART` the
  wait continues after the handler and returns 0 when the entries complete (contrast
  `aio_suspend`, which is not restarted). The case is sequenced, not timed: the signal is sent
  once the file write is complete and the calling thread is parked (`/proc/self/task/<tid>`: state
  `S` and no context switch over 10 samples 10 ms apart), and with `SA_RESTART` the pipe is fed
  once the handler has run and the caller is parked again. (Until 2026-10-22 the signal came
  after a fixed 200 ms; under load the write was sometimes still pending then, and the reference
  was killed by `SIGABRT` when it completed, before the `aio_error` line.)

### Notifications

* `SIGEV_NONE`: no signal. A zeroed `aio_sigevent` (`SIGEV_SIGNAL` with signal 0): the request
  succeeds and no signal is sent.
* `SIGEV_SIGNAL`: exactly one signal, `si_signo` the requested one, `si_code` `SI_ASYNCIO`,
  `si_value` the requested value (an integer, or `sival_ptr` pointing at the `aiocb`), `si_pid`
  the process, `si_uid` the user, `si_errno` 0. Non-real-time signals work too (`SIGUSR2`). At
  receipt the request is already complete (`aio_error` 0, `aio_return` the count). A failing
  request also notifies (`aio_error` `EBADF` at receipt).
* The signal is **process-directed**: with two application threads blocking it, both see it
  pending. An `SA_SIGINFO` handler unblocked only in the submitting thread runs there.
* `SIGEV_THREAD`: one call per request (three requests, three calls), in a thread other than the
  caller's, **detached** by default, receiving `sigev_value`, with the request complete at the
  call (`aio_error` 0, or `EBADF` for a failing request). The caller had `SIGUSR1` blocked and
  `SIGUSR2` unblocked; in the notification thread **neither is blocked** (the caller's mask is not
  inherited). `sigev_notify_attributes` are honoured: a 4 MiB stack is at least 4 MiB and the
  thread is joinable (`detached=0`).
* Invalid notifications are not refused at submission: signal 65 or -1 makes the **request**
  fail with `EINVAL` (through `aio_error`); `sigev_notify` 99 and `SIGEV_THREAD_ID` are accepted,
  the I/O is done and nothing is delivered; `SIGEV_THREAD` with a NULL function kills the process
  with `SIGSEGV` when the request completes (nothing is printed before; the probe waits 500 ms
  before printing so that the crash cannot race a line).

### Ordering per descriptor (§11.11 Guarantees)

* Same descriptor, same priority, completion in submission order, every time: eight writes to an
  `O_APPEND` file land in order (100/100); eight one-byte writes to offset 0 leave the last one
  (100/100); overlapping writes of 8, 6, 4, 2 bytes at offset 0 leave `DDCCBBAA` (100/100); eight
  writes to a pipe arrive in order (100/100); four reads queued on an empty pipe receive the bytes
  in submission order (20/20).
* A request on another descriptor is not held up by a held one (a `/dev/zero` read completes
  while a pipe read waits).

### fork, exec, close

* **fork with a request held** (`== fork with a request pending`): the child sees the parent's
  requests through the copied `aiocb`s: the held read is `EINPROGRESS`, the completed one 0 with
  `aio_return` 8; `aio_suspend` on the held one times out; `aio_cancel` on it, or on the
  descriptor, gives `AIO_NOTCANCELED`. **A new request in the child never runs** (`EINPROGRESS`
  after 3 s); a second one queued on the pipe can be cancelled (`AIO_CANCELED`). The parent is
  unaffected (the held read completes when fed; the completed one returns 8).
* **fork with nothing pending**, immediately after a request completed: again **a new request in
  the child never runs**. The parent's next request works. (Forking 1.5 s or more after the last
  completion, the child's requests do run: outside the record, below.)
* **exec** with a read and a write held: the exec does not wait; in the new image no completion
  signal is pending; the held write stopped at what was already in the pipe (65536 of 100000
  bytes, nothing more arrives after draining); the held read is gone (a byte written after the
  exec stays in the pipe).
* **close** of a descriptor with a request held (A, already reading) and one queued (B): `close`
  returns 0 at once. A still completes with the data written after the close (3 bytes). B fails
  with `EBADF`, not `ECANCELED`. A new request on the closed descriptor is `EBADF` (through
  `aio_error`); `aio_cancel` on it is -1 `EBADF`.

### The `*64` interfaces

* `aio_write64`, `aio_suspend64`, `lio_listio64`, `aio_cancel64`, `aio_fsync64`, `aio_error64`,
  `aio_return64` behave as the plain functions (also in the sections above); offsets past 4 GiB
  (5 GiB) work through both `struct aiocb64` and `struct aiocb`.

### Resource exhaustion

* With `RLIMIT_NPROC` 1 (so `pthread_create` fails with `EAGAIN`), every submission fails at
  submission with `EAGAIN`: `aio_read`, `aio_write`, `aio_fsync`, `aio_read` with `SIGEV_THREAD`;
  `lio_listio(LIO_WAIT)` returns -1 `EAGAIN` and each entry's `aio_error` is `EAGAIN` with
  `aio_return` 0. This shows the reference needs a new thread to accept a request when none is
  idle; a Liella io_uring path that creates no thread may accept the requests (see below).

## Spec, record and documentation disagree

* **Ordering** (§11.11 Guarantees: same descriptor and priority complete in submission order).
  The record agrees in every case tried, including writes to one offset of a regular file.
  POSIX guarantees order only for writes on an `O_APPEND` descriptor or a device that cannot
  seek, and the glibc manual says the operations "are carried out in arbitrary order". The spec
  promises more than POSIX and the manual; D-11.8 keeps the spec's promise, which the reference
  does honour.
* **Submission errors** (§11.11 Failure: "`EAGAIN` or `EINVAL`"). `EINVAL` at submission comes
  only from `aio_reqprio`, the `aio_fsync` operation, the `lio_listio` mode, and `aio_suspend`
  with `nent` -1; offsets, counts, descriptors and invalid signals are reported through
  `aio_error`. `aio_fsync` and `aio_cancel` also fail at submission with **`EBADF`**, which the
  Failure clause does not list. POSIX and the glibc manual allow either point for `EBADF` and
  `EINVAL`; the spec should list `EBADF` and say which errors are synchronous.
* **`aio_nbytes` larger than `SSIZE_MAX`**: `aio_read(3)`/`aio_write(3)` and POSIX list `EINVAL`
  for an invalid `aio_nbytes`; the reference accepts the request and it fails with `EFAULT`.
* **`aio_fsync` on a read-only descriptor**: `aio_fsync(3)` says `EBADF` for a descriptor "not
  open for writing"; the reference succeeds (POSIX's own `aio_fsync` text requires only a valid
  descriptor).
* **`aio_error`/`aio_return` after the status was retrieved**: POSIX and the manual allow
  `EINVAL`; it never happens (the value can be read again).
* **`aio_suspend` with nothing that can complete** (only NULL entries, `nent` 0, an already
  returned or never-submitted `aiocb`): POSIX returns 0 only "after one or more asynchronous I/O
  operations have completed" and otherwise waits for the timeout; the reference returns 0 at once,
  also with no timeout. `nent` -1 gives `EINVAL`, which neither POSIX nor the man page lists. A
  signal ends it with `EINTR` even under `SA_RESTART`; `signal(7)` does not mention
  `aio_suspend` either way.
* **`aio_cancel` with a mismatched descriptor**: POSIX leaves it unspecified; `aio_cancel(3)` and
  the manual list only `EBADF`; the reference fails with `EINVAL`.
* **`lio_listio` with an invalid `aio_reqprio` in `LIO_NOWAIT`**: -1 `EINVAL` although the other
  entries were performed. `lio_listio(3)` says that for any error other than `EAGAIN`, `EINTR` or
  `EIO` none of the operations was initiated. (In `LIO_WAIT` the same list gives `EIO`.)
* **`lio_listio` length**: POSIX and `lio_listio(3)` describe `EINVAL` for `nent` over
  `AIO_LISTIO_MAX`; the reference has no such limit (`AIO_LISTIO_MAX` undefined, `sysconf` -1)
  and a list of 1048576 entries, or `nent` `INT_MIN`, crashes the process. `nent` -1 is an empty
  list.
* **`lio_listio(LIO_WAIT)` and `EINTR`**: POSIX and the man page allow `EINTR` with some
  operations initiated; in the reference the completion of such an operation afterwards kills
  the process. Liella must not reproduce this; the record line is a reference defect.
* **fork**: POSIX says no asynchronous operation is inherited by the child (and use of the
  parent's control blocks is undefined). In the reference the child cannot run new requests at
  all when it was forked while a request was held or shortly after one completed, and
  `aio_cancel` in the child reports the parent's request as not cancellable. Nothing in POSIX, the
  man pages or the manual allows the first point; Liella should run the child's requests (a known
  difference for the differential test).
* **close**: POSIX says a request not cancelled by `close` "completes as if the `close()`
  operation had not yet occurred". The reference's queued request B neither is cancelled
  (`ECANCELED`) nor completes; it fails with `EBADF`. The request already reading (A) does
  complete.
* **exec**: POSIX allows outstanding requests to be cancelled, but a request that is not cancelled
  "shall complete as if the exec function had not yet occurred". The held write ended part-way
  (65536 of 100000 bytes reached the pipe) and its status is lost with the old image; the held
  read was dropped. No signal reached the new image, as POSIX requires.
* **Notification signals**: §11.11 Kernel names `rt_sigqueueinfo`/`rt_tgsigqueueinfo`. The
  record shows a process-directed signal; `strace` of the whole run (outside the record) shows
  `rt_sigqueueinfo` and no `rt_tgsigqueueinfo`. It also shows no `io_uring_*` or `io_submit`
  calls, and `fsync`, `fdatasync`, `pread64` and `pwrite64`, as the Kernel paragraph says.
* **`SIGPIPE` and `SIGXFSZ`**: the synchronous `write` would raise them in the caller (by default
  ending the process); the asynchronous write only reports `EPIPE`/`EFBIG`. Neither POSIX nor the
  man pages say which is right; the spec is silent. The record makes it the contract (no signal
  reaches the application).
* **`aio_init(3)`**: values below 1 for `aio_threads` are said to be raised to 1; with every field
  -1 nothing observable changes in the record, so this cannot be confirmed from results.

## Timing

Lines whose result depends on timing are kept out of the record: whether a request on a regular
file has completed when `aio_cancel` or `aio_error` is called right after submitting it, the
order in which requests on different descriptors complete, how many helper threads exist, and
whether a completion signal or the completion itself ends an `aio_suspend` first. The record
relies on time in these places only:

* **The 400 ms settle** before the cancel cases (and before reading `EINPROGRESS` from a held
  request): a held request must have *started* for `AIO_NOTCANCELED`. If the first read on the
  pipe had not started within 400 ms, the reference would report `AIO_CANCELED` for it. Under the
  recorded load it always started; on a much more loaded machine this is the line to suspect.
* **Coarse bounds**: "waited at least the timeout" (5 ms slack), "returned within 1 s" for the
  invalid timeouts, "a second signal within 300 ms: none", "signal within 400 ms: none" while an
  entry is held (it cannot complete), 3 s and 600 s limits that only end a wait that would
  otherwise hang (since 2026-10-05: under the full test suite an fsync on the timing VM's
  disk took over 120 s; a completed request ends its wait at once).
* **`LIO_WAIT` interrupted** uses none: its signal and pipe write wait for observed state (above);
  20 s bounds only end a wait that would otherwise hang, and print `sequencing failed: ...`.
* **The `SIGEV_THREAD` notification after `aio_init`** waits (up to 5 s) for the count to pass
  its value before the request, then 50 ms for a duplicate call; the line shows the section's
  total (`calls=2` after the second round of "aio_init twice"). Until 2026-10-23 the wait was
  for "not 0", which the second round already was, so under load that line could print `calls=1`
  on either side (AIO-PRODUCTS-INTERMITTENT). `aio_diff DIR init-twice` runs only that case.
* **fork with nothing pending** happens within milliseconds of the completion, well inside the
  reference's default idle time of 1 s (`aio_init(3)`); after that time the outcome differs (see
  below).

## What the record does not settle

* Whether a request that has started can be cancelled is implementation-defined (POSIX). The
  reference never cancels a started request; with io_uring Liella may be able to cancel a pipe or
  socket read that is still waiting, which would print `AIO_CANCELED` and `ECANCELED` on those
  lines. D-11.8 should say which; the record does not decide it.
* Requests of different priorities on one descriptor: kept outside the record (below), because the
  order a library chooses among queued requests of different priority is not a guaranteed
  completion order of the spec.
* `EAGAIN` from exhaustion other than `RLIMIT_NPROC` (memory), and the lines under `RLIMIT_NPROC`
  for an implementation that needs no thread: an io_uring path may accept those requests. If it
  does, those lines are a deliberate difference.
* `EOVERFLOW` (read past the offset maximum): `off_t` is 64 bits and no file system at hand has a
  smaller maximum, so it does not occur.
* Whether each `SIGEV_THREAD` call gets a new thread, and the notification thread's signal mask
  beyond `SIGUSR1`/`SIGUSR2` (the record shows the caller's mask is not inherited).
* `aio_suspend` interrupted by the completion signal of one of its own requests (POSIX's note):
  a race between the signal and the completion; not recorded.
* Async-signal safety of `aio_suspend` (`aio_suspend(3)` BUGS says the reference's is not), `O_DIRECT`,
  other file systems, and threads sharing one `aiocb`: not probed.

## Outside the record

`aio_run.sh PROGRAM SCRATCH extra` runs four observations that are about the reference's request
scheduling rather than the contract. Recorded on the same VM, 2026-10-04 (two runs, identical):

* **`aio_init` and concurrency**: without `aio_init`, a `/dev/zero` read completes within 1 s
  while a pipe read on another descriptor is held. After `aio_init` with `aio_threads=1` it does
  not; it completes once the pipe read has its data. So `aio_threads` limits how many requests run
  at once across all descriptors; a request can wait behind a blocked request on an unrelated
  descriptor.
* **Priorities**: four one-byte reads queued on an empty pipe, the first (`aio_reqprio` 0) held,
  then 5, 0, 2, fed one byte at a time: the bytes go to 0 (the held one), then 0, 2, 5. A lower
  `aio_reqprio` (a higher priority) is served first among queued requests.
* **dup**: a read held on a pipe descriptor and a second read on a `dup` of it: both are in
  progress (`aio_cancel` on the second gives `AIO_NOTCANCELED`), so requests are queued per
  descriptor number, not per open file.
* **fork after a delay**: forking 0 ms and 500 ms after the last completion, a request in the child
  never runs (`EINPROGRESS` after 3 s); forking 1500 ms and 3000 ms after it, the child's request
  runs. The boundary matches the default idle time of 1 s in `aio_init(3)`.

`strace -f -c` of a record run (only to see which system calls reach the kernel; outside the
record): `clone3`, `futex`, `pread64`, `pwrite64`, `read`, `write`, `fsync`, `fdatasync`,
`rt_sigqueueinfo`, `sched_getparam`/`sched_getscheduler`/`sched_setscheduler`, and no
`io_uring_setup`, `io_submit` or `rt_tgsigqueueinfo`. Two traced runs (with `-c`, and with
`-o /dev/null`) printed output identical to the record, so the record also holds when every
system call is slowed down.
