# System logging: openlog, syslog, vsyslog, closelog, setlogmask, __syslog_chk, __vsyslog_chk

**Spec:** §26.1 (`openlog`, `syslog`, `vsyslog`, `closelog`, `setlogmask`, `__syslog_chk`,
`__vsyslog_chk`; clauses `s26_1_c_1` to `s26_1_c_4`, `s26_1_g_1` to `s26_1_g_4`), with the decision
row D-26.1 (a `Logger` handle inside; at the boundary the global logger keeps the ident *by
pointer*) of implementation-plan.md. Public interfaces: `<syslog.h>` (`<sys/syslog.h>`, including
the `SYSLOG_NAMES` tables), `<paths.h>`. Normative references (plan §1.2): POSIX.1-2017 (`closelog`,
`openlog`, `setlogmask`, `syslog`), the man pages `syslog(3)` (with `openlog`, `closelog`,
`vsyslog`, `setlogmask(3)`), `unix(7)`, RFC 3164 (the BSD syslog message format) and RFC 5424, and
the glibc manual (Syslog). `__syslog_chk` and `__vsyslog_chk` are declared by
`<bits/syslog-decl.h>`, which `<syslog.h>` includes only when `_FORTIFY_SOURCE` is active; the
probe undefines `_FORTIFY_SOURCE` and repeats those two declarations.

**Clauses:** `s26_1_c_1`, `s26_1_c_1_1`, `s26_1_c_1_2`, `s26_1_c_1_3`, `s26_1_c_1_4`, `s26_1_c_2`,
`s26_1_c_3`, `s26_1_c_4`, `s26_1_g_1`, `s26_1_g_2`, `s26_1_g_3`, `s26_1_g_4`.

**Probe:** `crates/core/tests/oracle/syslog_diff.c`, run by `syslog_run.sh PROGRAM`.
**Record:** `syslog.glibc.txt` (354 lines). It was made on the VM (Ubuntu 24.04, glibc
2.39-0ubuntu8.9, kernel 6.8.0-146-generic, local time zone `Etc/UTC`) with
`cc -std=gnu17 -O1 -o p syslog_diff.c` and `env -i PATH=/usr/bin:/bin sh syslog_run.sh p`, twice:
the two outputs are byte-identical, and identical to an `-O0` build named `syslog-probe-other`
(a different program name, so a different default ident; SHA-256 `2e800f1d...e8597` for all
three). The record was made on 4 October, so every timestamp in it had a one-digit day.

**Isolation.** The probe never reaches the system log. `syslog_run.sh` runs, through `sudo -n`, a
private mount namespace (`unshare -m --propagation private`) in which a fresh tmpfs is mounted over
`/dev`; only `/dev/null` (a device node) and the marker `/dev/.liella-syslog-fixture` are created
there, and the program refuses to run without the marker. The program then runs as the calling
user (`setpriv`, line 1 `running as root: no`) with an empty environment. It binds its own
`/dev/log` (a datagram socket, later a stream socket) and reads what the library sends; its own
`/dev/console` is a regular file (once a FIFO). Everything vanishes with the namespace; after the
runs `mount` showed no fixture mount. Each run is wrapped in `timeout 120`; the program has
`alarm(120)`.

## What the probe does

Received bytes are printed after their length, in brackets, with every byte outside printable
ASCII as `\xNN` and a backslash as `\\`; a run of more than 60 equal bytes is printed as `<c>*<n>`.
A timestamp of the form `Mmm dd hh:mm:ss` equal to the local time of the last ten seconds (English
month abbreviation, the day as `%2d`, the rest as `%02d`) is printed as `<TS>`, one equal only to
UTC as `<TS-UTC>`, and any other text of that shape as `<TS?>`. `[n]` is printed `[<pid>]` when n
is the probe's process ID and `[<cpid>]` for a child's. The program's short name
(`program_invocation_short_name`) is printed `<prog>` where it starts the text or follows a blank
or `>` and is followed by `[` or `:`; a length is then printed `N+kP` (N bytes besides k copies of
the name), so the record does not depend on the program's name. Descriptor changes are printed as
`fds +a -b` with the new descriptor's kind, peer, close-on-exec and non-blocking flags, never its
number. Risky cases run in forked children (alarm 3 or 5 s); `... child exited N` or
`... child killed by SIG` follows the child's output, then the child's standard error if any.

* **Constants** (`== constants`): the priority, facility, option and mask macros, `_PATH_LOG`,
  `INTERNAL_NOPRI`, `INTERNAL_MARK`, and the `prioritynames` / `facilitynames` tables.
* **Default connection** (`== default connection (no openlog)`): `syslog` without `openlog`, the
  descriptor it opens, `closelog` (twice), `syslog` after `closelog`.
* **Priorities** (`== priorities and facilities (no openlog)`): every facility (including the
  unnamed 12 and 15) with every priority, as the header received; then eleven invalid priority
  values, with `errno` preset to `EDOM`.
* **`openlog` facility** (`== openlog facility`): ten facility arguments, each followed by
  `syslog` at `LOG_ERR`, `LOG_KERN|LOG_ERR` and `LOG_DAEMON|LOG_ERR`; the facility after
  `closelog` and after `openlog(NULL, 0, LOG_LOCAL2)`.
* **ident** (`== ident`): a caller's buffer changed and emptied after `openlog`; `closelog`; the
  empty ident with and without `LOG_PID`; NULL; blanks, colon, brackets, newline, control and
  UTF-8 bytes, conversion specifications; lengths 31 to 2999 bytes; `openlog("one")` then
  `openlog(NULL)`.
* **Options** (`== options`): `LOG_PID`, `LOG_NDELAY` (descriptors after `openlog`, a second
  `openlog`, `openlog` with 0 while connected), `LOG_ODELAY`, `LOG_NOWAIT`, unknown option bits,
  options after `closelog`.
* **`LOG_PERROR`** (`== LOG_PERROR`): the standard-error copy (read from a pipe on descriptor 2)
  beside the datagram, for `LOG_PID`, trailing and embedded newlines, the empty message, a NUL, a
  5000-byte message, NULL and empty idents, a masked priority; the orientation of `stderr`; the
  option after `closelog` and after `openlog` with 0; descriptor 2 closed (in a child).
* **`setlogmask`** (`== setlogmask`): return values for 0, `LOG_MASK`, `LOG_UPTO`, -1, `0x100`,
  `0x1ff`; which priorities pass; masks and invalid priorities; whether a masked message is
  formatted (a `%W` conversion registered with `register_printf_specifier` that counts its calls);
  whether a masked message connects; the mask after `closelog`.
* **Format** (`== format`): `%m` with `errno` `EACCES`, 0, 99999 and -5, twice, with width and
  precision, after `%%`, through `vsyslog`; `errno` after a delivered message; empty message,
  trailing newlines, embedded newline, CR LF, NUL, control and high bytes, ordinary conversions,
  `%ls`.
* **Long messages** (`== long messages (datagram)`): bodies of 900 to 1000000 bytes, `errno` after
  each; 300000 bytes with `LOG_CONS`; a 3000-byte literal format; a NULL format (child).
* **Time zone** (`== timestamp time zone`, child): `TZ=XST-5:30`, then `TZ=UTC0` without and with
  `tzset`.
* **fork** (`== fork`): a child's message on the inherited connection with `LOG_PID`, the child's
  `closelog`, the parent's next message.
* **The checking forms** (`== __syslog_chk and __vsyslog_chk`): flags 1 and -1 with ordinary
  conversions and `%m`; `%n` in a read-only and a writable format with flags 1 and 0, through both
  functions, and masked (children); `%n` through plain `syslog`.
* **Receiver queue full** (`== receiver queue full (datagram)`, child): 4000 messages while the
  receiver does not read.
* **Receiver gone** (`== receiver gone and reconnection`): the receiving socket closed while the
  file stays (with and without `LOG_CONS`), a new receiver at the same path, a receiver replaced
  between two messages.
* **Stream socket** (`== stream socket at /dev/log`): `/dev/log` a listening stream socket:
  `LOG_NDELAY`, five messages read as one byte stream, a lazy connection, a 100000-byte message;
  the peer closing its end while the listener remains, while `/dev/log` is removed, with and
  without `LOG_CONS` (children).
* **No `/dev/log`** (`== /dev/log absent`): without and with `LOG_CONS` (the console a regular
  file), `LOG_PID`, trailing newline, NUL, 5000 bytes, two messages, `LOG_PERROR`, no
  `/dev/console`; `/dev/log` a regular file.
* **FIFO console** (`== /dev/console a FIFO without a reader (no /dev/log)`, child).

## Observations the record shows directly

### Message format (§26.1 `s26_1_c_2`)

* A datagram is `<PRI>TIMESTAMP IDENT[PID]: MESSAGE` with no hostname and no terminator (`first:`
  is `<14><TS> <prog>: first message`, 35 bytes plus the name). `PRI` is decimal without leading
  zeros; `TIMESTAMP` is local time as `Mmm dd hh:mm:ss` (the day space-padded: `Oct  4`), followed
  by one blank. `[PID]` appears only with `LOG_PID` (`LOG_PID:` vs `LOG_NDELAY:`). An empty ident
  leaves the blank before the colon (`ident empty:` is `<6><TS> : x`; with `LOG_PID`,
  `<6><TS> [<pid>]: x`).
* Local time follows `TZ` (`TZ=XST-5:30:` is `<TS>`, and differs from UTC by 5:30). A change of
  `TZ` without `tzset` is not seen: `TZ changed to UTC0 without tzset:` gives `<TS?>` (neither the
  new zone nor UTC), and after `tzset` `<TS>` again.
* The message is the `printf` expansion with `%m` added (`%m` lines): `%m` is `strerror` of
  `errno` at entry, also when other conversions come first or when it appears twice; width and
  precision apply (`[%-30m]`, `[%.5m]`); `%%m` is a literal `%m`; 0 gives `Success`, 99999 and -5
  `Unknown error 99999` / `Unknown error -5`. `vsyslog` behaves the same (`vsyslog %m:`).
  Registered conversions (`%W`) work. `%n` stores the count (`syslog with %n:`, `n=6`).
* Nothing in the message is stripped, escaped or added: trailing newlines stay (`trailing
  newline:`, `two trailing newlines:`, `only a newline:`), embedded newlines, CR LF, control and
  high bytes stay, and a NUL produced by `%c` is sent with the bytes after it (`embedded NUL:`
  `a\x00b\x00c`). The empty message gives `<6><TS> fmt: `.
* There is no length limit in the library: bodies of 900 to 212000 bytes arrive whole in one
  datagram (`body 212000:` 212025 bytes), as do a 2999-byte ident and a 3000-byte format. The only
  limit is the datagram the kernel accepts: 213000 bytes and more are lost (`body 213000: nothing`)
  — see the system calls below. Over a stream, 100000 bytes arrive whole (`stream 100000:`).
* The ident is exactly the bytes given (blank, colon, brackets, newline, control, UTF-8,
  `%s%n%m`: not interpreted).

### Priorities and facilities (§26.1 `s26_1_c_1`, `s26_1_c_2`)

* `PRI` is facility | priority. A priority whose facility bits are 0 gets the default facility:
  `LOG_USER` (8) before any `openlog` (`fac KERN:` equals `fac USER:`, `<8>` to `<15>`). Every
  other facility value in `LOG_FACMASK` is sent as given, also the unnamed ones (`fac 12<<3:`
  `<96>`...) and facility 127 (`pri 0x3fb:` `<1019>`); values from 24<<3 up are not rejected
  (`pri 0xc3:` `<195>`).
* A priority with bits outside `LOG_PRIMASK|LOG_FACMASK` (`pri 0xffffffff`, `0x400`, `0x403`,
  `0x1003`, `0x10003`, `0x80000000`, `0x80000083`, `0x7fffffff`) first sends a separate message
  `<35><TS> IDENT: syslog: unknown facility/priority: %x` (`LOG_AUTH|LOG_ERR`, the value in lower
  case hex; `LOG_PID` was not set in this section), then the original message with the extra
  bits cleared (`0x400` → `<8>`, `0x403` → `<11>`, `-1` → `<1023>`, `0x80000083` → `<131>`).
  `errno` is unchanged (`errno after EDOM`).
* `openlog`'s facility (`== openlog facility`): a value with no bits outside `LOG_FACMASK`
  replaces the default, **including 0** (`openlog facility 0 after LOCAL3: ERR <3> KERN|ERR <3>`:
  messages then carry the kernel facility); any other value (`LOCAL3|ERR`, `0x400`, `-1`, `7`,
  `0x10000|LOCAL1`) is ignored and the previous default stays. `0x3f8` (facility 127) is accepted
  (`<1019>`). An explicit facility in the priority always wins (`DAEMON|ERR <27>`).
* The default facility survives `closelog` (`after openlog LOCAL2 and closelog: ERR <147>`).

### openlog, closelog and the ident (§26.1 `s26_1_c_1`, `s26_1_c_4`; ⚑ HIST; D-26.1)

* The ident is kept **by pointer**: changing the caller's buffer after `openlog` changes the next
  message (`ident buffer changed after openlog:` `second-changed`; emptied: `<6><TS> : x`).
* With no `openlog`, or after `closelog`, the ident is the program's short name (`after
  closelog: <prog>`). `openlog(NULL, ...)` keeps the current ident rather than restoring the
  program name: after `openlog("one")`, `openlog(NULL)` still sends `one`; after `openlog("")`,
  `openlog(NULL)` sends the empty ident (`ident NULL:`). After `closelog`, `openlog(NULL)` uses the
  program name (`perr after openlog 0:`).
* `closelog` resets only the ident (and closes the connection): options and facility stay (`LOG_PID,
  LOCAL1, closelog, syslog:` `<142>` with `[<pid>]`; `perr after closelog:` still copies to
  standard error), and so does the mask (`setlogmask(0) after closelog: 0x40`). `openlog` replaces
  the options entirely (`LOG_PID then openlog without it:` no PID; `perr after openlog 0:` no copy).
* `closelog` with nothing open, or twice, is harmless (`after second closelog: fds +0 -0`).

### The connection (§26.1 `s26_1_c_1_2`, `s26_1_c_4`, `s26_1_g_2`)

* The first message connects (`after first syslog: fds +1 -0; socket unix dgram peer=[/dev/log]
  cloexec=yes nonblock=no`); later messages reuse the descriptor; `closelog` closes it (`fds +0
  -1`). `LOG_NDELAY` connects inside `openlog`; a second `openlog` with `LOG_NDELAY`, or `openlog`
  with 0, while connected neither opens nor closes anything. `LOG_ODELAY` (and 0) connect lazily.
  `LOG_NOWAIT` and unknown option bits change nothing visible.
* A masked message does not connect (`masked message while not connected: fds +0 -0`).
* When `/dev/log` is a listening **stream** socket, the library connects with a stream socket
  (`after openlog LOG_NDELAY: ... socket unix stream`) and **each message is followed by a NUL
  byte** (`stream bytes:` `...one\x00<6><TS> stream: two\x0a\x00...`); there is no octet count.
  A NUL inside a message is sent as is, so the framing is ambiguous (`a\x00b\x00`).
* **Retry once** (`s26_1_g_2`): when a send fails on an established connection, the library
  reconnects and sends the message once more. The record shows it for a datagram receiver replaced
  between two messages (`replaced receiver:` receives `after replacement`) and for a stream peer
  that closed its end while the listener remained (`after the child, further connections:` the
  message arrived on a second connection). If the reconnection fails, the descriptor is closed
  (`after syslog to a closed receiver: fds +0 -1`) and the message goes to the console with
  `LOG_CONS`, otherwise nowhere; the next message tries again (`new receiver:`).
* A full datagram receiver **blocks** the caller: with the receiver not reading, the child's
  4000 messages stop after more than 10 and the child is killed by its alarm; every call that
  returned had queued its datagram (`calls returned == datagrams queued: yes`).
* `/dev/log` absent, or a regular file: nothing is sent, nothing stays open (`after syslog with no
  /dev/log: fds +0 -0`, `after openlog LOG_NDELAY with no /dev/log: fds +0 -0`).
* After `fork` the child sends on the inherited connection with its own PID (`child's message:`
  `fork[<cpid>]`), and the child's `closelog` does not disturb the parent (`parent after:`).

### Failure reporting and errno (§26.1 `s26_1_g_4`)

* No function reports failure. `errno` is preserved by a delivered message (`errno after a
  delivered syslog (EDOM before): EDOM`), by invalid priorities, and when `/dev/log` is absent
  (`no LOG_CONS: errno after EDOM`, also with `LOG_CONS`). It is **not** preserved when a send
  fails: `receiver closed: errno after ECONNREFUSED`, `body 213000: errno after EMSGSIZE` (also with
  `LOG_CONS`), and in children `returned, errno EPIPE` for a closed stream peer.
* A NULL format is a `SIGSEGV` (`NULL format: child killed by SIGSEGV`).

### LOG_CONS (§26.1 `s26_1_c_1_1`, `s26_1_g_3`)

* A message that cannot be delivered is written to `/dev/console` as `IDENT[PID]: MESSAGE\r\n`:
  no priority, no timestamp (`LOG_CONS: console 22 [cons: to the console\x0d\x0a]`). The message is
  cut at its first NUL (`LOG_CONS NUL:` `a\x0d\x0a`); a trailing newline stays before the CR LF
  (`trailing newline\x0a\x0d\x0a`); a 300000-byte message too large for a datagram is written
  whole (`body 300000, LOG_CONS: console 300008`).
* Cases that reach the console: no `/dev/log`, `/dev/log` a regular file, a receiver that closed,
  a stream peer that closed when `/dev/log` is gone, a message too long for a datagram. Not when
  the retry succeeded (`new receiver, LOG_CONS: console 0 []`) or when a stream peer closed but the
  listener remained.
* The console is opened for each message and not kept (`after a LOG_CONS message: fds +0 -0`);
  it is not opened with `O_APPEND` or `O_TRUNC`: on the fixture's regular file a second, shorter
  message overwrites the start of the first (`LOG_CONS two messages:` `...: 2\x0d\x0ast,
  longer\x0d\x0a`).
* No `/dev/console`: the message is lost silently (`LOG_CONS, no /dev/console: console ENOENT`,
  `still alive`).
* `LOG_CONS` and `LOG_PERROR` together write both (`LOG_CONS|LOG_PERROR:`).

### LOG_PERROR (§26.1 `s26_1_c_1_4`)

* The copy on standard error is `IDENT[PID]: MESSAGE` plus a newline unless the message already
  ends with one (`perr trailing newline: stderr ... trailing newline\x0a`, one newline; `two
  newlines` keeps both and adds none). No priority, no timestamp. It is cut at the first NUL
  (`perr NUL: stderr 17 [perr[<pid>]: a\x0a]`) while the datagram keeps `a\x00b`. It is written
  whether or not the message is delivered (`LOG_PERROR without LOG_CONS:`, with no `/dev/log`),
  not for a masked message (`perr masked priority: stderr 0 []`).
* `stderr`'s orientation is untouched (`stderr orientation after LOG_PERROR: 0`).

### setlogmask (§26.1 `s26_1_c_3`, `s26_1_g_1`)

* The initial mask is `0xff`. `setlogmask(m)` returns the previous mask and installs `m`, except
  that `m == 0` changes nothing (`setlogmask(0) again: 0xff`). Any other value is stored whole:
  `setlogmask(-1)` then `setlogmask(0)` gives `0xffffffff`; `0x100` is accepted and passes no
  priority at all.
* A message passes when its priority's bit (`LOG_MASK(pri & LOG_PRIMASK)`) is set; the facility
  plays no part (`mask ERR, LOCAL0|3: <131>`).
* The mask is applied before the invalid-priority check: masked invalid priorities send nothing,
  not even the `unknown facility/priority` message (`mask INFO, invalid pri 0x10003: nothing`).
* A masked message is not formatted: the counting `%W` conversion is not called (`count 0`), and
  `__syslog_chk` with a writable `%n` format does not abort (`chk masked writable %n: child exited
  0`).

### The checking forms (§26.1)

* `__syslog_chk(pri, flag, fmt, ...)` and `__vsyslog_chk(pri, flag, fmt, ap)` behave as `syslog` and
  `vsyslog` (`__syslog_chk flag 1:`, `__vsyslog_chk flag 1:`, flag -1). With flag > 0, a `%n` in a
  format in writable memory aborts with `*** %n in writable segment detected ***\n` on standard
  error (`chk writable %n: child killed by SIGABRT`; the fixture has no `/dev/tty`); in a
  read-only format it works (`chk read-only %n:` `n=3`). With flag 0 a writable `%n` works (`n=4`).

### System calls

A trace of the probe (`strace -f`, a separate run, not part of the record):

* Connecting: `socket(AF_UNIX, SOCK_DGRAM|SOCK_CLOEXEC, 0)`, `connect` to `/dev/log`; on
  `EPROTOTYPE`, `close` and the same with `SOCK_STREAM`. A reconnection after a failed stream send
  goes straight to `SOCK_STREAM`.
* Sending: one `sendto(fd, message, length, MSG_NOSIGNAL, NULL, 0)` per message (the stream form
  includes the trailing NUL in the same call). `MSG_NOSIGNAL` is why a closed stream peer gives
  `EPIPE` and no `SIGPIPE`.
* On a failed send (`ECONNREFUSED`, `EPIPE`, `EMSGSIZE`): `close`, a new `socket` and `connect`,
  and one more `sendto`; on a second failure `close`, then with `LOG_CONS`
  `openat(AT_FDCWD, "/dev/console", O_WRONLY|O_NOCTTY|O_CLOEXEC)`, `write` (in 2048-byte pieces for
  long messages), `close`. The `EMSGSIZE` limit on the VM was a datagram of 212025 bytes accepted
  and 213025 refused, matching `net.core.wmem_default` = 212992.
* `LOG_PERROR`: `write(2, ...)` (2048-byte pieces for a long message).
* `getpid` is called for messages; no `clock_gettime` system call appears (the vDSO serves it).
* The `%n` abort: `writev(2, ...)` of the message, then `SIGABRT`.

## Observations that look like historical artifacts

The implementer may decide these differently (and record the difference).

* `openlog` with facility 0 makes `LOG_KERN` the default, so a user process sends `<3>`-style
  kernel-facility messages; with no `openlog` the default is `LOG_USER`. Invalid facilities are
  silently ignored, but facilities 24 to 127 are accepted.
* `openlog(NULL)` keeps the previous ident instead of restoring the program name.
* `closelog` keeps options, facility and mask but resets the ident.
* The invalid-priority message uses `LOG_AUTH|LOG_ERR` and the hex value; the original message is
  then sent with the bad bits cleared rather than dropped.
* `errno` leaks from a failed send (`ECONNREFUSED`, `EMSGSIZE`, `EPIPE`) but not from a failed
  connect.
* A full receiver blocks the caller indefinitely; a FIFO `/dev/console` without a reader blocks
  `syslog` in `open` (`fifo console: child killed by SIGALRM`).
* With descriptor 2 closed, the socket becomes descriptor 2 and the `LOG_PERROR` copy is sent to
  `/dev/log` as an extra datagram (`stderr closed:` two datagrams, the first being the standard-
  error form `closed2: with stderr closed\x0a`).
* A NUL in a message is sent over the socket but cuts the console and standard-error copies; over
  a stream it breaks the NUL framing.
* Messages longer than a datagram can carry are dropped (or go to the console), not truncated.
* A `TZ` change without `tzset` is not picked up.

## Spec, record and documentation disagreements

* **PID in the prefix.** §26.1 `s26_1_c_2` says the message is "prefixed with priority,
  timestamp, identifier and PID"; the record (and `syslog(3)`, where `LOG_PID` is the option that adds
  the caller's PID) shows the PID only with `LOG_PID`.
* **`openlog(NULL)`.** `openlog(3)` says that a NULL ident means the program name; the record
  keeps the previous ident (it is the program name only when none was set, or after `closelog`).
  POSIX is silent about NULL.
* **"It never reports failure"** (`s26_1_g_4`): true of return values, but `errno` changes after a
  failed send. POSIX does not say `syslog` preserves `errno`.
* **Retry once** (`s26_1_g_2`): confirmed; the record adds that the retry also follows `EMSGSIZE`
  (trace) and that the console fallback happens only after the retry fails.
* **RFC 3164.** §26.1 calls the wire format "RFC 3164-style". RFC 3164 §4.1 puts a HOSTNAME after
  the TIMESTAMP and limits a packet to 1024 bytes; the record has no hostname and sends up to the
  kernel's datagram limit (212025 bytes here). RFC 3164 also describes UDP, not `/dev/log`; the
  record is the local form only.
* **Kernel line.** §26.1 lists `clock_gettime` and `getpid`; `clock_gettime` never appears as a
  system call (vDSO). `send` is `sendto` with `MSG_NOSIGNAL`.
* **`LOG_CONS`** "write to `/dev/console` on failure": the record shows the console form differs
  from the socket form (no priority, no timestamp, CR LF appended, cut at NUL).

## What the record does not settle

* Thread safety and concurrent use of the connection (out of scope for the record; D-26.1 makes
  the connection synchronised).
* D-26.1's optional structured journald protocol: the reference speaks only the text protocol to
  `/dev/log`; nothing here constrains a journald path.
* Two-digit days: the record was made on 4 October, so only the `%2d` space-padded form of a
  one-digit day was seen (a two-digit day prints the same with `%2d` or `%02d`).
* Locales: the probe runs in the C locale; whether month names or `%m` follow `LC_TIME` /
  `LC_MESSAGES` is not probed.
* `/dev/console` as a real terminal (the fixture is a regular file or a FIFO), and the case where
  opening the console fails for reasons other than `ENOENT`.
* The exact number of datagrams that fit before a full receiver blocks, and the largest datagram,
  depend on kernel settings (`net.unix.max_dgram_qlen`, `net.core.wmem_default`); the record states
  only "more than 10" and the outcome.
* What a signal does to a call blocked on a full receiver.
* `LOG_NOWAIT`: no visible effect was found; whether it has any (it historically concerned
  waiting for console children) is not observable here.
* `LOG_PERROR` with `stderr` wide-oriented or with pending stdio output: the trace shows direct
  `write(2, ...)` calls, but interleaving with buffered `stderr` output is not probed.
* `__syslog_chk`'s abort message destination when `/dev/tty` exists (the fixture has none).
