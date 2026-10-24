# File-system tree: working directory, directory streams, scandir, tree walks, realpath, temporary names

**Spec:** §13.1 (`getcwd`, `get_current_dir_name`, `getwd`, `chdir`, `fchdir`, `chroot`), §13.2
(`opendir`, `fdopendir`, `readdir`, `readdir64`, `readdir_r`, `readdir64_r`, `rewinddir`,
`seekdir`, `telldir`, `closedir`, `dirfd`, `getdirentries`, `getdents64`), §13.3 (`scandir`,
`scandirat` and their `64` forms, `alphasort`, `versionsort` and their `64` forms, `ftw`, `nftw`,
the `fts_*` functions and `fts64_*`), §13.8 (`realpath`, `canonicalize_file_name`), §13.9
(`mkstemp`, `mkostemp`, `mkstemps`, `mkostemps` and their `64` forms, `mkdtemp`, `mktemp`), with the
decision rows D-13.1 to D-13.6 and the phase-12 test surface of implementation-plan.md (iteration
while entries are created and removed, walks deeper than `PATH_MAX`, symbolic-link loops, the
`realpath` prefix on failure, `mkstemp` exhaustion). Public interfaces: `<unistd.h>`,
`<dirent.h>`, `<ftw.h>`, `<fts.h>`, `<stdlib.h>`. Normative references (plan §1.2): POSIX.1-2017
(`getcwd`, `chdir`, `fchdir`, `opendir`, `fdopendir`, `readdir`, `readdir_r`, `rewinddir`,
`seekdir`, `telldir`, `closedir`, `dirfd`, `scandir`, `alphasort`, `nftw`, `ftw`, `realpath`,
`mkstemp`, `mkdtemp`), the man pages `getcwd(3)`, `opendir(3)`, `readdir(3)`, `readdir_r(3)`,
`seekdir(3)`, `telldir(3)`, `scandir(3)`, `ftw(3)`, `fts(3)`, `realpath(3)`, `mkstemp(3)`,
`mkdtemp(3)`, `mktemp(3)`, `getdents64(2)`, `getcwd(2)`, `chroot(2)`, `path_resolution(7)`, and
the glibc manual (Working Directory, Accessing Directories, Working with Directory Trees, Temporary
Files).

**Clauses:** `s13_1_c_1`, `s13_1_c_1_1`, `s13_1_c_1_2`, `s13_1_c_2`, `s13_1_c_3`, `s13_1_c_4`,
`s13_1_c_5`, `s13_1_f_0`, `s13_2_c_1`, `s13_2_c_2`, `s13_2_c_2_1`, `s13_2_c_2_2`, `s13_2_c_3`,
`s13_2_c_4`, `s13_2_c_5`, `s13_2_c_6`, `s13_2_g_0`, `s13_2_f_0`, `s13_3_c_1`, `s13_3_c_2`,
`s13_3_c_2_1`, `s13_3_c_2_2`, `s13_3_c_2_3`, `s13_3_c_3`, `s13_3_c_4`, `s13_3_i_0`, `s13_3_f_0`,
`s13_8_c_0`, `s13_8_c_1`, `s13_8_c_2`, `s13_8_f_0`, `s13_9_c_1`, `s13_9_c_2`, `s13_9_c_3`,
`s13_9_c_4`, `s13_9_c_5`, `s13_9_i_0`, `s13_9_f_0`.

**Probe:** `crates/core/tests/oracle/fstree_diff.c`, run by `fstree_run.sh PROGRAM SCRATCH`.
**Record:** `fstree.glibc.txt` (1643 lines). It was made on the VM (Ubuntu 24.04, glibc
2.39-0ubuntu8.9, kernel 6.8.0-146-generic) with `cc -std=gnu17 -O1 -o p fstree_diff.c` and
`env -i PATH=/usr/bin:/bin LOCPATH=<repo>/target/test-locales sh fstree_run.sh p SCRATCH`, twice,
with scratch directories of different names and lengths (`~/fsprobe/run-a`,
`~/fsprobe/run-second`): the two outputs are byte-identical, and identical to an `-O0` build.
It was last re-recorded on 2026-10-03, after the removed-directory cases (`-- directory removed
while open`, in both phases) were added: the two runs and the `-O0` build again gave
byte-identical outputs (SHA-256 `5bfcd004...25ec9`), and every line of the previous record is
unchanged (the new lines are only inserted, at lines 165-193 and 1614-1642). The probe also has
a mode outside the record, `fstree_diff DIR exhaust FUNC`, used only by the fault-injection
section at the end.
The probe undefines `_FORTIFY_SOURCE` (on by default in Ubuntu's compiler at `-O1`), so the plain
functions are called and not their `__*_chk` forms.

**File system.** The scratch directory was on **ext4** (`stat -f -c %T` prints `ext2/ext3`;
statfs `f_type` 0xef53); the first two lines of the record say so. The root phase runs on a
**tmpfs** (`f_type` 0x1021994) mounted over the scratch directory; its section header says so.
Every line of the record whose order follows directory order (readdir and getdents order,
unsorted scandir, ftw/nftw, fts without a comparison function, telldir positions, the counts of
entries seen while the directory changes) names the file system on the line or its header, and
every such observation below is marked **(order: ext4)** or **(order: tmpfs)**. Runs on other file
systems are deferred by the owner; the record makes no claim about them.

The harness has two phases.

1. **Unprivileged** (the harness user, `claude`, not root): every case below except the root
   phase. The permission cases need a non-root user; the record's third line confirms
   `running as root: no`. The fixture is written into SCRATCH and removed by the program at the
   end (`cleanup: 0 entries left in <dir>`); the harness then removes SCRATCH.
2. **`root`**: run as root by `sudo -n unshare -m --propagation private` with a 16 MiB tmpfs
   mounted over SCRATCH, so nothing the phase creates reaches a disk and every mount vanishes
   with the namespace. It covers `chroot` (in a forked child), a lazy unmount of the working
   directory's file system, nested mounts for `FTW_MOUNT` and `FTS_XDEV`, and device nodes.

## What the probe does

Every scratch path is printed as `<dir>`; a 200-byte component of the deep tree as `{d200}`; a
run of more than 60 equal bytes as `<c>*<n>`; lengths of absolute results as `<dir>+N`. Nothing
prints an inode, a descriptor number, a `d_off` or `telldir` cookie, or a time. Each run has an
`alarm(600)` and the harness a `timeout`; walks that might kill the process run in forked
children (`in a child process`), and core dumps are disabled (`RLIMIT_CORE` 0).

Fixture (unprivileged phase): `t/` holds `a/{f1, sub/x, s_up -> ..}`, `b`, `e/` (empty), `p`
(FIFO), `so` (socket), `s_file -> b`, `s_dir -> a`, `s_dangling -> nowhere`, `noread/` (mode
0333: search, no read) and `nosearch/` (mode 0600: read, no search), each holding one file. `u/`
is `t` without `noread`, `nosearch`, `p` and `so` (FTW_CHDIR stops at `nosearch`). `lp/` holds `f`
and the self loop `s_loop -> s_loop`. `deep/` is a chain of 25 directories with 200-byte names
and a file `bottom` (paths up to 5036 bytes, absolute paths over 5000; the cap is 25 levels and
the chain is created and removed through descriptors, never by a path over `PATH_MAX`).

* **Working directory** (`== getcwd`): `getcwd` with buffers of `len+1`, `len`, 2, 1 and 0 bytes
  (each pre-filled to show whether a failure writes it) and with a NULL buffer and sizes 0,
  `len+1`, `len`, 1, 100000; `getwd`; `get_current_dir_name` with `PWD` unset, physical, through
  a symbolic link, with a trailing slash, `/./`, `/../`, `//`, naming another directory, missing,
  relative, empty. A removed working directory: `getcwd`, `getwd`, `get_current_dir_name`,
  `opendir(".")`, `realpath`, `chdir("..")`. `chdir` errors (empty, missing, file, file with a
  slash, mode 0600, mode 0333, loop, dangling link, a 4200-byte path, a 256-byte component),
  `chdir` through a link, `fchdir` on `O_RDONLY` and `O_PATH` directory descriptors, a file, a
  closed descriptor and -1, unprivileged `chroot`.
* **Deeper than `PATH_MAX`** (`== working directory deeper than PATH_MAX`): from the bottom of
  `deep/`, `getcwd` with `PATH_MAX`, large, exact and one-short buffers, NULL with 0 and
  `PATH_MAX`; `getwd`; `get_current_dir_name`; `realpath(".")`, `realpath("bottom")`,
  `canonicalize_file_name`; then with an ancestor at level 10 made mode 0111 (search only).
  `realpath` of a 2009-byte relative path whose result is over `PATH_MAX`, and of a 5036-byte
  input.
* **Directory streams** (`== directory streams`): `r/` holds a file, a directory, a link, a FIFO,
  a socket and a 255-byte name. The descriptor flags `opendir` and `fdopendir` set; entries in
  directory order and sorted with `d_type`; `d_reclen`; `errno` at the end (cleared, and preset
  to `EINTR`); the storage two calls return; `readdir_r`, `readdir64`, `readdir64_r`; two streams
  read alternately; `opendir` errors; `fdopendir` on a descriptor already read to the end, a
  file, an `O_PATH` descriptor, -1, a closed descriptor; `readdir`, `readdir_r` and `closedir`
  after the stream's descriptor was closed underneath; a directory removed after `opendir`;
  `getdents64` and `getdirentries`; `rewinddir` after entries changed; `telldir`/`seekdir` over
  60 entries (round trips, end position, a position used in a second stream).
* **Removed directory** (`-- directory removed while open`, in the user phase on ext4 and again
  at the end of the root phase on tmpfs; every line starts with `[fs <name>]`): with each of
  `readdir`, `readdir64`, `readdir_r` and `readdir64_r`, a directory removed after `opendir`
  before the first read (and a second read after the end); an empty directory removed after its
  first entry was read; a directory of 3 files read 3 entries, then emptied and removed; a
  directory of 2000 files (far more than one `getdents64` buffer) read 50 entries, then emptied
  and removed. For comparison, the 2000-file directory emptied but not removed. `rewinddir` and
  `seekdir(0)` after the removal of a directory read to the end, and `rewinddir` when the
  removal came before the first read; `fdopendir` on the descriptor of a removed directory, and
  `fdopendir` followed by the removal (read with `readdir_r`). Then `getdents64` called
  directly: on an empty directory removed before the first call (twice, and after `lseek(fd, 0,
  SEEK_SET)`), on the 2000-file directory after one 1024-byte call and the removal (twice, and
  after `lseek`), the same emptied but not removed, and on `open(".")` in a removed working
  directory. Each line shows how the reading ended: `NULL errno=E` (`errno` cleared before each
  call) for `readdir`/`readdir64`, `ret=E result=NULL` for the `_r` forms, and the result or
  error of `getdents64`.
* **"Exactly once"** (`== readdir while entries are created and removed`): a directory of 1500
  stable and 300 removable entries (46-byte names, so the stream needs many `getdents64`
  calls); after every 50 entries read the probe creates 15 new entries and removes 8 removable
  ones. It counts how often each name is returned.
* **scandir** (`== scandir`): 35 names chosen for collation (case, `_`, `.h`, digits, `~`, UTF-8
  `é`, `ä`, `Å`, `ch`, version-like names) sorted with no comparison function, `alphasort` and
  `versionsort` in C, `en_US.UTF-8` and `cs_CZ.UTF-8` (from `LOCPATH`), `scandir64` with the
  `64` comparators; filters (calls, `.`/`..`, return values 1, 2, -1, 0); an empty directory;
  errors (and whether `*namelist` is written); `scandirat` with a directory descriptor,
  `AT_FDCWD`, a bad descriptor with relative and absolute paths, a file descriptor;
  `alphasort`/`versionsort` called directly on 13 pairs.
* **nftw, ftw** (`== nftw, ftw over t`): every walk prints each callback (type code, level,
  base offset, path, file type from the `stat` buffer except for `FTW_NS`, and the working
  directory for `FTW_CHDIR`) and the return value. Flags 0, `FTW_PHYS`, `FTW_DEPTH`, `FTW_CHDIR`
  (on `t` and `u`), `FTW_MOUNT`, their combinations; `FTW_ACTIONRETVAL` with `FTW_SKIP_SUBTREE`
  (at a directory, at its `FTW_DP`, at a file, at the root), `FTW_SKIP_SIBLINGS` (inside a
  directory, with `FTW_DEPTH`, at the root), `FTW_STOP` and 99; non-zero returns without
  `FTW_ACTIONRETVAL` (42, -7, 1, and 5 with `FTW_CHDIR`); the self loop with and without
  `FTW_PHYS`; roots that are a file, a link to a directory (with and without a trailing slash), a
  dangling link, the loop, missing, empty, below a file, below or inside `nosearch`, `noread`,
  with `/`, `//` and `./`, absolute, `.` and `..`; `nopenfd` 0, -1, 1; `ftw` over `t`, with a
  non-zero return, a dangling link, a missing root, and `t/a` (whose `s_up` leads back to `t`).
* **Deep walks** (`== walks over the deep tree`): `nftw` with `FTW_PHYS`, `FTW_DEPTH`, 0,
  `FTW_CHDIR` (`nopenfd` 20 and 1), `ftw`; `FTW_CHDIR` walks cut with `FTW_SKIP_SUBTREE` at
  levels 18 to 22 to find where the behaviour changes; `nopenfd` 1, 2, 5, 100 with the most
  descriptors open during any callback. Each callback's path is checked against the entry it
  must name (`callbacks had a path that does not name the entry`).
* **fts** (`== fts over t`): each `FTSENT` printed with `fts_info`, level, name, path, accpath,
  `fts_errno`, the cycle entry for `FTS_DC`, the file type (unless `FTS_NOSTAT`), and the
  working directory unless `FTS_NOCHDIR`. `FTS_PHYSICAL` (chdir and `FTS_NOCHDIR`),
  `FTS_LOGICAL`, `FTS_SEEDOT`, `FTS_NOSTAT` (physical and logical), `FTS_XDEV`, no comparison
  function, `FTS_COMFOLLOW` on link roots; root lists (sorted and unsorted, missing, below a
  file, unsearchable, unreadable, empty strings, an empty `argv`, trailing and doubled slashes,
  absolute); option validation and the constants; `fts_set` with `FTS_SKIP` (preorder and
  postorder), `FTS_AGAIN` (file, preorder, postorder), `FTS_FOLLOW` (links to a directory, to a
  file, dangling, the loop, a plain file, in chdir mode), invalid instructions; `fts_children`
  before the first read, at a directory (also `FTS_NAMEONLY` and an invalid option), at a file,
  an empty, an unreadable and an unsearchable directory, and after the end; `fts_number`,
  `fts_pointer`, `fts_parent`; `fts_close` in the middle of a chdir walk; `fts64`; the deep
  tree with `FTS_PHYSICAL`, `FTS_NOCHDIR`, `FTS_LOGICAL`.
* **realpath** (`== realpath, canonicalize_file_name`): each line shows the caller-buffer result
  (the buffer is pre-filled, so a failure line says `untouched` or shows what was written); the
  allocating form and `canonicalize_file_name` are shown only when they differ. Plain paths,
  `.`, `..`, `//`, trailing slashes, relative and absolute links, a link with `..`, file
  components with `/`, `/.`, `/..`, `/x`, missing components, dangling links, loops (self and
  two-link), an unsearchable directory (`rp/ns`, 0600) and an unreadable one (`rp/nr`, 0311),
  `/` variants, empty, NULL, a 256-byte and a 255-byte component, a 4204-byte input of `/.`
  components. Link chains of 0 to 45 hops as the last component, as a directory component, and
  as up to 45 separate one-hop links in one path; `stat` on the same chains for comparison.
* **Temporary names** (`== temporary names`): each case runs eight times on fresh copies of the
  template; the line shows the template with `?` at every position that changed in any run
  (always a letter or digit), the descriptor's mode, type, size, access mode and status flags,
  close-on-exec, and whether the name is the descriptor's file. Template rules for `mkstemp`
  (six, five and eight `X`, lowercase, `X` not last, empty, trailing `/`, two `X` runs, UTF-8
  prefix, relative), creation errors (read-only, missing, below a file), four umasks, 20 calls on
  one template; `mkstemps` suffix lengths (4, 5, 3, 0, -1, 100, 14, 8, and a suffix of `X`);
  `mkostemp` with 20 flag sets; `mkostemps`; the `64` forms; `mkdtemp` (same template and error
  cases, four umasks); `mktemp`.
* **Root phase** (`== root phase`): `chroot` into a subdirectory while the working directory is
  outside it, then `getcwd`, `getwd`, `get_current_dir_name`, `realpath` of `.` and of a
  relative and an absolute path, `chdir("/in")`, `chdir("/..")`; `chroot` errors as root. A tmpfs
  mounted at `lz`, entered, detached with `umount2(MNT_DETACH)`, then the same functions and
  `opendir(".")`. A tree `m/` holding `f`, a mode-0000 directory `zero` and a second tmpfs at
  `m/mnt` with a character and a block device: `nftw` with and without `FTW_MOUNT` (also rooted
  at the mount), `fts` with and without `FTS_XDEV`. Then the removed-directory cases on the
  tmpfs.

## Observations the record shows directly

### Working directory (§13.1)

* `getcwd` with a caller buffer (`getcwd(buf, ...)` lines): `len+1` bytes is enough; `len`, 2
  and 1 bytes fail with `ERANGE`, size 0 with `EINVAL`, and a failure leaves the buffer
  untouched. With a NULL buffer, size 0 allocates; a size of at least `len+1` works (100000
  included); a smaller size fails with `ERANGE`. `getwd` returns its argument.
* Removed working directory (`-- removed working directory`): `getcwd` (both forms), `getwd`,
  and `get_current_dir_name` fail with `ENOENT`, the latter also when `PWD` names the removed
  path; `getwd` leaves its buffer untouched. `opendir(".")` succeeds and returns no entries at
  all (not even `.` and `..`) with `errno` 0. `realpath(".")` and `realpath("..")` fail with
  `ENOENT`. `chdir("..")` succeeds and leads back to `<dir>`.
* `get_current_dir_name` (`PWD=...` lines, cwd `<dir>/c/sub`, `<dir>/c_link -> c/sub`): `PWD` is
  returned verbatim whenever it names the working directory: through a link, with a trailing
  slash, with `/./`, `/../` or `//` inside, and also the relative `.`. A relative `PWD` that does
  not name the working directory (`c/sub`), another directory, a missing path and an empty `PWD`
  give the physical path.
* `chdir` (`chdir(...)` lines): `""` and missing `ENOENT`; a file, with or without a trailing
  slash, `ENOTDIR`; mode 0600 `EACCES`; mode 0333 (search without read) succeeds; the self loop
  `ELOOP`; a dangling link `ENOENT`; a 4200-byte path and a 256-byte component `ENAMETOOLONG`.
  Through `t/s_dir` the working directory is the physical `<dir>/t/a`. `fchdir` works on
  `O_RDONLY` and `O_PATH` directory descriptors; a file is `ENOTDIR`, a closed descriptor and -1
  `EBADF`. Unprivileged `chroot` of an existing directory is `EPERM`, of a missing one `ENOENT`.
* Deeper than `PATH_MAX` (`== working directory deeper than PATH_MAX`, cwd length `<dir>+5030`):
  `getcwd` succeeds with a buffer of at least `len+1` bytes and with `getcwd(NULL, 0)`; a
  `PATH_MAX` buffer and `getcwd(NULL, PATH_MAX)` fail with `ERANGE` (not `ENAMETOOLONG`), the
  buffer untouched. `getwd` fails with `ERANGE` and leaves its buffer untouched.
  `get_current_dir_name` returns the full path, with `PWD` unset or set to it. With an ancestor
  that can be searched but not read (`chmod(level 10, 0111)`), `getcwd` (both forms),
  `get_current_dir_name` and `realpath(".", NULL)` fail with `EACCES`.
* Root phase, `chroot` with the working directory outside the new root (`chroot(c) with cwd
  <dir>`): `getcwd` fails with `ENOENT`, and the caller's buffer then holds
  `(unreachable)<dir>`, the text the kernel wrote; `getwd` fails with `ENOENT` and leaves its
  buffer untouched; `get_current_dir_name` fails with `ENOENT` with `PWD` unset or `/`;
  `realpath(".")` and of a relative path fail with `ENOENT` and write an empty string to the
  buffer; absolute paths resolve inside the new root (`/in/..` is `/`); `chdir("/in")` gives
  `/in` and `chdir("/..")` stays at `/`. As root, `chroot` of a missing path and of `""` is
  `ENOENT`, of a file `ENOTDIR`.
* Root phase, lazy unmount (`-- lazy unmount`): after `umount2(MNT_DETACH)` of the file system
  holding the working directory, the same as after `chroot`: `ENOENT` with
  `(unreachable)/in` left in the buffer, `get_current_dir_name` `ENOENT` even with `PWD` naming
  the old path, `realpath` `ENOENT` with an empty buffer; `opendir(".")` still reads 2 entries.

### Directory streams (§13.2)

* `opendir` gives a descriptor with close-on-exec and `O_NONBLOCK` set (`opendir(r): ok`).
  `fdopendir` keeps the caller's descriptor (`dirfd==fd: yes`) and sets close-on-exec on it.
  `closedir` closes the descriptor (`EBADF` afterwards). `telldir` before the first read is 0
  (order: ext4).
* Entries (`r in directory order`, `r sorted, with d_type`): 8 entries including `.` and `..`,
  which are not first in directory order **(order: ext4)**. `d_type` is set for every kind:
  `DIR`, `REG`, `LNK`, `FIFO`, `SOCK` on ext4, and `CHR`, `BLK` on tmpfs (root phase, `m/mnt
  sorted, with d_type`). A 255-byte name is returned whole. `d_reclen` is 24 for one-byte names
  and 280 for the 255-byte name.
* End of stream: `readdir` returns NULL and leaves `errno` as it was (0 stays 0, `EINTR` stays
  `EINTR`); a further call returns NULL again. `readdir_r` returns 0 with a NULL result at the
  end and after it. `readdir64`, `readdir64_r` and two interleaved streams on one directory
  return all 8 entries each.
* Storage (`two readdir calls return the same storage: no; ... still intact: yes`): two
  consecutive `readdir` calls return different pointers and the first entry is still intact
  after the second call. The contract's "overwritten by the next call" is a permission, not
  what happens here; a caller cannot rely on either.
* `opendir` errors (`-- opendir errors`): `""` and missing `ENOENT`; a file, a file with `/`, a
  link to a file and a FIFO `ENOTDIR` (the FIFO open does not block); mode 0333 `EACCES`; a path
  below mode 0600 `EACCES`; the loop `ELOOP`; a 256-byte name `ENAMETOOLONG`. Mode 0600 (read
  without search) opens and lists 3 entries; a link to a directory and a trailing slash work.
* `fdopendir` (`-- fdopendir, dirfd`): on a descriptor already read to the end with
  `getdents64` the stream returns 0 entries until `rewinddir`, then 8 (no implicit rewind). A
  file descriptor gives `ENOTDIR` and stays open (ownership is not taken on failure); an
  `O_PATH` directory descriptor, -1 and a closed descriptor give `EBADF`.
* Errors while reading (`-- errors while reading`): with the stream's descriptor closed
  underneath, `readdir` returns NULL with `EBADF`, `readdir_r` returns 9 (`EBADF`) with a NULL
  result, and `closedir` returns -1 with `EBADF`. A directory removed after `opendir` reads as
  empty: 0 entries and `errno` 0 (not `ENOENT`) **(fs: ext4)**; the next item shows the same on
  tmpfs and in every variant.
* Removed directory (`-- directory removed while open`; record lines 165-193 on **ext4**, each
  starting `[fs ext2/ext3]`, and lines 1614-1642 on **tmpfs**, `[fs tmpfs]`). The two file
  systems agree on every line except one `getdents64` comparison line (the last item below).
  * No reading function reports an error for a removed directory. `readdir` and `readdir64` end
    with NULL and `errno` 0, `readdir_r` and `readdir64_r` return 0 with a NULL result, in
    every variant: removed before the first read (0 entries, and the same at a second call),
    removed after the first entry of an empty directory, after 3 of the 5 entries of a 3-file
    directory, and after 50 entries of a 2000-file directory (the last two emptied first).
  * After the removal a stream still returns the entries it had already buffered, then ends:
    `..` after `.` for the empty directory, the remaining 2 of 5, and for the 2000-file
    directory some entries but fewer than the 1952 left. How many come from the buffer depends
    on the stream's buffer size (a `strace` outside the record shows 32768-byte `getdents64`
    calls, the first returning 456 of the 2002 entries on ext4), so the record prints only
    `some` and `fewer than the 1952 left: yes`.
  * `rewinddir`, and `seekdir(0)`, after the removal give 0 entries and `errno` 0: the entries
    cannot be read again. The same when the removal came before the first read.
  * `fdopendir` on the descriptor of a removed directory succeeds (`ok`, `errno` 0) and the
    stream reads 0 entries, `errno` 0; `fdopendir` followed by the removal reads 0 entries with
    `readdir_r` returning 0.
  * The kernel does report the removal: `getdents64` on a descriptor of a removed directory fails
    with **`ENOENT`** on both file systems, before the first call and after a partial read (the
    2000-file directory emptied and removed), on every further call, and after `lseek(fd, 0,
    SEEK_SET)` (which itself returns 0). `open(".")` in a removed working directory succeeds and
    its `getdents64` fails with `ENOENT` too. The library therefore turns the kernel's `ENOENT`
    into an end of stream; a `strace` of the probe (both phases, outside the record) shows each
    such reading call issuing `getdents64(fd, buf, 32768)` = -1 `ENOENT` while `readdir`
    returns NULL with `errno` 0 and `readdir_r` returns 0.
  * Comparison, emptied but **not** removed: the stream returns some buffered entries and then
    ends with `errno` 0 on both file systems. A direct `getdents64` after one 1024-byte call
    differs by file system: on **ext4** the next call still returns records (`bytes>0`), on
    **tmpfs** it returns 0 (end). Only the removal produces `ENOENT`.
* `getdents64` (`-- getdents64, getdirentries`): a 1-byte buffer is `EINVAL`, a file `ENOTDIR`,
  -1 `EBADF`; the records list the same 8 names and the call returns 0 at the end.
  `getdirentries` stores 0 in `*basep` on the first call and lists the same 8 entries.
* `rewinddir` (`-- rewinddir sees entries`): at the end of a stream, after creating `c` and
  removing `a`, `readdir` still returns NULL; after `rewinddir` the stream lists `. .. b c`.
* `telldir`/`seekdir` (`-- telldir, seekdir`, 60 entries; the cookies themselves are not
  printed) **(order: ext4)**: all positions are distinct and differ from the end position;
  `seekdir` to a recorded position makes `telldir` return that position and `readdir` return the
  entry recorded there; `seekdir` to the end position gives NULL; after `rewinddir`, `telldir`
  equals the start position; a position from one stream works in a second stream on the same
  directory; after `seekdir` to position 10 the remaining entries repeat in the same order.
* "Exactly once" (`== readdir while entries are created and removed`): all 1500 stable entries
  are returned exactly once, `.` and `..` once each, and no name (stable, removed or created) is
  returned twice; the 4 removable entries never removed are returned once each. Which of the
  removed entries were returned before removal (193 of 296) and which created entries were
  returned (195 of 555) depends on directory order **(order: ext4)** and on how the stream
  buffers; the contract allows any of them.

### scandir, alphasort, versionsort (§13.3)

* Without a comparison function the list is in directory order and includes `.` and `..`
  (`scandir(sc, NULL, NULL) in directory order`) **(order: ext4)**.
* `alphasort C`: byte order (`. .. .h 0a 1 10 9 A ... ~ Å ä é`). `alphasort en_US.UTF-8`:
  locale collation (`. .. ~ 0a 1 10 9 a A Å ä a0 ...`, `.h` sorted under `h`, `_c` under `c`).
  `alphasort cs_CZ.UTF-8`: `ch` after `h`, digits after letters. `scandir64` with
  `alphasort64` gives the same lists.
* `versionsort` gives the same list in all three locales: `strverscmp` order, so `9 < 10`,
  `a2 < a10`, `a1.9 < a1.10`, and names with leading zeros before the rest (`a001 a00 a01 a0
  a1`); `.`, `..`, `.h` first, otherwise byte order.
* The filter is called once per entry, `.` and `..` included (37 calls for 37 entries); any
  non-zero return (1, 2, -1) keeps the entry. An empty directory gives 2 entries.
* Errors (`scandir(...)` lines): missing and `""` `ENOENT`, a file `ENOTDIR`, mode 0333
  `EACCES`, the loop `ELOOP`; `*namelist` is not written on failure. Mode 0600 lists 3 entries
  with their `d_type` (`ns_f:REG`) although the entries cannot be `stat`ed.
* `scandirat` works with a directory descriptor and `AT_FDCWD`; a bad descriptor is `EBADF` with
  a relative path and ignored with an absolute one; a file descriptor gives `ENOTDIR`.
  `scandirat64` with `versionsort64` gives 37.
* Direct calls (`[a] vs [b]: ...`): `alphasort` and `versionsort` return the sign of the
  comparison; the 13 pairs show C byte order, `en_US.UTF-8` collation (`a < B`, `é < f`,
  `_c > a`) and `strverscmp` (`a0 > a00`, `a01 < a1`).

### ftw and nftw (§13.3)

Every walk line below is **(order: ext4)** unless marked tmpfs; the set of callbacks is given in
the text, the order is in the record.

* Type codes over `t` with `FTW_PHYS`: directories `FTW_D`; regular files, the FIFO and the
  socket `FTW_F` (devices too, root phase); every symbolic link, dangling or not, `FTW_SL`; the
  file inside mode-0600 `nosearch` `FTW_NS` (its `stat` fails), while `nosearch` itself is
  `FTW_D` (its listing is read); mode-0333 `noread` is `FTW_DNR` and none of its entries is
  reported. Levels count from 0 at the root; `base` is the offset of the last component.
* Without `FTW_PHYS` (`nftw(t, nopenfd=10, 0)`, `nftw(u, ...)`): a link to a file is `FTW_F`
  with the target's `stat`; a link to a directory is `FTW_D` and its target's entries are
  reported under the link's path (`t/s_dir/f1`); a dangling link is `FTW_SLN` with the link's
  own `stat`. Each directory is reported once only: `t/a` is never reported, because its
  contents were already reached through `t/s_dir` (which came first in directory order), and
  `t/s_dir/s_up` (a link back to `t`) is skipped silently. Which path a directory is reported
  under therefore depends on directory order. The same holds for a link-to-directory root
  (`nftw(t/s_dir, ..., 0)`: the walk goes through `s_up` into `t` and reports `t`'s other
  entries under `t/s_dir/s_up/`).
* The self loop: `nftw(lp, ..., 0)` reports `lp` and then fails with -1 `ELOOP` (the loop is
  not reported as `FTW_SLN`); with `FTW_PHYS` it is `FTW_SL`. As a root without `FTW_PHYS`:
  -1 `ELOOP`, no callback.
* `FTW_DEPTH`: each directory is reported as `FTW_DP` after its contents, the root last;
  `noread` is still `FTW_DNR`.
* `FTW_CHDIR` (`u` lines with `cwd=`): during an entry's callback the working directory is the
  directory containing the entry; for an `FTW_D` callback that is the parent; with `FTW_DEPTH`
  an `FTW_DP` callback runs inside the directory itself (`u/e` with `cwd=<dir>/u/e`, the root's
  `FTW_DP` with `cwd=<dir>/u`). Following a link to a directory changes into the target
  (`cwd=<dir>/u/a` for `u/s_dir/f1`). The caller's working directory is restored after the walk,
  also when a callback stops it (`5 at u/a/sub/x`). Over `t`, `FTW_CHDIR` fails with -1 `EACCES`
  at `nosearch` after reporting its `FTW_D` (with `FTW_DEPTH`, before reporting anything in it).
* `FTW_MOUNT` on one file system is the same walk as `FTW_PHYS`. Root phase **(order: tmpfs)**:
  with `FTW_MOUNT` neither the mount point `m/mnt` nor anything below it is reported (also with
  `FTW_DEPTH`); rooted at the mount point the walk covers that file system. Root bypasses
  permissions: the mode-0000 directory is `FTW_D`.
* `FTW_ACTIONRETVAL` (constants `FTW_CONTINUE=0 FTW_STOP=1 FTW_SKIP_SUBTREE=2
  FTW_SKIP_SIBLINGS=3`): `FTW_SKIP_SUBTREE` at a directory's `FTW_D` skips its contents and the
  walk continues; at its `FTW_DP` and at a file it has no effect; at the root it ends the walk
  with 0. `FTW_SKIP_SIBLINGS` at `t/a/f1` skips the rest of `t/a` and the walk continues (with
  `FTW_DEPTH`, `t/a`'s `FTW_DP` is still reported); at the root it ends the walk with 0.
  `FTW_STOP` ends the walk and `nftw` returns 1; an undefined value (99) also ends it and is
  returned.
* Without `FTW_ACTIONRETVAL` any non-zero return (42, -7, 1) ends the walk and is returned.
* Roots (`-- roots`): a file is reported at level 0 with `base` 2; a link to a directory with
  `FTW_PHYS` is `FTW_SL`, and a trailing slash (`t/s_dir/`) does not change that, the path being
  reported without the slash; `t/a/` is reported as `t/a`; `t//a` keeps the double slash (base
  3); `./t/a/sub` keeps `./` (base 6); an absolute root gives absolute paths. A dangling link
  root is `FTW_SLN` (or `FTW_SL` with `FTW_PHYS`). `"."` is reported as `.` with base 0, `".."`
  with children `../x` (base 3). Missing and `""` roots fail with -1 `ENOENT`, a root below a
  file with `ENOTDIR`, a file below `nosearch` with `EACCES`, all without a callback. Root
  `nosearch`: `FTW_D` then `FTW_NS` for its file; with `FTW_CHDIR`, `FTW_D` then -1 `EACCES`.
  Root `noread`: `FTW_DNR` only (also with `FTW_DEPTH`). `nftw("/")` reports `/` with base 0.
* `nopenfd` 0, -1 and 1 are accepted and give the full walk of `t/a`.
* `ftw` (`-- ftw`): symbolic links are always followed; there is no `FTW_SL`/`FTW_SLN`, and a
  dangling link (also as the root) is `FTW_NS`. Directories are reported once, as with `nftw`
  without `FTW_PHYS` (`t/a` not reported). A non-zero return (5) ends the walk and is returned.
  `ftw(t/a, 0)` follows `s_up` into `t` and reports `t`'s other entries under `t/a/s_up/`.
* Deep tree (`== walks over the deep tree`, paths up to 5036 bytes): `nftw` with `FTW_PHYS`,
  `FTW_PHYS|FTW_DEPTH` and 0, and `ftw`, report all 27 entries with correct paths. With
  `nopenfd=1` (no `FTW_CHDIR`) the walk fails with -1 `ENAMETOOLONG` after 21 callbacks (once a
  path passes `PATH_MAX`); with `nopenfd` 2, 5 and 100 it succeeds. The most descriptors open
  beyond the caller's during a callback equal `nopenfd` (1, 2, 5) and the depth (26) for 100.
* Deep tree with `FTW_CHDIR` and `nopenfd=20` (also with `FTW_DEPTH`): the process is killed by
  `SIGABRT` (the library prints a "Fatal glibc error ... assertion failed" message on standard
  error). The cut walks locate the change: up to level 20 (paths up to 4024 bytes) all is
  correct; at level 21 (4225 bytes) the walk completes but the level-21 callback receives an
  empty path (length 0) with `base` 4025; descending to level 22 aborts. With `FTW_CHDIR` and
  `nopenfd=1` the whole deep walk succeeds with correct paths.

### fts (§13.3)

* `FTS_PHYSICAL` (chdir mode): the working directory during an entry is its parent and
  `fts_accpath` is the name (the root's accpath is its path); the walk returns to `<dir>` for the
  root's `FTS_DP` and after `fts_close`. With `FTS_NOCHDIR` the accpath is the path and the
  working directory never changes. `FTS_LOGICAL` never changes the working directory either.
* `fts_info` values (constants line: `D=1 DC=2 DEFAULT=3 DNR=4 DOT=5 DP=6 ERR=7 F=8 INIT=9 NS=10
  NSOK=11 SL=12 SLNONE=13 W=14`): files `FTS_F`; FIFO, socket and devices `FTS_DEFAULT`; every
  link under `FTS_PHYSICAL` `FTS_SL`. Mode-0333 `noread` is returned twice: `FTS_D`, then
  `FTS_DNR` with `fts_errno` `EACCES`, and no `FTS_DP`. Mode-0600 `nosearch`: in chdir mode
  `FTS_D`, then `FTS_DP` with `fts_errno` `EACCES` and no children; with `FTS_NOCHDIR`, `FTS_D`,
  its file as `FTS_NS` (`EACCES`), then `FTS_DP`. `FTS_ERR` never appeared.
* `FTS_LOGICAL`: links are followed: a link to a file is `FTS_F`, a link to a directory is
  walked under the link's path, a dangling link and the self loop are `FTS_SLNONE`, and
  `s_up` (a link to an ancestor) is `FTS_DC` and not entered; its `fts_cycle` is the ancestor
  it repeats (`t`, level 0, by name and level: its `fts_path` is the shared path buffer, which
  then holds `t/a/s_up`). Unlike `nftw`, a directory reached by two paths
  is walked under both (`t/a` and `t/s_dir`).
* `FTS_SEEDOT`: every directory's `.` and `..` are returned as `FTS_DOT` (inside `nosearch` as
  `FTS_NS`). `FTS_NOSTAT` with `FTS_PHYSICAL`: directories stay `FTS_D`/`FTS_DP`/`FTS_DNR`,
  everything else (links included) is `FTS_NSOK`; with `FTS_LOGICAL` the entries are classified
  as without `FTS_NOSTAT`. `FTS_XDEV` on one file system changes nothing.
* Without a comparison function entries come in directory order **(order: ext4)**; roots come
  in argument order.
* `FTS_COMFOLLOW`: a root link to a directory is followed (`FTS_D`, path and name kept);
  without it the root is `FTS_SL`. Root links that are dangling or the loop are `FTS_SLNONE`, a
  link to a file `FTS_F` (with `FTS_COMFOLLOW` or `FTS_LOGICAL`).
* Roots (`-- roots`): with a comparison function the roots are sorted by their whole argument
  strings (`missing`, `t/a/sub`, `t/b`, `t/e`), and `fts_children` before the first read shows
  root names as given (`t/b`, `t/e`); the entries `fts_read` returns have the last component as
  `fts_name` (`sub`, `b`). A missing root is `FTS_NS` (`ENOENT`) at level 0; a root below a file
  `FTS_NS` (`ENOTDIR`); a file below `nosearch` `FTS_NS` (`EACCES`). An empty string among the
  roots makes `fts_open` fail with `ENOENT`. `t/a/` is returned with an empty `fts_name` and path
  `t/a/`, its children as `t/a/f1`; `t//e` and `./t/a/sub` keep their spelling. An absolute root
  in chdir mode gives absolute paths and name accpaths.
* Options (`-- options`): `fts_open` accepts 0 (neither `FTS_LOGICAL` nor `FTS_PHYSICAL`), both
  together, and `FTS_NOCHDIR` alone; bits outside `FTS_OPTIONMASK` (0x1000, `FTS_NAMEONLY`,
  `FTS_STOP`) give `EINVAL`.
* `fts_set` (`-- fts_set`): `FTS_SKIP` at a directory's `FTS_D` skips its contents but its
  `FTS_DP` is still returned; at `FTS_DP` it has no effect. `FTS_AGAIN` returns the entry again
  at once: a file as `FTS_F`, a directory at preorder as `FTS_D` (then walked once), a
  directory at postorder as `FTS_D` and the directory is walked again. `FTS_FOLLOW` at an
  `FTS_SL` returns the entry again resolved: a link to a directory as `FTS_D`, walked under the
  link's path (in chdir mode the working directory is the target, `<dir>/t/a`); a link to a
  file as `FTS_F`; a dangling link and the self loop as `FTS_SLNONE`. `FTS_FOLLOW` at a regular
  file does nothing. Instructions 7 and -1 return **1** with `EINVAL`.
* `fts_children` (`-- fts_children`): before the first `fts_read` it lists the roots (sorted).
  At a directory it lists the children with their `fts_info` without changing the working
  directory; `FTS_NAMEONLY` gives `FTS_NSOK` entries; option 5 gives NULL with `EINVAL`. At a
  file and at an empty directory it returns NULL with `errno` 0; at `noread` NULL with `EACCES`
  (then `fts_read` returns `FTS_DNR`); at `nosearch` it returns the `FTS_NS` entries with
  `errno` `EACCES` under `FTS_NOCHDIR`, and NULL with `EACCES` in chdir mode. Called after
  `fts_read` has returned NULL it crashes the process (`SIGSEGV`, in a child).
* At the end `fts_read` returns NULL with `errno` 0, and again on a further call.
  `fts_close` returns 0 and, called in the middle of a chdir walk (cwd `<dir>/t/a/sub`),
  restores `<dir>`.
* `FTSENT` fields: the root is level 0 and its `fts_parent` level -1 (`FTS_ROOTPARENTLEVEL`);
  `fts_number` starts at 0; `fts_number` and `fts_pointer` set at a directory's `FTS_D` are
  seen at its `FTS_DP` (the same entry) and through the children's `fts_parent`.
  `fts_namelen`/`fts_pathlen` always matched the strings (no `LENGTHS-DIFFER`).
* `fts64` walks `t` with the same 22 entries.
* Deep tree: `FTS_PHYSICAL` in chdir mode returns all 53 entries (one extra descriptor at most).
  `FTS_PHYSICAL|FTS_NOCHDIR` and `FTS_LOGICAL` (with or without `FTS_NOCHDIR`) return the
  level-21 directory (path 4225 bytes) as `FTS_NS` with `ENAMETOOLONG`, do not descend below it,
  and finish normally (`errno` 0).
* Root phase **(order: tmpfs)**: `FTS_XDEV` returns the mount point as `FTS_D` and `FTS_DP`
  without descending into it, in chdir and `FTS_NOCHDIR` mode; a root on the mount is walked
  fully. Device nodes are `FTS_DEFAULT`.

### realpath, canonicalize_file_name (§13.8)

* The allocating form and `canonicalize_file_name` gave the same result as the caller-buffer
  form on every line: no line shows a difference.
* Results: `.` and `..` components, `//` and trailing slashes on directories are removed;
  relative and absolute links, a link whose target contains `..`, and a path through a link to
  `..` resolve to physical paths. `/`, `//`, `///`, `/..`, `/../..`, `//.` are all `/`.
* A file followed by `/`, `/.`, `/..` or `/x` (also through a link) is `ENOTDIR`, buffer
  untouched.
* `ENOENT` (`realpath(rp/missing...)`): the buffer holds the resolved prefix **including the
  missing component**: `<dir>/rp/missing` for `rp/missing`, `rp/missing/`, `rp/missing/x` and
  `rp/missing/../a` (the `..` after a missing component is not applied). A dangling link writes
  the prefix with the link's target substituted (`rp/l_dang` gives `<dir>/rp/missing`).
* `ELOOP` (self loop, two-link loop, also followed by `/x`): buffer untouched.
* `EACCES`: a path through mode-0600 `ns` fails and the buffer holds the path up to and
  including the component that could not be examined (`<dir>/rp/ns/f`; `<dir>/rp/ns/d` for
  `rp/ns/d/..`). `rp/ns/..` succeeds (`<dir>/rp`): `..` after an unsearchable directory is
  removed without examining it. Mode 0311 (search without read) resolves.
* `""` is `ENOENT` with the buffer untouched; a NULL path is `EINVAL` for both functions.
* Lengths: a 256-byte component is `ENAMETOOLONG` (buffer untouched); a 255-byte missing one
  `ENOENT` with the prefix. A 4204-byte input made of `/.` components succeeds (the input length
  itself is not limited). An input over `PATH_MAX` whose components are real (5036 bytes) fails
  with `ENAMETOOLONG` in all three forms. From inside the deep tree, a relative path whose
  result would exceed `PATH_MAX` fails with `ENAMETOOLONG` in all three forms, the caller's
  buffer untouched (no prefix is written for this error); `realpath(".", NULL)` with a working
  directory longer than `PATH_MAX` succeeds (`len=<dir>+5030`) while `realpath("bottom", NULL)`
  from the same directory fails with `ENAMETOOLONG`, and `realpath(".", buf)` fails with
  `ENAMETOOLONG`, buffer untouched.
* Link limits (`-- symbolic-link chains`): a chain of 40 links resolves and 41 fail with
  `ELOOP`, the same limit as `stat`; the same as a directory component (`m<k>/f`); and the
  count is per call: 40 separate one-hop links (`h -> .`) in one path resolve, 41 fail with
  `ELOOP`.

### Temporary names (§13.9)

* `mkstemp` replaces exactly the last six `X` with letters and digits (`mk/aXXXXXXXX` gives
  `mk/aXX??????`; `mk/a.XXXXXX.XXXXXX` keeps the first run), creates a regular file of size 0
  with mode 0600 masked by the umask (0600 under 022, 077 and 000; 0400 under 277; 0000 under
  777), open `O_RDWR` without close-on-exec, and the name is the descriptor's file. A UTF-8 prefix
  and a template with no directory work. 20 calls on one template give 20 distinct files.
* Template errors (`EINVAL`, template unchanged): five `X`, none, lowercase `x`, a character
  after the `X`, fewer than six characters, empty, a trailing `/`.
* Creation errors: a read-only directory `EACCES`, a missing directory `ENOENT`, a path below a
  file `ENOTDIR`. The template then holds the last name tried (six varying characters), not the
  original.
* `mkstemps(t, n)`: the six `X` must end exactly `n` bytes before the end. `n` = 4 with `.txt`
  works; 5, 3, 14, 8, -1 and 100 give `EINVAL` with the template unchanged; `n` = 0 is
  `mkstemp`; a suffix of `XX` is kept (`mk/b??????XX`).
* `mkostemp` flags: `O_CLOEXEC`, `O_APPEND`, `O_SYNC`, `O_DSYNC`, `O_NONBLOCK`, `O_NOATIME`,
  `O_DIRECT` and combinations are applied; the access mode is always `O_RDWR` whatever is passed
  (`O_WRONLY`, `O_RDWR`, `O_ACCMODE`); `O_TRUNC`, `O_CREAT|O_EXCL`, `O_NOFOLLOW`, `O_NOCTTY` and
  the unknown bit 0x40000000 are accepted without visible effect. `O_DIRECTORY` and `O_TMPFILE`
  fail with `EINVAL`, `O_PATH` with `ENOENT`, each after a name was tried (the template holds
  it). `mkostemps` combines the suffix rule and the flags; the `64` forms behave the same.
* `mkdtemp`: returns its argument; the directory has mode 0700 masked by the umask (0700, 0700,
  0500, 0000 under 077, 000, 277, 777); the template and creation errors are those of `mkstemp`
  (a trailing `/` is `EINVAL`).
* `mktemp`: returns its argument with a name that does not exist and creates nothing, also in a
  missing or read-only directory. On `EINVAL` (five `X`, no `X`, empty) it returns its argument
  with `template[0]` set to NUL; below a file it does the same with `errno` `ENOTDIR`.
* `EEXIST` after exhausting the attempts is not in the record; it was provoked with `strace`
  fault injection (section "Exhaustion under fault injection" at the end): 238,328 attempts
  (62^3) for every function.

### System calls

A separate small program (not part of the record) was traced with `strace` on the VM to see
which calls reach the kernel, as the spec's "Kernel" lines document them:

* `opendir`, `scandir`: `openat(AT_FDCWD, path, O_RDONLY|O_NONBLOCK|O_CLOEXEC|O_DIRECTORY)`,
  `fstat`, `getdents64` until it returns 0, `close`. `fdopendir`: `fstat`, `fcntl(F_GETFL)`,
  `fcntl(F_SETFD, FD_CLOEXEC)`. `seekdir`/`rewinddir`: `lseek`.
* A removed directory (a `strace` of the probe, both phases): each `readdir`, `readdir64`,
  `readdir_r` or `readdir64_r` call that needs more entries issues `getdents64(fd, buf,
  32768)`, which returns -1 `ENOENT`; the function then reports the end of the stream.
* `getcwd`: the `getcwd` system call. `get_current_dir_name` with `PWD` set: `getcwd`, then
  `newfstatat` of `.` and of `PWD`; with `PWD` unset: `getcwd` only.
* `realpath` of a relative path: `getcwd`, then `readlink` of each successive absolute prefix.
* `mkstemp`: `openat(AT_FDCWD, name, O_RDWR|O_CREAT|O_EXCL, 0600)`; `mkostemp` adds its flags
  to that call; `mkdtemp`: `mkdir(name, 0700)`; `mktemp`: `newfstatat(..., AT_SYMLINK_NOFOLLOW)`
  of the name, and a `getrandom` call was visible before it.
* `nftw`: `newfstatat` of the root, `openat` of each directory (`O_NONBLOCK|O_DIRECTORY`, the
  first also `O_CLOEXEC`), `getdents64`, `newfstatat(dirfd, name, AT_SYMLINK_NOFOLLOW)` per
  entry; with `FTW_CHDIR` also `openat(".")` and `fchdir` into each directory and back.
* `fts` in chdir mode: `openat(".")`, `fchdir` into each directory, `newfstatat(AT_FDCWD, name)`,
  `openat("..")` and `fchdir` to go up; with `FTS_NOCHDIR`, `newfstatat(AT_FDCWD, path)`.

## Observations that look like historical artifacts

The implementer may decide these differently (and record the difference).

* `nftw` with `FTW_CHDIR` over paths longer than about `PATH_MAX` aborts the process (an
  assertion), and one level earlier passes the callback an empty path; with `nopenfd=1` the same
  walk works, while without `FTW_CHDIR` `nopenfd=1` fails with `ENAMETOOLONG`. D-13.4 (walkers
  with no depth limit) removes all three.
* `fts_open` with an empty `argv` returns a stream whose first `fts_read` crashes, and
  `fts_children` after the end of the walk crashes; both are use of a stream with no current
  entry.
* `fts_set` reports an invalid instruction by returning 1, not -1 (the man page says -1).
* `fts_open` accepts options with neither or both of `FTS_LOGICAL` and `FTS_PHYSICAL`, which
  fts(3) says is required to be exactly one.
* `realpath(".", NULL)` succeeds for a working directory longer than `PATH_MAX`, but any other
  relative path from there fails with `ENAMETOOLONG` even in the allocating form (D-13.5
  intends no length limit internally).
* `nftw` without `FTW_PHYS` reports a self loop by failing the whole walk with `ELOOP`, while a
  dangling link is `FTW_SLN`; `fts` reports both as `FTS_SLNONE`.
* `nftw` reports each directory once, under whichever path reached it first, so the paths a
  walk reports depend on directory order; `fts` walks such a directory under every path.
* `get_current_dir_name` returns a relative `PWD` (`.`) verbatim.
* `readdir` (all four forms) on a directory removed after `opendir`, and `opendir(".")` in a
  removed working directory, read as empty streams with `errno` 0, on ext4 and on tmpfs,
  although `getdents64` itself fails with `ENOENT`: the library turns the kernel's `ENOENT`
  into the end of the stream. §13.2's Failure clause (`s13_2_f_0`) lists `ENOENT` for this
  case; with glibc 2.39 on Linux 6.8 it is not reachable through `readdir`, only by calling
  `getdents64` directly.
* `mkostemp` silently replaces the caller's access mode with `O_RDWR`; `O_PATH` fails with
  `ENOENT` after a name was tried.
* `mktemp` below a file empties the template and leaves `ENOTDIR`, while a missing or read-only
  directory still yields a name.
* `getwd` leaves its buffer untouched on every failure.
* `getcwd` with a too-small buffer for a working directory longer than `PATH_MAX` fails with
  `ERANGE`, never `ENAMETOOLONG`.

## What the record does not settle

* `EEXIST` after exhausted attempts (§13.9, the spec's "about 238,000 attempts"): not in the
  record, since no file system at hand makes every exclusive create fail with `EEXIST`. It was
  provoked outside the record with `strace` fault injection (the last section).
* `ENOENT` from `readdir` for a removed directory (§13.2): not reachable on ext4 or tmpfs (see
  "Removed directory" above); other file systems are deferred with the rest.
* `EOVERFLOW` from `readdir` (§13.2): `ino_t` is 64 bits on x86_64, so it cannot occur.
* Directory order on file systems other than ext4 and tmpfs, and any convention for comparing
  unordered results: deferred by the owner. Lines marked **(order: ...)** hold only on that file
  system; the differential test runs in a scratch directory under `target/`, which must be on
  the same kind of file system for those lines to match.
* File systems that report `DT_UNKNOWN` (callers must `stat`), and `readdir_r` with names longer
  than 255 bytes: not available here.
* The `__getcwd_chk`, `__getwd_chk` and `__realpath_chk` forms (the probe disables
  `_FORTIFY_SOURCE`); `__realpath_chk`'s use of the real buffer size (D-13.5) is not probed.
* The exact allocation size of `getcwd(NULL, 0)` (§13.1 "exactly the needed size"); it is not
  observable through public interfaces.
* `getdirentries`' `*basep` after the first call, `seekdir` to values that `telldir` did not
  return, and streams shared between threads.
* `FTW_MOUNT` and `FTS_XDEV` together with following links that cross file systems; `FTS_W`
  (no whiteouts on Linux file systems).
* How many created or removed entries a stream returns while the directory changes: the
  contract allows any number, and the counts depend on directory order and buffering.
* The source of randomness for the temporary names (`getrandom`, or a clock fallback): only
  the variation of the characters is observable.

## Exhaustion under fault injection (strace, outside the record)

`strace` output is not part of the record, so this section stands on its own. The functions
were made to exhaust their attempts by injecting a failure into every create: `openat` fails
with `EEXIST` (`mkstemp`, `mkostemp`, `mkstemps`, `mkostemps`, `mkstemp64`, `tmpfile`), `mkdir`
fails with `EEXIST` (`mkdtemp`), and `newfstatat` succeeds with `retval=0`, so that every name
appears to exist (`mktemp`). Each attempt is one injected call, so the number of attempts is the
number of `(INJECTED)` lines in the trace.

**Setup.** The VM above: Ubuntu 24.04, glibc 2.39-0ubuntu8.9, kernel 6.8.0-146-generic,
`strace -- version 6.8`. The program is the probe itself (`cc -std=gnu17 -O1 -o p
fstree_diff.c`) in the mode `p DIR exhaust FUNC`: it enters `DIR` (a fresh, empty scratch
directory `~/fsprobe/ex-<run>-<func>`, on ext4), makes one call on the template `aXXXXXX`
(`aXXXXXX.sfx` with suffix length 4 for the `s` forms; flags `O_CLOEXEC` for the `o` forms),
and prints the return value, `errno` and the template. Nothing before the call opens or creates
a file. The dynamic loader's two `openat` calls (`/etc/ld.so.cache`, `libc.so.6`) come before
`main`, so `when=3+` starts the injection at the function's first call; the count check below
confirms that nothing else was injected. `-P` was not used: it matches whole paths, and the
names tried are relative and random.

**Commands** (as the ordinary user; `LOG` and `DIR` per run and function):

```
env -i PATH=/usr/bin:/bin strace -o LOG -e trace=openat -e inject=openat:error=EEXIST:when=3+ \
    -e signal=none ./p DIR exhaust mkstemp   # likewise mkostemp, mkstemps, mkostemps, mkstemp64, tmpfile
env -i PATH=/usr/bin:/bin strace -o LOG -e trace=mkdir -e inject=mkdir:error=EEXIST \
    -e signal=none ./p DIR exhaust mkdtemp
env -i PATH=/usr/bin:/bin strace -o LOG -e trace=newfstatat -e inject=newfstatat:retval=0 \
    -e signal=none ./p DIR exhaust mktemp
grep -c INJECTED LOG                                                       # attempts
grep INJECTED LOG | grep -c '"a[A-Za-z0-9]\{6\}\(\.sfx\)\{0,1\}"'          # of them, template names
grep INJECTED LOG | tail -1                                                # the last name tried
ls -A DIR | wc -l                                                          # files left
```

**Results.** Four complete runs of all eight functions on 2026-10-03, each call in a fresh
directory (the first with an earlier build of the probe whose `mktemp` line did not yet print
the bytes after the NUL; the other three with the final one). The counts were identical in
every run, and no run left a file in its directory.

| Function | Attempts (injected calls) | Result | Template afterwards |
|---|---|---|---|
| `mkstemp` | 238,328 × `openat(AT_FDCWD, name, O_RDWR\|O_CREAT\|O_EXCL, 0600)` | -1, `EEXIST` | the last name tried (six letters or digits in place of the `X`) |
| `mkostemp` (`O_CLOEXEC`) | 238,328 | -1, `EEXIST` | the last name tried |
| `mkstemps` (`.sfx`, 4) | 238,328 | -1, `EEXIST` | the last name tried, suffix kept |
| `mkostemps` (`.sfx`, 4, `O_CLOEXEC`) | 238,328 | -1, `EEXIST` | the last name tried, suffix kept |
| `mkstemp64` | 238,328 | -1, `EEXIST` | the last name tried |
| `mkdtemp` | 238,328 × `mkdir(name, 0700)` | NULL, `EEXIST` | the last name tried |
| `mktemp` | 238,328 × `newfstatat` of the name | its argument, `errno` `EEXIST` | `template[0]` set to NUL; the bytes after it are the rest of the last name tried |
| `tmpfile` | 1 × `openat(AT_FDCWD, "/tmp", O_RDWR\|O_EXCL\|O_TMPFILE, 0600)`, then 238,328 × `openat(AT_FDCWD, "/tmp/tmpf??????", O_RDWR\|O_CREAT\|O_EXCL, 0600)`: 238,329 in all | NULL, `EEXIST` | no template; nothing was created |

* 238,328 is exactly 62^3. The spec's "about 238,000 attempts" is this number; it is the same
  for every function and does not depend on the suffix or the flags.
* For every template function, every injected call named a path made from the template (the
  two counts were equal, so the loader's calls were untouched), and the template afterwards
  held exactly the last name passed to the kernel (for `mktemp`, that name with its first byte
  replaced by NUL).
* `tmpfile` first tries an unnamed `O_TMPFILE` file in `/tmp`, then named files `/tmp/tmpf` plus
  six characters, with the same limit; it does not use the caller's working directory. Every
  create failed, so nothing was created in `/tmp`.
* `getrandom(buf, 8, GRND_NONBLOCK)` calls appear between attempts (149,754 in one traced
  `mkstemp` run, which also traced `getrandom`); how the names are drawn is not part of the
  contract and was not examined further.
