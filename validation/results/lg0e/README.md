# LG-0e LEDGER-COREUTILS: the two coreutils ledger items of phase 15

Run 2026-10-23 on shiki (Ubuntu 24.04, glibc 2.39, gcc 13), in a fresh tmpfs scratch tree, by
job `ledger-coreutils-validate` (rc 0, 244 s). Liella: `cargo xtask so` on the story branch
(base origin/next `01a1d6d0`; synced tree `3b3e3f64`), linked through `scripts/liella-cc` as is (no
stand-ins, no `--allow-multiple-definition`). glibc: the host `gcc`, dynamic. Sources: coreutils 9.12
and findutils 4.11.0, the tarballs listed in `validation/README.md` (SHA-256 equal, signatures good),
configured with `--without-selinux` and built with `make -k`; both builds succeeded under both
libraries. Only configure output was read (`config.log`'s cache and output variables, its `checking`
lines and the test programs it prints); no gnulib source.

## 1. gnulib replacements selected by configure (owner ledger, 2026-10-03)

Files: `{glibc,liella}-coreutils.{replace,cache}`, `{glibc,liella}-findutils.replace`, and their diff
`replace-diff.txt` (extracted the way `validation/suite.sh` does).

**No replacement is selected under Liella that is not selected under glibc.** The earlier
Liella-only ones (`memchr`, `strstr`, `strcasestr`, the printf family, `strndup`, `futimens`,
`getgroups`, `frexp`/`ldexp`) are all gone. Every remaining difference is gnulib working around
glibc:

| Replaced under glibc only | configure check (glibc / Liella) | Differences row |
|---|---|---|
| `fflush`, `fclose`, `fopen`, `fseek`, `fseeko` (`REPLACE_*` 1 / 0; coreutils and findutils) | "fflush works on input streams": no / yes. glibc's `fflush` after an `ungetc` of a different byte leaves the descriptor at the end of its read-ahead (probe `validation/probes/fflush_ungetc.c`, below). `fclose`, `fopen` and the seeks follow the `fflush` replacement. | `fflush` on a seekable input stream |
| `pthread_rwlock_init`, `pthread_rwlockattr_init` (coreutils and findutils) | "reasonable pthread_rwlock wait queue handling": no / yes | `pthread_rwlock` waiting order |
| gnulib's regex (`lib/libcoreutils_a-regex.o` in the glibc build only; no `REPLACE_` variable) | "working re_compile_pattern": no / yes | `re_compile_pattern` |
| (not a replacement) `HAVE_DLOPEN` 1 / 0, `gl_cv_lib_dl` none / no | Liella's static `libc.a` has no loader functions until phase 14 | none: scope, not a difference |

The other cache lines in the diff (`ac_cv_func__fseeki64`, `ac_cv_have_decl_isblank`,
`ac_cv_header_libintl_h`, `gl_cv_func_fclose_stdin`) are checks configure runs only on one branch
of the replacements above; none changes a result.

Probe (`validation/probes/fflush_ungetc.c`; read 5 bytes, `fflush`; backup `ungetc` then `fflush`;
non-backup `ungetc('@')` then `fflush`, then `lseek(fd, 0, SEEK_CUR)`, `ftell`, `fgetc`):

```
glibc   fflush_after_read rc=0 fd_off=5 | backup_ungetc read=f after_fflush=f | nonbackup_ungetc read=g fflush=0 fd_off=25 ftell=6 next=h
liella  fflush_after_read rc=0 fd_off=5 | backup_ungetc read=f after_fflush=f | nonbackup_ungetc read=g fflush=0 fd_off=6 ftell=6 next=g
```

## 2. A directory removed during the walk (D-13.2)

`validation/traversal.sh` (unchanged) on the four programs of the builds above; `traversal.tsv`
(the program path prefix removed from the standard error column). `top/b` (three files and a
subdirectory) is removed while one `getdents64` call on it is held by strace: before its first
read (read 1) or after its entries were read (read 2).

| Program | glibc | Liella |
|---|---|---|
| `ls -R` | exit 0, same listing | identical |
| `find` | read 1: exit 0, silent; read 2: `‘top/b/sub’: No such file or directory`, exit 1 | `‘top/b’: No such file or directory`, exit 1 (both moments; read 2 also reports `top/b/sub`); same number of lines printed |
| `du` | lists `top/b` with what it counted, exit 0 | prints one line fewer (3 instead of 4; at milestone 4 the missing line was `top/b`'s; this run kept only the counts), exit 0 |
| `rm -r` | `cannot remove` for each vanished entry, exit 1 | the same, plus `traversal failed: top/b: No such file or directory` |

All four behave sensibly: none crashes, hangs, loops or reports a directory that still exists as
missing. `find` and `rm` say what happened; `du` leaves out the vanished directory and still exits
0. This is the milestone-4 result again, now without the `modf` stand-in for `find` (findutils
links as is). It follows from D-13.2 (owner ruling, differences row `readdir` on a removed
directory) and needs no new row.
