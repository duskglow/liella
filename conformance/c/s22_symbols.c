/* Spec §22 and §21: every thread-library symbol that no scenario program calls, each through
 * its exported name with a check of its contract. The glibc-internal and alias names have no
 * header declaration; they are declared here under their symbol names. */
#define _GNU_SOURCE
#include <errno.h>
#include <pthread.h>
#include <sched.h>
#include <semaphore.h>
#include <setjmp.h>
#include <signal.h>
#include <stdlib.h>
#include <string.h>
#include <threads.h>
#include <time.h>
#include <unistd.h>

/* Alias and internal names. */
extern int a_mutex_init(pthread_mutex_t *, const pthread_mutexattr_t *) __asm__("__pthread_mutex_init");
extern int a_mutex_lock(pthread_mutex_t *) __asm__("__pthread_mutex_lock");
extern int a_mutex_trylock(pthread_mutex_t *) __asm__("__pthread_mutex_trylock");
extern int a_mutex_unlock(pthread_mutex_t *) __asm__("__pthread_mutex_unlock");
extern int a_mutex_destroy(pthread_mutex_t *) __asm__("__pthread_mutex_destroy");
extern int a_mutexattr_init(pthread_mutexattr_t *) __asm__("__pthread_mutexattr_init");
extern int a_mutexattr_settype(pthread_mutexattr_t *, int) __asm__("__pthread_mutexattr_settype");
extern int a_mutexattr_destroy(pthread_mutexattr_t *) __asm__("__pthread_mutexattr_destroy");
extern int a_mutexattr_setkind_np(pthread_mutexattr_t *, int) __asm__("pthread_mutexattr_setkind_np");
extern int a_mutexattr_getkind_np(const pthread_mutexattr_t *, int *) __asm__("pthread_mutexattr_getkind_np");
extern int a_mutexattr_setrobust_np(pthread_mutexattr_t *, int) __asm__("pthread_mutexattr_setrobust_np");
extern int a_mutexattr_getrobust_np(const pthread_mutexattr_t *, int *) __asm__("pthread_mutexattr_getrobust_np");
extern int a_mutex_consistent_np(pthread_mutex_t *) __asm__("pthread_mutex_consistent_np");
extern int a_rwlock_init(pthread_rwlock_t *, const pthread_rwlockattr_t *) __asm__("__pthread_rwlock_init");
extern int a_rwlock_rdlock(pthread_rwlock_t *) __asm__("__pthread_rwlock_rdlock");
extern int a_rwlock_wrlock(pthread_rwlock_t *) __asm__("__pthread_rwlock_wrlock");
extern int a_rwlock_tryrdlock(pthread_rwlock_t *) __asm__("__pthread_rwlock_tryrdlock");
extern int a_rwlock_trywrlock(pthread_rwlock_t *) __asm__("__pthread_rwlock_trywrlock");
extern int a_rwlock_unlock(pthread_rwlock_t *) __asm__("__pthread_rwlock_unlock");
extern int a_rwlock_destroy(pthread_rwlock_t *) __asm__("__pthread_rwlock_destroy");
extern int a_key_create(pthread_key_t *, void (*)(void *)) __asm__("__pthread_key_create");
extern int a_setspecific(pthread_key_t, const void *) __asm__("__pthread_setspecific");
extern void *a_getspecific(pthread_key_t) __asm__("__pthread_getspecific");
extern int a_once(pthread_once_t *, void (*)(void)) __asm__("__pthread_once");
extern int a_yield(void) __asm__("pthread_yield");
extern void a_kill_other_threads_np(void) __asm__("pthread_kill_other_threads_np");
extern void a_cleanup_push(struct _pthread_cleanup_buffer *, void (*)(void *), void *) __asm__("_pthread_cleanup_push");
extern void a_cleanup_pop(struct _pthread_cleanup_buffer *, int) __asm__("_pthread_cleanup_pop");
extern void a_cleanup_push_defer(struct _pthread_cleanup_buffer *, void (*)(void *), void *) __asm__("_pthread_cleanup_push_defer");
extern void a_cleanup_pop_restore(struct _pthread_cleanup_buffer *, int) __asm__("_pthread_cleanup_pop_restore");
struct cleanup_frame { void (*routine)(void *); void *arg; int do_it; int cancel_type; };
extern void a_cleanup_routine(struct cleanup_frame *) __asm__("__pthread_cleanup_routine");
extern int a_setjmp(jmp_buf) __asm__("setjmp") __attribute__((returns_twice));
extern void a_longjmp(jmp_buf, int) __asm__("_longjmp") __attribute__((noreturn));
extern const unsigned int __rseq_flags;
/* Under _FORTIFY_SOURCE (the distribution compilers' default) longjmp and siglongjmp become
 * __longjmp_chk, and pthread_equal/thrd_equal are header inlines: reach the symbols by name. */
extern void p_longjmp(jmp_buf, int) __asm__("longjmp") __attribute__((noreturn));
extern void p_siglongjmp(sigjmp_buf, int) __asm__("siglongjmp") __attribute__((noreturn));
extern int p_pthread_equal(pthread_t, pthread_t) __asm__("pthread_equal");
extern int p_thrd_equal(thrd_t, thrd_t) __asm__("thrd_equal");

static struct timespec past(clockid_t c)
{
    struct timespec ts;
    clock_gettime(c, &ts);
    ts.tv_sec -= 1;
    return ts;
}

/* --- locks held by another thread: the timed forms time out ------------------------------ */
static pthread_mutex_t held_m = PTHREAD_MUTEX_INITIALIZER;
static pthread_rwlock_t held_rw = PTHREAD_RWLOCK_INITIALIZER;
static mtx_t held_c11;
static int timed_results[8];

static void *try_held(void *arg)
{
    (void)arg;
    struct timespec rt = past(CLOCK_REALTIME), mono = past(CLOCK_MONOTONIC);
    timed_results[0] = pthread_mutex_clocklock(&held_m, CLOCK_MONOTONIC, &mono);
    timed_results[1] = pthread_rwlock_clockrdlock(&held_rw, CLOCK_MONOTONIC, &mono);
    timed_results[2] = pthread_rwlock_clockwrlock(&held_rw, CLOCK_MONOTONIC, &mono);
    timed_results[3] = pthread_rwlock_timedrdlock(&held_rw, &rt);
    timed_results[4] = pthread_rwlock_timedwrlock(&held_rw, &rt);
    timed_results[5] = mtx_trylock(&held_c11);
    timed_results[6] = mtx_timedlock(&held_c11, &rt);
    timed_results[7] = a_rwlock_tryrdlock(&held_rw);
    return NULL;
}

static int locks_held_elsewhere(void)
{
    if (mtx_init(&held_c11, mtx_timed) != thrd_success)
        return 1;
    pthread_mutex_lock(&held_m);
    pthread_rwlock_wrlock(&held_rw);
    mtx_lock(&held_c11);
    pthread_t t;
    pthread_create(&t, NULL, try_held, NULL);
    pthread_join(t, NULL);
    const int want[8] = {ETIMEDOUT, ETIMEDOUT, ETIMEDOUT, ETIMEDOUT, ETIMEDOUT, thrd_busy, thrd_timedout, EBUSY};
    for (int i = 0; i < 8; i++)
        if (timed_results[i] != want[i])
            return 10 + i;
    pthread_mutex_unlock(&held_m);
    pthread_rwlock_unlock(&held_rw);
    mtx_unlock(&held_c11);
    /* Free now: each succeeds at once despite the past deadline. */
    struct timespec rt = past(CLOCK_REALTIME), mono = past(CLOCK_MONOTONIC);
    if (pthread_mutex_clocklock(&held_m, CLOCK_MONOTONIC, &mono) != 0 || pthread_mutex_unlock(&held_m) != 0)
        return 20;
    if (pthread_rwlock_clockrdlock(&held_rw, CLOCK_MONOTONIC, &mono) != 0 || pthread_rwlock_unlock(&held_rw) != 0)
        return 21;
    if (pthread_rwlock_clockwrlock(&held_rw, CLOCK_MONOTONIC, &mono) != 0 || pthread_rwlock_unlock(&held_rw) != 0)
        return 22;
    if (pthread_rwlock_timedrdlock(&held_rw, &rt) != 0 || pthread_rwlock_unlock(&held_rw) != 0)
        return 23;
    if (pthread_rwlock_timedwrlock(&held_rw, &rt) != 0 || pthread_rwlock_unlock(&held_rw) != 0)
        return 24;
    if (pthread_rwlock_tryrdlock(&held_rw) != 0 || pthread_rwlock_unlock(&held_rw) != 0)
        return 25;
    if (mtx_timedlock(&held_c11, &rt) != thrd_success || mtx_unlock(&held_c11) != thrd_success)
        return 26;
    if (mtx_trylock(&held_c11) != thrd_success || mtx_unlock(&held_c11) != thrd_success)
        return 27;
    mtx_destroy(&held_c11);
    if (pthread_rwlock_destroy(&held_rw) != 0)
        return 28;
    return 0;
}

/* --- attributes ----------------------------------------------------------------------------- */
static void *report_frame(void *arg)
{
    int local;
    *(char **)arg = (char *)&local;
    return NULL;
}

static int attributes(void)
{
    pthread_attr_t a;
    pthread_attr_init(&a);
    int v;
    if (pthread_attr_getdetachstate(&a, &v) != 0 || v != PTHREAD_CREATE_JOINABLE)
        return 1;
    if (pthread_attr_getinheritsched(&a, &v) != 0 || v != PTHREAD_INHERIT_SCHED)
        return 2;
    if (pthread_attr_getschedpolicy(&a, &v) != 0 || v != SCHED_OTHER)
        return 3;
    if (pthread_attr_getscope(&a, &v) != 0 || v != PTHREAD_SCOPE_SYSTEM)
        return 4;
    if (pthread_attr_setscope(&a, PTHREAD_SCOPE_PROCESS) != ENOTSUP)
        return 5;
    struct sched_param sp = {.sched_priority = 7};
    if (pthread_attr_getschedparam(&a, &sp) != 0 || sp.sched_priority != 0)
        return 6;
    /* Affinity: what is set comes back. */
    cpu_set_t set, got;
    CPU_ZERO(&set);
    CPU_SET(0, &set);
    if (pthread_attr_setaffinity_np(&a, sizeof set, &set) != 0)
        return 7;
    if (pthread_attr_getaffinity_np(&a, sizeof got, &got) != 0 || !CPU_EQUAL(&set, &got))
        return 8;
    /* Signal mask: none set is reported as such; a set mask comes back. */
    sigset_t m;
    if (pthread_attr_getsigmask_np(&a, &m) != PTHREAD_ATTR_NO_SIGMASK_NP)
        return 9;
    sigemptyset(&m);
    sigaddset(&m, SIGUSR1);
    if (pthread_attr_setsigmask_np(&a, &m) != 0)
        return 10;
    sigemptyset(&m);
    if (pthread_attr_getsigmask_np(&a, &m) != 0 || !sigismember(&m, SIGUSR1) || sigismember(&m, SIGUSR2))
        return 11;
    pthread_attr_destroy(&a);

    /* A caller-supplied stack: the thread runs on it. */
    size_t sz = 256 * 1024;
    char *stack = aligned_alloc(4096, sz);
    pthread_attr_init(&a);
    if (pthread_attr_setstack(&a, stack, sz) != 0)
        return 12;
    void *addr;
    size_t got_sz;
    if (pthread_attr_getstack(&a, &addr, &got_sz) != 0 || addr != stack || got_sz != sz)
        return 13;
    char *frame = NULL;
    pthread_t t;
    if (pthread_create(&t, &a, report_frame, &frame) != 0)
        return 14;
    pthread_join(t, NULL);
    if (frame < stack || frame >= stack + sz)
        return 15;
    pthread_attr_destroy(&a);
    free(stack);
    /* The obsolete stack-address form: the top of the stack (stacks grow down). */
    pthread_attr_init(&a);
    if (pthread_attr_setstackaddr(&a, (void *)0x10000) != 0)
        return 16;
    if (pthread_attr_getstackaddr(&a, &addr) != 0 || addr != (void *)0x10000)
        return 17;
    pthread_attr_destroy(&a);

    /* Process defaults: a new default stack size applies to later threads. */
    pthread_attr_t d, saved;
    pthread_getattr_default_np(&saved);
    pthread_attr_init(&d);
    pthread_attr_setstacksize(&d, 1 << 20);
    if (pthread_setattr_default_np(&d) != 0)
        return 18;
    pthread_attr_destroy(&d);
    pthread_getattr_default_np(&d);
    size_t dsz;
    pthread_attr_getstacksize(&d, &dsz);
    if (dsz != 1u << 20)
        return 19;
    pthread_attr_destroy(&d);
    if (pthread_setattr_default_np(&saved) != 0)
        return 20;
    pthread_attr_destroy(&saved);

    /* Mutex attributes, including the alias and _np names. */
    pthread_mutexattr_t ma;
    if (a_mutexattr_init(&ma) != 0)
        return 21;
    if (pthread_mutexattr_gettype(&ma, &v) != 0 || v != PTHREAD_MUTEX_NORMAL)
        return 22;
    if (pthread_mutexattr_settype(&ma, PTHREAD_MUTEX_RECURSIVE) != 0 || pthread_mutexattr_gettype(&ma, &v) != 0 || v != PTHREAD_MUTEX_RECURSIVE)
        return 23;
    if (a_mutexattr_settype(&ma, PTHREAD_MUTEX_ERRORCHECK) != 0 || a_mutexattr_getkind_np(&ma, &v) != 0 || v != PTHREAD_MUTEX_ERRORCHECK)
        return 24;
    if (a_mutexattr_setkind_np(&ma, PTHREAD_MUTEX_NORMAL) != 0 || pthread_mutexattr_gettype(&ma, &v) != 0 || v != PTHREAD_MUTEX_NORMAL)
        return 25;
    if (pthread_mutexattr_settype(&ma, 99) != EINVAL)
        return 26;
    if (pthread_mutexattr_getprotocol(&ma, &v) != 0 || v != PTHREAD_PRIO_NONE)
        return 27;
    if (pthread_mutexattr_getpshared(&ma, &v) != 0 || v != PTHREAD_PROCESS_PRIVATE)
        return 28;
    if (pthread_mutexattr_getrobust(&ma, &v) != 0 || v != PTHREAD_MUTEX_STALLED)
        return 29;
    if (a_mutexattr_setrobust_np(&ma, PTHREAD_MUTEX_ROBUST) != 0 || a_mutexattr_getrobust_np(&ma, &v) != 0 || v != PTHREAD_MUTEX_ROBUST)
        return 30;
    if (pthread_mutexattr_getrobust(&ma, &v) != 0 || v != PTHREAD_MUTEX_ROBUST)
        return 31;
    /* consistent_np on a robust mutex that is not inconsistent: EINVAL. */
    pthread_mutex_t rm;
    pthread_mutex_init(&rm, &ma);
    pthread_mutex_lock(&rm);
    if (a_mutex_consistent_np(&rm) != EINVAL)
        return 32;
    pthread_mutex_unlock(&rm);
    pthread_mutex_destroy(&rm);
    if (pthread_mutexattr_destroy(&ma) != 0)
        return 33;
    a_mutexattr_init(&ma);
    if (a_mutexattr_destroy(&ma) != 0)
        return 34;

    /* Condition variable attributes. */
    pthread_condattr_t ca;
    pthread_condattr_init(&ca);
    clockid_t clk;
    if (pthread_condattr_getclock(&ca, &clk) != 0 || clk != CLOCK_REALTIME)
        return 35;
    if (pthread_condattr_setclock(&ca, CLOCK_MONOTONIC) != 0 || pthread_condattr_getclock(&ca, &clk) != 0 || clk != CLOCK_MONOTONIC)
        return 36;
    if (pthread_condattr_getpshared(&ca, &v) != 0 || v != PTHREAD_PROCESS_PRIVATE)
        return 37;
    if (pthread_condattr_setpshared(&ca, PTHREAD_PROCESS_SHARED) != 0 || pthread_condattr_getpshared(&ca, &v) != 0 || v != PTHREAD_PROCESS_SHARED)
        return 38;
    if (pthread_condattr_setpshared(&ca, 5) != EINVAL)
        return 39;
    if (pthread_condattr_destroy(&ca) != 0)
        return 40;

    /* Read-write lock attributes. */
    pthread_rwlockattr_t ra;
    if (pthread_rwlockattr_init(&ra) != 0)
        return 41;
    if (pthread_rwlockattr_getkind_np(&ra, &v) != 0 || v != PTHREAD_RWLOCK_PREFER_READER_NP)
        return 42;
    if (pthread_rwlockattr_setkind_np(&ra, PTHREAD_RWLOCK_PREFER_WRITER_NONRECURSIVE_NP) != 0 || pthread_rwlockattr_getkind_np(&ra, &v) != 0 || v != PTHREAD_RWLOCK_PREFER_WRITER_NONRECURSIVE_NP)
        return 43;
    if (pthread_rwlockattr_setkind_np(&ra, 9) != EINVAL)
        return 44;
    if (pthread_rwlockattr_getpshared(&ra, &v) != 0 || v != PTHREAD_PROCESS_PRIVATE)
        return 45;
    if (pthread_rwlockattr_setpshared(&ra, PTHREAD_PROCESS_SHARED) != 0 || pthread_rwlockattr_getpshared(&ra, &v) != 0 || v != PTHREAD_PROCESS_SHARED)
        return 46;
    pthread_rwlock_t rw;
    if (pthread_rwlock_init(&rw, &ra) != 0 || pthread_rwlock_destroy(&rw) != 0)
        return 47;
    if (pthread_rwlockattr_destroy(&ra) != 0)
        return 48;

    /* Barrier attributes. */
    pthread_barrierattr_t ba;
    if (pthread_barrierattr_init(&ba) != 0)
        return 49;
    if (pthread_barrierattr_getpshared(&ba, &v) != 0 || v != PTHREAD_PROCESS_PRIVATE)
        return 50;
    if (pthread_barrierattr_setpshared(&ba, PTHREAD_PROCESS_SHARED) != 0 || pthread_barrierattr_getpshared(&ba, &v) != 0 || v != PTHREAD_PROCESS_SHARED)
        return 51;
    pthread_barrier_t b;
    if (pthread_barrier_init(&b, &ba, 1) != 0 || pthread_barrier_wait(&b) != PTHREAD_BARRIER_SERIAL_THREAD)
        return 52;
    if (pthread_barrier_destroy(&b) != 0 || pthread_barrierattr_destroy(&ba) != 0)
        return 53;
    return 0;
}

/* --- aliases of the locking and key functions ---------------------------------------------- */
static int once_runs;
static void once_fn(void) { once_runs++; }

static int aliases(void)
{
    pthread_mutex_t m;
    if (a_mutex_init(&m, NULL) != 0 || a_mutex_lock(&m) != 0)
        return 1;
    if (a_mutex_trylock(&m) != EBUSY)
        return 2;
    if (a_mutex_unlock(&m) != 0 || a_mutex_trylock(&m) != 0 || a_mutex_unlock(&m) != 0 || a_mutex_destroy(&m) != 0)
        return 3;
    pthread_rwlock_t rw;
    if (a_rwlock_init(&rw, NULL) != 0 || a_rwlock_rdlock(&rw) != 0)
        return 4;
    if (a_rwlock_trywrlock(&rw) != EBUSY || a_rwlock_unlock(&rw) != 0)
        return 5;
    if (a_rwlock_wrlock(&rw) != 0 || a_rwlock_unlock(&rw) != 0 || a_rwlock_trywrlock(&rw) != 0 || a_rwlock_unlock(&rw) != 0)
        return 6;
    if (a_rwlock_destroy(&rw) != 0)
        return 7;
    pthread_key_t k;
    if (a_key_create(&k, NULL) != 0 || a_getspecific(k) != NULL)
        return 8;
    if (a_setspecific(k, &k) != 0 || a_getspecific(k) != &k || pthread_getspecific(k) != &k)
        return 9;
    if (pthread_key_delete(k) != 0 || pthread_key_delete(k) != EINVAL)
        return 10;
    pthread_once_t o = PTHREAD_ONCE_INIT;
    a_once(&o, once_fn);
    a_once(&o, once_fn);
    if (once_runs != 1)
        return 11;
    if (a_yield() != 0)
        return 12;
    a_kill_other_threads_np(); /* a no-op since 2.34 */
    if (!p_pthread_equal(pthread_self(), pthread_self()) || p_pthread_equal(pthread_self(), (pthread_t)1))
        return 13;
    pthread_spinlock_t s;
    if (pthread_spin_init(&s, PTHREAD_PROCESS_PRIVATE) != 0 || pthread_spin_destroy(&s) != 0)
        return 14;
    if (pthread_setconcurrency(3) != 0 || pthread_getconcurrency() != 3)
        return 15;
    if (pthread_setconcurrency(-1) != EINVAL)
        return 16;
    pthread_setconcurrency(0);
    /* SCHED_OTHER: priority 0 only. */
    if (pthread_setschedprio(pthread_self(), 0) != 0)
        return 17;
    if (pthread_setschedprio(pthread_self(), 5) != EINVAL)
        return 18;
    if (__rseq_flags != 0)
        return 19;
    return 0;
}

/* --- cleanup buffers ------------------------------------------------------------------------- */
static int cleaned;
static void cleanup(void *arg) { cleaned += *(int *)arg; }

static int cleanup_buffers(void)
{
    struct _pthread_cleanup_buffer b;
    int one = 1, ten = 10, hundred = 100;
    a_cleanup_push(&b, cleanup, &one);
    a_cleanup_pop(&b, 1);
    a_cleanup_push(&b, cleanup, &ten);
    a_cleanup_pop(&b, 0);
    if (cleaned != 1)
        return 1;
    /* The deferring forms: asynchronous type is restored on pop. */
    int old;
    pthread_setcanceltype(PTHREAD_CANCEL_ASYNCHRONOUS, &old);
    a_cleanup_push_defer(&b, cleanup, &ten);
    pthread_setcanceltype(PTHREAD_CANCEL_DEFERRED, &old);
    if (old != PTHREAD_CANCEL_DEFERRED)
        return 2; /* push_defer switched to deferred */
    a_cleanup_pop_restore(&b, 1);
    pthread_setcanceltype(PTHREAD_CANCEL_DEFERRED, &old);
    if (cleaned != 11 || old != PTHREAD_CANCEL_ASYNCHRONOUS)
        return 3;
    /* The GNU macros use __pthread_register_cancel_defer and __pthread_unregister_cancel_restore. */
    pthread_cleanup_push_defer_np(cleanup, &hundred);
    pthread_cleanup_pop_restore_np(1);
    if (cleaned != 111)
        return 4;
    /* __pthread_cleanup_routine: runs the routine when do_it is set. */
    struct cleanup_frame f = {cleanup, &one, 1, PTHREAD_CANCEL_DEFERRED};
    a_cleanup_routine(&f);
    f.do_it = 0;
    a_cleanup_routine(&f);
    if (cleaned != 112)
        return 5;
    return 0;
}

/* --- condition variables and semaphores ------------------------------------------------- */
static int waits(void)
{
    pthread_mutex_t m = PTHREAD_MUTEX_INITIALIZER;
    pthread_cond_t c;
    pthread_condattr_t ca;
    pthread_condattr_init(&ca);
    pthread_condattr_setclock(&ca, CLOCK_MONOTONIC);
    pthread_cond_init(&c, &ca);
    pthread_mutex_lock(&m);
    struct timespec mono = past(CLOCK_MONOTONIC), rt = past(CLOCK_REALTIME);
    if (pthread_cond_clockwait(&c, &m, CLOCK_MONOTONIC, &mono) != ETIMEDOUT)
        return 1;
    if (pthread_cond_clockwait(&c, &m, CLOCK_REALTIME, &rt) != ETIMEDOUT)
        return 2;
    if (pthread_cond_clockwait(&c, &m, CLOCK_PROCESS_CPUTIME_ID, &rt) != EINVAL)
        return 3;
    pthread_mutex_unlock(&m);
    if (pthread_cond_signal(&c) != 0 || pthread_cond_destroy(&c) != 0)
        return 4;
    sem_t s;
    sem_init(&s, 0, 0);
    errno = 0;
    if (sem_timedwait(&s, &rt) != -1 || errno != ETIMEDOUT)
        return 5;
    errno = 0;
    if (sem_clockwait(&s, CLOCK_MONOTONIC, &mono) != -1 || errno != ETIMEDOUT)
        return 6;
    sem_post(&s);
    sem_post(&s);
    if (sem_timedwait(&s, &rt) != 0 || sem_clockwait(&s, CLOCK_MONOTONIC, &mono) != 0)
        return 7;
    struct timespec bad = {0, 2000000000};
    errno = 0;
    if (sem_timedwait(&s, &bad) != -1 || errno != EINVAL)
        return 8;
    if (sem_destroy(&s) != 0)
        return 9;
    return 0;
}

/* --- C11 threads ------------------------------------------------------------------------------ */
static cnd_t c11_cnd;
static mtx_t c11_mtx;
static int c11_ready;
static tss_t c11_key;
static int c11_dtors;
static void c11_dtor(void *p) { (void)p; c11_dtors++; }

static int c11_signaller(void *arg)
{
    (void)arg;
    mtx_lock(&c11_mtx);
    c11_ready = 1;
    cnd_signal(&c11_cnd);
    mtx_unlock(&c11_mtx);
    return 0;
}

static int c11_exiter(void *arg)
{
    tss_set(c11_key, arg);
    if (tss_get(c11_key) != arg)
        return 1;
    thrd_exit(42);
}

static int c11_detached(void *arg)
{
    (void)arg;
    return 0;
}

static int c11(void)
{
    if (mtx_init(&c11_mtx, mtx_plain) != thrd_success || cnd_init(&c11_cnd) != thrd_success)
        return 1;
    thrd_t t;
    mtx_lock(&c11_mtx);
    thrd_create(&t, c11_signaller, NULL);
    while (!c11_ready)
        if (cnd_wait(&c11_cnd, &c11_mtx) != thrd_success)
            return 2;
    struct timespec rt = past(CLOCK_REALTIME);
    if (cnd_timedwait(&c11_cnd, &c11_mtx, &rt) != thrd_timedout)
        return 3;
    mtx_unlock(&c11_mtx);
    thrd_join(t, NULL);
    if (cnd_broadcast(&c11_cnd) != thrd_success)
        return 4;
    cnd_destroy(&c11_cnd);
    mtx_destroy(&c11_mtx);

    /* thrd_exit's result reaches thrd_join; a tss destructor runs for a non-null value. */
    if (tss_create(&c11_key, c11_dtor) != thrd_success)
        return 5;
    int r = 0;
    thrd_create(&t, c11_exiter, &r);
    if (thrd_join(t, &r) != thrd_success || r != 42)
        return 6;
    if (c11_dtors != 1)
        return 7;
    if (tss_get(c11_key) != NULL)
        return 8;
    tss_delete(c11_key);

    if (thrd_create(&t, c11_detached, NULL) != thrd_success || thrd_detach(t) != thrd_success)
        return 9;
    if (!p_thrd_equal(thrd_current(), thrd_current()) || p_thrd_equal(thrd_current(), t))
        return 10;
    struct timespec d = {0, 1000000}, rem;
    if (thrd_sleep(&d, &rem) != 0)
        return 11;
    thrd_yield();
    return 0;
}

/* --- §21: the plain jump names --------------------------------------------------------------- */
static int jumps(void)
{
    jmp_buf env;
    volatile int n = 0;
    int r = a_setjmp(env);
    n++;
    if (r == 0)
        a_longjmp(env, 5);
    if (r != 5 || n != 2)
        return 1;
    n = 0;
    r = setjmp(env);
    n++;
    if (r == 0)
        p_longjmp(env, 0); /* 0 becomes 1 */
    if (r != 1 || n != 2)
        return 2;
    sigjmp_buf senv;
    n = 0;
    r = sigsetjmp(senv, 1);
    n++;
    if (r == 0)
        p_siglongjmp(senv, 3);
    if (r != 3 || n != 2)
        return 3;
    return 0;
}

int main(void)
{
    int r;
    if ((r = locks_held_elsewhere()) != 0)
        return r;
    if ((r = attributes()) != 0)
        return 100 + r;
    if ((r = aliases()) != 0)
        return 160 + r;
    if ((r = cleanup_buffers()) != 0)
        return 180 + r;
    if ((r = waits()) != 0)
        return 190 + r;
    if ((r = c11()) != 0)
        return 200 + r;
    if ((r = jumps()) != 0)
        return 220 + r;
    return 0;
}
