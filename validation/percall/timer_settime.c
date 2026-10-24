/* timer_settime per-call counts (TIMER-SETTIME-WINDOW, #544): a SIGEV_THREAD timer (Liella's timerfd path, which counts
 * itself in and out of the helper's read window) and a SIGEV_NONE kernel timer (unchanged path, control). Each call
 * disarms an already disarmed timer, so nothing ever expires and no notification runs.
 * Run: validation/wl_percall.sh --spec validation/percall/timer_settime.c <wt> */
#include "percall.h"
#include <signal.h>
#include <time.h>

static void pc_timer_noop(union sigval v) { (void)v; }

#define TS(NAME, NOTIFY) \
CASE(NAME, 1000, 1) { \
    struct sigevent ev; memset(&ev, 0, sizeof ev); \
    ev.sigev_notify = NOTIFY; ev.sigev_notify_function = pc_timer_noop; \
    timer_t t; if (timer_create(CLOCK_MONOTONIC, &ev, &t) != 0) abort(); \
    struct itimerspec off; memset(&off, 0, sizeof off); \
    for (long i = 0; i < n; i++) pc_sink += timer_settime(t, 0, &off, NULL); \
    PC_CHECKING { struct itimerspec o; int r = timer_settime(t, 0, &off, &o); \
        pc_check("%d %ld %ld", r, (long)o.it_value.tv_sec, (long)o.it_value.tv_nsec); } \
    timer_delete(t); \
}
TS(timer_settime_thread, SIGEV_THREAD)
TS(timer_settime_none, SIGEV_NONE)
