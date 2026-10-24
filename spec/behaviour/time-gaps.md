# Time spec gaps settled by probe (TIME-SPEC-GAPS-SETTLE)

**Spec:** §23.5 (`mktime`), §23.6 (`TZ`, `tzset`, the globals) and §23.7 (`strptime`, `getdate`):
the gaps phase 9 listed with no probe behind them (plan `06-phases/11-phase-9-time-conversion-and.md`).

**Clauses:** `s23_5_c_2`, `s23_6_c_0`, `s23_6_c_1`, `s23_7_c_2`, `s23_7_c_3`.

**Probe and test:** `crates/core/tests/time_gaps_probe.rs` (the C probe is the `PROBE` text in it,
written to `target/test-oracles/time_gaps_probe.c`), built against the host C library (glibc 2.39,
Ubuntu 24.04, x86_64) and statically against Liella's products, run with an empty environment.
Every zone is a POSIX rule string; `TZDIR` names an empty directory of the test's own, so no zone
file of the host is read. The `/etc/localtime` cases run in a private user and mount namespace
(`unshare -r -m`) with a tmpfs over `/etc`. Recorded on keke, 2026-10-24.

## Settled

| Gap | Reference (glibc) | Liella | Status |
|---|---|---|---|
| `mktime` in a skipped hour (`EST5EDT,M3.2.0,M11.1.0`, `AEST-10AEDT,M10.1.0,M4.1.0/3`, a 30-minute change `<+1030>-10:30<+11>-11,M10.1.0,M4.1.0`), `tm_isdst` -1, 0, 1 | The wall time is read with the offset `tm_isdst` names (-1: the offset before the change) and normalised past the gap. | Same, line for line. | Equal |
| `mktime` in a repeated hour, `tm_isdst` 0 and 1 | The occurrence with that offset. | Same. | Equal |
| `mktime` in a repeated hour, `tm_isdst` -1 | Follows the previous `mktime` call: DST after a summer or DST call, standard time after a winter one. | The first occurrence (DST), always. | Kept: D-23.5, differences row (gain: a result that does not depend on earlier calls) |
| POSIX `TZ` rule before 1970 (years 1600-1969, rules in `M`, `J` and day-number form, negative and over-24-hour change times, a rule-less `EST5EDT`) | Standard time all year: no change applies before 1970. | The rule applies in every year (POSIX gives the rule no start year). | Kept: D-23.6, differences row |
| `TZ` unset, `/etc/localtime` missing, empty, not a zone file, a directory, unreadable (`EACCES`, all capabilities dropped) or a dangling link | UTC: `tzname` `UTC`/`UTC`, `timezone` 0, `daylight` 0, offset 0. | Same. | Equal |
| `getdate` with `%Z` (`UTC`, `GMT`, `EST`, `EDT`, an unknown name, `+0100`; alone, after a time, after a date and time; `TZ` UTC and `EST5EDT`) | The zone name is read and ignored: the result is the local time of the given fields in `TZ`'s zone. | Same. | Equal |
| `getdate` with partial times (`%H:%M`, `%H`, `%M`, `%S`, `%a`, `%A`, `%a %H:%M`, `%Y`, `%d`, `%b %d`, `%y`, `%Y %H:%M`, dates with a weekday) | POSIX's rules for missing fields (today or tomorrow for a time, the next such weekday, the current time of day for a date). | Same. | Equal |
| `getdate` with the month but no day (`%Y-%m`) | Another day of the month (the 5th for `2030-06` on 2026-10-24). | The first of the month (POSIX: "the first day of month is assumed if no day is given"). | Kept: new differences row (gain: correctness, and no dependence on the date of the call) |
| `strptime` `%y` with a sign (`+5`, `+05`, `-0`, `+99`; `%C%y` `20+5`) | Rejected (NULL), any sign. | Accepted (POSIX allows a leading `+` or `-`); a negative value is rejected as out of range (`-5`, `-99`). | Kept: D-23.7, differences row |

The test fails when one of the kept differences goes away (the record would be stale) or when
Liella's line no longer matches what the decision says, so a change on either side shows.
The `getdate` `%Y-%m` difference is not required to show (the reference's day may be the first).
Times relative to the call (`getdate` partial times) are printed as day offsets from the current
date and `cur` for fields equal to the current time, so the two runs compare.
