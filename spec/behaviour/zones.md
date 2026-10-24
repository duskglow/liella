# Broken-down time and time zones

**Spec:** §23.5 (`gmtime`, `localtime`, `mktime`, `timegm`, `timelocal`, `difftime`, `asctime`,
`ctime`, `dysize` and the `_r` forms) and §23.6 (`tzset`, `tzname`, `timezone`, `daylight` and the
`__` names). Public interfaces: `<time.h>`. Normative references for the formats (plan §1.2): RFC
8536 (the TZif format, versions 1 to 4) and POSIX.1-2017 §8.3 (the `TZ` variable's rule grammar).

**Clauses:** `s23_5_c_0`, `s23_5_c_1`, `s23_5_c_2`, `s23_5_c_3`, `s23_5_c_4`, `s23_5_c_5`,
`s23_5_c_6`, `s23_5_g_0`, `s23_5_f_1`, `s23_5_f_2`, `s23_5_f_3`, `s23_6_c_0`, `s23_6_c_1`,
`s23_6_c_2`, `s23_6_c_3`, `s23_6_g_0`.

**Probe:** `crates/core/tests/oracle/zones_diff.c`. **Record:** `zones.glibc.txt`. The zone files
are the system's (`/usr/share/zoneinfo`, Ubuntu's tzdata); `/etc/localtime` on the reference
system names `Etc/UTC`.

## What the probe does

Each `TZ` value (and the implicit-`tzset` section) is examined in a fresh child process, so that
no line depends on the zones examined before it; the zone section does not use `strftime`.

* `gmtime_r` at the edges: the epoch, `±2^31`, years 1, 0, 9999, 10000, the largest and smallest
  `time_t` whose year fits an `int`, and the extremes of `time_t`.
* `timegm` with out-of-range fields (day 0 and 32, month 13 and −1, hour −1, second 60 and −1,
  a million minutes, `INT_MAX`/`INT_MIN` seconds, `tm_isdst` 1, `INT_MAX`/`INT_MIN` years), showing
  the result and the normalised `struct tm`.
* `asctime_r` for years −1900 to 2147481747 and for out-of-range fields; `dysize`; `difftime`.
* For each `TZ` value (unset, empty, names, `:name`, absolute paths with and without `:`, POSIX
  rule strings in every form the grammar allows, quoted names, extreme offsets, invalid names and
  rules, a path that escapes the zone directory) and two `TZDIR` values:
  * `tzname`, `timezone` and `daylight` after `tzset`;
  * every change of offset, DST flag or abbreviation during 2024 (found every 15 minutes, then to
    the second), shown as the local time at the first second of the new period;
  * the local time of 12:00 UTC on 15 January and 15 July in years from 1850 to 2500;
  * `mktime` of wall times just before, inside and after each transition (gap or overlap), with
    `tm_isdst` −1, 0 and 1, as seconds relative to the transition, with the normalised result;
  * `mktime` normalisation, and overflow in `mktime` and `localtime_r`.
* Implicit `tzset`: which functions see a `TZ` changed with `setenv` without a `tzset` call, and
  whether `gmtime`/`localtime` and `ctime`/`asctime` share their result storage.

## Observations the record shows directly

* `gmtime_r` succeeds for every `time_t` whose year fits an `int` (`tm_year` is year − 1900, so
  years up to 2147485547 fit) and fails with `EOVERFLOW` beyond; `tm_zone` is `GMT`.
* `timegm` normalises every field, ignores `tm_isdst` (sets 0), and fails with `EOVERFLOW` only
  when the normalised year does not fit; on failure the `struct tm` is left as given.
* `asctime_r` fails with `EOVERFLOW` for years below 0 and above 9999, and formats years 0 to 999
  without padding (`... 01:03:52 0`). §23.5 says glibc returns `NULL` outside 1000–9999; the
  record shows otherwise (a spec inaccuracy about the reference, not a contract). Out-of-range day
  and hour fields are printed as numbers wider than the fixed layout.
* `gmtime` and `localtime` return the same static object; so do `ctime` and `asctime`.
* `localtime` and `mktime` see a changed `TZ` without a `tzset` call; `localtime_r` does not (it
  uses the zone last loaded). §23.6 lists `localtime`, `mktime` and `strftime`.
* `TZ` unset uses `/etc/localtime`. An empty `TZ` gives UTC with the abbreviation `Universal`.
  `:name` and `name` are equivalent for files; an absolute path works with or without `:`.
  `TZDIR` replaces the zone directory.
* A name with no file and no valid rule (`Invalid//Zone`, `Nonexistent/Zone`, `Europe/Berlin`
  under a missing `TZDIR`) gives UTC with the leading alphabetic part as the abbreviation and
  `daylight` 0. A path that leaves the zone directory gives UTC with empty abbreviations.
* POSIX rules: `Mm.w.d`, `Jn` (no February 29) and `n` (counting it) forms; transition times may
  be negative or above 24 hours (to ±167 hours); a DST offset defaults to one hour less than the
  standard one; `EST5EDT` (also the name of a zone file) shows the U.S. dates; a rule with only
  a start date ends on the U.S. end date. Offsets of 25 hours (`AAA25`, `AAA-25`) and 24:59:59 are
  accepted. Names shorter than three characters are rejected (`tzname` empty, UTC).
* Out-of-range rule fields (month 13, week 6, `J0`, day 366) are accepted and give transitions
  at odd times rather than an error.
* `tzname` keeps values from earlier zones in some cases: after `Asia/Tokyo` as a path without
  `:`, `tzname[1]` is `JDT` (a historical abbreviation of that file) but `JST` with `:`; after
  `Asia/Kolkata`, `tzname[1]` is `+0630` with `daylight` 1, from the file's history.
* `mktime` in a gap with `tm_isdst` −1 and in an overlap with each flag: see the record.

## What the record does not settle

* Re-reading `/etc/localtime` when it changes (§23.6) cannot be probed without changing the
  system's configuration.
* Several of the observations above (`tzname` carrying earlier values, odd transitions for
  invalid rule fields, the abbreviation taken from an invalid name) look like historical
  artifacts rather than contract; the spec does not decide them.
