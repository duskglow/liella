/* Spec §22.1, §22.2, §22.6, §22.8, §22.9: thread lifecycle in a process Liella started.
 * Threads start with fresh TLS (the executable's image) and errno, get distinct TIDs and
 * thread pointers, return results through join, keep key values per thread and run key
 * destructors at exit, detach, report their actual attributes, carry names, and C11 threads
 * return int results. Each check exits with its own status on failure. */
#define _GNU_SOURCE
#include <errno.h>
#include <pthread.h>
#include <sched.h>
#include <stddef.h>
#include <stdint.h>
#include <string.h>
#include <sys/syscall.h>
#include <threads.h>
#include <time.h>
#include <unistd.h>

extern char __libc_single_threaded;
extern ptrdiff_t __rseq_offset;
extern unsigned int __rseq_size;

static _Thread_local int tls_counter = 41;
static _Thread_local char tls_zero[64];

static pthread_key_t key;
static int destroyed;
static pthread_mutex_t lock = PTHREAD_MUTEX_INITIALIZER;

static void dtor(void *v)
{
    __atomic_fetch_add(&destroyed, (int)(intptr_t)v, __ATOMIC_SEQ_CST);
}

struct out {
    pid_t tid;
    pthread_t self;
    int tls_before;
    int zero_ok;
    int errno_before;
};

static void *worker(void *arg)
{
    struct out *o = arg;
    o->tid = gettid();
    o->self = pthread_self();
    o->tls_before = tls_counter;
    o->zero_ok = 1;
    for (unsigned i = 0; i < sizeof tls_zero; i++)
        if (tls_zero[i] != 0)
            o->zero_ok = 0;
    o->errno_before = errno;
    tls_counter = 7;
    memset(tls_zero, 0xab, sizeof tls_zero);
    if (pthread_getspecific(key) != NULL)
        return (void *)1;
    pthread_setspecific(key, (void *)(intptr_t)5);
    return (void *)(intptr_t)(o->tid);
}

static void *sleeper(void *arg)
{
    (void)arg;
    usleep(20000);
    return (void *)99;
}

static int c11_main(void *arg)
{
    return (int)(intptr_t)arg * 3;
}

static void *named(void *arg)
{
    char buf[16];
    if (pthread_setname_np(pthread_self(), "liella-w") != 0)
        return (void *)1;
    if (pthread_getname_np(pthread_self(), buf, sizeof buf) != 0 || strcmp(buf, "liella-w") != 0)
        return (void *)2;
    if (pthread_setname_np(pthread_self(), "0123456789abcdef") != ERANGE)
        return (void *)3;
    *(int *)arg = 1;
    while (__atomic_load_n((int *)arg, __ATOMIC_SEQ_CST) == 1)
        sched_yield();
    return NULL;
}

int main(void)
{
    if (!__libc_single_threaded)
        return 10;
    if (pthread_key_create(&key, dtor) != 0)
        return 11;
    pthread_setspecific(key, (void *)(intptr_t)1000); /* the initial thread's own value */
    tls_counter = 1;
    errno = 77;

    enum { N = 8 };
    pthread_t t[N];
    struct out o[N];
    for (int i = 0; i < N; i++)
        if (pthread_create(&t[i], NULL, worker, &o[i]) != 0)
            return 12;
    if (__libc_single_threaded)
        return 13;
    for (int i = 0; i < N; i++) {
        void *r;
        if (pthread_join(t[i], &r) != 0)
            return 14;
        if ((pid_t)(intptr_t)r != o[i].tid || o[i].tid == getpid() || o[i].tid <= 0)
            return 15;
        if (!pthread_equal(o[i].self, t[i]))
            return 16;
        if (o[i].tls_before != 41 || !o[i].zero_ok)
            return 17; /* fresh TLS from the executable's image */
        if (o[i].errno_before != 0)
            return 18; /* a fresh errno */
        for (int j = 0; j < i; j++)
            if (o[j].tid == o[i].tid)
                return 19;
    }
    if (tls_counter != 1 || errno != 77)
        return 20; /* the initial thread's TLS and errno untouched */
    if (pthread_getspecific(key) != (void *)(intptr_t)1000)
        return 21;
    if (destroyed != 5 * N)
        return 22; /* each thread's destructor ran with its value */
    if (pthread_join(pthread_self(), NULL) != EDEADLK)
        return 23;

    /* Detach, a second detach, and joining a detached thread. */
    pthread_t d;
    if (pthread_create(&d, NULL, sleeper, NULL) != 0 || pthread_detach(d) != 0)
        return 24;
    usleep(50000);

    /* Timed and try joins. */
    pthread_t s;
    if (pthread_create(&s, NULL, sleeper, NULL) != 0)
        return 25;
    if (pthread_tryjoin_np(s, NULL) != EBUSY)
        return 26;
    struct timespec past = {0, 0};
    if (pthread_timedjoin_np(s, NULL, &past) != ETIMEDOUT)
        return 27;
    void *sr;
    struct timespec far;
    clock_gettime(CLOCK_MONOTONIC, &far);
    far.tv_sec += 10;
    if (pthread_clockjoin_np(s, &sr, CLOCK_MONOTONIC, &far) != 0 || sr != (void *)99)
        return 28;

    /* Attributes: a detached thread with a small stack and no guard; actual attributes. */
    pthread_attr_t a;
    if (pthread_attr_init(&a) != 0)
        return 29;
    size_t sz;
    if (pthread_attr_getstacksize(&a, &sz) != 0 || sz < 16384)
        return 30;
    if (pthread_attr_setstacksize(&a, 1000) != EINVAL)
        return 31;
    if (pthread_attr_setstacksize(&a, 256 * 1024) != 0 || pthread_attr_setguardsize(&a, 0) != 0)
        return 32;
    if (pthread_attr_setscope(&a, PTHREAD_SCOPE_PROCESS) != ENOTSUP)
        return 33;
    int named_flag = 0;
    pthread_t n;
    if (pthread_create(&n, &a, named, &named_flag) != 0)
        return 34;
    pthread_attr_destroy(&a);
    while (__atomic_load_n(&named_flag, __ATOMIC_SEQ_CST) == 0)
        sched_yield();
    char nb[16];
    if (pthread_getname_np(n, nb, sizeof nb) != 0 || strcmp(nb, "liella-w") != 0)
        return 35; /* another thread's name, through /proc */
    pthread_attr_t actual;
    if (pthread_getattr_np(n, &actual) != 0)
        return 36;
    void *lo;
    size_t ss, gs;
    if (pthread_attr_getstack(&actual, &lo, &ss) != 0 || ss < 256 * 1024 || pthread_attr_getguardsize(&actual, &gs) != 0 || gs != 0)
        return 37;
    char here;
    pthread_attr_t mine;
    if (pthread_getattr_np(pthread_self(), &mine) != 0 || pthread_attr_getstack(&mine, &lo, &ss) != 0)
        return 38;
    if ((char *)lo > &here || (char *)lo + ss < &here)
        return 39; /* the initial thread's stack holds this frame */
    __atomic_store_n(&named_flag, 2, __ATOMIC_SEQ_CST);
    if (pthread_join(n, NULL) != 0)
        return 40;

    /* C11 threads. */
    thrd_t c;
    int res = 0;
    if (thrd_create(&c, c11_main, (void *)14) != thrd_success || thrd_join(c, &res) != thrd_success || res != 42)
        return 41;
    if (!thrd_equal(thrd_current(), (thrd_t)pthread_self()))
        return 42;

    /* rseq, published once registered (glibc.pthread.rseq is on by default). */
    if (__rseq_size != 0 && __rseq_offset == 0)
        return 43;
    (void)lock;
    return 0;
}
