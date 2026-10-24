# POSIX message queues and shared memory: mq_*, __mq_open_2, shm_open, shm_unlink

**Spec:** §25.3 (`mq_open`, `mq_close`, `mq_unlink`, `mq_send`, `mq_timedsend`, `mq_receive`,
`mq_timedreceive`, `mq_notify`, `mq_getattr`, `mq_setattr`, `__mq_open_2`; clauses `s25_3_c_0` to
`s25_3_c_4`, `s25_3_f_0`, `s25_3_x_0`) and §25.4 (`shm_open`, `shm_unlink`; clause `s25_4_c_0`),
with the decision rows D-25.2 (shared memory stays on `/dev/shm`) and D-25.3 (`SIGEV_THREAD`
notification uses the kernel's netlink protocol) of implementation-plan.md. Public interfaces:
`<mqueue.h>`, `<sys/mman.h>`, `<signal.h>` (`struct sigevent`). Normative references (plan §1.2):
POSIX.1-2017 (`mq_open`, `mq_close`, `mq_unlink`, `mq_send`, `mq_timedsend`, `mq_receive`,
`mq_timedreceive`, `mq_notify`, `mq_getattr`, `mq_setattr`, `shm_open`, `shm_unlink`, XSH 2.4.2
Realtime Signal Generation and Delivery), the man pages `mq_overview(7)`, `mq_open(3)`,
`mq_send(3)`, `mq_receive(3)`, `mq_notify(3)`, `mq_getattr(3)`, `mq_unlink(3)`, `sigevent(7)`,
`shm_open(3)`, `shm_overview(7)`, and the glibc manual. `__mq_open_2` is declared by
`<bits/mqueue2.h>`, which `<mqueue.h>` includes only when `_FORTIFY_SOURCE` is active; the probe
undefines `_FORTIFY_SOURCE` and repeats that declaration.

**Probe:** `crates/core/tests/oracle/posixipc_diff.c`, run by `posixipc_run.sh PROGRAM SCRATCH`.
**Record:** `posixipc.glibc.txt` (457 lines). It was made on the VM (Ubuntu 24.04, glibc
2.39-0ubuntu8.9, kernel 6.8.0-146-generic, scratch directory on ext4) with
`cc -std=gnu17 -O1 -o p posixipc_diff.c` and `env -i PATH=/usr/bin:/bin sh posixipc_run.sh p
SCRATCH`, twice, with scratch directories of different names and lengths (`~/ipcprobe/run-a`,
`~/ipcprobe/run-second-longer`): the two outputs are byte-identical, and identical to an `-O0`
build named `posixipc-other` run with a third scratch name (SHA-256 `41771e92...8fe7a3` for all
three). Nothing was left behind: the system's `/dev/shm` and `/dev/mqueue` were empty before and
after, no fixture mount survived, and the scratch directories were removed by the harness.

**Isolation.** `posixipc_run.sh` runs, through `sudo -n`, a private IPC namespace (`unshare -i`:
every queue lives and dies with it) and a private mount namespace (`--propagation private`). The
shell first checks that its IPC namespace differs from that of process 1, then mounts a fresh
`mqueue` file system (of the new IPC namespace) on `/dev/mqueue` and a fresh 4 MiB tmpfs (mode
1777) on `/dev/shm`, holding the marker `.liella-posixipc-fixture`; the program refuses to run
without it. The new IPC namespace has the kernel's default limits (`msg_max` 10, `msgsize_max`
8192, `queues_max` 256; printed at the top of `== mq_open attributes`). Phases:

1. **`setup`** (root; lines 1-2): root-owned queues `/rootq` (mode 0600) and `/rootq-ro` (0644),
   `/dev/shm` entries `rootshm` (0600), `rootshm-ro` (0644), a symbolic link `link` →
   `rootshm-ro`, a directory `dir` and a FIFO `fifo`.
2. **`user`** (the calling user through `setpriv`, empty environment; line 3 `running as root:
   no`, up to `done user`): everything else. **`/dev/shm` is a tmpfs** (line 4 `/dev/shm: file
   system tmpfs`, mode 1777, owner root), as on the VM's real system.
3. **`ext4`** (`== /dev/shm on another file system`): `SCRATCH/shmdir` (ext4) bind-mounted over
   `/dev/shm`.
4. **`nodev`** (`== no /dev/shm`): an empty tmpfs mounted over `/dev`, so `/dev/shm` and
   `/dev/mqueue` do not exist (marker `/dev/.liella-posixipc-nodev`).

## What the probe does

Strings are printed in brackets with every byte outside printable ASCII as `\xNN`; a run of more
than 60 equal bytes as `<c>*<n>`. `rc` results are `0`, `ok` (a descriptor or a size) or the errno
name; IDs are compared and printed as `self`/`other`/`main`/`new`. Directory listings (of
`/dev/mqueue` and `/dev/shm`, without the marker) are sorted by `strcmp`. Modes are read with
`lstat` on `/dev/mqueue/NAME` or `/dev/shm/NAME`. Risky cases run in forked children with an alarm
(3 or 5 s). The umask is 022 unless a line says otherwise.

* **Names** (`== mq_open names`, `== shm_open names`): 17 and 19 names (`/q`, `q`, `//q`, `/a/b`,
  `/`, empty, `/.`, `/..`, `.`, `..`, trailing slashes, a blank, a newline, UTF-8, 255 and 256
  bytes after the slash, 255 bytes without a slash, `///`), each created with
  `O_RDWR|O_CREAT|O_EXCL`, the directory listed, reopened without `O_CREAT`, and unlinked.
* **Queue attributes** (`== mq_open attributes`): the namespace limits, `MQ_PRIO_MAX`,
  `sysconf(_SC_MQ_PRIO_MAX)`, `sysconf(_SC_MQ_OPEN_MAX)`, the type sizes; NULL attributes; 13
  attribute sets (zero, negative, at and over the limits, `LONG_MAX`, `mq_flags` and `mq_curmsgs`
  set); invalid attributes on an existing queue.
* **Queue flags and modes** (`== mq_open flags and modes`): descriptor flags, `O_EXCL` with and
  without `O_CREAT`, `O_NONBLOCK`, `O_CLOEXEC`, access mode 3, `O_TRUNC`, the wrong direction
  (`EBADF`), modes 0666, 0600, 0400, 0200, 0 and 07777 under umask 022 (and 0666 under 0), the
  owner reopening each way; root's queues opened, created and unlinked by the user.
* **Send and receive** (`== mq_send and mq_receive`): eight messages with priorities 0 to 32767
  (two equal priorities, an empty message) received in order; sizes at, over and under
  `mq_msgsize` in both directions; priorities `MQ_PRIO_MAX` and `UINT_MAX`; `O_NONBLOCK` on an
  empty and a full queue; bad descriptors (-1, a pipe, a closed queue).
* **Timed forms** (`== timed send and receive`): five timespecs (`tv_nsec` -1 and 10^9, `tv_sec`
  -1, zero, both negative) on a full queue, a queue with room, an empty queue and a queue with a
  message; NULL timeouts; 150 ms timeouts (printed as "waited at least 100 ms").
* **Attributes** (`== mq_getattr and mq_setattr`): `O_NONBLOCK` with the other fields set to 99,
  other flags, the old attributes, the effect on another descriptor and on a `dup`, -1, NULL
  arguments (children).
* **Notification** (`== mq_notify with SIGEV_SIGNAL and SIGEV_NONE`, `== mq_notify with
  SIGEV_THREAD`): with `SIGUSR1` blocked and collected by `sigtimedwait`, or a semaphore posted by
  the notification function; registration by the same and another process, one-shot behaviour, a
  non-empty queue, a sender in another process, removal with NULL (by the owner, by another
  process, with nothing registered), registration through one descriptor and close of another,
  `SIGEV_NONE`, invalid `sigevent`s; for `SIGEV_THREAD`, the function's argument, whether it runs
  in a new thread, its detach state, stack size (with attributes destroyed right after
  `mq_notify`) and signal mask, re-registration from inside the function, two queues at once.
* **Unlink** (`== mq_unlink`), **`__mq_open_2`** (`== __mq_open_2`), **limits** (`== limits`:
  `RLIMIT_MSGQUEUE` in a child, the number of queues).
* **Shared-memory flags and modes** (`== shm_open flags and modes`): descriptor flags for each
  access mode, `O_EXCL`, `O_APPEND`, `O_NONBLOCK`, `O_DIRECTORY`, `O_TRUNC` (also with
  `O_RDONLY`), `ftruncate` on read-only descriptors, modes, root's entries, the symbolic link, the
  directory, a name inside it, unlinking them, the FIFO (child).
* **Round trip** (`== ftruncate, mmap and shm_unlink`): sizes, `mmap` of a new object, a second
  read-only mapping, `PROT_WRITE` on a read-only descriptor, `pread`, a forked child reading and
  writing through its own `shm_open`, `shm_unlink` with a live mapping, unlink errors.

## Observations the record shows directly

### Queue names (§25.3 `s25_3_c_1`, `s25_3_f_0`)

* A name must start with `/` (`name [q]: mq_open EINVAL`, also for `[]` and a 255-byte name
  without a slash; `mq_unlink` gives `EINVAL` for them too, `missing, no slash: EINVAL`).
* After the leading slash, the rest must not contain `/`: `//q`, `/a/b`, `/q/`, `/a/`, `///`
  give `EACCES`; so do `/.` and `/..`. The name `/` alone gives `ENOENT` (for `mq_open` with
  `O_CREAT` and for `mq_unlink`). 255 bytes after the slash work; 256 give `ENAMETOOLONG`.
* Blanks, newlines and UTF-8 are ordinary bytes; the queue appears in `/dev/mqueue` under the name
  without its slash (`[sp ace]`, `[nl\x0a]`).

### Queue attributes and limits (§25.3 `s25_3_c_4`, `s25_3_f_0`)

* NULL attributes give `mq_maxmsg` 10 and `mq_msgsize` 8192 (the namespace's `msg_default` and
  `msgsize_default`).
* `mq_maxmsg` and `mq_msgsize` must be positive and at most `msg_max` (10) and `msgsize_max`
  (8192) for an unprivileged user: 11, 8193, 0, -1 and `LONG_MAX` give `EINVAL`.
* `mq_flags` and `mq_curmsgs` in the creation attributes are ignored (`flags 0x800`, `0x7fff`,
  `curmsgs 5` all give a queue with flags 0 and no messages).
* Attributes are ignored when the queue exists (`existing queue, O_CREAT, invalid attr: ok`, the
  old attributes) or `O_CREAT` is absent.
* `MQ_PRIO_MAX` and `sysconf(_SC_MQ_PRIO_MAX)` are 32768; `sysconf(_SC_MQ_OPEN_MAX)` is -1.
  `struct mq_attr` is 64 bytes, `mqd_t` 4.
* With `RLIMIT_MSGQUEUE` 4096, a 10 × 8192 queue gives `EMFILE` and a 1 × 8 queue works. The
  257th queue in the namespace gives `ENOSPC` (`queues created before the first failure: 254`,
  with root's two).

### Opening a queue (§25.3)

* The descriptor is close-on-exec whether or not `O_CLOEXEC` is given (`O_RDWR|O_CREAT|O_EXCL
  descriptor: cloexec=yes`), and carries the access mode and `O_NONBLOCK` as `F_GETFL` shows;
  `O_NONBLOCK` at open appears as `mq_flags 0x800`.
* `O_CREAT|O_EXCL` on an existing queue: `EEXIST`. `O_EXCL` without `O_CREAT`, and `O_TRUNC`, are
  accepted and ignored. Access mode 3: `EINVAL`. A missing queue without `O_CREAT`: `ENOENT`.
* Receiving on an `O_WRONLY` descriptor and sending on an `O_RDONLY` one: `EBADF`.
* The mode is masked by the umask (0666 → 0644; under umask 0, 0666) and keeps the set-ID and
  sticky bits (07777 → 7755). Permissions are checked for the owner too: mode 0400 opens only for
  reading, 0200 only for writing, 0 not at all (`EACCES`).
* Another user's queues: opening against the mode gives `EACCES`, also with `O_CREAT` on the
  existing name; `mq_unlink` of root's queue gives `EACCES`.

### Send and receive (§25.3 `s25_3_c_2`, `s25_3_f_0`)

* Messages are received highest priority first and in sending order within a priority
  (`f32767`, then `b5`, `d5`, `g5`, then `c3`, the empty message at 2, `a1`, `e0`); the receive
  returns the length and stores the priority. An empty message is allowed. A NULL priority
  pointer is accepted.
* Sizes: sending exactly `mq_msgsize` works, one byte more gives `EMSGSIZE`; receiving with a
  buffer smaller than `mq_msgsize` gives `EMSGSIZE` even when the waiting message would fit
  (`receive with 15-byte buffer, a message waiting: -1 EMSGSIZE` for a 16-byte message size), also
  with 0; a larger buffer works.
* Priority `MQ_PRIO_MAX` or more gives `EINVAL` (`UINT_MAX` too), also when the message is too long
  (`send 17 bytes with prio MQ_PRIO_MAX: EINVAL`: the priority is checked first).
* `O_NONBLOCK`: `EAGAIN` on an empty queue and a full one; `mq_timedsend` on a full non-blocking
  queue gives `EAGAIN`, not a wait.
* Bad descriptors (-1, a pipe, a closed queue) give `EBADF` for every function; `mq_close` twice
  gives `EBADF`.

### Timed forms (§25.3 `s25_3_f_0`)

* An invalid timeout gives `EINVAL` **whether or not the call would have to wait**: `tv_nsec` -1
  or 10^9, **and a negative `tv_sec`** (`tv_sec -1 tv_nsec 0`), for a full queue, a queue with
  room, an empty queue and a queue with a message waiting.
* A valid timeout in the past gives `ETIMEDOUT` only if the call would block (`timedsend with
  room, tv_sec 0 tv_nsec 0: 0`; `timedreceive with a message ... 1 0`).
* A NULL timeout works when the call does not block.
* A timeout 150 ms ahead returns `ETIMEDOUT` after waiting (at least 100 ms measured).

### mq_getattr and mq_setattr (§25.3 `s25_3_c_4`)

* `mq_setattr` changes only `O_NONBLOCK`; `mq_maxmsg`, `mq_msgsize`, `mq_curmsgs` in the new
  attributes are ignored; the old attributes are returned (`old flags 0 maxmsg 3 msgsize 8 curmsgs
  1`). Any other bit in `mq_flags` (`O_APPEND`, `O_RDWR`, `O_CREAT`, -1) gives `EINVAL` and changes
  nothing; 0 clears `O_NONBLOCK`.
* The flag belongs to the open description: a `dup` sees it, another `mq_open` of the same queue
  does not. It is also the descriptor's `O_NONBLOCK` in `F_GETFL`.
* `mq_setattr(q, NULL, NULL)` and `mq_getattr(q, NULL)` return 0.

### mq_notify (§25.3 `s25_3_c_3`, `s25_3_f_0`)

* `SIGEV_SIGNAL`: a message arriving on an empty queue sends the signal with `si_code SI_MESGQ`,
  the registered `sival_int`, `si_pid` the sender (`self` for the probe's own send, `other` for a
  child's) and `si_uid` `self` (the probe and its child run as the same user).
* One-shot: after a notification the registration is gone (`second message (one-shot): no
  signal`), and a new one is accepted.
* A message to a non-empty queue does not notify, and the registration stays: after the queue is
  emptied, a message from another process notifies.
* One registrant per queue: a second registration gives `EBUSY`, **also from the same process**
  (`register again (same process): EBUSY`) and through another descriptor of the same queue
  (`register through the first descriptor: EBUSY`). `SIGEV_NONE` registers too (a child then gets
  `EBUSY`), sends nothing, and is consumed by an arrival.
* `mq_notify(q, NULL)` removes the caller's registration (through any of its descriptors of the
  queue); from a process that is not the registrant, or with nothing registered, it returns 0 and
  changes nothing (`child mq_notify(NULL): 0`, the parent still notified).
* Closing any descriptor of the queue in the registering process removes the registration
  (`registered through the first descriptor, second descriptor closed, message: no signal`).
* Invalid requests give `EINVAL`: `sigev_notify` 99, `SIGEV_SIGNAL` with signal 65 or -1,
  `SIGEV_THREAD_ID`. Signal 0 is accepted (`SIGEV_SIGNAL signo 0: 0`).
* `SIGEV_THREAD`: the function runs in a new thread with `sigev_value` as its argument, once per
  registration. The thread is **detached** even when the attributes ask for a joinable thread; a
  requested stack size is honoured, and the attributes may be destroyed after `mq_notify`
  returns. No signal is blocked in the thread (`signals 1-31 blocked 0`), although the
  registering thread blocked `SIGUSR2`. The function may register again (`re-register inside: 0`)
  and the next message calls it again. Registration rules are as above (`EBUSY` for the same
  process and another). Two queues registered at once each call the function (7 calls in all).

### mq_unlink and mq_close (§25.3)

* `mq_unlink` of a missing name: `ENOENT`; without a slash: `EINVAL`; 256 bytes: `ENAMETOOLONG`.
* Unlinking an open queue removes the name at once (`/dev/mqueue` no longer lists it, reopening
  gives `ENOENT`) while open descriptors keep working (`receive through the open descriptor: 4`).
  A new queue may take the name; it is a different queue (a message sent to the old descriptor
  does not appear in it).

### __mq_open_2 (§25.3)

* Without `O_CREAT` it behaves as `mq_open` (existing queue `ok`, missing `ENOENT`, no slash
  `EINVAL`). With `O_CREAT` it aborts, creating nothing, after writing
  `*** invalid mq_open call: O_CREAT without mode and attr ***: terminated\n` to standard error
  (`O_CREAT: child killed by SIGABRT`).

### shm_open names (§25.4 `s25_4_c_0`)

* Leading slashes are optional and any number is accepted: `s`, `/s` and `//s` all create
  `/dev/shm/s`. A 255-byte name without a slash works.
* A slash after the name (`/a/b`, `/s/`, `s/`) and a name that is empty after the leading slashes
  (`/`, empty, `///`) give `EINVAL`; for `shm_unlink` the same names give `ENOENT`.
* 255 bytes after the slash work; 256 give `ENAMETOOLONG` (both functions).
* `.` and `..` (with or without the slash) are not rejected: they name `/dev/shm` itself and
  `/dev`, so `O_CREAT|O_EXCL` gives `EEXIST` and `shm_unlink` gives `EISDIR`.
* Blanks, newlines and UTF-8 are ordinary bytes.

### shm_open flags and modes (§25.4)

* The descriptor is close-on-exec, with the requested access mode. `O_WRONLY` and access mode 3
  are accepted (`access mode 3 descriptor: ... access=3`). `O_APPEND` and `O_NONBLOCK` are kept on
  the descriptor; `O_DIRECTORY` on a file gives `ENOTDIR`; `O_EXCL` without `O_CREAT` is ignored.
* `O_TRUNC` truncates the object to 0, **also with `O_RDONLY`** (`O_RDONLY|O_TRUNC on a 4096-byte
  object: ok`, then size 0).
* `ftruncate` on a read-only descriptor gives `EINVAL` (also right after `O_RDONLY|O_CREAT`).
* `O_CREAT` on an existing object ignores the mode. Modes are masked by the umask and keep the
  set-ID and sticky bits (07777 → 7755, 01777 → 1755); the owner is checked like anyone else
  (0400: `O_RDWR` `EACCES`; 0: both `EACCES`).
* Another user's objects: `EACCES` against the mode, also with `O_CREAT`; `shm_unlink` of root's
  object in the sticky `/dev/shm` gives `EACCES` (the trace shows the kernel's `EPERM` from
  `unlink`).
* A symbolic link in `/dev/shm` is not followed: `ELOOP`. A directory opens read-only (`directory,
  O_RDONLY: ok`); read-write gives `EINVAL` (the kernel's `EISDIR`, per the trace). A name inside
  the directory is rejected (`EINVAL`, inner slash). `shm_unlink` of the directory and of the link
  (root's, sticky directory) give `EACCES`.
* A FIFO is opened without `O_NONBLOCK`: with no writer, `shm_open` blocks (`FIFO: child killed
  by SIGALRM`).

### Mapping round trip (§25.4)

* A new object has size 0; after `ftruncate(8192)`, `mmap` works, the pages are zero, a second
  `shm_open` (read-only) mapped `PROT_READ` sees the same bytes, `pread` too, and a forked child
  opening the name reads the data and its write is visible to the parent.
* `PROT_WRITE` with `MAP_SHARED` on a read-only descriptor: `EACCES`.
* After `shm_unlink` the mappings keep working and share data; reopening gives `ENOENT`; a second
  `shm_unlink` gives `ENOENT`.

### /dev/shm itself (§25.4 ⚑ HIST, D-25.2)

* The objects are plain files in `/dev/shm` (listed there by name, mode and owner as given).
* `/dev/shm` on ext4 works the same (`== /dev/shm on another file system`: create, `ftruncate`,
  `mmap`, unlink all succeed): no file-system type check.
* No `/dev/shm` at all: `shm_open` (with and without `O_CREAT`) and `shm_unlink` give `ENOENT`.
  Message queues do not need `/dev` (`mq_open without /dev/mqueue: ok`).

### System calls

A trace of the `user` phase (`strace -f`, a separate run, not part of the record):

* `mq_open(name)` calls the kernel's `mq_open` with the name **without its leading slash**
  (`//q` reaches the kernel as `/q`, `/` as the empty name); a name without a slash never reaches
  the kernel. `mq_unlink` likewise. The creation attributes are passed through.
* `shm_open(name, oflag, mode)` is `openat(AT_FDCWD, "/dev/shm/NAME", oflag|O_NOFOLLOW|O_CLOEXEC,
  mode)` with the leading slashes removed; `shm_unlink` is `unlink("/dev/shm/NAME")`. Kernel
  `EISDIR` from `openat` was reported as `EINVAL`, and `EPERM` from `unlink` as `EACCES`.
* `SIGEV_THREAD` (D-25.3): at the first such registration the process creates
  `socket(AF_NETLINK, SOCK_RAW|SOCK_CLOEXEC, NETLINK_ROUTE)` and a helper thread; the kernel's
  `mq_notify` receives `SIGEV_THREAD` with that socket; each notification creates a new thread
  (`clone3`) which unblocks all signals before calling the function. A re-registration from
  inside the function is an ordinary `mq_notify` call.

## Observations that look like historical artifacts

The implementer may decide these differently (and record the difference).

* `shm_open` accepts names without a leading slash and with several, and `.`/`..` reach
  `/dev/shm` and `/dev`; `shm_unlink` gives `ENOENT` where `shm_open` gives `EINVAL` for the same
  bad name.
* Error translation in `shm_open`/`shm_unlink` (`EISDIR` → `EINVAL`, `EPERM` → `EACCES`).
* `O_RDONLY|O_TRUNC` truncates; a FIFO in `/dev/shm` blocks `shm_open`.
* Set-ID and sticky bits in the creation mode are kept for queues and objects.
* The `SIGEV_THREAD` thread is detached whatever the attributes say.
* A negative `tv_sec` is `EINVAL` for the timed queue functions, and invalid timeouts are checked
  even when the call would not block (both come from the kernel).

## Spec, record and documentation disagreements

* **Queue name errors.** §25.3 `s25_3_f_0` gives `EINVAL` for "a bad name"; the record gives
  `EINVAL` only for a missing leading slash, `EACCES` for a further slash (and `/.`, `/..`) and
  `ENOENT` for `/`. `mq_open(3)` agrees with the record: it lists `EACCES` for a name with more
  than one slash and `ENOENT` for the name `/` alone. `s25_3_c_1` ("exactly one leading slash") is the
  contract; the errors are the kernel's.
* **Shared-memory names.** §25.4 says the name is `"/name"` and "cannot contain another slash";
  the record accepts names without a slash and with several leading slashes (rejecting only a
  slash after the name). `shm_open(3)` describes `/somename` as the portable form. `shm_open(3)`
  ties `ENAMETOOLONG` to `PATH_MAX`; the record fails at 256 bytes
  after the slash (`NAME_MAX`).
* **`/dev/shm` must be mounted** (§25.4 ⚑ HIST "it fails if `/dev/shm` is not mounted"): the
  record fails only when `/dev/shm` does not exist (`ENOENT`); an ordinary directory on ext4 works.
* **Timeout validity.** POSIX (`mq_timedsend`, `mq_timedreceive`) says the timeout's validity
  need not be checked when the call can proceed at once and lists `EINVAL` only for `tv_nsec`
  out of range; the record checks always and rejects a negative `tv_sec`.
* **`mq_notify` failures.** §25.3 lists `EBUSY` when "already registered"; the record shows it
  also for the registering process itself. POSIX's `mq_notify` says a NULL notification removes
  the calling process's registration and does not describe a call from another process; the record
  returns 0 (no error) and changes nothing then.
* **`SIGEV_THREAD` attributes.** `sigevent(7)` describes `sigev_notify_attributes` as the
  attributes of the new thread; the record creates a detached thread even for a joinable
  request (the stack size is honoured).
* **`shm_unlink` permission error.** POSIX lists `EACCES`; the kernel gives `EPERM` for the sticky
  directory and the record shows `EACCES` (consistent with POSIX).
* **`mq_setattr`.** POSIX says the other fields are ignored; the record also rejects any flag
  other than `O_NONBLOCK` with `EINVAL` (the kernel's check), which POSIX does not list.

## What the record does not settle

* Cancellation (`s25_3_x_0`: send and receive are cancellation points): not probed.
* Behaviour as root (`CAP_SYS_RESOURCE` lifts `msg_max` / `msgsize_max`), and limits other than
  the new namespace's defaults: the record ran unprivileged with defaults.
* `mq_notify` when a receiver is already blocked in `mq_receive` (POSIX: the blocked receiver gets
  the message and no notification): needs a timing-dependent setup; not probed.
* The `SIGEV_THREAD` thread's scheduling attributes, guard size, and what happens when its
  function is NULL, or when the netlink socket cannot be created.
* `__mq_open_2`'s message when standard error is closed, and whether it uses `/dev/tty`.
* `mq_open` with `O_CREAT` and a mode but NULL attributes passed through the variadic interface
  with garbage in the fourth argument (the probe passes NULL explicitly).
* Shared-memory objects on file systems other than tmpfs and ext4, `/dev/shm` read-only, or
  full (`ENOSPC` on `ftruncate`/write).
* Huge objects, `ftruncate` shrinking a mapped object (`SIGBUS`), and `mmap` beyond the end.
