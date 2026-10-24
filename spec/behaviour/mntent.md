# Mount tables and fstab: setmntent, getmntent, getmntent_r, addmntent, endmntent, hasmntopt, the fsent functions

**Spec:** §13.13 (`setmntent`, `getmntent`, `getmntent_r`, `__getmntent_r`, `addmntent`,
`endmntent`, `hasmntopt`, `setfsent`, `getfsent`, `getfsspec`, `getfsfile`, `endfsent`; clauses
`s13_13_c_1` to `s13_13_c_4`), with the decision rows D-13.8 (mount tables come from
`/proc/self/mountinfo` internally, `getmntent` keeps its text format) and D-13.10 (the functions
move to phase 12 because they return or take `FILE *`, phase 8's streams) of
implementation-plan.md. Public interfaces: `<mntent.h>`, `<fstab.h>`, `<paths.h>`. Normative
references (plan §1.2): the man pages `getmntent(3)`, `getfsent(3)`, `fstab(5)`, `proc(5)`
(`/proc/[pid]/mounts`), and the glibc manual (Mount Information: mtab, fstab).
`__getmntent_r`, `__setmntent` and `__endmntent` are exported names that no installed header
declares (a compile with `-Werror` against `<mntent.h>` with `_GNU_SOURCE` fails for each); the
probe uses public headers only and does not call them.

**Probe:** `crates/core/tests/oracle/mntent_diff.c`, run by `mntent_run.sh PROGRAM SCRATCH`.
**Record:** `mntent.glibc.txt` (911 lines). It was made on the VM (Ubuntu 24.04, glibc
2.39-0ubuntu8.9, kernel 6.8.0-146-generic, scratch directory on ext4) with
`cc -std=gnu17 -O1 -o p mntent_diff.c` and `env -i PATH=/usr/bin:/bin sh mntent_run.sh p SCRATCH`,
twice, with scratch directories of different names and lengths (`~/mntprobe/run-a`,
`~/mntprobe/run-second-longer`): the two outputs are byte-identical, and identical to an `-O0`
build run with a third scratch name (SHA-256 `d1edf051...869c1` for all three). Nothing was left
behind: the scratch directories are removed by the harness and no mount survives the namespace.
The probe undefines `_FORTIFY_SOURCE`.

The harness has two phases.

1. **Unprivileged** (the harness user, `claude`; line 1 `running as root: no`): parsing of fixture
   files, the storage of `getmntent`, the buffer protocol of `getmntent_r`, `setmntent` modes and
   errors, `endmntent`, `addmntent`, `hasmntopt`, and only the *structure* of the real
   `/proc/self/mounts`, `/proc/mounts` and `/etc/mtab`. The fixture files are created in SCRATCH
   and removed by the program; it then leaves `SCRATCH/etc` (a fixture `/etc`: `fstab`, `mtab`
   and a marker file `liella-mntent-fixture`) and empty mount points `SCRATCH/m/<name>`. This
   phase ends with `done` (line 719).
2. **`root`** (from line 720): run by `sudo -n env LC_ALL=C timeout 120 unshare -m --propagation
   private`. Inside the namespace the shell mounts a 64 KiB tmpfs on each `SCRATCH/m/<name>`
   (in `LC_ALL=C` glob order), bind-mounts `SCRATCH/etc` over `/etc`, and only then runs the
   program, which refuses to run unless `/etc/liella-mntent-fixture` exists. The phase changes
   only fixture files (it renames, empties and replaces the fixture `/etc/fstab` and replaces the
   fixture `/etc/mtab` with a link); the real `/etc` and the real mount table are never written,
   and every mount vanishes with the namespace. The loader cache is copied into the fixture so
   that the dynamically linked reference build starts.

## What the probe does

Strings are printed in brackets with every byte outside printable ASCII as `\xNN` and a
backslash as `\\`; a run of more than 60 equal bytes is printed as `<c>*<n>`; the scratch
directory as `<dir>`. An entry line is `fsname dir type opts freq passno`. Nothing prints a
descriptor number or an address; `getmntent_r`'s field positions are printed as offsets into the
caller's buffer, positions as `ftell` values. Risky calls (`getmntent_r` with each buffer size,
NULL arguments) run in forked children with an `alarm` (3 s for the buffer sizes, 20 s otherwise);
the line `... child killed by signal S` or `child exited N` follows the child's own output (a
child killed by a signal loses its unflushed output). The program has `alarm(300)` and core dumps
off; the harness wraps each phase in `timeout 120`.

* **Parsing** (`== parsing`, one `-- case` per fixture file, read with `getmntent` to the end):
  each entry, then `pos` (`ftell` after the entry), then the end (`errno`, `feof`, `ferror`), a
  further call with `errno` preset to `EINTR`, and `endmntent`'s value. Cases: space and tab
  separators, runs of blanks, leading and trailing blanks; the escapes `\040`, `\011`, `\012`,
  `\134` in each string field and in `freq`; other octal sequences (`\101`, `\041`, `\177`,
  `\200`, `\377`, `\0`, `\00`, `\000`, `\1`, `\12`, `\04x`, `\400`, `\777`) and non-octal ones
  (`\8`, `\9`, `\x41`, `\\`, `\n`), a backslash at the end of a field, of a line, and before a
  blank; comment lines (indented, after a tab, a lone `#`) and `#` inside and after fields; blank
  and blank-only lines; 1 to 5 fields, 7 and 8 fields, short lines between good ones; `freq` and
  `passno` as words, with signs, over `INT_MAX`, hex and octal looking, with trailing letters; an
  options field that is only `\040`; a last line without a newline (six and five fields); CRLF
  line ends and a CR inside a field; an empty file and one of only comments; NUL bytes (inside a
  line, followed by two lines, in an unterminated last line, a line that is only a NUL); UTF-8
  bytes; form feed and vertical tab between fields.
* **Long lines** (`== long lines (getmntent)`): a first field of 100 to 100000 bytes (around 4096
  and 8192 in detail) followed by a short line; an options field of 5000 bytes; a `passno` of 5000
  digits on an unterminated last line.
* **Storage** (`== getmntent storage`): whether two `getmntent` calls, and calls on two different
  streams, return the same structure and overwrite each other's strings; whether `getmntent_r`
  returns its argument, puts the strings inside the caller's buffer (and where), and leaves
  `getmntent`'s entry alone.
* **`getmntent_r` buffer sizes** (`== getmntent_r buffer sizes`): a 36-byte first line (35
  characters and the newline) read with buffers of 1 to 64 bytes, each in a child: result,
  `errno`, position, how many buffer bytes were written, whether the 32 bytes past the size are
  untouched, the entry, and what a following call with 256 bytes returns. A 292-byte line into a
  256-byte buffer, three calls. Sizes 0 and -1 in children.
* **`setmntent`** (`== setmntent modes and errors`): 15 mode strings on a fixture file, each
  showing the descriptor's access mode, `O_APPEND`, `O_NONBLOCK`, close-on-exec, the initial
  position and the first `getmntent`; a missing file (`r` and `w`, and what `w` creates), a path
  below a file, a directory (`r`, `w`, `a`), a mode-0000 file (as a user), an empty name,
  `/dev/null`. The path and option macros of `<mntent.h>`, `<fstab.h>` and `<paths.h>`, and the
  structure sizes.
* **`endmntent`** (`-- endmntent`): on an open stream, a stream with pending output (and whether
  it was flushed), `/dev/full` with pending output, NULL (in a child), a stream from `fopen`;
  `getmntent` on a stream from `fopen`.
* **`addmntent`** (`== addmntent`): 13 entries with space, tab, newline, backslash, `#` (inside a
  field, starting a field, starting the first field), CR, VT, FF, a literal `\040`, UTF-8 and
  control bytes, empty strings in all four fields, negative and extreme `freq`/`passno`; return
  values, `ftell`, the file size before `endmntent`, the file's bytes, and the file read back.
  The stream position: an `r+` stream after reading one entry, a `w+` stream after `rewind`, an
  `a` stream. Errors: a read-only stream, `/dev/full`. Flushing: the file size right after one
  `addmntent` on a stream with a 64 KiB buffer. NULL `mnt_dir` and NULL `mnt_opts` in children.
* **`hasmntopt`** (`== hasmntopt`): 56 pairs of options string and query (prefixes, suffixes,
  `=value` forms on either side, empty options and queries, leading, trailing and doubled commas,
  case, blanks and tabs inside the string, quotes, a query containing a comma, a trailing
  newline, an option inside another option's value, UTF-8), printing the offset of the result in
  `mnt_opts` and the text there; whether the result points into `mnt_opts`; NULL `mnt_opts` in a
  child.
* **The real mount table, structure only** (`== /proc/self/mounts, structure only`): for
  `/proc/self/mounts`, `/proc/mounts` and `_PATH_MOUNTED`, whether there are entries, whether
  every entry has four non-empty strings, `freq` and `passno` 0, exactly one of `rw` and `ro`
  (by `hasmntopt`), and a `/` entry. No entry, count or order of the machine's mounts is printed.
* **Root phase.** `== fstab functions over the fixture /etc/fstab`: `getfsent` before
  `setfsent`, the whole fixture (39 lines: comments, a blank line, `defaults`, the options `rw`,
  `ro`, `rq`, `sw`, `xx` alone and in pairs in both orders, `noauto,ro`, `user,rw`, `ro=1`,
  `noro,rwx`, types `swap` and `ignore`, an escaped space, tabs and leading blanks, a 2-field and
  a 5-field line, a duplicate device and a duplicate mount point, an options field of `\040`, a
  last line without a newline), the end, the storage of two calls, `setfsent` on an open file;
  `getfsspec` and `getfsfile` on 13 keys each (found, first-of-duplicates, escaped and decoded
  forms, prefixes, empty, missing); the cursor after successful and failed lookups and after
  `endfsent`; whether lookups and `getfsent` share the structure. `== _PATH_MOUNTED as a fixture
  file`: `/etc/mtab` as a regular fixture file. `== _PATH_MNTTAB through getmntent`: the fixture
  fstab read by `getmntent`. `== own mounts`: the probe's nine tmpfs mounts below `<dir>` (mount
  points with a space, a tab, a newline, a backslash and a `#`; sources with spaces and a
  backslash; one read-only; one with `nosuid,nodev,noexec,mode=0700,uid=1,gid=2`) as
  `/proc/self/mounts` shows them, then again through `_PATH_MOUNTED` after the fixture's
  `/etc/mtab` was replaced by the link `../proc/self/mounts`; then `setfsent`/`getfsent`/
  lookups with no `/etc/fstab`, with an empty one, with a directory in its place, and with the
  file rewritten in place while open.

## Observations the record shows directly

### Parsing (getmntent; §13.13 `s13_13_c_2`)

* Fields are separated by runs of spaces and tabs only; leading and trailing blanks are ignored
  (`-- runs of blanks`, `-- trailing blanks only after passno`). Form feed and vertical tab are
  ordinary bytes: `a\f/b\vt o 0 0` is the entry `[a\x0c/b\x0bt] [o] [0] [0] 0 0`.
* A line whose first non-blank byte is `#` is a comment; blank and blank-only lines are skipped
  (`-- comments`, `-- blank lines`). A `#` anywhere else is data: `a#b /c#d` and a second field
  `#b` are kept, and `# trailing` after the sixth field is ignored like any extra field.
* Escapes (`-- escapes ...`): exactly the four sequences `\040` (space), `\011` (tab), `\012`
  (newline) and `\134` (backslash) are decoded, in each of the four string fields. Every other
  backslash sequence is kept verbatim, backslash included: `\101`, `\041`, `\177`, `\200`,
  `\377`, `\0`, `\00`, `\000`, `\1`, `\12`, `\04x`, `\400`, `\777`, `\8`, `\9`, `\x41` and `\n`.
  `\\` is decoded to one backslash, as getmntent(3) says (the record's `o\\p\n` line reads back
  with one backslash before `p`; corrected 2026-10-03, after the implementation's probe: an
  earlier version listed `\\` among the sequences kept verbatim). A backslash at the end of a field or of a line stays (`[a\\]`, `[o\\]`), and a backslash
  does not escape a following blank (`a\ b` gives the fields `a\` and `b`). In `freq` an escape is
  not decoded (`\0601 2` gives `0 0`).
* Missing fields (`-- one field` to `-- five fields`, `-- missing fields between good lines`):
  every non-comment line with at least one field is an entry; missing string fields are empty
  strings and missing numbers are 0. Fields after the sixth are ignored.
* Numbers (`-- freq and passno ...`): each is read as an optionally signed decimal integer up to
  the first non-digit (`-1 +2` gives -1 2; `1abc 2x` gives 1 0; `5 q` gives 5 0). When `freq`
  does not end at a blank, `passno` is 0 whatever follows (`0x10 010` gives 0 0, `x y` 0 0).
  Values outside `int` are not rejected: `2147483648` gives -2147483648, `-2147483649` gives
  2147483647, `99999999999` gives 1215752191 (the value modulo 2^32), and 5000 digits give -1.
* Line ends (`-- no newline at end`, `-- CRLF`, `-- CR inside`): an unterminated last line is an
  entry like any other. A CR is an ordinary byte: with CRLF line ends it ends up in the last
  string field present on the line (`[o\x0d]`, `[t\x0d]`), while a number before the CR is read
  normally (`0 1\r` gives 0 1).
* NUL bytes (`-- NUL in a line`, `-- NUL in a line, then two lines`, `-- NUL in the last line, no
  newline`, `-- a line of exactly one NUL`): a line is cut at its first NUL, and the **next line
  is lost** (`a\0b ...` followed by `c ...` gives only `[a] [] [] []`; followed by `c` and `d`,
  `d` is returned and `c` is not). A line that is only a NUL is skipped like a blank line, and
  the line after it is lost too. An unterminated last line with a NUL loses nothing.
* UTF-8 and other high bytes pass through unchanged.
* Long lines (`== long lines (getmntent)`): `getmntent` keeps the first 4095 bytes of a line and
  discards the rest of it up to and including its newline; the next line is read intact
  (`[<L>*4095] [] [] [] 0 0` for every first field of 4095 bytes or more, up to 100000; a
  4090-byte first field keeps ` /a t` of what follows; a 5000-byte options field becomes
  `[<o>*4088]`). Nothing reports the truncation: `errno` stays 0.
* End of file: `getmntent` returns NULL, `feof` is set, `errno` is left as it was (0 stays 0;
  `EINTR` stays `EINTR`), and further calls return NULL. An empty file and a file of comments
  return no entry.

### getmntent's storage and getmntent_r's buffer

* `getmntent` returns the same structure on every call and on every stream; the next call on
  *any* stream overwrites the strings of the previous result (`first fsname pointer still names:
  [second]`, `stream 1's result after stream 2's call: [other] ...`). A stream's position is its
  own (`stream 1 next: [second]`). `getmntent_r` does not disturb `getmntent`'s entry.
* `getmntent_r` returns its `mntbuf` argument, and the four strings lie in the caller's buffer at
  the positions of the fields in the line (`fsname 0 dir 6 type 13 opts 16` for
  `first /first t1 o1 1 1`).
* Buffer sizes (36-byte line `/dev/sda1 /mnt ext4 rw,relatime 0 2` and its newline): there is
  **no `ERANGE`**. A buffer of `n` bytes receives the first `n-1` bytes of the line and a NUL
  (`written=n` up to 37, then 37), the rest of the line is discarded (`pos=36` for every size), the
  entry is parsed from what fits (size 10: `[/dev/sda1] [] [] [] 0 0`; size 25: `[rw,r]`; size
  35: `passno` 0; size 36 and up: the whole entry), and the next call returns the next line.
  Nothing past the given size is written (`beyond untouched=yes`). A 292-byte line in a 256-byte
  buffer gives `[<F>*255] [] [] []`, then the next line.
* Size 1 never returns: the child is killed by its 3-second alarm (`size 1 (in a child, 3 s
  limit): child killed by signal SIGALRM`). Sizes 0 and -1 return NULL with `errno` 0 and read
  nothing (`pos=0`; the next call returns the first line).

### setmntent and endmntent (§13.13 `s13_13_c_1`)

* `setmntent` behaves as `fopen` with close-on-exec added: every successful mode gives a
  descriptor with `FD_CLOEXEC` and without `O_NONBLOCK`; `r` is `O_RDONLY`, `r+` and `w+`
  `O_RDWR`, `w` `O_WRONLY`, `a` `O_WRONLY|O_APPEND` (position 13, the end, right after the open),
  `a+` `O_RDWR|O_APPEND` (position 0). Extra letters after the first are accepted (`re`, `rm`,
  `rb`, `rx`, `rc`, `ra` all open read-only); `z`, an empty mode and `+r` fail with `EINVAL`.
* Errors are the open's: missing `ENOENT`, below a file `ENOTDIR`, mode 0000 `EACCES`, empty
  name `ENOENT`, a directory with `w` or `a` `EISDIR`. A directory with `r` opens, and the first
  `getmntent` returns NULL with `errno` `EISDIR` and the stream's error flag set. `w` on a missing
  file creates it empty. `getmntent` on a write-only stream returns NULL with `EBADF` and the
  error flag set. `/dev/null` reads as an empty table.
* `endmntent` returns 1 in every case: an open stream, a stream with pending output (which it
  writes: the file then holds `unflushed`), `/dev/full` with pending output (the flush fails,
  `errno` is `ENOSPC`, the result is still 1), NULL (no crash), and a stream from `fopen`.
  `getmntent` works on a stream from `fopen`.
* Macros: `_PATH_MOUNTED` and `MOUNTED` are `/etc/mtab`; `_PATH_MNTTAB`, `MNTTAB` and
  `_PATH_FSTAB` are `/etc/fstab`; `MNTTYPE_IGNORE` `ignore`, `MNTTYPE_NFS` `nfs`, `MNTTYPE_SWAP`
  `swap`, `MNTOPT_DEFAULTS` `defaults`, `MNTOPT_RO` `ro`, `MNTOPT_RW` `rw`, `MNTOPT_SUID` `suid`,
  `MNTOPT_NOSUID` `nosuid`, `MNTOPT_NOAUTO` `noauto`; `FSTAB_RW` `rw`, `FSTAB_RQ` `rq`,
  `FSTAB_RO` `ro`, `FSTAB_SW` `sw`, `FSTAB_XX` `xx`. `sizeof(struct mntent)` is 40 and
  `sizeof(struct fstab)` 48 (as the spec's layout table says).

### addmntent (§13.13 `s13_13_c_3`)

* Format: `fsname dir type opts freq passno` and a newline, single spaces, the numbers as signed
  decimal (`-1 -2147483648`). In each of the four strings, space, tab, newline and backslash are
  written as `\040`, `\011`, `\012`, `\134`; every other byte is written as is (`#`, CR, VT, FF,
  `\x01`, `\x7f`, UTF-8). A literal `\040` in the input is written `\134040` and reads back as
  `\040`. Every such entry reads back equal, with two exceptions: empty strings give the line
  `    0 0`, which reads back as `[0] [0] [] [] 0 0`; and an `fsname` starting with `#` gives a
  line that reads back as a comment (`#hash /h t o 0 0` is not among the entries read back).
* Return value 0 on success; 1 on failure, with `errno` from the write and the stream's error
  flag set (a read-only stream: `EBADF`; `/dev/full`: `ENOSPC`). A failed add on a read-only
  stream leaves the file unchanged.
* Each entry is written at the **end of the file** whatever the stream's position: on an `r+`
  stream after reading the first of three entries (position 15) the new entry is appended
  (position 68 afterwards, the end), and the next `getmntent` returns NULL; on a `w+` stream after
  `rewind` the third entry is appended after the first two, not over the first; `a` appends.
* Each `addmntent` writes its entry out before returning: the file's size equals `ftell` before
  `endmntent` (309), and with a 64 KiB stream buffer set by `setvbuf` the file holds the 17
  bytes right after the call.
* NULL `mnt_dir` or NULL `mnt_opts`: `SIGSEGV`.

### hasmntopt (§13.13 `s13_13_c_3`)

* The query matches only at the start of an option (offset 0 or right after a comma), and only if
  the text that follows the match in `mnt_opts` is the end of the string, a comma or `=`. The
  result points at the start of that option inside `mnt_opts` (the same pointer on every call),
  and the first such option wins (`noro,ro` / `ro` gives offset 5; `aro,ro` gives 4; `xrw,rw` /
  `rw` gives 4). Otherwise NULL.
* Hence: `rw,noro` / `ro`, `rwx` / `rw`, `xrw` / `rw`, `relatime` / `atime`, `defaults` /
  `default`, `user_xattr` / `user`, `ro` / `r`, `ro` / `o`, `=ro` / `ro`, `uid=0,gid=0` / `id`
  are NULL; `rw=1` / `rw`, `ro=` / `ro`, `size=10,rw` / `size`, `size==1` / `size=` (followed by
  `=`) match; `rw=1` / `rw=` is NULL (followed by `1`); `size=10` / `size=1` and `size=100` are
  NULL; `rw=1` / `rw=1` and `size=10` / `size=10` match.
* Values are not skipped as such: `x=a,b` / `b` matches at 4 (the comma starts an option), while
  `x=a,b` / `a` and `errors=remount-ro` / `ro` or `remount-ro` are NULL. Quotes mean nothing:
  `context="a,b",ro` / `b"` matches at 11.
* A query containing a comma can match across options (`rw,ro` / `rw,ro` at 0); `rw,ro` / `w,r`
  is NULL.
* The empty query matches an empty option: `""` in `""` at 0, in `,` at 0, in `a,,b` at 2; not in
  `rw`. Blanks, tabs and a newline are ordinary bytes (`a b,c` / `b` NULL, `ro\n` / `ro` NULL).
  Matching is case-sensitive. Trailing, leading and doubled commas need no special handling
  (`,rw,` / `rw` at 1; `ro,` / `ro` at 0).
* NULL `mnt_opts`: `SIGSEGV`.

### The kernel's tables and /etc/mtab (§13.13 `s13_13_c_1`, D-13.8)

* `/proc/self/mounts`, `/proc/mounts` and `/etc/mtab` (on the VM a link to
  `../proc/self/mounts`) read as tables whose entries all have four non-empty strings, `freq` and
  `passno` 0, exactly one of `rw` and `ro`, and a `/` entry.
* The probe's mounts (`== own mounts`, mounts in `LC_ALL=C` glob order of the mount points):
  mount points with a space, a tab, a newline and a backslash, and sources with spaces and a
  backslash come back decoded (`[<dir>/m/nl\x0ac]`, `[back\\slash]`); a `#` in a mount point
  needs no escape. The options are the kernel's text (`rw,relatime,size=64k,inode64`,
  `ro,relatime,...`, `rw,nosuid,nodev,noexec,relatime,size=64k,mode=700,uid=1,gid=2,inode64`);
  `hasmntopt` finds `rw`, `ro`, `nosuid` and `size` (with its value) in them.
* `_PATH_MOUNTED` is read as whatever `/etc/mtab` is: a regular fixture file is parsed as such
  (`== _PATH_MOUNTED as a fixture file`); after it is replaced by the link
  `../proc/self/mounts`, the same nine mounts appear as through `/proc/self/mounts`.

### The fstab functions (§13.13 `s13_13_c_4`)

* `getfsent` without `setfsent` opens `/etc/fstab` by itself (`getfsent before setfsent` returns
  the first entry). `setfsent` returns 1 when the file is open (also when it was already open,
  and then the next `getfsent` starts again at the first entry), 0 with `ENOENT` when there is no
  `/etc/fstab`; then `getfsent`, `getfsspec` and `getfsfile` return NULL.
* Entries are parsed as by `getmntent` (`== _PATH_MNTTAB through getmntent` lists the same 37
  entries in the same order): comments and the blank line skipped, escapes decoded
  (`/dev/sp ace`), short lines kept with empty strings and zeros, the unterminated last line
  returned. `fs_spec`, `fs_file`, `fs_vfstype`, `fs_mntops`, `fs_freq`, `fs_passno` are the six
  fields.
* `fs_type` is one of `rw`, `rq`, `ro`, `sw`, `xx`, or `??`, chosen from `fs_mntops` only (not
  from `fs_vfstype`: `swap defaults` and `ignore defaults` give `??`; `swap ro` gives `ro`). An
  option counts as by `hasmntopt` (`ro=1` gives `ro`; `noro,rwx` gives `??`). When several are
  present the first in the order `rw`, `rq`, `ro`, `sw`, `xx` wins, regardless of their order in
  the field (`ro,rw` and `xx,rw` give `rw`; `ro,rq` and `rq,ro` give `rq`; `sw,ro` gives `ro`;
  `xx,sw` gives `sw`). `defaults`, `nosuid`, `size=...` and an options field of a single space
  give `??`.
* At the end `getfsent` returns NULL and leaves `errno` alone.
* Storage: two `getfsent` calls return the same structure, the second overwriting the first's
  strings; `getfsspec`, `getfsfile` and `getfsent` share it.
* Lookups: `getfsspec` and `getfsfile` search from the first entry (whatever the cursor was) and
  return the first entry whose decoded field is byte-equal to the key: `/dev/sda1` gives the `/`
  line, not the later `/dup` line; `/` gives `/dev/sda1`, not the later `/dev/sda17`; `/dev/sp
  ace` is found and `/dev/sp\040ace` is not; prefixes (`/dev/sda`, `/ho`), the empty string and
  missing keys give NULL. After a successful lookup `getfsent` continues with the entry after the
  one found; after a failed lookup it returns NULL (the end). After `endfsent`, a lookup opens the
  file again by itself. `endfsent` twice is harmless.
* `/etc/fstab` empty: `setfsent` 1, `getfsent` NULL. A directory: `setfsent` 1 with `errno` 0,
  `getfsent` NULL. Rewritten in place while open (truncated and given one new line): the next
  `getfsent` still returns the old second entry (`/dev/b`), and after `setfsent` the new content
  (`/dev/c`).

### System calls

A separate small program (not part of the record) was traced with `strace` on the VM:

* `setmntent(path, "r")`: `openat(AT_FDCWD, path, O_RDONLY|O_CLOEXEC)`; `"a+"`:
  `O_RDWR|O_CREAT|O_APPEND|O_CLOEXEC, 0666`. Reading `/proc/self/mounts`: `fstat`, then `read`
  calls of 1024 bytes until 0; `endmntent`: `close`.
* `addmntent` on an `r+` stream: `fstat`, `lseek(fd, 0, SEEK_SET)`, a `read` of the whole file,
  then one `write` of the escaped line, within the call.
* `setfsent`: `openat("/etc/fstab", O_RDONLY|O_CLOEXEC)`; `getfsspec` after a `getfsent`:
  `lseek(fd, 0, SEEK_SET)` and reads from the start; `endfsent`: `close`.

## Observations that look like historical artifacts

The implementer may decide these differently (and record the difference).

* `getmntent_r` with a 1-byte buffer never returns (an endless loop); with 0 or a negative size
  it returns NULL with `errno` 0, the same as the end of the table.
* `getmntent_r` never reports a short buffer: it silently truncates the line, parses what fits
  and discards the rest of the line. `getmntent` truncates lines at 4095 bytes the same way.
* A NUL byte in a line makes the following line disappear.
* Only four octal escapes are decoded; other well-formed octal escapes (`\101`) stay verbatim.
* `freq` and `passno` wrap modulo 2^32 rather than failing, and a malformed `freq` turns a
  valid `passno` into 0.
* `addmntent` writes entries that do not read back: empty strings (the line `    0 0`) and an
  `fsname` starting with `#` (a comment); it does not escape `#`.
* `addmntent` always appends, moving the stream to the end, even on a stream positioned
  elsewhere (§13.13 says "appends", so this may be the contract rather than an artifact).
* `endmntent` returns 1 even when its flush failed, and accepts NULL.
* `hasmntopt` accepts a query containing a comma and matches it across options, and treats
  `name=` in the query as a prefix that must be followed by `,`, `=` or the end.
* `setfsent` returns 1 for a directory, which then reads as an empty table with no error.
* `getfsent` opens `/etc/fstab` implicitly; a failed `getfsspec`/`getfsfile` leaves the cursor at
  the end.
* `fs_type` ignores the option order and picks by a fixed priority (`rw`, `rq`, `ro`, `sw`,
  `xx`).

## What the record does not settle

* `__getmntent_r`, `__setmntent` and `__endmntent`: no public header declares them, so the probe
  does not call them; they are listed in §13.13 and the symbol table.
* D-13.8's internal use of `/proc/self/mountinfo`: not observable through these functions, which
  read whatever file they are given; the record shows the text format `getmntent` returns for
  `/proc/self/mounts`.
* The kernel's option text (`inode64` for tmpfs, the order `rw,nosuid,...,relatime,size=...`)
  depends on the kernel and its configuration; the differential test compares on one machine.
* Whether `getmntent_r` with a 1-byte buffer loops in every case or only here (the child is
  killed after 3 s; the record shows only the kill).
* Lines longer than 4095 bytes through `getfsent` (the fixture's lines are short), and the
  `getmntent` buffer size on other systems.
* Concurrency: streams or the fsent cursor shared between threads; `/proc/self/mounts` changing
  while it is read.
* `addmntent` on unseekable streams (pipes), and the behaviour when the append's seek fails.
* `hasmntopt` and the fsent functions with `struct mntent` fields other than `mnt_opts` NULL, and
  `getfsspec`/`getfsfile` with a NULL key: not probed.
