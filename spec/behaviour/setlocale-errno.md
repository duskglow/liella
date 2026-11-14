# `setlocale`: `errno` on failure and on success (spec §7.2, clause `s7_2_f_0`)

Black-box probe of the reference (glibc 2.39, x86-64, installed locales `C`, `C.utf8`, `en_US.utf8`,
`POSIX`), run by SMOKE-ERRNO-FIXES on 2026-11-13 with `LANG`, `LC_ALL` and `LANGUAGE` unset. The probe
sets `errno` to 0 (first pass) and then to a marker, 1234 (second pass, the same calls again in the same
process), calls `setlocale`, and prints the result and `errno`. Found by AQOURS-SMOKE (F3): bash prints
`strerror(errno)` in its warning when `setlocale` fails.

| Call | Reference, first pass (errno 0 before) | Reference, second pass (1234 before) | Liella |
| --- | --- | --- | --- |
| `LC_ALL`, unknown name `zh_HK.big5hkscs` | NULL, `ENOENT` | NULL, 1234 (unchanged) | NULL, `ENOENT` |
| `LC_CTYPE`, `xx_YY`; `LC_MESSAGES`, `xx_YY.UTF-8` | NULL, `ENOENT` | NULL, 1234 | NULL, `ENOENT` |
| `LC_ALL`, `../x` | NULL, `EINVAL` | NULL, `EINVAL` | NULL, `ENOENT` |
| `LC_ALL`, `/abs` | NULL, `ENOENT` | NULL, 1234 | NULL, `ENOENT` |
| `LC_ALL`, a 399-byte name | NULL, `EINVAL` | NULL, `EINVAL` | NULL, `ENOENT` |
| `LC_ALL`, `LC_CTYPE=xx_YY;LC_NUMERIC=C` (not every category named) | NULL, `EINVAL` | NULL, `EINVAL` | NULL, `ENOENT` |
| category 99 or -1, name `C` or NULL | NULL, `EINVAL` | NULL, `EINVAL` | NULL, `EINVAL` |
| `LC_ALL`, `""` with `LANG=xx_YY.bogus` | NULL, `ENOENT` | NULL, 1234 | NULL, `ENOENT` |
| query; `C`; `POSIX`; `en_US.UTF-8`; `""` with nothing set | ok, `errno` unchanged | ok, unchanged | ok, unchanged |
| `LC_ALL`, `C.UTF-8` (first time in the process) | ok, `errno` **2** | ok, unchanged | ok, unchanged |

Reading of the rows for Liella (decision record `records/pending/SMOKE-ERRNO-FIXES-2.toml`, ruling #5469):

* A `setlocale` that fails for a name sets `ENOENT`, on every attempt: the locale data of that name is not
  available, as Liella's `newlocale` reports it (D-7.18). The reference leaves `errno` unchanged when a name
  that already failed in the process fails again, and gives `EINVAL` for some names it refuses before any
  lookup; Liella does not distinguish these (POSIX defines no `setlocale` errors).
* An unknown category is `EINVAL`, as the reference.
* A successful call leaves `errno` alone. The reference's `ENOENT` after the first successful
  `setlocale(LC_ALL, "C.UTF-8")` is a change on success, recorded here as a row and not copied.
