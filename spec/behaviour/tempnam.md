# tmpnam, tmpnam_r, tempnam (item 5)

**Spec:** §12.12 (`tmpnam`, `tmpnam_r`, `tempnam`, `P_tmpdir`, `L_tmpnam`, `TMP_MAX`).
Public interface: `<stdio.h>`.

**Clauses:** `s12_12_c_2`.

**Probe:** `crates/core/tests/oracle/stdio_diff.c`, function `names`. **Record:**
`stdio.glibc.txt`, section `== names`.

## What the probe does

Each generated name is shown with the probe's scratch directory as `<dir>` and its last six
characters as `######`, followed by `len` (the length after `<dir>`, or of the whole name), whether
those six characters are all ASCII letters or digits (`letters`), and whether the name exists
(`exists`, via `lstat`).

Cases: `L_tmpnam`, `TMP_MAX`; `tmpnam` into a caller buffer; `tmpnam(NULL)` (and whether two calls
return the same buffer); `tmpnam_r(NULL)`; `tmpnam_r` into a buffer; `tempnam` with no directory
and no prefix, the prefix `abcdefgh`, an empty prefix, the directory `<dir>` with prefix `p`,
`<dir>///` with prefix `q`, the missing directory `/nonexistent/dir`, the directory `/`; then with
`TMPDIR=<dir>` (directory argument `/var/tmp`), with `TMPDIR=/nonexistent`, and `tmpnam` with
`TMPDIR` unset.

The probe runs as an ordinary user (not in secure mode).
