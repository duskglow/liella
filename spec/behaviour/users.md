# User, group and shadow databases, login shells, login accounting

**Spec:** §17.3 (passwd and group), §17.4 (shadow and gshadow, `lckpwdf`), §17.5a
(`getusershell`), §17.6 (utmp and wtmp). Public interfaces: `<pwd.h>`, `<grp.h>`, `<shadow.h>`,
`<gshadow.h>`, `<unistd.h>`, `<utmp.h>`, `<utmpx.h>`. Normative references (plan §1.2): POSIX
(`getpwnam`, `getgrnam`, the `_r` forms, `getutx*`), the `passwd(5)`, `group(5)`, `shadow(5)`,
`gshadow(5)`, `shells(5)` and `utmp(5)` file formats.

**Clauses:** `s17_3_c_1`, `s17_3_c_2`, `s17_3_c_3`, `s17_3_c_4`, `s17_3_c_5`, `s17_3_g_0`,
`s17_3_f_1`, `s17_4_c_0`, `s17_5a_c_0`, `s17_6_c_0`, `s17_6_c_1`, `s17_6_c_2`, `s17_6_c_3`,
`s17_6_c_4`, `s17_6_c_5`, `s17_6_c_6`, `s17_6_g_0`, `s17_3_f_2`; no clause ID for `lckpwdf`,
`ulckpwdf` (§17.4: a paragraph under no field label).

**Probe:** `crates/core/tests/oracle/users_diff.c`, run by `users_run.sh PROGRAM SCRATCH`.
**Record:** `users.glibc.txt`. The first phase runs the stream functions and utmp on files in the
scratch directory and writes a fixture `etc/` there; the second runs as root with that directory
bind-mounted over `/etc` in a private mount namespace (`unshare -m`), so the database lookups
read only the fixture and nothing on the system is read or written (the loader cache is copied in
so that the reference's dynamically linked build starts).

## What the probe does

* `fgetpwent`, `fgetgrent`, `fgetspent`, `fgetsgent` over fixture files with comments, blank
  lines, missing and extra fields, overflowing and negative numbers, hex and octal numbers, NIS
  `+`/`-` entries, leading white space, member lists with empty and spaced entries, a last line
  without a newline; `sgetspent`/`sgetsgent` on single lines.
* The `ERANGE` protocol of `fgetpwent_r`, `fgetgrent_r` and `getpwnam_r` with buffers from 1 byte
  up (and the stream position after a refused record).
* `putpwent`, `putgrent`, `putspent`, `putsgent`, including a name with a colon and NULL.
* utmp: record sizes, `utmpname`, `pututline` (replacement of a record with the same id),
  `getutent`, `getutid` by each type class, `getutline`, `getutent_r`, `updwtmp`, `getutmp`/
  `getutmpx`, the `utmpx` names.
* With the fixture `/etc`: `getpwnam`/`getpwuid` (found, missing, malformed entries), the
  enumerations of all four databases, `getgrnam`/`getgrgid`, `getspnam`, `getsgnam`,
  `getgrouplist` (with a short array), `getpw`, `getusershell`, `lckpwdf`/`ulckpwdf`.

## Observations the record shows directly

* Lines that do not parse (too few fields, a UID or GID that does not fit, negative or hex
  numbers, comments, blank lines) are skipped silently by both the stream and the database
  functions; "not found" leaves `errno` 0 for `getpwnam`/`getpwuid`/`getgrnam`, and the `_r` forms
  return 0 with a NULL result. Leading white space before a name is skipped. Fields after the
  seventh stay in the shell field (`/bin/sh:extra`), and an entry with too many empty fields
  keeps the leftover colons there (`:`).
* NIS compatibility entries (`+name`, `-name`, `+@netgroup`, `+`) are returned as ordinary
  records by the stream and enumeration functions (all numeric fields 0) but are not found by
  name.
* Group member lists drop empty entries (a trailing comma, two commas) and keep surrounding
  spaces inside names.
* Shadow aging fields that are empty are -1 (`sp_flag` all ones); a line with a non-numeric
  aging field or a tenth field is rejected.
* `ERANGE`: the `_r` forms return 34 with a NULL result while the record does not fit, and the
  stream functions leave the stream where the refused record started.
  `sysconf(_SC_GETPW_R_SIZE_MAX)` and `_SC_GETGR_R_SIZE_MAX` are 1024.
* `putpwent` writes empty numeric fields for `+`/`-` entries and refuses a field containing a
  colon or a NULL entry with `EINVAL`; `putspent` writes empty fields for -1.
* `getgrouplist` lists the base group first, then the supplementary groups in file order; with
  a short array it returns -1 and the needed count; an unknown user gets the base group alone.
  `getpw` of an unknown UID is -1 without setting `errno`.
* `getusershell` trims surrounding white space and skips comments and blank lines.
* `lckpwdf` succeeds as root and creates `/etc/.pwd.lock`; a second `ulckpwdf` is -1.
* utmp: records are 384 bytes (`ut_tv` 8 bytes: two 32-bit fields); `pututline` replaces the
  record with the same `ut_id` for the process types (`INIT`, `LOGIN`, `USER`, `DEAD`), and the
  same type for `BOOT_TIME` and friends; `getutid` with an unknown type is `EINVAL`; `getutline`
  matches only `LOGIN_PROCESS` and `USER_PROCESS` records (`ESRCH` otherwise); `updwtmp` appends
  and does not create a missing file; `utmpname("")` succeeds.
* `cuserid` (§12.12; added 2026-10-04, milestone 5, with two more fixture users and the
  effective UID switched with `seteuid` as root in the namespace): the name of the effective
  UID's entry, cut to `L_cuserid` - 1 = 8 bytes (`longusername12` gives `longuser`); in a
  caller's buffer the name is zero-filled to `L_cuserid` (9) bytes and nothing beyond is
  written; `cuserid(NULL)` returns the same library-owned string each call. A UID without an
  entry gives NULL (no buffer) or an empty string with only the first byte written (buffer),
  `errno` untouched.

## What the record does not settle

* `getlogin` (it reads `/proc/self/loginuid`, which depends on the session), `ttyslot`,
  `login`/`logout`/`logwtmp` (fixed system paths), and the lock timeouts of `lckpwdf` and utmp
  (§17.4, §17.6) are tested separately in phase 10.
