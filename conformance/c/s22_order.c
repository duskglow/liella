/* Spec §22.5 and §22.6, two ordering guarantees.
 * 1. A condition-variable signal wakes only a waiter that blocked before it (glibc 2.25+): a
 *    waiter that arrives after the signal cannot take it from the one already waiting.
 * 2. At thread exit, key destructors run for non-NULL values, repeated while values remain, up
 *    to PTHREAD_DESTRUCTOR_ITERATIONS (4) rounds; a NULL value's destructor is not called. */
#define _GNU_SOURCE
#include <errno.h>
#include <limits.h>
#include <pthread.h>
#include <time.h>

enum { ROUNDS = 300 };
static pthread_mutex_t m = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t c = PTHREAD_COND_INITIALIZER;
static int waiting_a, waiting_b;

static struct timespec in(long ms)
{
    struct timespec ts;
    clock_gettime(CLOCK_REALTIME, &ts);
    ts.tv_sec += ms / 1000;
    ts.tv_nsec += (ms % 1000) * 1000000L;
    if (ts.tv_nsec >= 1000000000L) {
        ts.tv_sec++;
        ts.tv_nsec -= 1000000000L;
    }
    return ts;
}

static void nap(void)
{
    struct timespec ts = {0, 10000};
    nanosleep(&ts, NULL);
}

static void *early(void *arg)
{
    (void)arg;
    pthread_mutex_lock(&m);
    waiting_a = 1;
    struct timespec t = in(5000);
    int r = pthread_cond_timedwait(&c, &m, &t);
    pthread_mutex_unlock(&m);
    return (void *)(long)r;
}

static void *late(void *arg)
{
    (void)arg;
    pthread_mutex_lock(&m);
    waiting_b = 1;
    struct timespec t = in(5000);
    int r = pthread_cond_timedwait(&c, &m, &t); /* released by the broadcast below */
    pthread_mutex_unlock(&m);
    return (void *)(long)r;
}

static int signal_order(void)
{
    for (int round = 0; round < ROUNDS; round++) {
        waiting_a = waiting_b = 0;
        pthread_t a, b;
        pthread_create(&a, NULL, early, NULL);
        for (;;) { /* A holds m until it waits: seeing its flag under m means it is waiting */
            pthread_mutex_lock(&m);
            if (waiting_a)
                break;
            pthread_mutex_unlock(&m);
            nap(); /* block: on one core the waiter runs only when this thread sleeps */
        }
        pthread_cond_signal(&c); /* m held: A is the only waiter */
        pthread_create(&b, NULL, late, NULL);
        pthread_mutex_unlock(&m); /* A and B now race for m; B blocks after the signal */
        void *ra;
        pthread_join(a, &ra);
        if ((long)ra != 0)
            return 10; /* A timed out: the signal went to B */
        for (;;) {
            pthread_mutex_lock(&m);
            if (waiting_b)
                break;
            pthread_mutex_unlock(&m);
            nap(); /* block: on one core the waiter runs only when this thread sleeps */
        }
        pthread_cond_broadcast(&c);
        pthread_mutex_unlock(&m);
        pthread_join(b, NULL);
    }
    return 0;
}

/* --- destructor rounds ----------------------------------------------------------------------- */
static pthread_key_t again, empty;
static int again_calls, empty_calls;

static void reset_value(void *v)
{
    again_calls++;
    pthread_setspecific(again, v); /* non-NULL again: another round, if rounds remain */
}

static void never(void *v)
{
    (void)v;
    empty_calls++;
}

static void *exiting(void *arg)
{
    pthread_setspecific(again, arg);
    pthread_setspecific(empty, NULL);
    return NULL;
}

static int destructor_rounds(void)
{
    pthread_key_create(&again, reset_value);
    pthread_key_create(&empty, never);
    pthread_t t;
    static int token;
    pthread_create(&t, NULL, exiting, &token);
    pthread_join(t, NULL);
    if (again_calls != PTHREAD_DESTRUCTOR_ITERATIONS)
        return 20 + again_calls;
    if (empty_calls != 0)
        return 30;
    return 0;
}

int main(void)
{
    int r;
    if ((r = signal_order()) != 0)
        return r;
    if (PTHREAD_DESTRUCTOR_ITERATIONS != 4)
        return 2;
    return destructor_rounds();
}
