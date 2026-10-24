/* The diagnostic channel's `timers` topic from a real start (D-2.9, D-23.3): a SIGEV_THREAD
 * timer whose notification thread cannot be created for a while (RLIMIT_NPROC lowered to 0,
 * as a thread limit would) and then can. The library reads LIELLA_DIAG lazily, at the first
 * failure; the test (`diag_channel.rs`) runs this with and without it and reads standard error.
 * With an argument, the program itself sets LIELLA_DIAG to it (setenv) after start and before
 * the failure: a library that read the variable at start would miss it.
 * Exit 0: notified after the limit was restored; 77: the limit did not refuse a thread (a
 * privileged user), so nothing was tested; 1: a failure. */
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/resource.h>
#include <time.h>
#include <unistd.h>

static volatile int calls;

static void notified(union sigval v) {
    (void)v;
    __atomic_add_fetch(&calls, 1, __ATOMIC_SEQ_CST);
}

static void pause_ms(long ms) {
    struct timespec ts = {ms / 1000, (ms % 1000) * 1000000L};
    nanosleep(&ts, NULL);
}

static int wait_calls(int n, long ms) {
    for (long i = 0; i < ms && __atomic_load_n(&calls, __ATOMIC_SEQ_CST) < n; i++)
        pause_ms(1);
    return __atomic_load_n(&calls, __ATOMIC_SEQ_CST) >= n;
}

int main(int argc, char **argv) {
    struct sigevent ev;
    memset(&ev, 0, sizeof ev);
    ev.sigev_notify = SIGEV_THREAD;
    ev.sigev_notify_function = notified;
    timer_t t;
    if (timer_create(CLOCK_MONOTONIC, &ev, &t) != 0)
        return 1;
    struct itimerspec one = {{0, 0}, {0, 1000000}};
    /* A first expiry with threads available: the timers' helper is running from here on. */
    if (timer_settime(t, 0, &one, NULL) != 0 || !wait_calls(1, 5000))
        return 1;
    if (argc > 1 && setenv("LIELLA_DIAG", argv[1], 1) != 0)
        return 1;
    struct rlimit old, none;
    if (getrlimit(RLIMIT_NPROC, &old) != 0)
        return 1;
    none = old;
    none.rlim_cur = 0;
    if (setrlimit(RLIMIT_NPROC, &none) != 0)
        return 1;
    if (timer_settime(t, 0, &one, NULL) != 0)
        return 1;
    /* The refusal and a few retries (10, 30, 70, 150 ms after it). */
    pause_ms(300);
    int refused = __atomic_load_n(&calls, __ATOMIC_SEQ_CST) == 1;
    if (setrlimit(RLIMIT_NPROC, &old) != 0)
        return 1;
    if (!wait_calls(2, 5000))
        return 1;
    timer_delete(t);
    return refused ? 0 : 77;
}
