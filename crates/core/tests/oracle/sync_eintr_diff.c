/* Which thread waits end with EINTR when a handled signal interrupts them, with and without SA_RESTART.
   Each case: a waiter thread blocks; main waits until /proc/self/task/<tid>/stat shows state S (bounded),
   sends SIGUSR1 with pthread_kill (handler installed with or without SA_RESTART), gives the waiter a bounded
   time to return, prints whether it returned early (rc, errno) or stayed blocked, then releases it. */
#define _GNU_SOURCE
#include <errno.h>
#include <pthread.h>
#include <semaphore.h>
#include <signal.h>
#include <stdio.h>
#include <string.h>
#include <time.h>
#include <unistd.h>
#include <sys/syscall.h>

static volatile int hits, ready, done, wtid, wrc, werr;
static sem_t sem, sem2;
static pthread_mutex_t mx = PTHREAD_MUTEX_INITIALIZER, cmx = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t cv = PTHREAD_COND_INITIALIZER;
static pthread_rwlock_t rw = PTHREAD_RWLOCK_INITIALIZER;
static pthread_barrier_t bar;
static pthread_t other, waiter;
static int pred;

static void h(int s) { (void)s; hits++; }
static void ms(int n) { struct timespec t = {0, n * 1000000L}; nanosleep(&t, 0); }
static struct timespec far(clockid_t c) { struct timespec t; clock_gettime(c, &t); t.tv_sec += 60; return t; }
static void *sleeper(void *a) { (void)a; while (sem_wait(&sem2) != 0) {} return 0; }
#define WAIT(call) do { wtid = (int)syscall(SYS_gettid); ready = 1; errno = 0; wrc = (call); werr = errno; done = 1; } while (0)

enum { SEM_WAIT, SEM_TIMED, SEM_CLOCK, COND_WAIT, COND_TIMED, MUTEX_TIMED, MUTEX_CLOCK, RD_TIMED, WR_TIMED, RD_CLOCK, WR_CLOCK, BARRIER, JOIN, SIGWAIT, SIGTIMED, SIGSUSP, PAUSE, N };
static const char *names[N] = {"sem_wait", "sem_timedwait", "sem_clockwait", "pthread_cond_wait", "pthread_cond_timedwait",
    "pthread_mutex_timedlock", "pthread_mutex_clocklock", "pthread_rwlock_timedrdlock", "pthread_rwlock_timedwrlock",
    "pthread_rwlock_clockrdlock", "pthread_rwlock_clockwrlock", "pthread_barrier_wait", "pthread_join", "sigwait",
    "sigtimedwait", "sigsuspend", "pause"};
static int which;

static void *body(void *a) {
    (void)a;
    sigset_t s; siginfo_t si; struct timespec t;
    sigemptyset(&s); sigaddset(&s, SIGUSR2);
    switch (which) {
    case SEM_WAIT: WAIT(sem_wait(&sem)); break;
    case SEM_TIMED: t = far(CLOCK_REALTIME); WAIT(sem_timedwait(&sem, &t)); break;
    case SEM_CLOCK: t = far(CLOCK_MONOTONIC); WAIT(sem_clockwait(&sem, CLOCK_MONOTONIC, &t)); break;
    case COND_WAIT: pthread_mutex_lock(&cmx); WAIT(pthread_cond_wait(&cv, &cmx)); pthread_mutex_unlock(&cmx); break;
    case COND_TIMED: pthread_mutex_lock(&cmx); t = far(CLOCK_REALTIME); WAIT(pthread_cond_timedwait(&cv, &cmx, &t)); pthread_mutex_unlock(&cmx); break;
    case MUTEX_TIMED: t = far(CLOCK_REALTIME); WAIT(pthread_mutex_timedlock(&mx, &t)); break;
    case MUTEX_CLOCK: t = far(CLOCK_MONOTONIC); WAIT(pthread_mutex_clocklock(&mx, CLOCK_MONOTONIC, &t)); break;
    case RD_TIMED: t = far(CLOCK_REALTIME); WAIT(pthread_rwlock_timedrdlock(&rw, &t)); break;
    case WR_TIMED: t = far(CLOCK_REALTIME); WAIT(pthread_rwlock_timedwrlock(&rw, &t)); break;
    case RD_CLOCK: t = far(CLOCK_MONOTONIC); WAIT(pthread_rwlock_clockrdlock(&rw, CLOCK_MONOTONIC, &t)); break;
    case WR_CLOCK: t = far(CLOCK_MONOTONIC); WAIT(pthread_rwlock_clockwrlock(&rw, CLOCK_MONOTONIC, &t)); break;
    case BARRIER: WAIT(pthread_barrier_wait(&bar)); break;
    case JOIN: WAIT(pthread_join(other, 0)); break;
    case SIGWAIT: { int g = 0; WAIT(sigwait(&s, &g)); break; }
    case SIGTIMED: t.tv_sec = 60; t.tv_nsec = 0; WAIT(sigtimedwait(&s, &si, &t)); break;
    case SIGSUSP: { sigset_t e; sigemptyset(&e); WAIT(sigsuspend(&e)); break; }
    case PAUSE: WAIT(pause()); break;
    }
    return 0;
}

static const char *en(int e) { return e == EINTR ? "EINTR" : e == ETIMEDOUT ? "ETIMEDOUT" : e == 0 ? "0" : "other"; }

static void run(int w, int restart) {
    struct sigaction sa; sigset_t blk;
    memset(&sa, 0, sizeof sa); sa.sa_handler = h; sa.sa_flags = restart ? SA_RESTART : 0;
    sigaction(SIGUSR1, &sa, 0);
    hits = ready = done = 0; which = w;
    sem_init(&sem, 0, 0); sem_init(&sem2, 0, 0);
    pthread_mutex_init(&mx, 0);
    pthread_rwlock_init(&rw, 0);
    pthread_barrier_init(&bar, 0, 2);
    pred = 0;
    if (w == MUTEX_TIMED || w == MUTEX_CLOCK) pthread_mutex_lock(&mx);
    if (w == RD_TIMED || w == RD_CLOCK) pthread_rwlock_wrlock(&rw);
    if (w == WR_TIMED || w == WR_CLOCK) pthread_rwlock_rdlock(&rw);
    if (w == JOIN) pthread_create(&other, 0, sleeper, 0);
    sigemptyset(&blk); sigaddset(&blk, SIGUSR2);
    if (w == SIGWAIT || w == SIGTIMED) pthread_sigmask(SIG_BLOCK, &blk, 0); /* inherited by the waiter */
    pthread_create(&waiter, 0, body, 0);
    if (w == SIGWAIT || w == SIGTIMED) pthread_sigmask(SIG_UNBLOCK, &blk, 0);
    for (int i = 0; i < 4000 && !ready; i++) ms(1);
    int blocked = 0;
    for (int i = 0; i < 4000 && !blocked; i++) {
        char p[64], b[256]; snprintf(p, sizeof p, "/proc/self/task/%d/stat", wtid);
        FILE *f = fopen(p, "r");
        if (f) { size_t n = fread(b, 1, sizeof b - 1, f); b[n] = 0; fclose(f); char *q = strrchr(b, ')'); if (q && q[2] == 'S') blocked = 1; }
        if (!blocked) ms(1);
    }
    ms(20);
    pthread_kill(waiter, SIGUSR1);
    for (int i = 0; i < 2000 && !hits; i++) ms(1);
    for (int i = 0; i < 300 && !done; i++) ms(1);
    int early = done, hn = hits;
    printf("%s restart=%d: ", names[w], restart);
    if (early) printf("returned rc=%d errno=%s handler=%d\n", wrc, en(werr), hn);
    else printf("stayed blocked handler=%d\n", hn);
    /* release */
    switch (w) {
    case SEM_WAIT: case SEM_TIMED: case SEM_CLOCK: sem_post(&sem); break;
    case COND_WAIT: case COND_TIMED: pthread_mutex_lock(&cmx); pthread_cond_broadcast(&cv); pthread_mutex_unlock(&cmx); break;
    case MUTEX_TIMED: case MUTEX_CLOCK: pthread_mutex_unlock(&mx); break;
    case RD_TIMED: case RD_CLOCK: case WR_TIMED: case WR_CLOCK: pthread_rwlock_unlock(&rw); break;
    case BARRIER: pthread_barrier_wait(&bar); break;
    case JOIN: sem_post(&sem2); break;
    case SIGWAIT: case SIGTIMED: pthread_kill(waiter, SIGUSR2); break;
    case SIGSUSP: case PAUSE: pthread_kill(waiter, SIGUSR1); break;
    }
    pthread_join(waiter, 0);
    if (w == JOIN && early) pthread_join(other, 0); /* the waiter's join did not take it */
}

int main(void) {
    alarm(120);
    for (int w = 0; w < N; w++) for (int r = 0; r < 2; r++) run(w, r);
    return 0;
}
