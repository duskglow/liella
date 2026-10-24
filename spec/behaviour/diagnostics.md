# err, warn, error, error_at_line, strerror_r (items 6 and 7)

**Spec:** §2.2 (`strerror`, `strerror_r` in both forms, `__xpg_strerror_r`, `strerrorname_np`,
`strerrordesc_np`, `perror`), §2.3 (`err`, `errx`, `verr`, `verrx`, `warn`, `warnx`, `vwarn`,
`vwarnx`, `error`, `error_at_line`, `error_message_count`, `error_one_per_line`,
`error_print_progname`). Public interfaces: `<string.h>`, `<err.h>`, `<error.h>`.

**Clauses:** `s2_2_c_0`, `s2_2_c_1`, `s2_2_c_3`, `s2_2_c_3_1`, `s2_2_c_3_2`, `s2_2_c_4`, `s2_2_c_5`,
`s2_2_c_6`, `s2_2_f_0`, `s2_3_c_0`, `s2_3_c_1`, `s2_3_c_2`, `s2_3_c_3`, `s2_3_c_4`.

**Probe:** `crates/core/tests/oracle/diag_diff.c`. **Record:** `diag.glibc.txt`.

The probe sets `program_invocation_name` to `/usr/bin/prog` and
`program_invocation_short_name` to `prog` first.

## Sections of the record

* `== strerror`: for -2 to 140, `strerror`, `strerrorname_np` and `strerrordesc_np` (`NULL` for
  none).
* `== strerror_r`: for the numbers 0, 2, 22, 133, 134, -1, 1000 and buffer sizes 0, 1, 5, 12,
  64: the GNU form (`gnu`: whether the result points into the buffer, the result, the buffer's
  first 16 bytes after the call; it was filled with `Z` first), and the XSI form (`xpg`: the
  return value, `errno` afterwards, the buffer's first 16 bytes).
* `== perror`: `perror` for four `errno` values and the strings NULL, empty, `ctx` and
  `two words`, with `errno` after the call and what reached standard error.
* `== families`: each function runs in a child process; a line shows the child's exit status
  (77 when the function returned) and everything it wrote to standard error. Lines printed by
  a child to standard output come before its line.
  * `warn`: with `errno` `ENOENT` and a format; with `errno` `EACCES` and a NULL format.
  * `warnx`: a format; NULL; an empty message.
  * `err` (status 3, `EPERM`); `err_null` (status 4, NULL format, `errno` 9999); `errx`
    (status 5).
  * `error0`: three `error` calls with status 0 (no error number, `ENOENT`, 9999), then
    `error_message_count`.
  * `error_exit`: text pending on `stdout`, then `error(6, EINTR, ...)`.
  * `at_line`: `error_at_line` with files `a.c` and NULL; then `error_one_per_line = 1` and
    repeated and changed locations (the same file name by another pointer included), then
    `error_message_count`.
  * `hook`: `error_print_progname` set to a function writing `HOOK|`; `error`,
    `error_at_line` and `warnx`.
  * `orient`: the orientation of `stderr` (`fwide(stderr, 0)`) before and after `perror` and
    `warnx`. Liella keeps `stderr` unoriented after `perror`, as §2.2 says; this line is a
    recorded difference.
  * `wide`: `stderr` made wide-oriented, then `perror`, `warnx`, `error`.
