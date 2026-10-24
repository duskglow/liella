/* Spec §22.4, §22.5, §22.7: the synchronisation objects from C, with glibc's headers and static
 * initialisers. A robust mutex whose owner thread exits reports EOWNERDEAD (the kernel walks
 * the robust list Liella registers per thread), then ENOTRECOVERABLE if not made consistent;
 * priority-inheritance mutexes exclude; the _NP initialisers give their types; condition
 * variables, read-write locks, barriers, once, semaphores (unnamed and named), spin locks and
 * the C11 forms work across threads. */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <pthread.h>
#include <semaphore.h>
#include <stdint.h>
#include <threads.h>
#include <time.h>
#include <unistd.h>

static pthread_mutex_t robust;
static void *die_holding(void *arg)
{
    (void)arg;
    if (pthread_mutex_lock(&robust) != 0)
        return (void *)1;
    return NULL; /* exits holding it */
}

static pthread_mutex_t pi;
static long pi_count;
static void *pi_worker(void *arg)
{
    (void)arg;
    for (int i = 0; i < 20000; i++) {
        pthread_mutex_lock(&pi);
        pi_count++;
        pthread_mutex_unlock(&pi);
    }
    return NULL;
}

static pthread_mutex_t m = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t cv = PTHREAD_COND_INITIALIZER;
static int ready, served;
static void *consumer(void *arg)
{
    (void)arg;
    pthread_mutex_lock(&m);
    while (!ready)
        pthread_cond_wait(&cv, &m);
    served++;
    pthread_mutex_unlock(&m);
    return NULL;
}

static pthread_rwlock_t rw = PTHREAD_RWLOCK_WRITER_NONRECURSIVE_INITIALIZER_NP;
static pthread_barrier_t bar;
static int serial;
static void *at_barrier(void *arg)
{
    (void)arg;
    for (int i = 0; i < 100; i++)
        if (pthread_barrier_wait(&bar) == PTHREAD_BARRIER_SERIAL_THREAD)
            __atomic_fetch_add(&serial, 1, __ATOMIC_SEQ_CST);
    return NULL;
}

static pthread_once_t once = PTHREAD_ONCE_INIT;
static int once_runs;
static void init_once(void) { once_runs++; }
static void *do_once(void *arg)
{
    (void)arg;
    pthread_once(&once, init_once);
    return NULL;
}

static sem_t sem;
static void *poster(void *arg)
{
    (void)arg;
    for (int i = 0; i < 1000; i++)
        sem_post(&sem);
    return NULL;
}

static once_flag c11_once = ONCE_FLAG_INIT;
static int c11_runs;
static void c11_init(void) { c11_runs++; }
static tss_t tkey;
static int tss_freed;
/* Atomic: the three threads run their destructors concurrently as they exit. */
static void tss_dtor(void *v) { __atomic_fetch_add(&tss_freed, (int)(intptr_t)v, __ATOMIC_RELAXED); }
static int c11_thread(void *arg)
{
    call_once(&c11_once, c11_init);
    tss_set(tkey, (void *)(intptr_t)3);
    return arg == NULL ? 0 : 1;
}

int main(void)
{
    /* Robust: the owner thread exits holding it. */
    pthread_mutexattr_t ma;
    pthread_mutexattr_init(&ma);
    if (pthread_mutexattr_setrobust(&ma, PTHREAD_MUTEX_ROBUST) != 0)
        return 1;
    pthread_mutex_init(&robust, &ma);
    pthread_t t;
    pthread_create(&t, NULL, die_holding, NULL);
    pthread_join(t, NULL);
    if (pthread_mutex_lock(&robust) != EOWNERDEAD)
        return 2;
    if (pthread_mutex_unlock(&robust) != 0)
        return 3; /* unlocked without pthread_mutex_consistent */
    if (pthread_mutex_lock(&robust) != ENOTRECOVERABLE)
        return 4;
    pthread_mutex_destroy(&robust);
    pthread_mutex_init(&robust, &ma);
    pthread_create(&t, NULL, die_holding, NULL);
    pthread_join(t, NULL);
    if (pthread_mutex_lock(&robust) != EOWNERDEAD || pthread_mutex_consistent(&robust) != 0)
        return 5;
    if (pthread_mutex_unlock(&robust) != 0 || pthread_mutex_lock(&robust) != 0 || pthread_mutex_unlock(&robust) != 0)
        return 6; /* consistent again */

    /* Priority inheritance. */
    pthread_mutexattr_init(&ma);
    if (pthread_mutexattr_setprotocol(&ma, PTHREAD_PRIO_INHERIT) != 0)
        return 7;
    pthread_mutex_init(&pi, &ma);
    pthread_t pw[4];
    for (int i = 0; i < 4; i++)
        pthread_create(&pw[i], NULL, pi_worker, NULL);
    for (int i = 0; i < 4; i++)
        pthread_join(pw[i], NULL);
    if (pi_count != 80000)
        return 8;
    if (pthread_mutex_lock(&pi) != 0 || pthread_mutex_lock(&pi) != EDEADLK || pthread_mutex_unlock(&pi) != 0)
        return 9;

    /* Static initialisers of the other types. */
    pthread_mutex_t rec = PTHREAD_RECURSIVE_MUTEX_INITIALIZER_NP;
    pthread_mutex_t chk = PTHREAD_ERRORCHECK_MUTEX_INITIALIZER_NP;
    if (pthread_mutex_lock(&rec) != 0 || pthread_mutex_lock(&rec) != 0 || pthread_mutex_unlock(&rec) != 0 || pthread_mutex_unlock(&rec) != 0)
        return 10;
    if (pthread_mutex_lock(&chk) != 0 || pthread_mutex_lock(&chk) != EDEADLK || pthread_mutex_unlock(&chk) != 0 || pthread_mutex_unlock(&chk) != EPERM)
        return 11;
    /* trylock by the owner is EBUSY for every type but the recursive one (POSIX: "locked by any
     * thread, including the current thread"; validation T19). */
    if (pthread_mutex_lock(&chk) != 0 || pthread_mutex_trylock(&chk) != EBUSY || pthread_mutex_unlock(&chk) != 0)
        return 111;
    if (pthread_mutex_lock(&rec) != 0 || pthread_mutex_trylock(&rec) != 0 || pthread_mutex_unlock(&rec) != 0 || pthread_mutex_unlock(&rec) != 0)
        return 112;
    struct timespec past = {0, 0};
    pthread_mutex_lock(&m);
    pthread_t tl;
    if (pthread_mutex_timedlock(&m, &past) != EDEADLK && pthread_mutex_trylock(&m) != EBUSY)
        return 12;
    pthread_mutex_unlock(&m);
    (void)tl;

    /* Condition variable: broadcast reaches every waiter. */
    pthread_t cw[5];
    for (int i = 0; i < 5; i++)
        pthread_create(&cw[i], NULL, consumer, NULL);
    usleep(20000);
    pthread_mutex_lock(&m);
    ready = 1;
    pthread_cond_broadcast(&cv);
    pthread_mutex_unlock(&m);
    for (int i = 0; i < 5; i++)
        pthread_join(cw[i], NULL);
    if (served != 5)
        return 13;
    pthread_condattr_t ca;
    pthread_condattr_init(&ca);
    pthread_condattr_setclock(&ca, CLOCK_MONOTONIC);
    pthread_cond_t mono;
    pthread_cond_init(&mono, &ca);
    struct timespec soon;
    clock_gettime(CLOCK_MONOTONIC, &soon);
    soon.tv_nsec += 20000000;
    if (soon.tv_nsec >= 1000000000) {
        soon.tv_sec++;
        soon.tv_nsec -= 1000000000;
    }
    pthread_mutex_lock(&m);
    if (pthread_cond_timedwait(&mono, &m, &soon) != ETIMEDOUT)
        return 14;
    pthread_mutex_unlock(&m);

    /* Read-write lock with the writer-preferring static initialiser. */
    if (pthread_rwlock_rdlock(&rw) != 0 || pthread_rwlock_trywrlock(&rw) != EBUSY || pthread_rwlock_unlock(&rw) != 0)
        return 30;
    if (pthread_rwlock_wrlock(&rw) != 0 || pthread_rwlock_wrlock(&rw) != EDEADLK || pthread_rwlock_unlock(&rw) != 0)
        return 15;

    /* Barrier: one serial thread per round. */
    pthread_barrier_init(&bar, NULL, 4);
    pthread_t bw[4];
    for (int i = 0; i < 4; i++)
        pthread_create(&bw[i], NULL, at_barrier, NULL);
    for (int i = 0; i < 4; i++)
        pthread_join(bw[i], NULL);
    if (serial != 100)
        return 16;

    /* Once. */
    pthread_t ow[6];
    for (int i = 0; i < 6; i++)
        pthread_create(&ow[i], NULL, do_once, NULL);
    for (int i = 0; i < 6; i++)
        pthread_join(ow[i], NULL);
    if (once_runs != 1)
        return 17;

    /* Unnamed semaphore across threads. */
    sem_init(&sem, 0, 0);
    pthread_t sp;
    pthread_create(&sp, NULL, poster, NULL);
    for (int i = 0; i < 1000; i++)
        if (sem_wait(&sem) != 0)
            return 18;
    pthread_join(sp, NULL);
    int v;
    if (sem_getvalue(&sem, &v) != 0 || v != 0 || sem_trywait(&sem) != -1 || errno != EAGAIN)
        return 19;

    /* Named semaphore: same address twice, O_EXCL, unlink. */
    char name[64] = "/liella-conf-";
    {
        /* the PID in decimal (no stdio before phase 8) */
        char digits[16];
        int nd = 0;
        for (unsigned p = (unsigned)getpid(); p != 0 || nd == 0; p /= 10)
            digits[nd++] = (char)('0' + p % 10);
        int at = 13;
        while (nd > 0)
            name[at++] = digits[--nd];
        name[at] = 0;
    }
    sem_t *a = sem_open(name, O_CREAT | O_EXCL, 0600, 2);
    if (a == SEM_FAILED)
        return 20;
    sem_t *b = sem_open(name, 0);
    if (b != a)
        return 21;
    if (sem_open(name, O_CREAT | O_EXCL, 0600, 0) != SEM_FAILED || errno != EEXIST)
        return 22;
    if (sem_trywait(a) != 0 || sem_getvalue(b, &v) != 0 || v != 1)
        return 23;
    if (sem_close(b) != 0 || sem_close(a) != 0 || sem_unlink(name) != 0)
        return 24;
    if (sem_open(name, 0) != SEM_FAILED || errno != ENOENT)
        return 25;

    /* Spin lock. */
    pthread_spinlock_t sl;
    if (pthread_spin_init(&sl, PTHREAD_PROCESS_PRIVATE) != 0 || pthread_spin_lock(&sl) != 0 || pthread_spin_trylock(&sl) != EBUSY || pthread_spin_unlock(&sl) != 0)
        return 26;

    /* C11. */
    mtx_t cm;
    if (mtx_init(&cm, mtx_plain | mtx_recursive) != thrd_success || mtx_lock(&cm) != thrd_success || mtx_lock(&cm) != thrd_success)
        return 27;
    mtx_unlock(&cm);
    mtx_unlock(&cm);
    if (tss_create(&tkey, tss_dtor) != thrd_success)
        return 28;
    thrd_t ct[3];
    for (int i = 0; i < 3; i++)
        thrd_create(&ct[i], c11_thread, NULL);
    for (int i = 0; i < 3; i++) {
        int r;
        thrd_join(ct[i], &r);
    }
    if (c11_runs != 1 || tss_freed != 9)
        return 29;
    return 0;
}
