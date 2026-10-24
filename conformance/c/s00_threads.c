/* Spec C0.2 (clauses s0_6_c0_2_0, s0_6_c0_2_1, s0_6_c0_2_2):
 *  - MT-Safe by default: eight threads run a mix of functions the spec does not list as
 *    departures, at once, many times; every result equals the single-threaded one;
 *  - static result buffers: each of asctime, ctime, gmtime, localtime, inet_ntoa, l64a, ecvt,
 *    strerror and tmpnam(NULL) returns library storage that its next call overwrites; the _r form
 *    writes the caller's buffer; per thread (D-C0.2) for gmtime/localtime, asctime/ctime,
 *    inet_ntoa and strerror: another thread's calls do not clobber this thread's result
 *    (stricter than the reference, which shares one buffer); every other function with a
 *    library-owned result is checked across threads by s00_result_buffers.c;
 *  - process-global state: what one thread changes (environment, current directory, umask,
 *    signal disposition, global locale, rand's seed, getopt's optind) another thread sees.
 * Each failing CHECK exits with its line number. */
#define _GNU_SOURCE
#include <arpa/inet.h>
#include <errno.h>
#include <fcntl.h>
#include <getopt.h>
#include <locale.h>
#include <pthread.h>
#include <regex.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <time.h>
#include <unistd.h>

#define CHECK(cond) do { if (!(cond)) return __LINE__; } while (0)

static int cmp(const void *a, const void *b) { return *(const int *)a - *(const int *)b; }

/* One round of MT-Safe work, summarised as text. */
static void work(int seed, char *out, size_t n)
{
    char a[64];
    snprintf(a, sizeof a, "%d:%.3f:%x", seed, seed / 7.0, seed * 31);
    double d = strtod("2.5e3", NULL);
    long l = strtol("-123", NULL, 10);
    int v[16];
    for (int i = 0; i < 16; i++)
        v[i] = (seed * 7919 + i * 104729) % 1000;
    qsort(v, 16, sizeof v[0], cmp);
    regex_t re;
    int m = -1;
    if (regcomp(&re, "^[0-9]+:[0-9.]+:[0-9a-f]+$", REG_EXTENDED | REG_NOSUB) == 0) {
        m = regexec(&re, a, 0, NULL, 0);
        regfree(&re);
    }
    time_t t = 86400 * 365 + seed;
    struct tm tm;
    char ts[64];
    gmtime_r(&t, &tm);
    strftime(ts, sizeof ts, "%Y-%m-%d %H:%M:%S", &tm);
    char *dup = strdup(a);
    size_t len = strlen(dup);
    free(dup);
    snprintf(out, n, "%s|%g|%ld|%d,%d|%d|%s|%zu", a, d, l, v[0], v[15], m, ts, len);
}

static volatile int mismatches;
static void *worker(void *arg)
{
    int id = (int)(long)arg;
    for (int r = 0; r < 300; r++) {
        char got[256], want[256];
        work(id * 1000 + r, got, sizeof got);
        work(id * 1000 + r, want, sizeof want);
        if (strcmp(got, want) != 0)
            __atomic_add_fetch(&mismatches, 1, __ATOMIC_RELAXED);
    }
    return NULL;
}

/* The serial answers, for comparison with the threads' (computed before any thread starts). */
static char serial[8][300][256];

static void *worker_check(void *arg)
{
    int id = (int)(long)arg;
    for (int r = 0; r < 300; r++) {
        char got[256];
        work(id * 1000 + r, got, sizeof got);
        if (strcmp(got, serial[id][r]) != 0)
            __atomic_add_fetch(&mismatches, 1, __ATOMIC_RELAXED);
    }
    return NULL;
}

/* Another thread's static results: they must not clobber the main thread's (D-C0.2). */
static void *clobber(void *arg)
{
    (void)arg;
    time_t t = 0;
    (void)asctime(gmtime(&t));
    struct in_addr a = {htonl(0x7f000001)};
    (void)inet_ntoa(a);
    (void)strerror(ENOENT);
    return NULL;
}

static volatile sig_atomic_t usr1;
static void on_usr1(int s) { (void)s; usr1++; }

static void *changer(void *arg)
{
    (void)arg;
    setenv("C02_SHARED", "from-thread", 1);
    if (chdir("/") != 0)
        return (void *)1;
    umask(027);
    signal(SIGUSR1, on_usr1);
    setlocale(LC_ALL, "C.UTF-8");
    srand(42);
    optind = 7;
    return NULL;
}

int main(void)
{
    /* MT-Safe by default. */
    for (int id = 0; id < 8; id++)
        for (int r = 0; r < 300; r++)
            work(id * 1000 + r, serial[id][r], sizeof serial[id][r]);
    pthread_t th[8];
    for (long i = 0; i < 8; i++)
        CHECK(pthread_create(&th[i], NULL, i % 2 ? worker : worker_check, (void *)i) == 0);
    for (int i = 0; i < 8; i++)
        CHECK(pthread_join(th[i], NULL) == 0);
    CHECK(mismatches == 0);

    /* Static result buffers: the next call overwrites; the _r forms write the caller's. */
    time_t t0 = 0, t1 = 86400;
    struct tm *g0 = gmtime(&t0);
    CHECK(g0->tm_mday == 1);
    struct tm *g1 = gmtime(&t1);
    CHECK(g1 == g0 && g0->tm_mday == 2);
    struct tm mine;
    CHECK(gmtime_r(&t0, &mine) == &mine && mine.tm_mday == 1 && g0->tm_mday == 2);
    char *a0 = asctime(g0);
    char a0copy[32];
    snprintf(a0copy, sizeof a0copy, "%s", a0);
    char *a1 = asctime(&mine);
    CHECK(a1 == a0 && strcmp(a0, a0copy) != 0);
    char ab[32];
    CHECK(asctime_r(&mine, ab) == ab && strcmp(ab, a1) == 0);
    char *c0 = ctime(&t0);
    CHECK(c0 != NULL && ctime(&t1) == c0);
    struct in_addr x = {htonl(0x01020304)}, y = {htonl(0x05060708)};
    char *n0 = inet_ntoa(x);
    CHECK(strcmp(n0, "1.2.3.4") == 0);
    CHECK(inet_ntoa(y) == n0 && strcmp(n0, "5.6.7.8") == 0);
    char *l0 = l64a(1);
    CHECK(l64a(2) == l0);
    int dec, sign;
    char *e0 = ecvt(1.5, 4, &dec, &sign);
    CHECK(ecvt(2.5, 4, &dec, &sign) == e0 && e0[0] == '2');
    char eb[32];
    CHECK(ecvt_r(1.5, 4, &dec, &sign, eb, sizeof eb) == 0 && eb[0] == '1');
    char *s0 = strerror(ENOENT);
    char sb[64];
    CHECK(strerror_r(ENOENT, sb, sizeof sb) != NULL);
    char *tn = tmpnam(NULL);
    CHECK(tn != NULL && tmpnam(NULL) == tn);
    /* Per thread (D-C0.2): another thread's calls leave this thread's results alone. */
    struct tm *keep_g = gmtime(&t1);
    char *keep_a = asctime(keep_g);
    char keep_a_copy[32];
    snprintf(keep_a_copy, sizeof keep_a_copy, "%s", keep_a);
    char *keep_n = inet_ntoa(y);
    char keep_s[64];
    snprintf(keep_s, sizeof keep_s, "%s", s0);
    pthread_t c;
    CHECK(pthread_create(&c, NULL, clobber, NULL) == 0 && pthread_join(c, NULL) == 0);
    CHECK(keep_g->tm_mday == 2 && strcmp(keep_a, keep_a_copy) == 0);
    CHECK(strcmp(keep_n, "5.6.7.8") == 0);
    CHECK(strcmp(s0, keep_s) == 0);

    /* Process-global state: one thread's change is every thread's. */
    srand(1);
    int first = rand();
    pthread_t ch;
    CHECK(pthread_create(&ch, NULL, changer, NULL) == 0);
    void *rv;
    CHECK(pthread_join(ch, &rv) == 0 && rv == NULL);
    CHECK(getenv("C02_SHARED") && strcmp(getenv("C02_SHARED"), "from-thread") == 0);
    char cwd[64];
    CHECK(getcwd(cwd, sizeof cwd) && strcmp(cwd, "/") == 0);
    CHECK(umask(022) == 027);
    CHECK(raise(SIGUSR1) == 0 && usr1 == 1);
    CHECK(strcmp(setlocale(LC_ALL, NULL), "C.UTF-8") == 0);
    int after = rand();
    srand(42);
    CHECK(after == rand());
    srand(1);
    CHECK(rand() == first);
    CHECK(optind == 7);
    return 0;
}
