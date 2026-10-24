/* Hot-function rows that had no benchmark (BENCH-HOT, 2026-10-09; the call-frequency survey of
 * 15 real workloads under glibc ranked these functions HOT): string functions at the sizes the
 * workloads use, buffered getc and unlocked stdio, getenv, the per-call TLS accessors
 * (__errno_location, __ctype_b_loc, __ctype_get_mb_cur_max), uncontended mutex lock + unlock
 * (single-threaded, and again after a pthread_create + join: the _mt rows),
 * and three system call wrappers (read the user-instruction column: kernel time is noise, the
 * rows report the wrapper only). The same file builds against the products and against the
 * host C library. Prints `name<TAB>ns/op` lines, the best of several rounds; every call goes
 * through a volatile pointer so that it is really made (no builtins, no inlining). */
#undef _FORTIFY_SOURCE
#define _GNU_SOURCE
#include <ctype.h>
#include <errno.h>
#include <fcntl.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <time.h>
#include <unistd.h>
#include "cbench_counters.h"

static volatile long sink;

static double now(void)
{
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (double)ts.tv_sec * 1e9 + (double)ts.tv_nsec;
}

static void put(const char *name, double ns)
{
    char buf[64];
    int n = 0;
    while (name[n] && n < 40) {
        buf[n] = name[n];
        n++;
    }
    buf[n++] = '\t';
    unsigned long v = (unsigned long)(ns * 100.0 + 0.5);
    char digits[24];
    int d = 0;
    do {
        digits[d++] = (char)('0' + v % 10);
        v /= 10;
    } while (v || d < 3);
    while (d > 2)
        buf[n++] = digits[--d];
    buf[n++] = '.';
    buf[n++] = digits[1];
    buf[n++] = digits[0];
    buf[n++] = '\n';
    (void)!write(1, buf, (size_t)n);
}

#include "cbench_rows.h"

#define MEASURE(name, iters, expr)                         \
    do {                                                   \
        if (cb_skip(name))                                 \
            break;                                         \
        double best = 1e300;                               \
        ctr_begin();                                       \
        for (int round = 0; round < 7; round++) {          \
            double t0 = now();                             \
            long acc = 0;                                  \
            for (long i = 0; i < (iters); i++)             \
                acc += (long)(expr);                       \
            sink = acc;                                    \
            double per = (now() - t0) / (double)(iters);   \
            if (per < best)                                \
                best = per;                                \
        }                                                  \
        ctr_end(name, 7.0 * (double)(iters));              \
        put(name, best);                                   \
    } while (0)

extern void *__memcpy_chk(void *, const void *, size_t, size_t);
extern char *__strcpy_chk(char *, const char *, size_t);

static int (*volatile strncmp_fn)(const char *, const char *, size_t) = strncmp;
static char *(*volatile strchr_fn)(const char *, int) = strchr;
static char *(*volatile strchrnul_fn)(const char *, int) = strchrnul;
static void *(*volatile memrchr_fn)(const void *, int, size_t) = memrchr;
static void *(*volatile mempcpy_fn)(void *, const void *, size_t) = mempcpy;
static void *(*volatile memcpy_chk_fn)(void *, const void *, size_t, size_t) = __memcpy_chk;
static char *(*volatile strcpy_chk_fn)(char *, const char *, size_t) = __strcpy_chk;
static int (*volatile getc_fn)(FILE *) = getc;
static int (*volatile getc_unlocked_fn)(FILE *) = getc_unlocked;
static ssize_t (*volatile getline_fn)(char **, size_t *, FILE *) = getline;
static int (*volatile putc_unlocked_fn)(int, FILE *) = putc_unlocked;
static size_t (*volatile fwrite_unlocked_fn)(const void *, size_t, size_t, FILE *) = fwrite_unlocked;
static char *(*volatile getenv_fn)(const char *) = getenv;
static int *(*volatile errno_loc_fn)(void) = __errno_location;
static const unsigned short **(*volatile ctype_b_loc_fn)(void) = __ctype_b_loc;
static size_t (*volatile mb_cur_max_fn)(void) = __ctype_get_mb_cur_max;
static int (*volatile mutex_lock_fn)(pthread_mutex_t *) = pthread_mutex_lock;
static int (*volatile mutex_unlock_fn)(pthread_mutex_t *) = pthread_mutex_unlock;
static int (*volatile fstat_fn)(int, struct stat *) = fstat;
static ssize_t (*volatile read_fn)(int, void *, size_t) = read;
static int (*volatile close_fn)(int) = close;

/* 64-byte aligned buffers, so that the rows measure the function and not an alignment accident.
 * They live in one page-aligned block, 64 bytes into it, so that their page offsets are fixed
 * too (MEMCPY-NONAVX2-THRESHOLD-2): the program links statically, and as separate statics their
 * addresses moved with the size of the C library's data, which decides the string kernels' page
 * tests (a start at page offset 0 or a pair near a page end takes the longer path). With the
 * 64-byte lead no buffer starts at page offset 0 or within 64 bytes of a page end. */
static struct {
    char lead[64];
    char a64[128] __attribute__((aligned(64)));
    char b64[128] __attribute__((aligned(64)));
    char big[4096 + 64] __attribute__((aligned(64)));
    char dst[256] __attribute__((aligned(64)));
    char names_a[64] __attribute__((aligned(64)));
    char names_b[64] __attribute__((aligned(64)));
    char s16[32] __attribute__((aligned(64)));
    char s64[96] __attribute__((aligned(64)));
    char d8[32] __attribute__((aligned(64)));
    char e8[32] __attribute__((aligned(64)));
} bufs __attribute__((aligned(4096))) = {
    .names_a = "/usr/share/doc/libexample-dev/changelog.Debian.gz.orig-1",
    .names_b = "/usr/share/doc/libexample-dev/changelog.Debian.gz.orig-2",
    .s16 = "abcdefghijklmnop", /* 16 chars */
};
#define a64 (bufs.a64)
#define b64 (bufs.b64)
#define big (bufs.big)
#define dst (bufs.dst)
#define names_a (bufs.names_a)
#define names_b (bufs.names_b)
#define s16 (bufs.s16)
#define s64 (bufs.s64)
#define d8 (bufs.d8)
#define e8 (bufs.e8)

static void *idle_thread(void *arg)
{
    return arg;
}

int main(void)
{
    memset(a64, 'a', 127);
    memset(b64, 'a', 127);
    a64[16] = b64[16] = 0; /* 16 equal characters */
    memset(big, 'x', sizeof big);
    big[5] = 'q'; /* memrchr hit near the start: the scan runs the whole length */

    memset(s64, 'k', 63);
    s16[12] = 'Z';
    s64[60] = 'Z';

    memset(d8, 'm', 31);
    memset(e8, 'm', 31);
    e8[8] = 'n'; /* differ at index 8 */

    MEASURE("strncmp_eq16", 10000000L, strncmp_fn(a64, b64, 64));
    MEASURE("strncmp_diff8", 10000000L, strncmp_fn(d8, e8, 64));
    MEASURE("strncmp_names64", 10000000L, strncmp_fn(names_a, names_b, 64));

    MEASURE("strchr_16_hit", 10000000L, strchr_fn(s16, 'Z') != 0);
    MEASURE("strchr_16_miss", 10000000L, strchr_fn(s16, 'Q') == 0);
    MEASURE("strchr_64_hit", 10000000L, strchr_fn(s64, 'Z') != 0);
    MEASURE("strchr_64_miss", 10000000L, strchr_fn(s64, 'Q') == 0);
    MEASURE("strchrnul_16_hit", 10000000L, strchrnul_fn(s16, 'Z') != 0);
    MEASURE("strchrnul_16_miss", 10000000L, strchrnul_fn(s16, 'Q') != 0);
    MEASURE("strchrnul_64_hit", 10000000L, strchrnul_fn(s64, 'Z') != 0);
    MEASURE("strchrnul_64_miss", 10000000L, strchrnul_fn(s64, 'Q') != 0);

    MEASURE("memrchr_64", 10000000L, memrchr_fn(big, 'q', 64) == 0);
    MEASURE("memrchr_64_hit", 10000000L, memrchr_fn(big, 'q', 6) != 0);
    MEASURE("memrchr_4k", 2000000L, memrchr_fn(big, 'q', 4096) != 0);

    MEASURE("mempcpy_16", 10000000L, mempcpy_fn(dst, a64, 16) != 0);
    MEASURE("mempcpy_64", 10000000L, mempcpy_fn(dst, a64, 64) != 0);
    MEASURE("memcpy_chk_64", 10000000L, memcpy_chk_fn(dst, a64, 64, sizeof dst) != 0);
    MEASURE("strcpy_chk_16", 10000000L, strcpy_chk_fn(dst, s16, sizeof dst) != 0);
    MEASURE("strcpy_chk_64", 10000000L, strcpy_chk_fn(dst, s64, sizeof dst) != 0);

    /* Streams: a buffered read stream on /dev/zero (the refill is one read per 8 KiB) and a
     * fully buffered write stream on /dev/null. */
    static char rbuf[8192], wbuf[8192];
    FILE *rf = fopen("/dev/zero", "r");
    FILE *wf = fopen("/dev/null", "w");
    if (!rf || !wf)
        return 1;
    setvbuf(rf, rbuf, _IOFBF, sizeof rbuf);
    setvbuf(wf, wbuf, _IOFBF, sizeof wbuf);
    MEASURE("getc", 10000000L, getc_fn(rf));
    MEASURE("getc_unlocked", 10000000L, getc_unlocked_fn(rf));
    MEASURE("putc_unlocked", 10000000L, putc_unlocked_fn('x', wf));
    MEASURE("fwrite_unlocked_16", 10000000L, fwrite_unlocked_fn(a64, 1, 16, wf));
    MEASURE("fwrite_unlocked_64", 10000000L, fwrite_unlocked_fn(a64, 1, 64, wf));
    fclose(rf);
    fclose(wf);

    /* getline (GETDELIM): a file of 80-byte lines (the lines of text tools) and a file of 10000-byte
     * lines (longer than the 8 KiB buffer), the caller's buffer reused, rewound at end of file. */
    {
        static char line[10001];
        memset(line, 'a', sizeof line);
        FILE *lf80 = tmpfile(), *lflong = tmpfile();
        if (!lf80 || !lflong)
            return 1;
        line[79] = '\n';
        for (int i = 0; i < 800; i++)
            if (fwrite(line, 1, 80, lf80) != 80)
                return 1;
        line[79] = 'a';
        line[9999] = '\n';
        for (int i = 0; i < 20; i++)
            if (fwrite(line, 1, 10000, lflong) != 10000)
                return 1;
        fflush(lf80);
        fflush(lflong);
        rewind(lf80);
        rewind(lflong);
        char *lp = 0;
        size_t lcap = 0;
        MEASURE("getline_80", 2000000L, ({
                    ssize_t r_ = getline_fn(&lp, &lcap, lf80);
                    if (r_ < 0)
                        rewind(lf80);
                    r_;
                }));
        MEASURE("getline_long", 200000L, ({
                    ssize_t r_ = getline_fn(&lp, &lcap, lflong);
                    if (r_ < 0)
                        rewind(lflong);
                    r_;
                }));
        free(lp);
        fclose(lf80);
        fclose(lflong);
    }

    /* getenv over an environment of exactly 30 variables. */
    clearenv();
    for (int k = 0; k < 30; k++) {
        char name[16] = "BENCHVAR00";
        name[8] = (char)('0' + k / 10);
        name[9] = (char)('0' + k % 10);
        setenv(name, "value", 1);
    }
    MEASURE("getenv_hit_mid", 5000000L, getenv_fn("BENCHVAR15") != 0);
    MEASURE("getenv_hit_last", 5000000L, getenv_fn("BENCHVAR29") != 0);
    MEASURE("getenv_miss", 5000000L, getenv_fn("BENCHNOPE") == 0);
    MEASURE("setenv_30", 200000L, setenv("BENCHVAR29", (i & 1) ? "a" : "b", 1) == 0);
    {
        /* putenv of two fixed strings for a set name (no allocation): ENVIRON-TABLE-CONTRACT. */
        static char put_a[] = "BENCHVAR29=a", put_b[] = "BENCHVAR29=b";
        MEASURE("putenv_30", 200000L, putenv((i & 1) ? put_a : put_b) == 0);
        /* ENVIRON-UNSET-SCAN: removal. An absent name (the common unsetenv), then a set name
         * removed and put back (the last entry), by unsetenv and by putenv without '='. */
        static char put_name[] = "BENCHVAR29";
        MEASURE("unsetenv_miss_30", 200000L, unsetenv("BENCHNOPE") == 0);
        MEASURE("unsetenv_cycle_30", 200000L, (i & 1) ? putenv(put_a) == 0 : unsetenv("BENCHVAR29") == 0);
        MEASURE("putenv_remove_cycle_30", 200000L, putenv((i & 1) ? put_a : put_name) == 0);
    }

    /* ENVIRON-TABLE-EXP, ENVIRON-TABLE-CONTRACT: the same lookups in a large environment (300 and 2,000 variables
     * AAA_STRESS_nnnn of 132 bytes, as `bench --stress env`); BENCHVAR_MID sits in the middle and
     * BENCHVAR_LAST last. */
    {
        static const long sizes[2] = {300, 2000};
        static char filler[120];
        memset(filler, 'x', sizeof filler - 1);
        for (int s = 0; s < 2; s++) {
            long n = sizes[s];
            clearenv();
            for (long k = 0; k < n; k++) {
                char name[24];
                snprintf(name, sizeof name, "AAA_STRESS_%04ld", k);
                setenv(name, filler, 1);
                if (k == n / 2)
                    setenv("BENCHVAR_MID", "value", 1);
            }
            setenv("BENCHVAR_LAST", "value", 1);
            if (n == 300) {
                MEASURE("getenv_hit_mid_300", 1000000L, getenv_fn("BENCHVAR_MID") != 0);
                MEASURE("getenv_hit_last_300", 1000000L, getenv_fn("BENCHVAR_LAST") != 0);
                MEASURE("getenv_miss_300", 1000000L, getenv_fn("BENCHNOPE") == 0);
                MEASURE("setenv_300", 200000L, setenv("BENCHVAR_LAST", (i & 1) ? "a" : "b", 1) == 0);
                MEASURE("setenv_getenv_300", 200000L, setenv("BENCHVAR_LAST", (i & 1) ? "a" : "b", 1) == 0 && getenv_fn("BENCHVAR_MID") != 0);
            } else {
                MEASURE("getenv_hit_mid_2000", 200000L, getenv_fn("BENCHVAR_MID") != 0);
                MEASURE("getenv_hit_last_2000", 200000L, getenv_fn("BENCHVAR_LAST") != 0);
                MEASURE("getenv_miss_2000", 200000L, getenv_fn("BENCHNOPE") == 0);
                MEASURE("setenv_2000", 20000L, setenv("BENCHVAR_LAST", (i & 1) ? "a" : "b", 1) == 0);
                static char last_a[] = "BENCHVAR_LAST=a", last_b[] = "BENCHVAR_LAST=b";
                MEASURE("putenv_2000", 20000L, putenv((i & 1) ? last_a : last_b) == 0);
                MEASURE("setenv_getenv_2000", 20000L, setenv("BENCHVAR_LAST", (i & 1) ? "a" : "b", 1) == 0 && getenv_fn("BENCHVAR_MID") != 0);
                static char last_name[] = "BENCHVAR_LAST";
                MEASURE("unsetenv_miss_2000", 20000L, unsetenv("BENCHNOPE") == 0);
                MEASURE("unsetenv_cycle_2000", 20000L, (i & 1) ? putenv(last_a) == 0 : unsetenv("BENCHVAR_LAST") == 0);
                MEASURE("putenv_remove_cycle_2000", 20000L, putenv((i & 1) ? last_a : last_name) == 0);
            }
        }
    }

    MEASURE("errno_location", 20000000L, errno_loc_fn() != 0);
    MEASURE("ctype_b_loc", 20000000L, ctype_b_loc_fn() != 0);
    MEASURE("ctype_get_mb_cur_max", 20000000L, mb_cur_max_fn());

    pthread_mutex_t nm = PTHREAD_MUTEX_INITIALIZER;
    pthread_mutex_t rm;
    pthread_mutexattr_t ma;
    pthread_mutexattr_init(&ma);
    pthread_mutexattr_settype(&ma, PTHREAD_MUTEX_RECURSIVE);
    pthread_mutex_init(&rm, &ma);
    MEASURE("mutex_lock_unlock_normal", 10000000L, mutex_lock_fn(&nm) | mutex_unlock_fn(&nm));
    MEASURE("mutex_lock_unlock_recursive", 10000000L, mutex_lock_fn(&rm) | mutex_unlock_fn(&rm));

    /* System call wrappers: report the user-instruction column only (kernel time is noise). */
    int fd = open("/dev/zero", O_RDONLY);
    if (fd < 0)
        return 1;
    struct stat st;
    char one[8];
    MEASURE("sys_fstat", 2000000L, fstat_fn(fd, &st));
    MEASURE("sys_read_1", 2000000L, read_fn(fd, one, 1));
    MEASURE("sys_close_ebadf", 2000000L, close_fn(-1));
    close(fd);

    /* MUTEX-3: the same mutex rows once the process has had a second thread (created and
     * joined: a C library may keep a cheaper mode only while one thread exists). Last, so every
     * row above runs in a process that never had a thread. */
    pthread_t t;
    if (pthread_create(&t, NULL, idle_thread, NULL) != 0 || pthread_join(t, NULL) != 0)
        return 1;
    MEASURE("mutex_lock_unlock_normal_mt", 10000000L, mutex_lock_fn(&nm) | mutex_unlock_fn(&nm));
    MEASURE("mutex_lock_unlock_recursive_mt", 10000000L, mutex_lock_fn(&rm) | mutex_unlock_fn(&rm));
    return 0;
}
