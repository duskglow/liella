# The libc utilities (spec §31)

Black-box record of the reference system's utilities, made for UTIL-SPEC (UTIL-0 of `plan/utilities.md`) under the
same rules as the other behaviour documents: the probe runs the installed programs and records only what they print,
the exit status and the files they write. No program's own text is read (`ldd`, `tzselect` and the `ldconfig`
wrapper are scripts; they are run, never opened), and no libc or tzcode source is consulted.

* Probe: `tools/probe/util_spec.sh`, run as an ordinary user: `sh tools/probe/util_spec.sh > utilities.glibc.txt`.
  Each case runs with an empty environment (`PATH`, `HOME` and a scratch directory only), in the `POSIX` locale.
* Record: [utilities.glibc.txt](utilities.glibc.txt). Its first line names the packages: Ubuntu 24.04.5,
  `libc-bin`, `libc-dev-bin` and `locales` 2.39-0ubuntu8.9, `tzdata` 2026e, `dpkg` 1.22.6. Each case is `== <case>`,
  the command (`  $ `), standard output (`  | `), standard error (`  ! `) and `  rc=<status of the command line>`.
  Masks: the scratch directory is `<dir>`, addresses `0xADDR`, process ids `<pid>`, tzselect's clock lines `<now>`.
* The clauses cite the cases as `probe <case>`; this table maps them.

| Cases | Clauses |
|---|---|
| `paths`, `ldso-link`, `dpkg-needs-ldconfig`, `*-help`, `*-version` | `s31_1_c_r1` to `s31_1_c_r6`, `s31_1_c_1`, `s31_1_c_2`, `s31_1_g_1`, `s31_1_g_2`, `s31_1_f_1` |
| `ldconfig-*`, `ld.so.conf` | `s31_2_i_1` to `s31_2_i_4`, `s31_2_g_1` to `s31_2_g_6`, `s31_2_f_1` to `s31_2_f_3` |
| `ldd-*` | `s31_3_i_1`, `s31_3_g_1` to `s31_3_g_4`, `s31_3_f_1` to `s31_3_f_3` |
| `getent-*` | `s31_4_i_1`, `s31_4_i_2`, `s31_4_g_1`, `s31_4_g_2`, `s31_4_f_1` to `s31_4_f_3` |
| `getconf-*` | `s31_5_i_1`, `s31_5_i_2`, `s31_5_g_1` to `s31_5_g_3`, `s31_5_f_1`, `s31_5_f_2` |
| `iconv-*` | `s31_6_i_1`, `s31_6_g_1` to `s31_6_g_3`, `s31_6_f_1` to `s31_6_f_3` |
| `locale-*` | `s31_7_i_1`, `s31_7_g_1` to `s31_7_g_4`, `s31_7_f_1`, `s31_7_f_2` |
| `localedef-*` | `s31_8_i_1`, `s31_8_i_2`, `s31_8_g_1`, `s31_8_g_2`, `s31_8_f_1` |
| `zic-*` | `s31_9_i_1`, `s31_9_i_2`, `s31_9_g_1`, `s31_9_g_2`, `s31_9_f_1` |
| `zdump-*` | `s31_10_i_1`, `s31_10_g_1` to `s31_10_g_4` |
| `tzselect-*` | `s31_11_i_1`, `s31_11_g_1`, `s31_11_f_1` |
| `gencat-*` | `s31_12_i_1`, `s31_12_g_1`, `s31_12_f_1` |
| `pldd-*` | `s31_13_i_1`, `s31_13_f_1` to `s31_13_f_3` |
| `iconvconfig-help`, `devtools` | `s31_14_c_1` |

## What the probe settled against the design

* A static program that is not position independent gives `\tnot a dynamic executable` on standard error and exit 1;
  only a static-pie program gives `\tstatically linked` and exit 0 (`ldd-static`, `ldd-static-pie`). The design note
  (`plan/utilities.md` §2.3) had both as `statically linked`; §31.3 follows the record.
* `ldconfig -r ROOT` by an ordinary user succeeds on the reference (`ldconfig-r-user`), so running `-r` without
  privilege is not a difference; `openat2` is only Liella's mechanism (pending decision UTIL-SPEC-9).
* `iconv` with `//IGNORE` still reports the illegal sequence and exits 1 after writing the rest (`iconv-ignore`).

## Gaps (left to the stories)

* pldd's output on success: the probe host does not let a process trace its sibling (`pldd-self`); UTIL-15 probes it
  with a target it may trace.
* getconf's padding for names longer than the column, and its full name set (UTIL-5); the `ldconfig -p` label of
  hwcaps entries (UTIL-3); what `locale -a -v` prints for the built-in `C.utf8` (UTIL-8); `localedef`'s exit status 1
  when output was written with warnings (UTIL-12).
* Messages in other locales (`LC_MESSAGES`) were not recorded.
