# Timers

**Spec:** §23.4 (`alarm`, `ualarm`, `setitimer`, `getitimer`, `timer_create`, `timer_delete`,
`timer_settime`, `timer_gettime`, `timer_getoverrun`, `timerfd_create`, `timerfd_settime`,
`timerfd_gettime`). Public interfaces: `<unistd.h>`, `<sys/time.h>`, `<time.h>`, `<signal.h>`,
`<sys/timerfd.h>`. Normative references for the mechanism (plan §1.2): POSIX.1-2017 and the Linux
system call interface (`setitimer(2)`, `timer_create(2)`, `timerfd_create(2)`).

**Clauses:** `s23_4_c_1`, `s23_4_c_2`, `s23_4_c_3`, `s23_4_c_3_1`, `s23_4_c_3_2`, `s23_4_c_3_3`,
`s23_4_c_3_4`, `s23_4_c_4`, `s23_4_f_0`.

**Probe:** `crates/core/tests/oracle/timers_diff.c`. **Record:** `timers.glibc.txt`.

## What the probe does

Each line is a call's result and `errno`, or a fact about a delivery. Remaining times are shown
only against coarse thresholds (or rounded to whole seconds) so that the record does not depend
on the machine's load. Signals are blocked and collected with `sigtimedwait`.

* `alarm`, `ualarm`: the value returned for the previous alarm after various delays, the
  largest `alarm`, `ualarm` arguments of one second or more, and the interval `ualarm` sets.
* `setitimer`/`getitimer`: the shared `ITIMER_REAL` state with `alarm`, the old value, invalid
  `which` and invalid microsecond fields, and the signal an expiry delivers.
* POSIX timers: a NULL `sigevent`; `SIGEV_SIGNAL` with a value; a periodic timer whose signal
  stays blocked (overruns); the old value; invalid nanosecond fields and flags; an absolute time
  in the past; `SIGEV_NONE`; `SIGEV_THREAD_ID` to the caller and to a foreign thread id; invalid
  clocks, notification kinds and signal numbers; the clocks a timer accepts; invalid timer ids;
  200 timers at once; `SIGEV_THREAD` (in which thread the function runs, its detach state and
  signal mask, the value it receives, no calls after `timer_delete`, two timers at once).
* `timerfd`: invalid flags and clocks, reads before and after expiry, a short read buffer,
  periodic counts, the old value, invalid flags and fields, a descriptor that is not a timer.

## Observations the record shows directly

* `alarm` returns the previous alarm's remaining time rounded to the nearest second (0.7 s
  gives 1, 1.6 s gives 2), and accepts `UINT_MAX`.
* `ualarm` with a value or an interval of 1,000,000 microseconds or more fails with `EINVAL`
  and returns `(useconds_t)-1`; otherwise it sets `ITIMER_REAL`, interval included, so `alarm`
  sees it.
* `alarm` and `ITIMER_REAL` are one timer: `alarm(0)` after `setitimer(ITIMER_REAL)` returns the
  remaining seconds and clears the interval too.
* An itimer or alarm expiry is a signal with `si_code == SI_KERNEL`; a POSIX timer's has
  `SI_TIMER`.
* A NULL `sigevent` behaves as `SIGEV_SIGNAL` with `SIGALRM` and the timer's id as the value.
* An invalid or deleted timer id gives `EINVAL` (no crash); unknown `timer_settime` flag bits are
  ignored; `CLOCK_MONOTONIC_RAW` and `CLOCK_REALTIME_COARSE` give `EOPNOTSUPP`; a
  `SIGEV_THREAD_ID` naming a thread of another process gives `EINVAL`.
* The overrun count in the `siginfo_t` equals `timer_getoverrun`.
* `SIGEV_THREAD`: the function runs in a thread other than the caller's, detached, and receives
  the `sigev_value`; `timer_getoverrun` is 0 for such a timer; nothing runs after
  `timer_delete`. The creating thread had `SIGUSR1` blocked, so `usr1_blocked=1` does not tell
  whether the function's thread inherits the creator's mask or blocks every signal.
* The `timerfd` lines are the kernel's behaviour: `EINVAL` for unknown flags, CPU-time clocks, a
  read buffer under 8 bytes and a descriptor that is not a timer; `EAGAIN` for a non-blocking read
  before expiry; the read value is the number of expirations.

## What the record does not settle

* Whether each `SIGEV_THREAD` call runs in a new thread or a reused one (the spec says "a new
  thread"; D-23.3 records Liella's choice).
* The signal mask of the `SIGEV_THREAD` thread beyond the case above.
