/* Spec C0.2 (clause s0_6_c0_2_1; D-C0.2, D-8.8; milestone 6): every function that returns a
 * library-owned result buffer uses one per thread. For each, two threads call it at the same
 * moment with different arguments, then each checks that its result is still its own (not
 * overwritten by the other thread's call) and at another address; then eight threads call it
 * 2,000 times each, checking every result at once. Finally, a thread that has used them all
 * ends and its storage is released: the heap's bytes in use after joining it equal those after
 * joining a thread that used none, and so do they after a thread whose key destructor used
 * them all (destructors run before that storage is released). Each failing CHECK exits with its line number. */
#define _GNU_SOURCE
#include <arpa/inet.h>
#include <errno.h>
#include <fcntl.h>
#include <malloc.h>
#include <netinet/ether.h>
#include <pthread.h>
#include <pwd.h>
#include <resolv.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

#define CHECK(cond) do { if (!(cond)) return __LINE__; } while (0)

/* One result for key k (0 or 1 in the pair test, any in the stress test). */
typedef const char *(*Call)(int k);

static int dec, sgn;
static const char *c_ecvt(int k) { return ecvt(1.25 + k, 6, &dec, &sgn); }
static const char *c_fcvt(int k) { return fcvt(2.5 + k, 3, &dec, &sgn); }
static const char *c_qecvt(int k) { return qecvt(3.75L + k, 8, &dec, &sgn); }
static const char *c_qfcvt(int k) { return qfcvt(4.5L + k, 4, &dec, &sgn); }
static const char *c_l64a(int k) { return l64a(1000 + k); }
static const char *c_nsap(int k)
{
    unsigned char b[3] = {(unsigned char)k, (unsigned char)(k >> 8), 0x5a};
    return inet_nsap_ntoa(3, b, NULL);
}
static const char *c_ptype(int k) { return p_type(30000 + k); }
static const char *c_pclass(int k) { return p_class(30000 + k); }
static const char *c_prcode(int k) { return p_rcode(30000 + k); }
static const char *c_ptime(int k) { return p_time(3600u * (k + 1) + 61u); }
static const char *c_strerror(int k) { return strerror(10000 + k); }
static const char *c_strsignal(int k) { return strsignal(100 + k); }
static const char *c_inet_ntoa(int k)
{
    struct in_addr a = {htonl(0x0a000000u + (unsigned)k)};
    return inet_ntoa(a);
}
static const char *c_ether_ntoa(int k)
{
    struct ether_addr e = {{0, 1, 2, 3, (unsigned char)(k >> 8), (unsigned char)k}};
    return ether_ntoa(&e);
}
static const char *c_asctime(int k)
{
    time_t t = 86400L * (k + 1);
    return asctime(gmtime(&t));
}
static const char *c_ctime(int k)
{
    time_t t = 86400L * 30 * (k + 1);
    return ctime(&t);
}
/* A pseudo-terminal of the thread's own: ptsname and ttyname of its slave. */
static __thread int master = -1, slave = -1;
static void own_pty(void)
{
    if (master < 0) {
        master = posix_openpt(O_RDWR | O_NOCTTY);
        grantpt(master);
        unlockpt(master);
        char name[64];
        ptsname_r(master, name, sizeof name);
        slave = open(name, O_RDWR | O_NOCTTY);
    }
}
static const char *c_ptsname(int k) { (void)k; own_pty(); return ptsname(master); }
static const char *c_ttyname(int k) { (void)k; own_pty(); return ttyname(slave); }
static const char *c_tmpnam(int k) { (void)k; return tmpnam(NULL); }

static const struct { const char *name; Call f; int stress; } calls[] = {
    {"ecvt", c_ecvt, 1}, {"fcvt", c_fcvt, 1}, {"qecvt", c_qecvt, 1}, {"qfcvt", c_qfcvt, 1},
    {"l64a", c_l64a, 1}, {"inet_nsap_ntoa", c_nsap, 1}, {"p_type", c_ptype, 1},
    {"p_class", c_pclass, 1}, {"p_rcode", c_prcode, 1}, {"p_time", c_ptime, 1},
    {"strerror", c_strerror, 1}, {"strsignal", c_strsignal, 1}, {"inet_ntoa", c_inet_ntoa, 1},
    {"ether_ntoa", c_ether_ntoa, 1}, {"asctime", c_asctime, 1}, {"ctime", c_ctime, 1},
    {"ptsname", c_ptsname, 0}, {"ttyname", c_ttyname, 0}, {"tmpnam", c_tmpnam, 0},
};
#define NCALLS (sizeof calls / sizeof calls[0])

static pthread_barrier_t two;
static int which;
static volatile int failed;

/* The pair test: both threads call, both wait, both check. */
static void *pair(void *arg)
{
    int k = (int)(long)arg;
    pthread_barrier_wait(&two);
    const char *p = calls[which].f(k);
    char copy[256];
    snprintf(copy, sizeof copy, "%s", p ? p : "(null)");
    static const char *seen[2];
    seen[k] = p;
    pthread_barrier_wait(&two);
    if (!p || strcmp(p, copy) != 0 || seen[0] == seen[1])
        failed = 1;
    pthread_barrier_wait(&two);
    return NULL;
}

/* The stress test: every result checked against a copy made by the same thread at once. */
static void *stress(void *arg)
{
    int k = (int)(long)arg;
    for (int i = 0; i < 2000; i++) {
        int key = k * 3000 + i % 64;
        const char *p = calls[which].f(key);
        char copy[256];
        snprintf(copy, sizeof copy, "%s", p ? p : "(null)");
        sched_yield();
        const char *q = calls[which].f(key);
        if (!p || p != q || strcmp(q, copy) != 0)
            failed = 1;
    }
    return NULL;
}

/* Use every function once (the storage-release test). */
static void *use_all(void *arg)
{
    (void)arg;
    for (size_t i = 0; i < NCALLS; i++)
        (void)calls[i].f(1);
    setusershell();
    (void)getusershell();
    endusershell();
    (void)getpwnam("root");
    if (master >= 0) {
        close(master);
        close(slave);
    }
    return NULL;
}

static void *use_none(void *arg)
{
    return arg;
}

/* A key destructor that uses them all: it runs after the thread's own code, so storage it
 * allocates must still be released when the thread ends. */
static pthread_key_t late_key;

static void late_dtor(void *v)
{
    (void)use_all(v);
}

static void *use_all_late(void *arg)
{
    if (pthread_setspecific(late_key, (void *)1) != 0)
        failed = 1;
    return arg;
}

static size_t in_use(void)
{
    return mallinfo2().uordblks;
}

int main(void)
{
    CHECK(pthread_barrier_init(&two, NULL, 2) == 0);
    for (which = 0; which < (int)NCALLS; which++) {
        pthread_t a, b;
        failed = 0;
        CHECK(pthread_create(&a, NULL, pair, (void *)0L) == 0 && pthread_create(&b, NULL, pair, (void *)1L) == 0);
        CHECK(pthread_join(a, NULL) == 0 && pthread_join(b, NULL) == 0);
        if (failed) {
            fprintf(stderr, "shared between threads: %s\n", calls[which].name);
            return 100 + which;
        }
        if (!calls[which].stress)
            continue;
        pthread_t t[8];
        for (long i = 0; i < 8; i++)
            CHECK(pthread_create(&t[i], NULL, stress, (void *)i) == 0);
        for (int i = 0; i < 8; i++)
            CHECK(pthread_join(t[i], NULL) == 0);
        if (failed) {
            fprintf(stderr, "overwritten under load: %s\n", calls[which].name);
            return 150 + which;
        }
    }
    /* getusershell: two threads' results (the cursor is process-wide, the buffers are not). */
    setusershell();
    const char *mine = getusershell();
    char copy[256];
    snprintf(copy, sizeof copy, "%s", mine ? mine : "");
    pthread_t o;
    CHECK(pthread_create(&o, NULL, use_all, NULL) == 0 && pthread_join(o, NULL) == 0);
    CHECK(mine && strcmp(mine, copy) == 0);
    endusershell();
    /* Storage released at thread exit. */
    pthread_t n;
    CHECK(pthread_create(&n, NULL, use_none, NULL) == 0 && pthread_join(n, NULL) == 0);
    size_t base = in_use();
    CHECK(pthread_create(&n, NULL, use_none, NULL) == 0 && pthread_join(n, NULL) == 0);
    size_t none = in_use();
    CHECK(pthread_create(&n, NULL, use_all, NULL) == 0 && pthread_join(n, NULL) == 0);
    size_t all = in_use();
    if (all != none || none != base) {
        fprintf(stderr, "in use: %zu, after a thread using none %zu, after one using all %zu\n", base, none, all);
        return 99;
    }
    CHECK(pthread_key_create(&late_key, late_dtor) == 0);
    failed = 0;
    CHECK(pthread_create(&n, NULL, use_all_late, NULL) == 0 && pthread_join(n, NULL) == 0);
    CHECK(!failed);
    size_t late = in_use();
    if (late != base) {
        fprintf(stderr, "in use: %zu, after a thread whose key destructor used all %zu\n", base, late);
        return 98;
    }
    return 0;
}
