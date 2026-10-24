# Terminals, job control and pseudo-terminals

**Spec:** §19.1 (`tcgetattr`, `tcsetattr`, `cfgetispeed`, `cfgetospeed`, `cfsetispeed`,
`cfsetospeed`, `cfsetspeed`, `cfmakeraw`, `tcsendbreak`, `tcdrain`, `tcflush`, `tcflow`,
`tcgetsid`, `isatty`, `gtty`/`stty`), §19.2 (`tcgetpgrp`, `tcsetpgrp`, `ctermid` with §12.12),
§19.3 (`ttyname`, `ttyname_r`), §19.4 (`posix_openpt`, `getpt`, `grantpt`, `unlockpt`, `ptsname`,
`ptsname_r`, `openpty`, `forkpty`, `login_tty`), §19.5 (`getttyent`, `getttynam`, `setttyent`,
`endttyent`, `gtty`, `stty`, `revoke`); plan phase 11 test surface (the `tcsetattr`
partial-application semantics, D-19.1 to D-19.3). Public interfaces: `<termios.h>`,
`<unistd.h>`, `<stdlib.h>`, `<stdio.h>`, `<pty.h>`, `<utmp.h>`, `<ttyent.h>`, `<sgtty.h>`.
Normative references (plan §1.2): POSIX.1-2017 (General Terminal Interface, chapter 11, and the
functions above), the glibc manual (Low-Level Terminal Interface), the Linux man pages
(`termios(3)`, `tty_ioctl(4)`, `pts(4)`, `openpty(3)`, `ptsname(3)`, `ttyname(3)`,
`getttyent(3)`, `ttys(5)` on BSD).

**Clauses:** `s12_12_c_3`, `s19_1_c_1`, `s19_1_c_1_1`, `s19_1_c_1_2`, `s19_1_c_1_3`, `s19_1_c_2`,
`s19_1_c_3`, `s19_1_c_4`, `s19_1_c_5`, `s19_1_c_6`, `s19_1_f_0`, `s19_2_c_1`, `s19_2_c_2`,
`s19_3_c_0`, `s19_3_f_1`, `s19_3_f_2`, `s19_3_f_3`, `s19_4_c_1`, `s19_4_c_2`, `s19_4_c_3`,
`s19_4_c_4`, `s19_4_c_5`, `s19_4_c_6`, `s19_4_c_7`, `s19_4_f_0`, `s19_5_c_0`.

**Probe:** `crates/core/tests/oracle/terminals_diff.c`, run by `terminals_run.sh PROGRAM
SCRATCH`. **Record:** `terminals.glibc.txt` (636 lines). It was built with `cc -O0`
(`-std=gnu17 -O1` prints the same) and run on the reference system twice; the two outputs are
byte-identical, and a third run with two CPU-bound processes in the background was identical too.
No `-lutil` is needed on 2.39.

The first phase runs unprivileged on ptys the program creates; everything that needs a
controlling terminal happens in children that start their own session, so the caller's terminal
is never touched, and every child has an `alarm` and is reaped. The second phase runs as root in
a private mount namespace (`unshare -m`) with the scratch directory's fixture `etc/` bind-mounted
over `/etc` (the users_run.sh pattern); inside it the program mounts a tmpfs or a new devpts
instance over `/dev/pts`, binds that instance's `ptmx` over `/dev/ptmx` and puts a tmpfs over
`/proc`. All of these vanish with the namespace. A pty is printed as `/dev/pts/<n>`; process ids
only as comparisons (`tcgetpgrp(slave)==getpgrp: 1`). Termios values are the four flag fields in
hex, speeds by `B*` name, and the first 17 control characters.

## What the probe does

* Speeds, on structures only: `cfsetospeed`, `cfsetispeed` and `cfsetspeed` with every `B*`
  constant from `B0` to `B4000000`; with numeric rates (50 ... 4000000), values that are not
  codes (16, 17, 0x1010, 0x2000, ...), `BOTHER` (0x1000, the Linux value: `<termios.h>` does not
  define it); separate input and output speeds; the input speed 0 rule; the getters on a
  `c_cflag` written directly (with and without `CIBAUD` bits, all ones, `BOTHER`).
* `cfmakeraw` from all-ones and all-zero structures: the flags it clears, `VMIN`/`VTIME`, the
  other control characters and the speeds.
* `tcgetattr`/`tcsetattr` on a pty: the default settings, master against slave, the
  `c_cc` bytes past the kernel's control characters, every error (`/dev/null`, a file, a pipe,
  -1, a closed descriptor), the three actions and six invalid ones, pending input under each
  action, partial application (all flag bits requested; `CS7 PARENB PARODD CSTOPB`), every speed
  through the terminal, split speeds, input speed 0, `BOTHER`, settings written through the
  master, `B0`.
* `tcsendbreak`, `tcdrain`, `tcflush` (every queue, invalid queues, and the effect on data in
  each direction, flushed from either side), `tcflow` (output stopped and restarted, the STOP and
  START characters `TCIOFF`/`TCION` transmit, with changed and disabled `VSTOP`), each on the
  slave, the master, `/dev/null` and -1.
* `isatty` on every kind of descriptor; `ctermid` with NULL and a buffer, `L_ctermid`.
* `ttyname` and `ttyname_r`: slave, master, non-terminals, -1, the static buffer, every buffer
  size around the name's length, a NULL buffer (in a child), a slave opened through a symbolic
  link, a duplicate.
* `posix_openpt` with fifteen flag combinations, `getpt`, the locked slave, `grantpt` (the slave's
  mode, owner and group before and after), `unlockpt`, `ptsname`, `ptsname_r` (sizes, NULL buffer
  in a child, non-masters), a round trip, the master after the slave closes.
* `openpty`: descriptors opened and their flags, default settings and window size, `name`,
  `termp`, `winp`, a `termp` the pty does not take in full, NULL `amaster`/`aslave` (in children).
* In a new session (a child): which calls make a pty the controlling terminal
  (`posix_openpt`, `getpt`, `openpty`, `open` of the slave without `O_NOCTTY`), `tcgetpgrp`,
  `tcgetsid`, `tcsetpgrp` with every error, `ttyname` and `ctermid` of `/dev/tty`; a background
  job (its own process group) calling `tcsetpgrp`, `tcsetattr`, `tcdrain`, `tcflush`, `tcflow`,
  `tcsendbreak`, `tcgetattr` with `SIGTTOU` default, ignored and blocked; the same from an
  orphaned process group.
* `login_tty` on a slave, `/dev/null`, -1, a master, descriptor 0, in a process that already
  leads a session, and on another session's controlling terminal.
* `forkpty` with and without `name`/`termp`/`winp`: the child's session, terminal, settings, window
  size and descriptors, the parent's descriptors, the bytes the child writes.
* A pending cancellation request at the entry of `tcdrain`, `tcsendbreak`, `tcsetattr`
  (`TCSANOW`, `TCSADRAIN`), `tcflush`, `tcflow`, `tcgetattr`, `isatty`, `ttyname_r`, `openpty`.
* `gtty`, `stty`, `revoke`.
* Namespace phase: `getttyent` over a fixture `/etc/ttys` (comments, blank and white-space
  lines, indented entries, quoted fields with escaped quotes and `#`, trailing comments with and
  without space, unknown and upper-case flags, `window=`, empty quoted fields, an unterminated
  quote, a duplicate name, a last line without a newline), the static entry, `endttyent` then
  `getttyent`, `setttyent` rewinding, `getttynam` (found, missing, empty, prefix, duplicate,
  during an enumeration), the three functions without the file, lines of 99, 100 and 300 bytes.
* Namespace phase: `ttyname` of a pty whose `/dev/pts` entry is gone (tmpfs), whose devpts
  instance is not the one mounted (new instance), whose number exists in the other instance, and
  with `/proc` hidden (for a pty of the mounted instance and of the other one); `ptsname` and
  `openpty` without `/proc`.

## Observations the record shows directly

**Speeds** (`cfsetospeed B...`, `cfsetspeed 9600`, `after ispeed 0`, `direct cflag ...`).

* Every `B*` code is accepted by all three setters; the code goes into the `CBAUD` bits of
  `c_cflag` (`B57600` and up carry `CBAUDEX`, 0x1000), and both getters then return it. The
  structure holds one speed: `cfsetispeed` with a non-zero code changes `cfgetospeed` too, and
  `cfsetospeed` changes `cfgetispeed`. The getters ignore the `CIBAUD` bits (`direct cflag B9600
  CIBAUD=B2400: ospeed=B9600 ispeed=B9600`) and read `c_cflag & CBAUD`, so a structure filled in
  directly reports its `CBAUD` code.
* `cfsetospeed`/`cfsetispeed` with a value that is not a code fail with `EINVAL` and leave the
  structure unchanged; this includes the numeric rates (9600, 115200). `cfsetspeed` accepts both
  the codes and the numeric rates of the standard speeds (`cfsetspeed 9600: 0 ... ospeed=B9600`,
  4000000 gives `B4000000`); other numbers are `EINVAL`.
* Input speed 0: `cfsetispeed(&t, 0)` succeeds, leaves `c_cflag` and the output speed alone and
  makes `cfgetispeed` return `B0`; a later `cfsetospeed` leaves it at `B0`. Through the terminal,
  input speed 0 means "the output speed": `tcsetattr` then `tcgetattr` gives
  `ospeed=B19200 ispeed=B19200`.
* `BOTHER` (0x1000): `cfsetospeed` and `cfsetispeed` accept it and the getters return 0x1000;
  `cfsetspeed` rejects it. The pty takes it and reads it back as 0x1000. No public function
  supplies a rate to go with it, so arbitrary rates are not reachable through the 2.39 API
  (§19.1 ⚑ KERN; D-19.1 is internal).

**cfmakeraw** (`cfmakeraw ...`). It clears exactly `IGNBRK BRKINT PARMRK ISTRIP INLCR IGNCR
ICRNL IXON`, `OPOST`, `ECHO ECHONL ICANON ISIG IEXTEN` and `PARENB`, sets `CSIZE` to `CS8`, sets
`VMIN=1` and `VTIME=0`, and leaves every other bit, every other control character and both
speeds as they were.

**tcgetattr/tcsetattr** (`slave default`, `tcsetattr ...`, `... applied`, `all flag bits read
back`, `read back: CSIZE=...`, `cc read back`, `pty speed ...`).

* A new pty's settings: `iflag=0x500 oflag=0x5 cflag=0xbf lflag=0x8a3b`, `B38400`, the control
  characters shown on the `slave default` line; the master reports the same settings, and
  settings written through the master are the slave's.
* Errors: `ENOTTY` for `/dev/null`, a file, a pipe; `EBADF` for -1 and a closed descriptor.
  `tcsetattr` with an action other than 0, 1, 2 (3, 4, -1, 0x10, 0x7fffffff, 0x100) is `EINVAL`
  and changes nothing, also on `/dev/null` and on -1 (the action is checked first).
  `TCSANOW=0 TCSADRAIN=1 TCSAFLUSH=2`; `TCSASOFT` is not defined.
* `TCSAFLUSH` discards input that is waiting; `TCSANOW` and `TCSADRAIN` keep it.
* Partial application (§19.1 ⚑ HIST): `tcsetattr` reports success (0) when the terminal takes
  only part of the request. Requesting every flag bit reads back
  `iflag=0x7fffffff cflag=0xcff0eefd`; requesting `CS7 PARENB PARODD CSTOPB` together with an
  `ECHO` change returns 0, and the re-read shows `CS8`, `PARENB` cleared, `PARODD`/`CSTOPB` kept,
  and the `ECHO` change applied. Callers must re-read to verify, as POSIX says.
* Control characters 0-18 round-trip (`cc[17]` and `cc[18]` included); bytes 19-31 of `c_cc`
  (`NCCS` is 32) read back as 0 whatever was written, and `tcgetattr` writes 0 there even into a
  structure filled with 0x77 (§19.1 ⚑ KERN: the two structures differ).
* Every speed from `B50` to `B4000000` is set and read back unchanged on a pty. `B0` is accepted;
  afterwards `tcgetattr` reports `B0` and the slave still writes.

**Line control** (`tcsendbreak ...`, `tcdrain ...`, `tcflush ...`, `flush ...`, `tcflow ...`,
`TCIOFF ...`).

* `tcsendbreak` on a pty returns 0 for any duration (0, 10, -1) on either side and puts no bytes
  in either direction. `tcdrain` returns 0 on both sides. Both give `ENOTTY` and `EBADF` as above.
* `tcflush`: an unknown queue (3, -1) is `EINVAL` on a terminal. On the slave, `TCIFLUSH` and
  `TCIOFLUSH` discard input waiting to be read on the slave; `TCOFLUSH` and `TCIFLUSH` on the
  slave leave the slave's output readable at the master; `TCIFLUSH` on the master discards it;
  `TCOFLUSH` on the master leaves the slave's input.
* `tcflow(TCOOFF)` stops output: a non-blocking write on the slave fails with `EAGAIN` and the
  master sees nothing; after `TCOON` a write succeeds and reaches the master. `TCIOFF` and
  `TCION` transmit the `VSTOP` and `VSTART` characters (0x13 and 0x11 by default; 0x01 and 0x02
  when changed) to the master; with `VSTOP` disabled nothing is sent. An unknown action (4, -1)
  is `EINVAL`. The master accepts `TCOOFF`/`TCOON`.

**isatty, ctermid** (`isatty ...`, `ctermid ...`). `isatty` is 1 for both pty sides; 0 with
`ENOTTY` for `/dev/null`, a file, a directory, a pipe and a socket; 0 with `EBADF` for -1 and a
closed descriptor; on success `errno` is left as it was. `ctermid` returns `"/dev/tty"`
always (also with a controlling terminal); with NULL the same static buffer each time, with a
buffer that buffer. `L_ctermid` is 9.

**ttyname** (`ttyname ...`, `ttyname_r ...`).

* The slave's name is its `/dev/pts/<n>` path, equal to `ptsname` of the master, also for a
  slave opened through a symbolic link and for a duplicate; the master's name is `/dev/ptmx`.
  `ttyname` returns the same static buffer for every descriptor.
* Errors: `ENOTTY` for `/dev/null`, a pipe, a file; `EBADF` for -1 and a closed descriptor;
  `ttyname` returns NULL and sets `errno`. `ttyname_r` returns the error number and also sets
  `errno` to it.
* `ttyname_r` with a buffer of 0, 1 or `len-1` bytes is `ERANGE` with the buffer untouched;
  `len+1` succeeds. `ttyname_r` on `/dev/null` with size 0 is `ERANGE` (the size is checked
  before the descriptor). A NULL buffer is `EINVAL` (in a child; the parameter is declared
  `nonnull`).
* §19.3 `ENODEV` (namespace phase): when the `/dev/pts` entry is gone (a tmpfs over
  `/dev/pts`), when the pty belongs to a devpts instance other than the one mounted, and when the
  mounted instance has a pty with the same number, `ttyname` is NULL with `ENODEV` and
  `ttyname_r` returns 19; `isatty` is still 1 and `ptsname` of the master still returns the
  `/dev/pts/<n>` name (it is computed from the pty number, not checked against `/dev`).

**Pseudo-terminals** (`posix_openpt ...`, `getpt`, `grantpt ...`, `unlockpt ...`, `ptsname ...`,
`ptsname_r ...`, `round trip`, `openpty ...`).

* `posix_openpt` returns a master with the access mode, `O_NONBLOCK`, `O_APPEND` and `O_CLOEXEC`
  of the flags given; `O_NOCTTY`, `O_CREAT`, `O_EXCL`, `O_TRUNC` and unknown bits are accepted
  without effect. `getpt` returns an `O_RDWR` master without `FD_CLOEXEC`.
* Opening the slave before `unlockpt` fails with `EIO`. The slave exists with mode 620, the
  caller's uid and gid 5 before `grantpt`; `grantpt` changes nothing visible. `grantpt` and
  `unlockpt` succeed repeatedly on a master.
* Not a master: `grantpt` and `unlockpt` give `EINVAL` for a slave, `/dev/null` and a file, and
  `EBADF` for -1; `ptsname` and `ptsname_r` give `ENOTTY` for those and `EBADF` for -1.
* `ptsname` returns one static buffer (two masters, same pointer, different names).
  `ptsname_r` needs `strlen(name)+1` bytes: smaller sizes are `ERANGE` with the buffer
  untouched; it returns the error number and sets `errno`.
* A round trip in raw mode carries the bytes both ways; after the slave closes, a read on the
  master fails with `EIO`.
* `openpty` opens exactly two descriptors, both `O_RDWR` without `FD_CLOEXEC`; the master's
  `ttyname` is `/dev/ptmx`, the slave's equals `ptsname(master)`. Without `termp` the settings are
  the defaults above and the window size is 0x0. `name` receives the slave's path; `termp` is
  applied exactly when the pty can take it, and when it cannot `openpty` still returns 0 (the
  same partial application); `winp` sets the window size, seen on both sides. With `/proc`
  hidden `openpty` still works and names the slave.

**Controlling terminal and job control** (child `ctty`: `new session ...`, `tcgetpgrp ...`,
`tcsetpgrp ...`, `background ...`, `orphaned background ...`).

* In a new session without a terminal, `/dev/tty` cannot be opened (`ENXIO`); `tcgetpgrp`,
  `tcgetsid` and `tcsetpgrp` on a slave that is not the controlling terminal are `ENOTTY`.
* Opening a master (`posix_openpt(O_RDWR)` without `O_NOCTTY`, `getpt`) does not give the
  session a controlling terminal, and neither does `openpty`; opening the slave by name without
  `O_NOCTTY` does.
* With the slave as controlling terminal: `tcgetpgrp` on the slave, the master and `/dev/tty`
  returns the caller's group; `tcgetsid` on the slave and the master returns the session;
  `ttyname` of a `/dev/tty` descriptor is `"/dev/tty"`.
* `tcsetpgrp`: the caller's own group succeeds, also through the master; -1 and `INT_MIN` are
  `EINVAL`; 0 and an unused id are `ESRCH`; a group of another session is `EPERM`; `/dev/null`
  is `ENOTTY`, -1 is `EBADF`.
* A background process (§19.2 and §19.1 `EIO`): with `SIGTTOU` at its default action,
  `tcsetpgrp`, `tcsetattr`, `tcdrain`, `tcflush`, `tcflow` and `tcsendbreak` stop it with
  `SIGTTOU`; with `SIGTTOU` ignored or blocked they succeed (`tcsetpgrp` then makes its group the
  foreground). `tcgetattr`, `tcgetpgrp` and `tcgetsid` work from the background without a signal.
  In an orphaned background group, `tcsetattr`, `tcdrain` and `tcflush` fail with `EIO` instead of
  stopping, and `tcgetattr` still works.

**login_tty** (`login_tty ...`). On a slave: returns 0, the process leads a new session with the
slave as controlling terminal and its own group in the foreground, descriptors 0-2 are the slave
(without `FD_CLOEXEC`), and the original descriptor (above 2) is closed. On `/dev/null` it is -1
with `ENOTTY`; on -1, `EBADF`. On another session's controlling terminal it fails with `EPERM`
(unprivileged) and the other session keeps the terminal. When the descriptor is 0, it stays open
and 1 and 2 become the terminal.

**forkpty** (`forkpty ...`, `child: ...`, `parent: ...`). The parent gets one new descriptor, the
master (the slave is not left open in the parent), `name` equals `ptsname(master)`; the child
leads a new session with the slave as controlling terminal on 0-2, no other terminal descriptors
open, `*amaster` not set in the child, and the `termp` and `winp` given. Without `termp` the
output processing of the default settings turns the child's `\n` into `\r\n`. The child's exit
status reaches the parent.

**Cancellation** (`cancel pending, ...`). With a cancellation request pending, `tcdrain` and
`openpty` act on it; `tcsendbreak`, `tcsetattr` (`TCSANOW` and `TCSADRAIN`), `tcflush`, `tcflow`,
`tcgetattr`, `isatty` and `ttyname_r` return normally.

**Stubs** (`gtty ...`, `stty ...`, `revoke ...`). `gtty`, `stty` and `revoke` return -1 with
`ENOSYS` for every argument, and `revoke` leaves the terminal usable (§19.5). The link editor
warns that they are not implemented.

**ttyent** (namespace phase; `getttyent N`, `getttynam ...`, `long lines N`).

* `_PATH_TTYS` is `/etc/ttys`; `TTY_ON` 1, `TTY_SECURE` 2. `setttyent` returns 1 when it opens
  the file and 0 with `ENOENT` without it; `endttyent` returns 1, also when nothing is open and
  when called twice.
* Fields are separated by spaces and tabs; leading white space is skipped; comment lines and
  blank or white-space-only lines are skipped. The fields after name, getty and type are the
  words `on` (sets `TTY_ON`), `off`, `secure` (sets `TTY_SECURE`) and `window=VALUE` (in any
  order, `on off` leaving it off). A word starting with `#` starts `ty_comment`, which runs to the
  end of the line without the `#` and the white space after it (`# trailing comment` gives
  `trailing comment`). Any other word in the flags starts the comment too, kept whole
  (`on#nospace`, `insecure`, `unknown`). Words are compared case-sensitively (`ON SECURE` is a
  comment).
* A double-quoted field may contain white space, `#` and `\"` (unescaped in the result); an
  empty quoted field is `""`; a quote left open runs to the end of the line. In the getty and
  type fields, a `#` inside an unquoted word ends the field and starts the comment (`t#z` gives
  type `t` and the comment `z\x09on`, the tab kept), whereas in the flags `on#nospace` is one
  word.
* Missing fields: a line with only a name gives NULL getty, type, window and comment; a name
  followed by a comment gives an empty getty and NULL type.
* `getttyent` returns the same static entry each time, returns NULL with `errno` 0 at the end and
  again after it, and after `endttyent` starts again from the first entry without `setttyent`.
  `setttyent` rewinds.
* `getttynam` returns the first entry with that exact name (no prefix match), NULL with `errno`
  0 for a missing or empty name, and the next `getttyent` after it starts at the first entry.
  Without the file it is NULL with `ENOENT`.

## Observations that look like historical artifacts

* `ttyname_r` with a buffer of exactly `strlen(name)` bytes fails with `ENODEV`, not `ERANGE`,
  and writes into the buffer (`ttyname_r size len: 19 (ENODEV) ... buf=[written]`); one byte
  less is `ERANGE` with the buffer untouched.
* `ptsname_r` with a NULL buffer crashes (`SIGSEGV`), although the man page lists `EINVAL` for
  it, while `ttyname_r` returns `EINVAL`. `openpty` with a NULL `amaster` or `aslave` crashes.
  Both parameters are outside the headers' `nonnull` contract.
* `posix_openpt` passes its flags to `open` unchecked: `O_RDONLY` and `O_WRONLY` masters,
  `O_DIRECTORY` (and -1) fail with `ENOTDIR`, and `O_PATH` returns a descriptor that is not a
  terminal. POSIX allows `EINVAL` for flags other than `O_RDWR`/`O_NOCTTY`; it never occurs.
* Errors are not ordered consistently: `tcsetattr` reports an invalid action as `EINVAL` even on
  `/dev/null` and -1, while `tcflush` and `tcflow` on `/dev/null` with an invalid argument report
  `ENOTTY`. `grantpt`/`unlockpt` call a non-master `EINVAL`, `ptsname`/`ptsname_r` call it
  `ENOTTY` (§19.4 lists both).
* In an orphaned background group `tcsetpgrp` fails with `ENOTTY`, not `EIO` as the other
  calls do (`orphaned background tcsetpgrp ...`). The same `ENOTTY` would be what the kernel
  reports; it is recorded as seen.
* `tcgetpgrp` on a master whose slave is no session's controlling terminal returns 0 with no
  error, while on the slave it is `ENOTTY`.
* `login_tty` in a process that already leads a session returns 0 with `errno` left at `EPERM`;
  a failed `login_tty` (on `/dev/null`) has already made the caller a session leader. `login_tty`
  accepts a master and makes it the controlling terminal.
* `tcsendbreak` accepts a negative duration as 0.
* `cfsetspeed` takes numeric rates while `cfsetospeed`/`cfsetispeed` do not; `BOTHER` is the
  reverse. The input speed is either "same as output" or `B0`: separate input and output speeds
  cannot be expressed in the structure, and `CIBAUD` is ignored.
* ttyent: a line of 100 bytes or more (newline included) is skipped whole, as is a last line
  without a newline, however short (`nonl`); `getttynam` cannot find either. A line of 99 bytes
  is read.
* `isatty` leaves `errno` unchanged when it returns 1.
* Bytes 19-31 of `c_cc` are not kept and read back as 0.

## System calls (strace, outside the record)

Observed once with `strace` on small programs that call each function between marker writes
(the §19 "Kernel" lines; not part of the differential test, and only the requests that reach the
kernel are stated):

* `posix_openpt(f)`: `openat("/dev/ptmx", f)`; `getpt`: `openat("/dev/ptmx", O_RDWR)` (no
  `O_NOCTTY`). `grantpt`: `TIOCGPTN` only. `unlockpt`: `TIOCSPTLCK` with 0. `ptsname`/`ptsname_r`:
  `TIOCGPTN`.
* `tcgetattr` and `isatty`: `TCGETS`. `tcsetattr`: `TCGETS`, then `TCSETS`/`TCSETSW`/`TCSETSF`
  for `TCSANOW`/`TCSADRAIN`/`TCSAFLUSH`, then `TCGETS` again; with an invalid action, only the
  first `TCGETS`. No `TCSETS2`/`TCGETS2` is used (§19.1 ⚑ KERN; D-19.1 differs internally).
* `tcsendbreak(fd, d)`: `TCSBRK 0` for `d <= 0`; `TCSBRKP` with `d` rounded up to tenths of a
  second (`ceil(d/100)`: 1, 99, 100 give 1; 101, 150 give 2; 250 gives 3; 1000 gives 10).
  `tcdrain`: `TCSBRK 1`. `tcflush`: `TCFLSH`. `tcflow`: `TCXONC`. `tcgetsid`: `TIOCGSID`.
  `tcgetpgrp`/`tcsetpgrp`: `TIOCGPGRP`/`TIOCSPGRP`. `ctermid`: none.
* `ttyname`/`ttyname_r`: `TCGETS`, `fstat`, `readlink("/proc/self/fd/N")`, `newfstatat` of the
  link target. Without `/proc` (namespace): `readlink` fails with `ENOENT`, then `/dev/pts/` and
  then `/dev/` are opened and read with `getdents64`, and entries are `newfstatat`'d; the slave of
  the mounted instance is found, the master is found as `/dev/ptmx`.
* `openpty`: `openat("/dev/ptmx", O_RDWR)`, `TIOCGPTN`, `TIOCSPTLCK 0`, then
  `TIOCGPTPEER` with `O_RDWR|O_NOCTTY` (the slave is never opened by name, which is why it works
  without `/proc` and never becomes a controlling terminal); with `termp`, `tcsetattr(TCSAFLUSH)`
  on the slave; with `winp`, `TIOCSWINSZ` on the slave; with `name`, `TIOCGPTN`. (D-19.2 matches.)
* `forkpty`: the `openpty` sequence, `clone`; the parent closes the slave, the child closes the
  master and runs `login_tty`: `setsid`, `TIOCSCTTY 0`, `dup2` to 0, 1, 2, `close`.

## What the record does not settle

* D-19.3: without `/proc`, the reference finds the name by scanning `/dev/pts` and `/dev`
  (`no /proc: ttyname /dev/pts/<n> errno=0`) and reports `ENOTTY` (not `ENODEV`) for a pty of
  another instance (`no /proc, old instance: ttyname NULL errno=ENOTTY`). D-19.3 decides
  `ENODEV` without `/proc`, so these two lines are expected to differ by decision.
* `EINTR` (§19.1): `tcdrain` and `TCSADRAIN` never block on a pty, so an interrupted wait cannot
  be produced; the same goes for the time `tcsendbreak` takes on a real line.
* Partial application where the driver refuses a speed: ptys accept every speed, so whether
  `tcsetattr` ever reports failure after the kernel took part of a request is not shown (the
  record has only success with bits dropped).
* `EAGAIN`/`ENOENT` "no ptys available" (§19.4): reaching the system-wide pty limit would affect
  the machine and was not tried.
* The contents of the structure's `c_ispeed`/`c_ospeed` members: deliberately not printed (the
  probe observes speeds only through the getters).
* Real terminals (`/dev/ttyN`, serial lines, `/dev/console`): none on the reference machine;
  `ttyname` through the `/dev` scan for such devices and their break and speed behaviour are not
  recorded.
* `__ttyname_r_chk` and `__ptsname_r_chk` (fortified builds) are not exercised.
* As root, `login_tty`/`TIOCSCTTY` on another session's terminal may behave differently from the
  unprivileged `EPERM` recorded; the root phase does not try it.
