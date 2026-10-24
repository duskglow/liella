/* Spec §22.5 (s22_5_c_1_2, LG-3, D-22.13): the pre-2.3.2 condition variables across real threads.
 * The `pthread_cond_*@GLIBC_2.2.5` versions are `.symver` stubs in libc.so.6 only (R15-2); this
 * static program calls the translations behind them, `liella_compat_pthread_cond_*`, on the old
 * one-word object, from Liella's own threads. A lost wakeup leaves a waiter blocked forever: the
 * alarm then kills the program and the test fails. */
#define _GNU_SOURCE
#include <errno.h>
#include <pthread.h>
#include <stdint.h>
#include <time.h>
#include <unistd.h>

typedef unsigned long oldcond; /* the old object: one 8-byte word */
extern int liella_compat_pthread_cond_init(oldcond *, const pthread_condattr_t *);
extern int liella_compat_pthread_cond_destroy(oldcond *);
extern int liella_compat_pthread_cond_wait(oldcond *, pthread_mutex_t *);
extern int liella_compat_pthread_cond_timedwait(oldcond *, pthread_mutex_t *, const struct timespec *);
extern int liella_compat_pthread_cond_signal(oldcond *);
extern int liella_compat_pthread_cond_broadcast(oldcond *);

#define WORKERS 4
#define ITEMS 20000
#define ROUNDS 2000

static pthread_mutex_t m = PTHREAD_MUTEX_INITIALIZER;
static oldcond nonempty, gen_changed, arrived;
static long queued, consumed, generation, waiting;
static int done;

/* Signal only: each item wakes at most one consumer, so a lost signal strands one. */
static void *consumer(void *arg)
{
    long mine = 0;
    (void)arg;
    pthread_mutex_lock(&m);
    for (;;) {
        while (queued == 0 && !done)
            if (liella_compat_pthread_cond_wait(&nonempty, &m) != 0)
                return (void *)-1;
        if (queued == 0)
            break;
        queued--;
        consumed++;
        mine++;
    }
    pthread_mutex_unlock(&m);
    return (void *)mine;
}

/* Broadcast: every worker waits for each new generation. */
static void *round_worker(void *arg)
{
    (void)arg;
    pthread_mutex_lock(&m);
    for (long seen = 0; seen < ROUNDS;) {
        waiting++;
        liella_compat_pthread_cond_signal(&arrived);
        while (generation == seen)
            if (liella_compat_pthread_cond_wait(&gen_changed, &m) != 0)
                return (void *)1;
        seen = generation;
    }
    pthread_mutex_unlock(&m);
    return NULL;
}

/* Timedwait woken by a signal well before a far deadline. */
static oldcond tw;
static int tw_ready, tw_go;
static void *timed_waiter(void *arg)
{
    struct timespec ts;
    int r = 0;
    (void)arg;
    clock_gettime(CLOCK_REALTIME, &ts);
    ts.tv_sec += 60;
    pthread_mutex_lock(&m);
    tw_ready++;
    liella_compat_pthread_cond_signal(&arrived);
    while (!tw_go && r == 0)
        r = liella_compat_pthread_cond_timedwait(&tw, &m, &ts);
    pthread_mutex_unlock(&m);
    return (void *)(intptr_t)r;
}

int main(void)
{
    pthread_t t[WORKERS];
    void *res;
    alarm(60); /* a lost wakeup hangs: SIGALRM fails the test */

    if (liella_compat_pthread_cond_init(&nonempty, NULL) || liella_compat_pthread_cond_init(&gen_changed, NULL)
        || liella_compat_pthread_cond_init(&arrived, NULL) || liella_compat_pthread_cond_init(&tw, NULL))
        return 1;

    /* 1. producer/consumer through signal */
    for (int i = 0; i < WORKERS; i++)
        if (pthread_create(&t[i], NULL, consumer, NULL))
            return 2;
    for (long i = 0; i < ITEMS; i++) {
        pthread_mutex_lock(&m);
        queued++;
        liella_compat_pthread_cond_signal(&nonempty);
        pthread_mutex_unlock(&m);
        if ((i & 63) == 0)
            sched_yield();
    }
    pthread_mutex_lock(&m);
    done = 1;
    liella_compat_pthread_cond_broadcast(&nonempty);
    pthread_mutex_unlock(&m);
    long total = 0;
    for (int i = 0; i < WORKERS; i++) {
        if (pthread_join(t[i], &res) || (long)res < 0)
            return 3;
        total += (long)res;
    }
    if (total != ITEMS || consumed != ITEMS)
        return 4;

    /* 2. generations through broadcast: all WORKERS wait, then one broadcast releases them */
    for (int i = 0; i < WORKERS; i++)
        if (pthread_create(&t[i], NULL, round_worker, NULL))
            return 5;
    pthread_mutex_lock(&m);
    for (long g = 1; g <= ROUNDS; g++) {
        while (waiting < WORKERS)
            liella_compat_pthread_cond_wait(&arrived, &m);
        waiting = 0;
        generation = g;
        liella_compat_pthread_cond_broadcast(&gen_changed);
    }
    pthread_mutex_unlock(&m);
    for (int i = 0; i < WORKERS; i++)
        if (pthread_join(t[i], &res) || res != NULL)
            return 6;

    /* 3. timedwait: woken by signal before a 60 s deadline (0); a past deadline gives ETIMEDOUT */
    for (int i = 0; i < WORKERS; i++)
        if (pthread_create(&t[i], NULL, timed_waiter, NULL))
            return 7;
    pthread_mutex_lock(&m);
    while (tw_ready < WORKERS)
        liella_compat_pthread_cond_wait(&arrived, &m);
    tw_go = 1;
    for (int i = 0; i < WORKERS; i++)
        liella_compat_pthread_cond_signal(&tw); /* one each: a lost one strands a waiter for 60 s */
    pthread_mutex_unlock(&m);
    for (int i = 0; i < WORKERS; i++)
        if (pthread_join(t[i], &res) || (intptr_t)res != 0)
            return 8;
    struct timespec past = {0, 0};
    pthread_mutex_lock(&m);
    int r = liella_compat_pthread_cond_timedwait(&tw, &m, &past);
    pthread_mutex_unlock(&m);
    if (r != ETIMEDOUT)
        return 9;

    /* 4. destroy: 0, and the word is a static initialiser again (D-22.13) */
    if (liella_compat_pthread_cond_destroy(&nonempty) || liella_compat_pthread_cond_destroy(&gen_changed)
        || liella_compat_pthread_cond_destroy(&arrived) || liella_compat_pthread_cond_destroy(&tw))
        return 10;
    if (nonempty || gen_changed || arrived || tw)
        return 11;
    return 0;
}
