# Formatting and parsing time

**Spec:** §23.7 (`strftime`, `strftime_l`, `wcsftime`, `wcsftime_l` and the `__` names,
`strptime`, `strptime_l`, `getdate`, `getdate_r`, `getdate_err`). Public interfaces: `<time.h>`,
`<wchar.h>`. Normative references (plan §1.2): ISO C and POSIX.1-2017 (`strftime`, `strptime`,
`getdate`), and POSIX `LC_TIME` (`era`, `alt_digits`, `t_fmt_ampm` and the other keywords) for
the locale data the directives read.

**Clauses:** `s23_7_c_1`, `s23_7_c_1_1`, `s23_7_c_1_2`, `s23_7_c_1_3`, `s23_7_c_1_4`,
`s23_7_c_2`, `s23_7_c_3`.

**Probe:** `crates/core/tests/oracle/timefmt_diff.c` (argument: a scratch directory).
**Record:** `timefmt.glibc.txt`. Run with `TZ=UTC0` and `LOCPATH` holding `en_US.UTF-8`,
`de_DE.UTF-8`, `ja_JP.UTF-8` (an era and kanji alternative digits), `th_TH.UTF-8` (the Buddhist
era) and `fa_IR.UTF-8` (Persian digits).

## What the probe does

* `strftime` of every directive letter (valid and invalid), `%%`, a lone `%`, every `E` and `O`
  combination (valid and invalid), and the GNU flags `_ - 0 ^ #` with widths, alone and combined,
  for eight `struct tm` values: ordinary dates, ISO-week edges (2021-01-03 is week 53 of 2020,
  2024-12-30 week 1 of 2025), a leap second, years 0, −1 and 12345, a NULL `tm_zone`, offsets
  of +05:30, −03:30 and +14:00. Each line is `tm-index directive [result] length`, with
  non-printing and non-ASCII bytes as `\xHH`. In C and `en_US.UTF-8` every directive; in the
  other locales the locale-dependent ones.
* Sizes: a result that exactly fits, one byte short, `max` 0 and 1, an empty format, an empty
  `%p`; `buf0` shows the buffer's first byte after the call. `%s` under `TZ=UTC0`.
* `wcsftime`, a subset, in each locale, and a short buffer.
* `strptime` of about 90 (format, input) pairs in C and `en_US.UTF-8`, and in every locale the
  round trip of what `strftime` printed. The `struct tm` starts with every field −77 and
  `tm_zone` `"sentinel"`; a line lists the fields that changed and the offset of the returned
  pointer (`+N`), or `NULL`.
* `getdate`/`getdate_r` with `DATEMSK` unset, missing, a directory, a template file (fully
  specified dates and times, so the result does not depend on the clock), invalid dates,
  leading and trailing spaces, and an empty template file.

## Observations the record shows directly

* An invalid directive (`%E`, `%i`, `%Q`, `%Ea`, `%OY`, `%+Y`, `%:z`, a lone `%`) is copied to the
  output unchanged. `%Ez`, `%Oz`, `%Ob` and `%OB` are accepted (`%Ob`/`%OB` give the alternative
  month names where the locale has them).
* Flags: `_` pads with spaces, `0` with zeros, `-` not at all; `^` upper-cases; `#` swaps the
  case of names (upper for `%a %A %b`, lower for `%Z %p`). A width pads to that many characters
  (`%10Y` `0000002024`, `%-10Y` and `%_10Y` space-padded). An absurd width (`%9999999999d`)
  gives an empty result (0).
* `%z` with a width or flag gives results such as `         +0000000000` (`%10z`) and `+   0`
  (`%_z`), padding the number after the sign; these look like historical artifacts.
* `strftime` returns 0 when the result plus its terminator does not fit (`max` one short), and
  the buffer then holds a partial result (`buf0`); an empty result also returns 0.
* `%s` is computed in the current zone; `%Z` with a NULL `tm_zone` prints the current zone's
  abbreviation (`UTC` here).
* Eras: `ja_JP` prints the era name and year (`令和06年`, and `紀元前` for year −1), `th_TH` the
  Buddhist era (2567 for 2024). Alternative digits: `ja_JP` `%Od` in kanji, `fa_IR` Persian digits
  for `%Od %Oy` and in `%x`. `fa_IR`'s `%EY` for year −1 is `-1`.
* `strptime`: fields are bounded (`%d` 1–31, `%m` 1–12, `%H` 0–23, `%S` 0–61, `%j` 1–366); a
  number reads at most as many digits as the field allows (`%Y` on `123456` reads `1234`; `%M` on
  `60` reads `6`); `%y` 69–99 is 19xx and 00–68 20xx; `%C` and `%y` combine in either order; names
  match case-insensitively, in full or abbreviated, and only as much input as matched is
  consumed; white space in the format matches any amount, including none; `%n` and `%t` behave as
  white space; `%z` accepts `±hh`, `±hhmm`, `±hh:mm` and `Z` (`+2400` too), and rejects minutes
  ≥ 60; `%Z` consumes a word and sets nothing; `%s` sets every broken-down field in the current
  zone, and rejects a negative or overflowing number.
* `strptime` sets `tm_wday` and `tm_yday` when it can compute them, but it also computes them from
  fields it did not parse: with the probe's −77 sentinels, `"september"` under `%b` reports
  `wday=0 yday=165`. This looks like a historical artifact (the computation reads fields the
  caller never set).
* `getdate`: `getdate_err` 1 for `DATEMSK` unset, 3 for a missing file (not 2), 4 for a directory,
  7 for no matching template (also for an empty file and an empty input), 8 for an invalid date
  (February 30, April 31); leading and trailing spaces are accepted; `getdate_r` returns the same
  codes.

## What the record does not settle

* Templates that leave the date or time unspecified fill them from the current time (§23.7 and
  POSIX describe the rules); the probe avoids them so that the record does not depend on the
  clock.
* Error codes 2, 5 and 6 (`DATEMSK` unreadable, a read error, `malloc` failure) are not
  produced.
