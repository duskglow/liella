# getopt and getsubopt (item 2)

**Spec:** §1.6 (`getopt`, `getopt_long`, `getopt_long_only`, `optarg`, `optind`, `opterr`,
`optopt`, `__posix_getopt`), §1.6a (`getsubopt`). Public interface: `<getopt.h>`, `<unistd.h>`,
`<stdlib.h>`.

**Clauses:** `s1_6_c_0`, `s1_6_i_1`, `s1_6_i_2`, `s1_6_i_3`, `s1_6_g_1`, `s1_6_g_2`, `s1_6_g_3`,
`s1_6_g_4`, `s1_6_g_5`, `s1_6_f_1`, `s1_6_f_2`, `s1_6a_c_0`; no clause ID for `__posix_getopt`
(§1.6; stated only in a ⚑ COMPAT note).

**Probe:** `crates/core/tests/oracle/getopt_diff.c`. **Record:** `getopt.glibc.txt`.

## What the probe does

Each case parses a fresh copy of an argument vector to the end. It sets `optind = 0`,
`opterr` (1, or 0 for some cases) and `optopt = '?'` first. The header line gives the mode
(`short`, `long`, `long_only`, `posix` = `__posix_getopt`), the option string, `opterr` and the
vector. Each following line is one call: the code returned, then `optind`, `optopt`, `optarg`
(text, or `NULL`), the long index (`li`; -7 when nothing was stored) and the flag variable
(`flag`, set by the `--flag` option). The `final:` line shows the vector after parsing (its
order, as the calls left it) and everything written to standard error during the case.

Option strings: `abc:d::` and the same with a leading `:`, `+` and `-`, plus `abc:d::W;` and
`ab:`. Long options: `verbose` (none, `v`), `file` (required, `f`), `color` and `colour`
(optional, both `c`), `flag` (none, stores 42), `fill` (required, `F`), `x` (none, `X`).

Two cases run with `POSIXLY_CORRECT` set.

`== getsubopt`: for each input, every call's result as *index*`/`*value*`@`*offset of the cursor*
(`unset` when `*valuep` was not written), then the buffer's bytes after parsing (showing which
bytes became terminators). Tokens: `ro`, `rw`, `size`.

## Observations the record shows directly

* Diagnostics name `argv[0]` (`prog` in the probe).
* The record of `optopt` across cases shows its value is not reset by `optind = 0`.
