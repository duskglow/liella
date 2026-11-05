# TIME-GAPS-PROBE-CLOCK: time_gaps_probe independent of when it runs

## Mechanism of the 11-05 07:03 failure (`glibc sec=cur` against `liella sec=51`)

How the old probe decided `cur`: `gd()` read `now = time(NULL)`, took `c = localtime(now)`, called
`getdate`, and repeated only when a second `time(NULL)` differed from `now`. A field printed `cur`
when it equalled `c`'s field.

The two programs read different clocks:
* Liella's `time()` is the vDSO `time` entry (`crates/boundary/src/sys.rs`, `fn time`): the kernel's
  tick-updated seconds, the same source as `CLOCK_REALTIME_COARSE`.
* Liella's `getdate` reads `clock_gettime(CLOCK_REALTIME)` (`crates/boundary/src/timefmt.rs`, `fn now`),
  the precise clock.
* The tick-updated seconds trail `CLOCK_REALTIME` by up to one tick. For the first tick after a
  second boundary, `CLOCK_REALTIME` already reads second N+1 while `time()` still reads N.

When a getdate call fell in that window, both of the probe's `time()` reads said 50, so the retry did
not fire. Liella's getdate had read 51, so it printed `sec=51`. The reference's result matched its own
`time()` reads (seen from outside the reference: it printed `cur`). The midnight run is about 0.26 s
long and about 30 calls fall inside one tick, so all 7 lines came from a single second edge. Neither
getdate is wrong: POSIX does not say which clock is the "current time". The probe assumed that
`time()` and the call read the same clock.

The second-0 wait (GETDATE-SECONDS-DEFAULT) and the minute-edge wait (PERF-NFTW-ENTRY-5) handled
two places where a boundary fell between the probe's read and the call. They did not handle this
cause, which can happen at any second.

Reproduced with a diagnostic (`edge.c`, scratch only, not committed). It spins until
`CLOCK_REALTIME` enters a new second, then does `time()`, `getdate("2030-06-15")`, `time()`. The
result is under "Probe-settled" below.

## Redesign (in force)

* **Bracket per call, per program.** The probe takes `lo()` before each call and `hi()` after it.
  `lo()` is the minimum of `time()`, `CLOCK_REALTIME_COARSE` and `CLOCK_REALTIME`; `hi()` is the
  maximum. Whatever clock an implementation reads, its second lies in `[lo, hi]`.
* **Raw output.** The probe prints the result's raw fields and the local time of every second in its
  bracket.
* **Classification in the test.** The test reads each result against every second of that program's
  own bracket (days, hour/min/sec `cur`, weekday). Two programs agree when some reading is the same.
  A boundary between the calls, or inside one call, no longer decides the classification.
* **Clock re-pinned before every call** (`pin()`), from `CLOCK_REALTIME`.
  * getdates: the clock is pinned at 12:30:30 rather than 12:30:00. Second 0 is therefore never the
    current second, so a result that zeroes the seconds still differs from one that keeps them.
  * midnight: the clock is pinned at 23:59:50 or 00:00:10. The 10 s margin now bounds one call, not
    the whole run.
* **Waits removed.** The second-0 wait and the `time()`-changed retry are gone. The minute-edge wait
  was never on `origin/next`, so there was nothing to remove.
* **Forced-edge test** `time_gaps_hold_across_forced_clock_edges`. Each boundary (second, minute,
  hour, day) is placed inside the call's second (edge 1), between `lo()` and the call (2), or between
  the call and `hi()` (3). The test runs both programs at each placement and compares every pairing
  of two different placements. A placement that is not achieved within 20 tries prints
  `edge not arranged`, which fails the test.
