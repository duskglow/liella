# Without /proc

**Spec:** the functions of §1.5, §11.2, §13.7, §17.1, §17.5, §18.5, §19.3, §22.2 and §24.4
whose Liella implementation opens a path below `/proc` or `/sys`. Plan decision D-X.12 (OD-5:
Linux 5.10 or later with `/proc` mounted is the supported environment, but no function may
crash or give a wrong answer without it). Official documentation consulted: proc(5),
getauxval(3), get_nprocs(3), sysconf(3), pathconf(3), pthread_getattr_np(3),
pthread_setname_np(3), pidfd_getpid(3), ttyname(3), getlogin(3), and the kernel's
Documentation/admin-guide/devices.txt (terminal device numbers).

**Clauses:** `s1_5_c_0`, `s1_5_g_0`, `s11_2_c_0`, `s13_7_c_1`, `s17_1_c_0`, `s17_5_c_1`,
`s17_5_c_2`, `s17_5_f_0`, `s18_5_c_4`, `s19_3_c_0`, `s19_3_f_1`, `s19_3_f_2`, `s22_2_c_4`,
`s24_4_c_1_2`, `s24_4_c_1_3`, `s24_4_c_2`, `s24_4_c_4`; no clause ID for
`pthread_setname_np`/`pthread_getname_np` (§22).

**Probe:** `crates/core/tests/oracle/noproc_diff.c`. **Record:** `noproc.glibc.txt` (glibc
2.39-0ubuntu8.9, dynamically linked, kernel 6.8.0-146, on Zen 1; the working directory on ext4).
The differential test `crates/core/tests/noproc_products.rs` runs the same probe against
Liella's products.

## What the probe does

It runs twice: as is, and in a private user and mount namespace (`unshare -r -m --propagation
private`) after an empty tmpfs is mounted over `/proc` and over `/sys`, so that every path below
them is missing (`ENOENT`). The first line says which. Values that differ between machines are
printed as relations (processor counts against the affinity mask's count, the stack size against
`RLIMIT_STACK`); names as present or not. On a pseudo-terminal it asks for the terminal's name,
and with that terminal as standard input for the login name and slot (the terminal has no login
record, and the session's audit login UID is set).

## Observations

With `/proc` and `/sys`, every call succeeds. Without them, the reference:

* counts processors from the affinity mask (`get_nprocs`, `_SC_NPROCESSORS_ONLN`), and
  `get_nprocs_conf` is at least `get_nprocs`;
* answers `_SC_NGROUPS_MAX` 65536, the page size, the clock tick, the physical pages, the
  auxiliary vector and the vDSO clocks as before;
* answers `pathconf(".", _PC_LINK_MAX)` 32000 on ext4 (65000 with `/proc`), leaving `ENOENT` in
  `errno`;
* fails `pthread_getattr_np` of the initial thread with `ENOENT`;
* fails `pthread_setname_np` and `pthread_getname_np` of another thread with `ENOENT`; the
  calling thread's own name still works;
* fails `pidfd_getpid` with `EBADF` for a valid pidfd, as for a closed descriptor and one that
  is not a pidfd;
* still names the terminal (`ttyname`, `ttyname_r`, also the master as `/dev/ptmx`), and
  `ttyname` of `/dev/null` is `ENOTTY`;
* `getlogin` and `getlogin_r` fail with `ENOENT` (the audit login UID cannot be read and the
  terminal has no login record); `ttyslot` is 0;
* `group_member`, `closefrom`, `lchmod` (through `fchmodat2` on this kernel) and a user lookup
  work as before.
