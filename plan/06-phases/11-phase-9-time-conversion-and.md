<!-- Part of the Liella implementation plan; index and reading guide: implementation-plan.md -->

### Phase 9: Time conversion and zones

* **Scope:**
  * §23.4–23.7: timers (including `SIGEV_THREAD`, D-23.3), broken-down time, time zones,
    `strftime`/`strptime`/`getdate`;
  * §23.2, clock setting and adjustment.
* **Symbols:** about 50.
* **Clauses:** about 45.
* **Needs:** P8 (`strftime` shares IP-22), P6 (timer threads).
* **Builds:** IP-23 (the zone engine), the TZif v1–v4 parser, POSIX TZ rules, and the
  `tzname`/`timezone`/`daylight` data symbols.
* **Test surface:**
  * Round trips `mktime`∘`localtime` across DST transitions in both directions, and
    normalisation of out-of-range fields.
  * `tm_isdst = -1`.
  * Years beyond `int` (`EOVERFLOW`).
  * `asctime` year limits.
  * `strftime` truncation versus empty result (D-23.8).
  * Every directive, including E/O and the GNU flags.
  * `TZ` string grammar.
  * Reloading when `TZ` or `/etc/localtime` changes.
  * Timer overrun counting.
* **Exit:** in-scope clauses and symbols green.
* **Delivered** (exit met, with the deviations listed):
  * **Method (§1.4):** each part began with a behaviour document from black-box probes of the
    reference library (`spec/behaviour/timers.md`, `zones.md`, `timefmt.md`, with their recorded
    outputs), then a fresh session implemented it from the spec, the normative documents (RFC 8536,
    POSIX's `TZ` grammar, `strftime`/`strptime`/`getdate` and `LC_TIME`) and those documents. The
    zones probe examines each `TZ` value in a fresh process, after the implementing session found
    that some reference results depend on earlier calls.
  * **Timers (D-23.2, D-23.3):** `ualarm`, the itimers, POSIX timers (kernel-notified ones are the
    kernel's), `SIGEV_THREAD` without an internal signal (timerfds, one helper thread, a new
    detached thread per notification, one at a time per timer with overruns counted), `timerfd`.
    `timer_settime`/`gettime`/`getoverrun` are async-signal-safe and in `as_safety.rs`.
  * **Broken-down time and zones (IP-23, D-23.4 to D-23.6):** the calendar in 128-bit arithmetic,
    the TZif v1–v4 parser with leap seconds, POSIX rules, zone selection and re-reading on `TZ`,
    `TZDIR` or file changes, the `tzset` globals, `mktime`'s resolution rule (the reference's,
    observed), per-thread result slots for the shared `struct tm` and `asctime` text.
  * **Formatting and parsing (D-23.7):** one `strftime` engine for bytes and wide characters with
    eras and alternative digits, `strptime`, `getdate` (legacy).
  * **Tests:** §23 has every clause (55) and symbol (70) specified. Differential tests against the
    host glibc: `timers_products` (no recorded differences), `zones_products` (45 lines by
    decision, each with its reason), `timefmt_products` (10 decisions covering 118 lines); C
    conformance programs `s23_zones.c` and `s23_timefmt.c`. `cargo xtask test`: plain,
    export+selftest, conformance 54/54; clippy clean.
  * **Budgets:** `benchmarks/progress.md` entry 7. No owner budgets exist for this phase; sanity
    bounds only (`cbench_time`). Liella is 1.3× glibc for `localtime_r` with a zone file and
    2–3.6× for `gmtime_r`/`timegm`, but 9–18× with UTC or a POSIX rule and 11× for `mktime`
    (the environment is read on every call; listed as an optimization target). One phase-7 sanity
    bound (`isalpha_c`) was raised from 10 to 12 ns: the phase-8 products measure the same 10.1 ns
    on the same day (machine variance, not a regression).
  * **Also fixed in this phase:** `s3_2_c_3` raced with harness threads allocating between two
    heap-statistics calls (it now compares a reading bracketed by two equal ones).
  * **Deviations and open items for the owner:**
    * Spec over reference, recorded in `docs/differences-from-glibc.md` (D-23.2 to D-23.7): among
      them `SIGEV_THREAD` one notification at a time per timer and `ENOTSUP` for CPU-time clocks;
      POSIX `TZ` rules in every year and out-of-range rule fields invalid; the first occurrence of a
      repeated hour; `asctime`'s fixed 26 bytes; `tzname` changing only at `tzset`; `strftime`'s
      `%z` with flags and `%C` digits; `strptime`'s POSIX E/O fallback, signs, and weekday/day of the
      year only from parsed fields; `getdate_err` 2.
    * Spec gaps found: whether `SIGEV_THREAD` notifications of one timer may run concurrently, and
      with what signal mask and attributes; how `mktime` resolves skipped and repeated hours;
      invalid `TZ` values; POSIX rules before 1970; whether `tzname` follows conversions; re-reading
      zone files other than `/etc/localtime`; `TZ` unset with `/etc/localtime` unreadable; leap-second
      zones (implemented from RFC 8536, none installed to compare); §23.5's statement that glibc's
      `asctime` rejects years outside 1000–9999 (the record shows otherwise); `getdate` with `%Z` and
      with partial times; a signed `%y`.
    * `SIGEV_THREAD` under another `ld.so` (D-22.10) is `EAGAIN`; the `timer_*@GLIBC_2.2.5`/`2.3.3`
      compat versions wait for phase 15 with the other compat-only versions.
