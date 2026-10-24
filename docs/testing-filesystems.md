# Tests and file systems

Rule (owner, 2026-10-21): nothing in the tests, harnesses, tools or product code depends on directory order or on
behaviour particular to one file system. Listings are sorted or compared as sets; no recorded result holds inode
numbers, timestamp granularity or `d_type`. We build on ext4; testers may use xfs, btrfs, zfs, ext2, vfat or tmpfs.

A few tests exercise a file-system *capability*, because that is what they test. They are listed here with what they
need and what to expect elsewhere. Nothing is engineered around them.

## Where the scratch trees live

The s11/s12/s13/s16/s18 clause tests, `native_v1`, `noproc_products`, `reload`, `nscache` and the boundary
`products/mod.rs` helpers build their scratch trees under `CARGO_TARGET_TMPDIR`, that is `target/` on the tester's
file system. There is no override variable. Every test below that needs a capability needs it **from the file system
holding `target/`**. To run the suite on a file system that lacks one, put `target/` (or `CARGO_TARGET_DIR`) on a tmpfs
or ext4.

## Tests that need a capability

| Test | Needs | What happens elsewhere |
|------|-------|------------------------|
| `appendix_a_primitives` `k_setuid`, `k_setgroups` (the full-subordinate-mapping part) | the setuid helpers `newuidmap` and `newgidmap` on `PATH` (Debian/Ubuntu package `uidmap`) and a range for the user in `/etc/subuid` and `/etc/subgid`, besides an unprivileged user namespace. | Without the helpers the part prints one `skipped: ... (install the uidmap package)` line and the test passes after its other parts; hosts that have them never skip. `LIELLA_FORCE_NO_UIDMAP=1` forces the skip path. (Hosts that deny user namespaces skip the whole test, as for the other namespace tests.) |
| `s13_10_c_1` (`s13_filesystem_clauses.rs`) | `statfs("/tmp")` and `stat -f /tmp` report the same `blocks` in two calls made moments apart. | zfs and btrfs may change the total between the two calls: a `blocks` mismatch that is the file system's, not Liella's. Put `/tmp` on ext4, xfs or tmpfs to avoid it; the other fields are stable. |
| `s13_filesystem_clauses`, `s13_filesystem_symbols`, `appendix_a_primitives` (symlinks, hard links, modes, `mkfifo`, 255-byte names) | symbolic and hard links (`nlink == 2` after `link`), permission bits, FIFOs, names of 255 bytes, case-sensitive names. | vfat/exfat (no links, modes, FIFOs, case-sensitive names): those tests fail at the first unsupported call. Not a Liella defect. |
| the same files, `utimensat`/`fstat` round trips (`123_456_789` ns) | nanosecond timestamps. | ext2 with 128-byte inodes, vfat, some NFS round the value: a mismatch of the stored time. |
| `fstree_products` | the scratch directory's file system for the unprivileged cases; passwordless `sudo -n` and `unshare -m` for the `root` cases (a fresh tmpfs mounted over its scratch directory inside a private mount namespace). | The unprivileged lines name the file system (`[fs xfs] ...`); the recorded differences are file-system neutral (`[fs *]`), so any file system works. The record `spec/behaviour/fstree.glibc.txt` was made on ext4 and tmpfs and may differ elsewhere (directory order, hash seed): the test only reports that. Without sudo the root cases are skipped, and the test says so and passes. |
| `fstree_diff.c` (case-differing names `a`/`A`/`B`, one non-ASCII name) | a case-sensitive, non-normalizing file system. | On a casefolding or normalizing file system the fixture names collide. Both builds see the same, so the comparison stays valid, but the lines differ from the record. |
| `posixipc_products` (`posixipc_run.sh`) | passwordless `sudo -n`, `unshare -i -m`; mounts a fresh tmpfs over `/dev/shm` and a fresh mqueue over `/dev/mqueue` inside a private namespace. | Whatever the host has in `/dev/shm` is hidden for the run (nothing is touched; the mounts vanish with the namespace). A tree the tester keeps in `/dev/shm`, for example a RAM-disk target, is not visible to the program while it runs. Without sudo the test says so and passes. |
| `appendix_a_primitives` `k_linkat` (the `EXDEV` part) | a writable directory on another device than the scratch directory: the first of `/dev/shm`, `/tmp`, `/var/tmp`, `/run`, `/dev` whose `st_dev` differs (a tree on tmpfs, or in `/dev/shm` itself, picks another). | A link name of its own (`liella-linkat-<pid>`) is tried; the call must fail with `EXDEV`. If none of those directories is writable and on another device, the test prints a `skipped:` line for that part and passes after its other parts. |
| `appendix_a_primitives` `k_preadv2` (`RWF_NOWAIT` read of a cached file) | non-blocking reads (`FMODE_NOWAIT`) from the file system holding the scratch file. | ext2/3/4, xfs and btrfs take them, and there the read must succeed. Elsewhere (tmpfs on Linux 6.8, for one) the kernel refuses the flag with `EOPNOTSUPP`, which the test accepts on any other file system (it reports `fstatfs`'s type when it fails). |
| `noproc_products` (`pathconf(".", _PC_LINK_MAX)` without `/proc`) | nothing; the recorded difference depends on the working directory's file system. | On ext2/3/4 (one magic number) the reference needs `/proc/self/mountinfo` to tell ext4 and leaves its failed open's `ENOENT` in `errno`: the recorded difference applies there only. On any other file system both must print the same line (observed on tmpfs; elsewhere a difference shows as an unrecorded one, to be recorded for that file system). |
| `s13_2_c_*`, `wordexp_budget_time`, `regex_budget_time`, `charmap_data`, glob tests | nothing: names are sorted or compared as sets. | same result on any file system. |
| `validation/traversal.sh` | `strace` with syscall injection; a scratch directory that is removed while a program reads it. | The standard-error lines are sorted before they are recorded, so directory order does not show. How a program reacts to a removed directory is still the kernel/file system's: the recorded behaviour was made on ext4. |
| `s15` `/sys/class/net`, `/proc` tests (`s13_10_c_2`) | procfs/sysfs with the usual layout. | A container without them fails the test. |

## Test programs built under `target/test-oracles`

`crates/core/tests/support/oracle.rs` decides whether a test program is up to date from a stamp holding the command line
and a content hash of every input (FNV-1a over the sorted file names and contents), not from modification times. A file
system with coarse or no timestamps therefore cannot take an edited header for built. It needs `flock` only to avoid
duplicate builds; without it concurrent builders each compile and rename, which stays correct.

## Checking a second file system: `validation/tmpfs-run.sh`

`validation/tmpfs-run.sh` runs every test of `liella` and `liella-boundary` twice in unprivileged namespaces
(`unshare -r -m`, no sudo): once with a directory of the reference file system (ext4) bind-mounted over `target/tmp`
(`CARGO_TARGET_TMPDIR`, compiled into the tests, so only a mount can move it) and over `/tmp`, once with a fresh tmpfs
there, and compares the sorted pass/fail lists (exit 0 identical, 1 different, 2 the run broke). The bind mount is there
because `target/` itself may be on a RAM disk (keke's is). Result of the first run (keke, 2026-10-24, 4760 tests,
`ext2/ext3` = ext4 by magic number): identical lists, so no test of the two crates turned out to depend on the file system
under its scratch tree. Not covered: `LIELLA_TEST_TMP`-style overrides, `target/test-oracles` (stays on the file system
of `target/`), `/dev/shm`, and the `sudo -n` tests (below).

In that namespace the tests run as the caller's own uid with no capabilities and no setuid helpers, so the tests that
need real privilege fail or skip **in both runs alike**; 25 failed in each run, none because of the file system:
`appendix_a_primitives` `k_fchown`, `k_fchownat`, `k_setgid`, `k_setgroups`, `k_setregid`, `k_setresgid`, `k_setresuid`,
`k_setreuid`, `k_setuid`; `s13_filesystem_symbols` `sym_chown`, `sym_lchown`; `nscache` (three tests, `sudo`-made
regions); the secure-mode (setuid) startup tests `s01_startup_clauses` `s1_1_c_7`, `s1_1_g_4`,
`s1_3_c_2`, `s1_8_c_0`, `s03_memory_clauses` `s3_3_c_3`, `s01_startup_symbols` (two), `secure_env`, `aio_engine`
`secure_mode_selects_threads`, `mallocdbg_products`, `s13_filesystem_clauses` `s13_7_c_2`. They need `chown` to another
uid, a setuid helper or `sudo -n`, which a user namespace cannot give (inferred from the test names and the first panic lines, not checked one by one). Whether they depend on the file system is the
question the ordinary gate answers on ext4, and `fstree_products`/`posixipc_products` above cover tmpfs with `sudo`.
