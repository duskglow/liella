/* Spec §22.3 and the cancellation clauses of §9.1, §11, §15, §20.5, §23.3: deferred
 * cancellation acts at cancellation points, including while blocked in one (read, a condition
 * wait, sem_wait, nanosleep, sigwait, pthread_join, getrandom's blocking is not exercised);
 * a call cancelled before its effect has no effect (the pipe byte stays); cleanup handlers run
 * newest first, a condition wait's mutex is held again when they run; disabled requests stay
 * pending; asynchronous requests act at the next safe point (owner ruling, D-22.11); a once
 * routine that is cancelled lets the next caller run it; pthread_exit runs handlers then key
 * destructors; pthread_kill and pthread_sigqueue reach one thread. */
#define _GNU_SOURCE
#include <errno.h>
#include <pthread.h>
#include <semaphore.h>
#include <signal.h>
#include <stdint.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

static int pipefd[2];
static char order[16];
static int norder;
static void note(void *c) { order[norder++] = (char)(intptr_t)c; }

static void *reader(void *arg)
{
    (void)arg;
    char c;
    pthread_cleanup_push(note, (void *)'b');
    pthread_cleanup_push(note, (void *)'a');
    (void)read(pipefd[0], &c, 1); /* blocks: cancelled here */
    pthread_cleanup_pop(0);
    pthread_cleanup_pop(0);
    return (void *)1;
}

static pthread_mutex_t m = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t cv = PTHREAD_COND_INITIALIZER;
static int held_in_cleanup;
static void unlock_m(void *arg)
{
    (void)arg;
    held_in_cleanup = pthread_mutex_trylock(&m) == EDEADLK || pthread_mutex_unlock(&m) == 0;
}
static void *cond_waiter(void *arg)
{
    (void)arg;
    pthread_mutex_lock(&m);
    pthread_cleanup_push(unlock_m, NULL);
    for (;;)
        pthread_cond_wait(&cv, &m);
    pthread_cleanup_pop(1);
    return NULL;
}

static sem_t sem;
static void *sem_waiter(void *arg)
{
    (void)arg;
    sem_wait(&sem);
    return (void *)2;
}

static void *sleeper(void *arg)
{
    (void)arg;
    struct timespec t = {60, 0};
    nanosleep(&t, NULL);
    return (void *)3;
}

static void *sigwaiter(void *arg)
{
    (void)arg;
    sigset_t s;
    sigemptyset(&s);
    sigaddset(&s, SIGUSR2);
    int sig;
    sigwait(&s, &sig);
    return (void *)4;
}

static pthread_t joined_forever;
static void *joiner(void *arg)
{
    (void)arg;
    pthread_join(joined_forever, NULL);
    return (void *)5;
}
static void *forever(void *arg)
{
    (void)arg;
    for (;;)
        pause();
    return NULL;
}

static volatile int go;
static void *disabled(void *arg)
{
    (void)arg;
    pthread_setcancelstate(PTHREAD_CANCEL_DISABLE, NULL);
    while (!go)
        usleep(1000); /* a cancellation point, but disabled */
    pthread_setcancelstate(PTHREAD_CANCEL_ENABLE, NULL);
    pthread_testcancel();
    return (void *)6;
}

static volatile int spun;
static void *async_one(void *arg)
{
    (void)arg;
    int old;
    while (!go)
        spun++;
    pthread_setcanceltype(PTHREAD_CANCEL_ASYNCHRONOUS, &old); /* a safe point: acts */
    return (void *)7;
}

static pthread_once_t once = PTHREAD_ONCE_INIT;
static int once_runs;
static void slow_init(void)
{
    once_runs++;
    if (once_runs == 1) {
        char c;
        (void)read(pipefd[0], &c, 1); /* cancelled inside the routine */
    }
}
static void *once_caller(void *arg)
{
    (void)arg;
    pthread_once(&once, slow_init);
    return NULL;
}

static pthread_key_t key;
static char exit_order[8];
static int nexit;
static void key_dtor(void *v) { exit_order[nexit++] = (char)(intptr_t)v; }
static void exit_note(void *c) { exit_order[nexit++] = (char)(intptr_t)c; }
static void *exiter(void *arg)
{
    (void)arg;
    pthread_setspecific(key, (void *)'k');
    pthread_cleanup_push(exit_note, (void *)'2');
    pthread_cleanup_push(exit_note, (void *)'1');
    pthread_exit((void *)8);
    pthread_cleanup_pop(0);
    pthread_cleanup_pop(0);
    return NULL;
}

static volatile pid_t handled_on;
static volatile int queued_value;
static void on_usr1(int sig, siginfo_t *si, void *ctx)
{
    (void)sig;
    (void)ctx;
    handled_on = gettid();
    queued_value = si->si_value.sival_int;
}
static volatile pid_t target_tid;
static void *target(void *arg)
{
    (void)arg;
    target_tid = gettid();
    while (handled_on == 0)
        usleep(1000);
    return NULL;
}

static int cancel_and_join(pthread_t t)
{
    usleep(30000);
    if (pthread_cancel(t) != 0)
        return 0;
    void *r;
    if (pthread_join(t, &r) != 0)
        return 0;
    return r == PTHREAD_CANCELED;
}

int main(void)
{
    if (pipe(pipefd) != 0)
        return 1;
    pthread_t t;

    pthread_create(&t, NULL, reader, NULL);
    if (!cancel_and_join(t) || norder != 2 || order[0] != 'a' || order[1] != 'b')
        return 2; /* handlers newest first */
    /* The cancelled read consumed nothing. */
    if (write(pipefd[1], "x", 1) != 1)
        return 3;
    char c = 0;
    if (read(pipefd[0], &c, 1) != 1 || c != 'x')
        return 4;

    pthread_create(&t, NULL, cond_waiter, NULL);
    if (!cancel_and_join(t) || !held_in_cleanup)
        return 5;
    if (pthread_mutex_trylock(&m) != 0)
        return 6; /* the handler released it */
    pthread_mutex_unlock(&m);

    sem_init(&sem, 0, 0);
    pthread_create(&t, NULL, sem_waiter, NULL);
    if (!cancel_and_join(t))
        return 7;
    pthread_create(&t, NULL, sleeper, NULL);
    if (!cancel_and_join(t))
        return 8;
    sigset_t s2;
    sigemptyset(&s2);
    sigaddset(&s2, SIGUSR2);
    pthread_sigmask(SIG_BLOCK, &s2, NULL);
    pthread_create(&t, NULL, sigwaiter, NULL);
    if (!cancel_and_join(t))
        return 9;
    pthread_create(&joined_forever, NULL, forever, NULL);
    pthread_create(&t, NULL, joiner, NULL);
    if (!cancel_and_join(t))
        return 10;
    if (!cancel_and_join(joined_forever))
        return 11; /* still joinable after its joiner was cancelled */

    pthread_create(&t, NULL, disabled, NULL);
    usleep(20000);
    pthread_cancel(t);
    usleep(20000);
    go = 1;
    void *r;
    pthread_join(t, &r);
    if (r != PTHREAD_CANCELED)
        return 12; /* pending while disabled, acted on at testcancel */
    go = 0;

    pthread_create(&t, NULL, async_one, NULL);
    for (int i = 0; i < 20000 && spun == 0; i++) /* wait until it has run: a fixed 20 ms is too short on a loaded machine (FL-13) */
        usleep(1000);
    pthread_cancel(t);
    usleep(20000);
    if (spun == 0)
        return 13;
    go = 1;
    pthread_join(t, &r);
    if (r != PTHREAD_CANCELED)
        return 14;

    pthread_create(&t, NULL, once_caller, NULL);
    if (!cancel_and_join(t) || once_runs != 1)
        return 15;
    pthread_once(&once, slow_init); /* reset: runs again (and does not block the second time) */
    if (once_runs != 2)
        return 16;

    pthread_key_create(&key, key_dtor);
    pthread_create(&t, NULL, exiter, NULL);
    pthread_join(t, &r);
    if (r != (void *)8 || nexit != 3 || memcmp(exit_order, "12k", 3) != 0)
        return 17; /* handlers, then key destructors */

    struct sigaction sa;
    memset(&sa, 0, sizeof sa);
    sa.sa_sigaction = on_usr1;
    sa.sa_flags = SA_SIGINFO;
    sigaction(SIGUSR1, &sa, NULL);
    pthread_create(&t, NULL, target, NULL);
    while (target_tid == 0)
        usleep(1000);
    if (pthread_kill(t, 0) != 0 || pthread_kill(t, 32) != EINVAL)
        return 18;
    union sigval v = {.sival_int = 42};
    if (pthread_sigqueue(t, SIGUSR1, v) != 0)
        return 19;
    pthread_join(t, NULL);
    if (handled_on != target_tid || queued_value != 42)
        return 20; /* delivered to that thread, with its value */
    return 0;
}
