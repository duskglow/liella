/* Timers end to end (spec §23.4; D-23.2, D-23.3): built against the host C library and against
 * Liella's products, the outputs must match line for line (timers_products.rs). Black-box: each
 * line is a result, an errno, or a fact about a delivery, with times shown only against coarse
 * thresholds so that both builds print the same on a loaded machine. */
#define _GNU_SOURCE
#include <errno.h>
#include <limits.h>
#include <pthread.h>
#include <semaphore.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <sys/syscall.h>
#include <sys/time.h>
#include <sys/timerfd.h>
#include <time.h>
#include <unistd.h>

static void line(const char *name, long a, int e)
{
    printf("%s: %ld errno=%d\n", name, a, e);
    fflush(stdout);
}

#define CALL(name, expr)          \
    do {                          \
        errno = 0;                \
        long r_ = (long)(expr);   \
        line(name, r_, errno);    \
    } while (0)

static void sleep_ms(long ms)
{
    struct timespec ts = {ms / 1000, (ms % 1000) * 1000000L};
    while (nanosleep(&ts, &ts) == -1 && errno == EINTR) {
    }
}

static long ms_of(const struct timespec *t) { return t->tv_sec * 1000L + t->tv_nsec / 1000000L; }
static long ms_tv(const struct timeval *t) { return t->tv_sec * 1000L + t->tv_usec / 1000L; }

/* ------------------------------------------------------------------ alarm, ualarm, itimers */

/* alarm(0) returns the time left rounded to the nearest second. The time left drifts while the
 * process runs, so a fixed expected number races the .5 boundary on a loaded machine. Instead
 * the true remaining time is read with getitimer just before alarm(0) (hi) and the time that
 * passed until alarm(0) returned is measured (lo = hi - elapsed); the true value at the moment
 * of the call lies in [lo, hi], and rounding is monotone, so the result must equal the
 * nearest-second rounding of lo or of hi. `ok=1` pins the rule (a truncating or rounding-up
 * alarm fails it) and cannot depend on the load. */
static long usec_of(const struct timeval *t) { return t->tv_sec * 1000000L + t->tv_usec; }
static long nearest_s(long us) { return (us + 500000L) / 1000000L; }

static void alarm_round(const char *name)
{
    struct itimerval it;
    struct timespec a, b;
    getitimer(ITIMER_REAL, &it);
    clock_gettime(CLOCK_MONOTONIC, &a);
    errno = 0;
    long r = (long)alarm(0);
    int e = errno;
    clock_gettime(CLOCK_MONOTONIC, &b);
    long hi = usec_of(&it.it_value);
    long el = (b.tv_sec - a.tv_sec) * 1000000L + (b.tv_nsec - a.tv_nsec) / 1000L;
    /* the getitimer call itself also took time before a was read: allow it in lo */
    long lo = hi - el - 20000L;
    if (lo < 0) lo = 0;
    printf("%s: ok=%d errno=%d\n", name, r == nearest_s(hi) || r == nearest_s(lo), e);
    fflush(stdout);
}


static void alarms(void)
{
    sigset_t s;
    sigemptyset(&s);
    sigaddset(&s, SIGALRM);
    sigprocmask(SIG_BLOCK, &s, NULL);

    CALL("alarm(0) idle", alarm(0));
    alarm(5);
    CALL("alarm(0) after alarm(5)", alarm(0));
    alarm(2);
    sleep_ms(1300);
    alarm_round("alarm(0) with about 0.7 s left (rounds up)");
    alarm(2);
    sleep_ms(200);
    alarm_round("alarm(0) with about 1.8 s left (rounds up)");
    alarm(2);
    sleep_ms(700);
    alarm_round("alarm(0) with about 1.3 s left (rounds down)");
    alarm(UINT_MAX);
    errno = 0;
    unsigned big = alarm(0);
    printf("alarm(UINT_MAX) then alarm(0): %s errno=%d\n", big >= INT_MAX ? ">=INT_MAX" : big > 0 ? "small" : "0", errno);

    CALL("ualarm(0,0) idle", ualarm(0, 0));
    ualarm(500000, 0);
    errno = 0;
    useconds_t u = ualarm(0, 0);
    printf("ualarm(0,0) after ualarm(500000): %s errno=%d\n", u > 400000 && u <= 500000 ? "(400000,500000]" : "other", errno);
    errno = 0;
    u = ualarm(1000000, 0);
    printf("ualarm(1000000,0): %ld errno=%d\n", (long)u, errno);
    alarm(0);
    errno = 0;
    u = ualarm(2500000, 0);
    printf("ualarm(2500000,0): %ld errno=%d\n", (long)u, errno);
    alarm_round("alarm(0) after ualarm(2500000)");
    ualarm(300000, 100000);
    struct itimerval it;
    getitimer(ITIMER_REAL, &it);
    printf("ualarm(300000,100000) itimer: interval=%ld value<=300ms=%d\n", ms_tv(&it.it_interval), ms_tv(&it.it_value) <= 300 && ms_tv(&it.it_value) > 200);
    alarm(0);

    struct itimerval nv = {{0, 250000}, {3, 0}}, ov;
    CALL("setitimer REAL 3s/250ms", setitimer(ITIMER_REAL, &nv, NULL));
    getitimer(ITIMER_REAL, &it);
    printf("getitimer REAL: interval=%ld value_s=%ld\n", ms_tv(&it.it_interval), (long)(it.it_value.tv_sec + (it.it_value.tv_usec > 0)));
    CALL("alarm(0) after setitimer REAL 3s", alarm(0));
    getitimer(ITIMER_REAL, &it);
    printf("getitimer REAL after alarm(0): interval=%ld value=%ld\n", ms_tv(&it.it_interval), ms_tv(&it.it_value));
    struct itimerval z = {{0, 0}, {0, 0}};
    setitimer(ITIMER_REAL, &z, NULL);

    nv = (struct itimerval){{0, 0}, {1, 0}};
    setitimer(ITIMER_VIRTUAL, &nv, NULL);
    memset(&ov, 0xff, sizeof ov);
    CALL("setitimer VIRTUAL 0 with old", setitimer(ITIMER_VIRTUAL, &z, &ov));
    printf("old VIRTUAL: interval=%ld value_ms(900,1100]=%d\n", ms_tv(&ov.it_interval), ms_tv(&ov.it_value) > 900 && ms_tv(&ov.it_value) <= 1100);
    CALL("setitimer PROF 0", setitimer(ITIMER_PROF, &z, NULL));
    CALL("setitimer which=3", setitimer(3, &z, NULL));
    CALL("setitimer which=-1", setitimer(-1, &z, NULL));
    CALL("getitimer which=3", getitimer(3, &it));
    struct itimerval bad = {{0, 0}, {0, 1000000}};
    CALL("setitimer usec=1000000", setitimer(ITIMER_REAL, &bad, NULL));
    bad = (struct itimerval){{0, 1000000}, {1, 0}};
    CALL("setitimer interval usec=1000000", setitimer(ITIMER_REAL, &bad, NULL));
    setitimer(ITIMER_REAL, &z, NULL);
    bad = (struct itimerval){{0, 0}, {-1, 0}};
    CALL("setitimer sec=-1", setitimer(ITIMER_REAL, &bad, NULL));
    bad = (struct itimerval){{0, 0}, {0, -1}};
    CALL("setitimer usec=-1", setitimer(ITIMER_REAL, &bad, NULL));
    setitimer(ITIMER_REAL, &z, NULL);

    /* An expiry delivers SIGALRM. */
    nv = (struct itimerval){{0, 0}, {0, 20000}};
    setitimer(ITIMER_REAL, &nv, NULL);
    siginfo_t si;
    struct timespec to = {2, 0};
    int sig = sigtimedwait(&s, &si, &to);
    printf("ITIMER_REAL expiry: sig=%d code_kernel=%d\n", sig, si.si_code == SI_KERNEL);
    alarm(1);
    sig = sigtimedwait(&s, &si, &(struct timespec){3, 0});
    printf("alarm expiry: sig=%d code_kernel=%d\n", sig, si.si_code == SI_KERNEL);
    sigprocmask(SIG_UNBLOCK, &s, NULL);
}

/* ------------------------------------------------------------------ POSIX timers */

static volatile int thread_calls;
static volatile int thread_value;
static volatile int thread_not_main;
static volatile int thread_detached;
static volatile int thread_usr1_blocked;
static pthread_t first_thread;
static pthread_t main_thread;

/* Waits until *count has not moved for 200 ms (at most 5 s): 1 once it settles. A notification
   whose thread was started before timer_delete may still call late (the reference starts a
   thread per expiration, and under load one can run well after the delete); a timer still
   firing would never settle. */
static int settles(volatile int *count)
{
    int last = *count, still = 0;
    for (int waited = 0; waited < 5000; waited += 10) {
        sleep_ms(10);
        int now = *count;
        if (now != last) {
            last = now;
            still = 0;
        } else if ((still += 10) >= 200) {
            return 1;
        }
    }
    return 0;
}

/* The delete cases' function: sigev_value -1 is the periodic timer's, any other value the
   one-shot attempt's of that number (a call of an earlier attempt is not counted). */
static volatile int armed_calls;
static volatile int attempt_no;
static volatile int attempt_calls;
static void notify_delete(union sigval v)
{
    if (v.sival_int == -1)
        __atomic_add_fetch(&armed_calls, 1, __ATOMIC_SEQ_CST);
    else if (v.sival_int == __atomic_load_n(&attempt_no, __ATOMIC_SEQ_CST))
        __atomic_add_fetch(&attempt_calls, 1, __ATOMIC_SEQ_CST);
}

static long now_ms(void)
{
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return ms_of(&ts);
}

static void notify(union sigval v)
{
    pthread_t me = pthread_self();
    int n = __atomic_add_fetch(&thread_calls, 1, __ATOMIC_SEQ_CST);
    if (n == 1) {
        thread_value = v.sival_int;
        thread_not_main = !pthread_equal(me, main_thread);
        sigset_t cur;
        pthread_sigmask(SIG_BLOCK, NULL, &cur);
        thread_usr1_blocked = sigismember(&cur, SIGUSR1);
        pthread_attr_t a;
        int ds = -1;
        if (pthread_getattr_np(me, &a) == 0) {
            pthread_attr_getdetachstate(&a, &ds);
            pthread_attr_destroy(&a);
        }
        thread_detached = ds == PTHREAD_CREATE_DETACHED;
        first_thread = me;
    }
}

/* The overrun count across timer_settime: the first call blocks until it is released while the
 * timer keeps expiring, so expirations pile up whatever the scheduling (an implementation that
 * notifies one expiration at a time counts them as overruns of the next call); the second call
 * signals that it ran. */
struct held_state {
    int calls;
    sem_t release, second;
};

static void held(union sigval v)
{
    struct held_state *h = v.sival_ptr;
    int n = __atomic_add_fetch(&h->calls, 1, __ATOMIC_SEQ_CST);
    if (n == 1)
        sem_wait(&h->release);
    else if (n == 2)
        sem_post(&h->second);
}

/* A SIGEV_THREAD timer that expired many times behind a blocked call, then set to `next`: the
 * overrun count after the setting. */
static void overrun_after_settime(struct held_state *h, const char *what, const struct itimerspec *next)
{
    struct sigevent ev;
    memset(&ev, 0, sizeof ev);
    ev.sigev_notify = SIGEV_THREAD;
    ev.sigev_notify_function = held;
    ev.sigev_value.sival_ptr = h;
    timer_t t;
    /* Each timer has its own state, never destroyed: a late call of an earlier timer is harmless. */
    h->calls = 0;
    sem_init(&h->release, 0, 0);
    sem_init(&h->second, 0, 0);
    if (timer_create(CLOCK_MONOTONIC, &ev, &t) != 0) {
        printf("%s: timer_create failed errno=%d\n", what, errno);
        return;
    }
    struct itimerspec p10 = {{0, 10000000}, {0, 10000000}};
    timer_settime(t, 0, &p10, NULL);
    sleep_ms(80);
    sem_post(&h->release);
    struct timespec dl;
    clock_gettime(CLOCK_REALTIME, &dl);
    dl.tv_sec += 30;
    int second = sem_timedwait(&h->second, &dl) == 0;
    timer_settime(t, 0, next, NULL);
    char name[96];
    snprintf(name, sizeof name, "timer_getoverrun SIGEV_THREAD after %s (second call ran=%d)", what, second);
    CALL(name, timer_getoverrun(t));
    timer_delete(t);
}

static void posix_timers(void)
{
    sigset_t s;
    sigemptyset(&s);
    sigaddset(&s, SIGALRM);
    sigaddset(&s, SIGUSR1);
    sigaddset(&s, SIGUSR2);
    sigprocmask(SIG_BLOCK, &s, NULL);
    siginfo_t si;
    timer_t t, t2;

    /* NULL sigevent: SIGEV_SIGNAL with SIGALRM, and the timer's id as the value. */
    CALL("timer_create REALTIME NULL", timer_create(CLOCK_REALTIME, NULL, &t));
    struct itimerspec on = {{0, 0}, {0, 10000000}}, cur;
    CALL("timer_settime 10ms", timer_settime(t, 0, &on, NULL));
    sigset_t alrm;
    sigemptyset(&alrm);
    sigaddset(&alrm, SIGALRM);
    int sig = sigtimedwait(&alrm, &si, &(struct timespec){2, 0});
    printf("NULL sigevent expiry: sig=%d code_timer=%d value_is_id=%d\n", sig, si.si_code == SI_TIMER, si.si_value.sival_ptr == (void *)t || si.si_value.sival_int == (int)(intptr_t)t);
    CALL("timer_delete", timer_delete(t));
    CALL("timer_delete again", timer_delete(t));

    struct sigevent ev;
    memset(&ev, 0, sizeof ev);
    ev.sigev_notify = SIGEV_SIGNAL;
    ev.sigev_signo = SIGUSR1;
    ev.sigev_value.sival_int = 42;
    CALL("timer_create MONOTONIC SIGUSR1", timer_create(CLOCK_MONOTONIC, &ev, &t));
    CALL("timer_gettime disarmed", timer_gettime(t, &cur));
    printf("disarmed: value=%ld interval=%ld\n", ms_of(&cur.it_value), ms_of(&cur.it_interval));
    on = (struct itimerspec){{0, 1000000}, {0, 1000000}};
    timer_settime(t, 0, &on, NULL);
    sleep_ms(60);
    sigset_t u1;
    sigemptyset(&u1);
    sigaddset(&u1, SIGUSR1);
    sig = sigtimedwait(&u1, &si, &(struct timespec){2, 0});
    errno = 0;
    int over = timer_getoverrun(t);
    printf("periodic 1ms, 60ms blocked: sig=%d value=%d overrun>10=%d errno=%d\n", sig, si.si_value.sival_int, over > 10, errno);
    printf("siginfo overrun matches=%d\n", si.si_overrun == over);
    struct itimerspec old;
    on = (struct itimerspec){{0, 0}, {5, 0}};
    CALL("timer_settime 5s with old", timer_settime(t, 0, &on, &old));
    printf("old: interval=%ld value<=1=%d\n", ms_of(&old.it_interval), ms_of(&old.it_value) <= 1);
    timer_gettime(t, &cur);
    printf("gettime: interval=%ld value_s=%ld\n", ms_of(&cur.it_interval), (long)(cur.it_value.tv_sec + (cur.it_value.tv_nsec > 0)));
    on = (struct itimerspec){{0, 0}, {0, 1000000000}};
    CALL("timer_settime nsec=1e9", timer_settime(t, 0, &on, NULL));
    on = (struct itimerspec){{0, 1000000000}, {1, 0}};
    CALL("timer_settime interval nsec=1e9", timer_settime(t, 0, &on, NULL));
    on = (struct itimerspec){{0, 0}, {-1, 0}};
    CALL("timer_settime sec=-1", timer_settime(t, 0, &on, NULL));
    CALL("timer_settime flags=2", timer_settime(t, 2, &(struct itimerspec){{0, 0}, {0, 0}}, NULL));
    struct timespec now;
    clock_gettime(CLOCK_MONOTONIC, &now);
    now.tv_sec -= 10;
    on = (struct itimerspec){{0, 0}, now};
    CALL("timer_settime ABSTIME past", timer_settime(t, TIMER_ABSTIME, &on, NULL));
    sig = sigtimedwait(&u1, &si, &(struct timespec){2, 0});
    printf("past absolute fires: sig=%d\n", sig);
    CALL("timer_delete", timer_delete(t));

    /* SIGEV_NONE: the timer runs, nothing is delivered. */
    ev.sigev_notify = SIGEV_NONE;
    CALL("timer_create SIGEV_NONE", timer_create(CLOCK_MONOTONIC, &ev, &t));
    on = (struct itimerspec){{0, 20000000}, {0, 20000000}};
    timer_settime(t, 0, &on, NULL);
    sleep_ms(50);
    timer_gettime(t, &cur);
    printf("SIGEV_NONE: interval=%ld value<=20=%d pending_usr1=%d\n", ms_of(&cur.it_interval), ms_of(&cur.it_value) <= 20, (sigpending(&u1), sigismember(&u1, SIGUSR1)));
    timer_delete(t);

    /* SIGEV_THREAD_ID: the signal goes to the named thread. */
    ev.sigev_notify = SIGEV_THREAD_ID;
    ev.sigev_signo = SIGUSR2;
    ev._sigev_un._tid = (pid_t)syscall(SYS_gettid);
    CALL("timer_create SIGEV_THREAD_ID self", timer_create(CLOCK_MONOTONIC, &ev, &t));
    on = (struct itimerspec){{0, 0}, {0, 5000000}};
    timer_settime(t, 0, &on, NULL);
    sigset_t u2;
    sigemptyset(&u2);
    sigaddset(&u2, SIGUSR2);
    sig = sigtimedwait(&u2, &si, &(struct timespec){2, 0});
    printf("SIGEV_THREAD_ID expiry: sig=%d code_timer=%d\n", sig, si.si_code == SI_TIMER);
    timer_delete(t);
    ev._sigev_un._tid = 1;
    CALL("timer_create SIGEV_THREAD_ID foreign tid", timer_create(CLOCK_MONOTONIC, &ev, &t));

    /* Invalid arguments. */
    memset(&ev, 0, sizeof ev);
    ev.sigev_notify = SIGEV_SIGNAL;
    ev.sigev_signo = SIGUSR1;
    CALL("timer_create clock=99", timer_create(99, &ev, &t));
    ev.sigev_notify = 99;
    CALL("timer_create notify=99", timer_create(CLOCK_MONOTONIC, &ev, &t));
    ev.sigev_notify = SIGEV_SIGNAL;
    ev.sigev_signo = 0;
    CALL("timer_create signo=0", timer_create(CLOCK_MONOTONIC, &ev, &t));
    ev.sigev_signo = 65;
    CALL("timer_create signo=65", timer_create(CLOCK_MONOTONIC, &ev, &t));
    ev.sigev_signo = SIGUSR1;
    CALL("timer_create PROCESS_CPUTIME", timer_create(CLOCK_PROCESS_CPUTIME_ID, &ev, &t));
    if (errno == 0)
        timer_delete(t);
    CALL("timer_create THREAD_CPUTIME", timer_create(CLOCK_THREAD_CPUTIME_ID, &ev, &t));
    if (errno == 0)
        timer_delete(t);
    CALL("timer_create BOOTTIME", timer_create(CLOCK_BOOTTIME, &ev, &t));
    if (errno == 0)
        timer_delete(t);
    CALL("timer_create MONOTONIC_RAW", timer_create(CLOCK_MONOTONIC_RAW, &ev, &t));
    CALL("timer_create REALTIME_COARSE", timer_create(CLOCK_REALTIME_COARSE, &ev, &t));
    CALL("timer_gettime bad id", timer_gettime((timer_t)(intptr_t)0x7fff1234, &cur));
    CALL("timer_getoverrun bad id", timer_getoverrun((timer_t)(intptr_t)0x7fff1234));
    CALL("timer_delete bad id", timer_delete((timer_t)(intptr_t)0x7fff1234));

    /* Many timers. */
    static timer_t many[200];
    int made = 0;
    ev.sigev_notify = SIGEV_NONE;
    for (int i = 0; i < 200; i++)
        if (timer_create(CLOCK_MONOTONIC, &ev, &many[i]) == 0)
            made++;
    printf("200 timers created: %d distinct_first_two=%d\n", made, many[0] != many[1]);
    for (int i = 0; i < made; i++)
        timer_delete(many[i]);

    /* SIGEV_THREAD: the function runs in a thread other than the caller's. */
    main_thread = pthread_self();
    memset(&ev, 0, sizeof ev);
    ev.sigev_notify = SIGEV_THREAD;
    ev.sigev_notify_function = notify;
    ev.sigev_value.sival_int = 7;
    CALL("timer_create SIGEV_THREAD", timer_create(CLOCK_MONOTONIC, &ev, &t));
    on = (struct itimerspec){{0, 20000000}, {0, 20000000}};
    timer_settime(t, 0, &on, NULL);
    for (int i = 0; i < 200 && thread_calls < 3; i++)
        sleep_ms(10);
    timer_settime(t, 0, &(struct itimerspec){{0, 0}, {0, 0}}, NULL);
    printf("SIGEV_THREAD: calls>=3=%d value=%d not_main=%d detached=%d usr1_blocked=%d\n", thread_calls >= 3, thread_value, thread_not_main, thread_detached, thread_usr1_blocked);
    CALL("timer_getoverrun SIGEV_THREAD", timer_getoverrun(t));
    CALL("timer_delete SIGEV_THREAD", timer_delete(t));
    /* The timer was disarmed first: a call after the delete is of an expiry before it. */
    printf("SIGEV_THREAD after delete: calls settle=%d\n", settles(&thread_calls));

    /* timer_delete of a periodic timer that is still armed: the calls stop (those already
       started may finish: the count settles instead of going on every 10 ms). */
    {
        struct sigevent dev;
        memset(&dev, 0, sizeof dev);
        dev.sigev_notify = SIGEV_THREAD;
        dev.sigev_notify_function = notify_delete;
        dev.sigev_value.sival_int = -1;
        timer_t d;
        CALL("timer_create SIGEV_THREAD periodic", timer_create(CLOCK_MONOTONIC, &dev, &d));
        timer_settime(d, 0, &(struct itimerspec){{0, 10000000}, {0, 10000000}}, NULL);
        for (int i = 0; i < 200 && armed_calls < 2; i++)
            sleep_ms(10);
        int before = armed_calls >= 2;
        CALL("timer_delete SIGEV_THREAD armed", timer_delete(d));
        printf("SIGEV_THREAD deleted while armed: called=%d calls settle=%d\n", before, settles(&armed_calls));

        /* A one-shot deleted before its expiry is never notified: nothing of it started. An
           attempt whose delete returns too close to the expiry (a stall under load) does not
           count, and its calls are not counted (another sigev_value). */
        int valid = 0;
        for (int n = 1; n <= 20 && !valid; n++) {
            __atomic_store_n(&attempt_no, n, __ATOMIC_SEQ_CST);
            __atomic_store_n(&attempt_calls, 0, __ATOMIC_SEQ_CST);
            dev.sigev_value.sival_int = n;
            if (timer_create(CLOCK_MONOTONIC, &dev, &d) != 0)
                break;
            long armed = now_ms();
            timer_settime(d, 0, &(struct itimerspec){{0, 0}, {0, 100000000}}, NULL);
            timer_delete(d);
            valid = now_ms() - armed < 50;
            sleep_ms(200);
        }
        printf("SIGEV_THREAD deleted before its expiry: attempt=%d called=%d\n", valid, attempt_calls);
    }

    /* Two SIGEV_THREAD timers at once. */
    thread_calls = 0;
    CALL("timer_create SIGEV_THREAD a", timer_create(CLOCK_MONOTONIC, &ev, &t));
    CALL("timer_create SIGEV_THREAD b", timer_create(CLOCK_MONOTONIC, &ev, &t2));
    on = (struct itimerspec){{0, 0}, {0, 10000000}};
    timer_settime(t, 0, &on, NULL);
    timer_settime(t2, 0, &on, NULL);
    for (int i = 0; i < 200 && thread_calls < 2; i++)
        sleep_ms(10);
    sleep_ms(30);
    printf("two one-shot SIGEV_THREAD timers: calls=%d\n", thread_calls);
    timer_delete(t);
    timer_delete(t2);

    /* timer_settime clears the overrun count, whether it disarms or arms again. */
    static struct held_state held_off, held_far;
    overrun_after_settime(&held_off, "disarm", &(struct itimerspec){{0, 0}, {0, 0}});
    overrun_after_settime(&held_far, "re-arm 10 s", &(struct itimerspec){{0, 0}, {10, 0}});
    sigprocmask(SIG_UNBLOCK, &s, NULL);
}

/* ------------------------------------------------------------------ timerfd */

static void timerfds(void)
{
    int fd;
    CALL("timerfd_create MONOTONIC 0", (fd = timerfd_create(CLOCK_MONOTONIC, 0)) >= 0);
    CALL("timerfd_create flags=1", timerfd_create(CLOCK_MONOTONIC, 1));
    CALL("timerfd_create clock=99", timerfd_create(99, 0));
    CALL("timerfd_create PROCESS_CPUTIME", timerfd_create(CLOCK_PROCESS_CPUTIME_ID, 0));
    int nb = timerfd_create(CLOCK_REALTIME, TFD_NONBLOCK | TFD_CLOEXEC);
    printf("timerfd_create NONBLOCK|CLOEXEC: ok=%d\n", nb >= 0);
    uint64_t n = 0;
    CALL("read disarmed nonblocking", read(nb, &n, sizeof n));
    struct itimerspec on = {{0, 0}, {0, 10000000}}, cur;
    CALL("timerfd_settime 10ms", timerfd_settime(fd, 0, &on, NULL));
    CALL("timerfd_gettime", timerfd_gettime(fd, &cur));
    printf("armed: interval=%ld value<=10=%d\n", ms_of(&cur.it_interval), ms_of(&cur.it_value) <= 10);
    uint32_t small;
    CALL("read 4 bytes", read(fd, &small, sizeof small));
    errno = 0;
    ssize_t r = read(fd, &n, sizeof n);
    printf("read after expiry: %ld count=%llu errno=%d\n", (long)r, (unsigned long long)n, errno);
    on = (struct itimerspec){{0, 1000000}, {0, 1000000}};
    timerfd_settime(fd, 0, &on, NULL);
    sleep_ms(30);
    r = read(fd, &n, sizeof n);
    printf("periodic 1ms after 30ms: %ld count>=10=%d\n", (long)r, n >= 10);
    struct itimerspec old;
    on = (struct itimerspec){{0, 0}, {0, 0}};
    CALL("timerfd_settime disarm with old", timerfd_settime(fd, 0, &on, &old));
    printf("old: interval=%ld\n", ms_of(&old.it_interval));
    CALL("timerfd_settime flags=4", timerfd_settime(fd, 4, &on, NULL));
    CALL("timerfd_settime CANCEL_ON_SET without ABSTIME", timerfd_settime(nb, TFD_TIMER_CANCEL_ON_SET, &on, NULL));
    struct timespec now;
    clock_gettime(CLOCK_REALTIME, &now);
    on = (struct itimerspec){{0, 0}, {now.tv_sec + 100, 0}};
    CALL("timerfd_settime ABSTIME|CANCEL_ON_SET", timerfd_settime(nb, TFD_TIMER_ABSTIME | TFD_TIMER_CANCEL_ON_SET, &on, NULL));
    on = (struct itimerspec){{0, 0}, {0, 1000000000}};
    CALL("timerfd_settime nsec=1e9", timerfd_settime(fd, 0, &on, NULL));
    int p[2];
    if (pipe(p) == 0) {
        CALL("timerfd_settime on a pipe", timerfd_settime(p[0], 0, &on, NULL));
        close(p[0]);
        close(p[1]);
    }
    CALL("timerfd_gettime bad fd", timerfd_gettime(-1, &cur));
    close(fd);
    close(nb);
}

int main(void)
{
    alarms();
    posix_timers();
    timerfds();
    printf("done\n");
    return 0;
}
