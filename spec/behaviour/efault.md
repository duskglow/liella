# efault

**Spec:** §0.6 C0.1 (error reporting; the codes are the kernel's), §13 (path-taking calls).
**Clauses:** `s0_6_c0_1_1`, `s0_6_c0_1_3`.

**Question (LV-4 finding, 2026-10-09):** a path-taking call is given a pointer the process
cannot read. What does the reference library do: fail with `EFAULT`, or crash?

**Probe:** `conformance/c/s13_efault.c` run with the argument `all` (the same program is the
conformance test, which runs without arguments). **Record:** `efault.glibc.txt` (one line per
call and pointer kind). Built with `cc -std=gnu17 -O1`, run on Zen 1 (glibc 2.39, Ubuntu 24.04).
Every trial runs in a forked child, so a crash only costs its own line. Public interfaces only.

## Pointer kinds

* `PROT_NONE`: the first byte of a `PROT_NONE` page.
* `ptr1`: the address 1.
* `unterminated`: 'a' bytes up to the end of a readable page, `PROT_NONE` after: a string whose
  terminator is never reached.

Calls with a second, valid path argument (`rename`, `link`, `symlink`) test each argument in
turn. `link` uses an existing first path for the new-path trial: the kernel looks the old path
up before it reads the new one, so with a missing old path the answer is `ENOENT`.

## Result

Every path-taking call passes the pointer to the kernel and reports `-1`/`EFAULT` for all three
kinds: `access`, `euidaccess`, `faccessat`, `open` (also with `O_CREAT`), `openat`, `creat`,
`stat`, `lstat`, `fstatat`, `unlink`, `unlinkat`, `mkdir`, `rmdir`, `rename` (both arguments),
`chdir`, `chmod`, `chown`, `lchown`, `readlink`, `symlink` (both), `link` (both), `truncate`,
`execve`, `statfs`, `statvfs`, `utimensat`, `utime`, `utimes`, `mknod`, `mkfifo`, `fopen`.
`getcwd` and `readlink` with an unwritable result buffer, and `stat` with an unwritable
`struct stat`, also give `EFAULT` (the kernel writes the result).

The exceptions, where the reference reads the text in user space first and crashes (SIGSEGV):

| Call | PROT_NONE | ptr1 | unterminated |
|---|---|---|---|
| `opendir` | SIGSEGV | SIGSEGV | EFAULT |
| `realpath` | SIGSEGV | SIGSEGV | SIGSEGV |
| `gethostname`, `getdomainname` (unwritable buffer) | SIGSEGV | SIGSEGV | not tried |

## Liella

Before (main at `def2834`, static binary): every path-taking call crashed in
`liella_machine_strlen`, because the export measured the string before handing it to the kernel.
After (this change): `KStr` holds the address and measures only when a consumer asks for the
bytes, so a forwarded path reaches the kernel unread and gets its `EFAULT`. The same table as
the reference results, except:

* `opendir` gives `EFAULT` for all three kinds (the reference crashes for two). Kept: S6 gives a defined
  error where the contract has an error channel.
* `realpath`, `gethostname`, `getdomainname`: SIGSEGV, as the reference.
* `stat` with an unwritable `struct stat`: SIGSEGV; the reference gives `EFAULT`. Liella receives
  the kernel's answer by value and stores it into the caller's `Option<&mut Stat>`. The same is likely for
  other calls that fill a caller structure (not probed; see `liella-historic:handoff/efault.md`).

Calls that do read the text themselves (`realpath`, `execve`'s argument lists, `posix_spawn`,
`mq_open`) are unchanged: they measure on demand, as before.
