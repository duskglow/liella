/* COMPAT-CONDVAR-OLD (LG-3): the old (pre-2.3.2) condition-variable entries pthread_cond_*@GLIBC_2.2.5.
 * Black-box: each old entry is bound with .symver; only return values, the bytes of the caller's object
 * (reported as which offsets changed and whether a word is zero, never their values) and wake counts are
 * printed. Every call that might touch more than the old object runs in a forked child with the object
 * against a PROT_NONE page, so a fault is a result (printed as the signal). Every line starts with the
 * versioned name it observes. */
#define _GNU_SOURCE
#include <errno.h>
#include <malloc.h>
#include <pthread.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

#define OLD(ret, local, sym, ver, ...) extern ret local(__VA_ARGS__); __asm__(".symver " #local ", " #sym "@" #ver);
OLD(int, ci_225, pthread_cond_init, GLIBC_2.2.5, void *, const pthread_condattr_t *)
OLD(int, cd_225, pthread_cond_destroy, GLIBC_2.2.5, void *)
OLD(int, cw_225, pthread_cond_wait, GLIBC_2.2.5, void *, pthread_mutex_t *)
OLD(int, ct_225, pthread_cond_timedwait, GLIBC_2.2.5, void *, pthread_mutex_t *, const struct timespec *)
OLD(int, cs_225, pthread_cond_signal, GLIBC_2.2.5, void *)
OLD(int, cb_225, pthread_cond_broadcast, GLIBC_2.2.5, void *)

/* A failing allocator: the executable's malloc/calloc interpose libc's (the documented
 * replacement interface) and fail while `failing` is set, counting the calls they see. */
extern void *__libc_malloc(size_t); extern void *__libc_calloc(size_t, size_t);
static volatile int failing, alloc_seen;
void *malloc(size_t n) { if (failing) { alloc_seen++; errno = ENOMEM; return NULL; } return __libc_malloc(n); }
void *calloc(size_t k, size_t n) { if (failing) { alloc_seen++; errno = ENOMEM; return NULL; } return __libc_calloc(k, n); }

static pthread_mutex_t fmx = PTHREAD_MUTEX_INITIALIZER;
/* In a child (a hang or a fault is a result): a zero old object, allocation failing, one call. */
static void alloc_fail_case(const char *what, int op) {
    fflush(stdout);
    pid_t p = fork();
    if (p == 0) {
        unsigned char o[8]; memset(o, 0, 8);
        struct timespec past = {1, 0};
        alarm(3);
        int rc = 0;
        pthread_mutex_lock(&fmx);
        failing = 1;
        switch (op) {
        case 0: rc = cs_225(o); break;
        case 1: rc = cb_225(o); break;
        case 2: rc = ct_225(o, &fmx, &past); break;
        case 3: rc = cw_225(o, &fmx); break;
        default: rc = ci_225(o, NULL); break;
        }
        failing = 0;
        uint64_t w; memcpy(&w, o, 8);
        printf("%s allocation failing, zero object: rc=%d, allocator calls=%s, word %s\n", what, rc,
               alloc_seen ? "yes" : "none", w == 0 ? "zero" : "nonzero");
        fflush(stdout);
        _exit(0);
    }
    int st; waitpid(p, &st, 0);
    if (WIFSIGNALED(st)) printf("%s allocation failing: child signal %d\n", what, WTERMSIG(st));
}

static void changed(const char *what, const unsigned char *b, const unsigned char *ref, size_t n) {
    printf("%s: changed bytes:", what);
    int any = 0;
    for (size_t i = 0; i < n; i++) if (b[i] != ref[i]) { printf(" %zu", i); any = 1; }
    if (!any) printf(" none");
    uint64_t w0; memcpy(&w0, b, 8);
    printf("; word0 %s\n", w0 == 0 ? "zero" : "nonzero");
}

static struct timespec in_ms(clockid_t c, long ms) {
    struct timespec t; clock_gettime(c, &t);
    t.tv_nsec += (ms % 1000) * 1000000L; t.tv_sec += ms / 1000 + t.tv_nsec / 1000000000L; t.tv_nsec %= 1000000000L;
    return t;
}
static long ms_since(const struct timespec *a) {
    struct timespec b; clock_gettime(CLOCK_MONOTONIC, &b);
    return (b.tv_sec - a->tv_sec) * 1000 + (b.tv_nsec - a->tv_nsec) / 1000000;
}

/* An object of `size` bytes ending at a PROT_NONE page; the child runs `fn` on it. */
static void guarded(const char *what, size_t size, int zero, void (*fn)(unsigned char *)) {
    fflush(stdout);
    pid_t p = fork();
    if (p == 0) {
        long pg = sysconf(_SC_PAGESIZE);
        unsigned char *m = mmap(NULL, 2 * pg, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
        mprotect(m + pg, pg, PROT_NONE);
        unsigned char *o = m + pg - size;
        memset(o, zero ? 0 : 0xa5, size);
        alarm(10);
        fn(o);
        fflush(stdout);
        _exit(0);
    }
    int st; waitpid(p, &st, 0);
    if (WIFSIGNALED(st)) printf("%s (object of %zu bytes before a guard page): killed by signal %d\n", what, size, WTERMSIG(st));
    else if (WEXITSTATUS(st)) printf("%s: child exit %d\n", what, WEXITSTATUS(st));
}

static pthread_mutex_t mx = PTHREAD_MUTEX_INITIALIZER;
static void seq_all(unsigned char *o) {
    struct timespec past = {1, 0}, bad = {0, 1000000000L};
    printf("pthread_cond_init@GLIBC_2.2.5 8-byte object: rc=%d\n", ci_225(o, NULL));
    printf("pthread_cond_signal@GLIBC_2.2.5 8-byte object, no waiter: rc=%d\n", cs_225(o));
    printf("pthread_cond_broadcast@GLIBC_2.2.5 8-byte object, no waiter: rc=%d\n", cb_225(o));
    pthread_mutex_lock(&mx);
    printf("pthread_cond_timedwait@GLIBC_2.2.5 8-byte object, past abstime: rc=%d\n", ct_225(o, &mx, &past));
    printf("pthread_cond_timedwait@GLIBC_2.2.5 8-byte object, tv_nsec=1e9: rc=%d\n", ct_225(o, &mx, &bad));
    pthread_mutex_unlock(&mx);
    printf("pthread_cond_destroy@GLIBC_2.2.5 8-byte object: rc=%d\n", cd_225(o));
}
static void seq_static(unsigned char *o) {
    struct timespec past = {1, 0};
    pthread_mutex_lock(&mx);
    printf("pthread_cond_timedwait@GLIBC_2.2.5 8-byte zero object, no init, past abstime: rc=%d\n", ct_225(o, &mx, &past));
    pthread_mutex_unlock(&mx);
    printf("pthread_cond_destroy@GLIBC_2.2.5 8-byte zero object after use: rc=%d\n", cd_225(o));
}
static void seq_4(unsigned char *o) {
    printf("pthread_cond_init@GLIBC_2.2.5 4-byte object: rc=%d\n", ci_225(o, NULL));
    printf("pthread_cond_signal@GLIBC_2.2.5 4-byte object: rc=%d\n", cs_225(o));
}

/* Wake counting: N waiters on one old object, each loops until its turn flag allows it to leave. */
static unsigned char obj[64] __attribute__((aligned(16)));
static int woken, tickets;
static void *waiter(void *a) {
    (void)a;
    pthread_mutex_lock(&mx);
    while (tickets == 0) cw_225(obj, &mx);
    tickets--; woken++;
    pthread_mutex_unlock(&mx);
    return NULL;
}
static void wake_case(const char *how, int n, int bcast) {
    pthread_t t[8]; woken = 0; tickets = 0;
    for (int i = 0; i < n; i++) pthread_create(&t[i], NULL, waiter, NULL);
    usleep(200000);
    pthread_mutex_lock(&mx);
    tickets = bcast ? n : 1;
    int rc = bcast ? cb_225(obj) : cs_225(obj);
    pthread_mutex_unlock(&mx);
    usleep(300000);
    pthread_mutex_lock(&mx); int w = woken; pthread_mutex_unlock(&mx);
    printf("%s with %d waiters: rc=%d, left after 300 ms=%d\n", how, n, rc, w);
    pthread_mutex_lock(&mx); tickets += n - w; cb_225(obj); pthread_mutex_unlock(&mx);
    for (int i = 0; i < n; i++) pthread_join(t[i], NULL);
    printf("pthread_cond_wait@GLIBC_2.2.5 waiters of that %s case: all %d returned after a final broadcast\n", bcast ? "broadcast" : "signal", woken);
}

int main(void) {
    setvbuf(stdout, NULL, _IOLBF, 0);
    unsigned char b[64], ref[64];
    memset(b, 0xa5, 64); memcpy(ref, b, 64);
    printf("pthread_cond_init@GLIBC_2.2.5 NULL attr: rc=%d\n", ci_225(b, NULL));
    changed("pthread_cond_init@GLIBC_2.2.5 on 0xa5 bytes", b, ref, 64);
    memcpy(ref, b, 64);
    printf("pthread_cond_signal@GLIBC_2.2.5 after init, no waiter: rc=%d\n", cs_225(b));
    changed("pthread_cond_signal@GLIBC_2.2.5 after init", b, ref, 64);
    memcpy(ref, b, 64);
    printf("pthread_cond_destroy@GLIBC_2.2.5 after init: rc=%d\n", cd_225(b));
    changed("pthread_cond_destroy@GLIBC_2.2.5 after init", b, ref, 64);

    memset(b, 0, 64); memcpy(ref, b, 64);
    printf("pthread_cond_signal@GLIBC_2.2.5 zero object, no init: rc=%d\n", cs_225(b));
    changed("pthread_cond_signal@GLIBC_2.2.5 zero object", b, ref, 64);
    printf("pthread_cond_broadcast@GLIBC_2.2.5 zero object, no init: rc=%d\n", cb_225(b));
    changed("pthread_cond_broadcast@GLIBC_2.2.5 zero object", b, ref, 64);
    struct timespec past = {1, 0};
    pthread_mutex_lock(&mx);
    printf("pthread_cond_timedwait@GLIBC_2.2.5 zero object, past abstime: rc=%d\n", ct_225(b, &mx, &past));
    printf("pthread_cond_timedwait@GLIBC_2.2.5 mutex held again after ETIMEDOUT: trylock=%d\n", pthread_mutex_trylock(&mx));
    pthread_mutex_unlock(&mx);
    changed("pthread_cond_timedwait@GLIBC_2.2.5 zero object", b, ref, 64);
    memcpy(ref, b, 64);
    printf("pthread_cond_destroy@GLIBC_2.2.5 after timedwait: rc=%d\n", cd_225(b));
    changed("pthread_cond_destroy@GLIBC_2.2.5 after timedwait", b, ref, 64);
    memset(b, 0, 64);
    printf("pthread_cond_destroy@GLIBC_2.2.5 zero object never used: rc=%d\n", cd_225(b));

    pthread_condattr_t at; pthread_condattr_init(&at);
    pthread_condattr_setpshared(&at, PTHREAD_PROCESS_SHARED);
    memset(b, 0xa5, 64); memcpy(ref, b, 64);
    printf("pthread_cond_init@GLIBC_2.2.5 pshared attr: rc=%d\n", ci_225(b, &at));
    changed("pthread_cond_init@GLIBC_2.2.5 pshared attr", b, ref, 64);
    cd_225(b);
    pthread_condattr_init(&at);
    pthread_condattr_setclock(&at, CLOCK_MONOTONIC);
    memset(b, 0xa5, 64);
    printf("pthread_cond_init@GLIBC_2.2.5 CLOCK_MONOTONIC attr: rc=%d\n", ci_225(b, &at));
    struct timespec t0; clock_gettime(CLOCK_MONOTONIC, &t0);
    struct timespec mono = in_ms(CLOCK_MONOTONIC, 300);
    pthread_mutex_lock(&mx);
    int rc = ct_225(b, &mx, &mono);
    pthread_mutex_unlock(&mx);
    printf("pthread_cond_timedwait@GLIBC_2.2.5 CLOCK_MONOTONIC attr, abstime monotonic+300ms: rc=%d, waited>=200ms=%s\n", rc, ms_since(&t0) >= 200 ? "yes" : "no");
    cd_225(b);
    memset(b, 0xa5, 64);
    ci_225(b, NULL);
    clock_gettime(CLOCK_MONOTONIC, &t0);
    struct timespec real = in_ms(CLOCK_REALTIME, 300);
    pthread_mutex_lock(&mx);
    rc = ct_225(b, &mx, &real);
    pthread_mutex_unlock(&mx);
    printf("pthread_cond_timedwait@GLIBC_2.2.5 NULL attr, abstime realtime+300ms: rc=%d, waited>=200ms=%s\n", rc, ms_since(&t0) >= 200 ? "yes" : "no");
    cd_225(b);

    guarded("pthread_cond_init@GLIBC_2.2.5 8-byte sequence", 8, 0, seq_all);
    guarded("pthread_cond_timedwait@GLIBC_2.2.5 8-byte zero sequence", 8, 1, seq_static);
    guarded("pthread_cond_init@GLIBC_2.2.5 4-byte sequence", 4, 0, seq_4);

    memset(obj, 0, sizeof obj);
    wake_case("pthread_cond_signal@GLIBC_2.2.5", 4, 0);
    wake_case("pthread_cond_broadcast@GLIBC_2.2.5", 4, 1);

    /* COMPAT-CONDVAR-OLD-2: what the old entries do when the allocator fails on first use, and
     * whether destroy gives memory back (mallinfo2 in-use bytes over many init/use/destroy). */
    alloc_fail_case("pthread_cond_signal@GLIBC_2.2.5", 0);
    alloc_fail_case("pthread_cond_broadcast@GLIBC_2.2.5", 1);
    alloc_fail_case("pthread_cond_timedwait@GLIBC_2.2.5", 2);
    alloc_fail_case("pthread_cond_wait@GLIBC_2.2.5", 3);
    alloc_fail_case("pthread_cond_init@GLIBC_2.2.5", 4);
    {
        unsigned char o[8];
        memset(o, 0, 8); cs_225(o); cd_225(o);
        size_t before = mallinfo2().uordblks;
        for (int i = 0; i < 1000; i++) { ci_225(o, NULL); cs_225(o); cd_225(o); }
        size_t after = mallinfo2().uordblks;
        printf("pthread_cond_destroy@GLIBC_2.2.5 1000 init/signal/destroy: in-use bytes %s\n",
               after <= before ? "not grown" : after - before >= 1000 * 8 ? "grown per cycle (leak)" : "grown a little");
    }
    return 0;
}
