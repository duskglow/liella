/* Remaining services (phase 13): message catalogs (§7.7 gettext, §7.8 catgets; D-7.12), POSIX
 * message queues and shared memory (§25; D-25.5), the system logger (§26; D-26.2) and the
 * allocator's introspection (§3; D-3.4, D-3.5). The same file builds against the products
 * (`cargo xtask bench`) and against the host C library (`cc -O2 bench/c/cbench_services.c`), so
 * benchmarks/progress.md compares identical code. Prints `name<TAB>ns/op` lines, the best of
 * several rounds, formatted by hand.
 *
 * Every object the program uses is its own and removed at the end (also when a check fails): a
 * scratch directory under /tmp (mkdtemp) holding a fixture `.mo` (spec Appendix D.2.1, no hash
 * table) at `en_US/LC_MESSAGES/cbench.mo` and a fixture `.cat` (D.2.2), a message queue and a
 * shared-memory object named after the process ID. The catalog rows run in the `en_US.UTF-8`
 * locale with LANGUAGE unset. The logger cannot be pointed at a socket of the program's own
 * without privileges, so nothing is logged: the rows are `vsyslog` with a priority the mask
 * excludes (the masked fast path) and `openlog`/`closelog` without LOG_NDELAY (no connection).
 * Every call is checked once before it is timed; a failure ends the program. */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <libintl.h>
#include <locale.h>
#include <malloc.h>
#include <mqueue.h>
#include <nl_types.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <syslog.h>
#include <time.h>
#include <unistd.h>

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
        put(name, best);                                   \
    } while (0)

/* ---- scratch objects, removed by `cleanup` (registered with atexit) */

static char top[] = "/tmp/cbench_services.XXXXXX";
static int have_top;
static char path_l1[96], path_l2[96], path_mo[128], path_cat[96];
static char mq_name[64], shm_name[64];
static mqd_t mq = (mqd_t)-1;

static void cleanup(void)
{
    if (mq != (mqd_t)-1) {
        mq_close(mq);
        mq_unlink(mq_name);
    }
    shm_unlink(shm_name);
    if (have_top) {
        unlink(path_mo);
        unlink(path_cat);
        rmdir(path_l2);
        rmdir(path_l1);
        rmdir(top);
    }
}

static void fail(const char *what)
{
    (void)!write(2, "cbench_services: unexpected result: ", 36);
    (void)!write(2, what, strlen(what));
    (void)!write(2, "\n", 1);
    exit(1);
}

static void put32(unsigned char *p, uint32_t v)
{
    p[0] = (unsigned char)v;
    p[1] = (unsigned char)(v >> 8);
    p[2] = (unsigned char)(v >> 16);
    p[3] = (unsigned char)(v >> 24);
}

static void put32be(unsigned char *p, uint32_t v)
{
    p[0] = (unsigned char)(v >> 24);
    p[1] = (unsigned char)(v >> 16);
    p[2] = (unsigned char)(v >> 8);
    p[3] = (unsigned char)v;
}

static void write_file(const char *path, const unsigned char *data, size_t n)
{
    int f = open(path, O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC, 0600);
    if (f < 0)
        fail(path);
    if (write(f, data, n) != (ssize_t)n)
        fail(path);
    close(f);
}

/* A little-endian .mo of revision 0 without a hash table (D.2.1): the header entry, a plural
 * entry and a singular one, the originals in increasing byte order. */
static void write_mo(void)
{
    static const char header[] = "Content-Type: text/plain; charset=UTF-8\nPlural-Forms: nplurals=2; plural=n != 1;\n";
    struct { const char *s; uint32_t len; } orig[3] = {
        {"", 0},
        {"apple\0apples", 12},
        {"hello", 5},
    }, trans[3] = {
        {header, sizeof header - 1},
        {"Apfel\0\xc3\x84pfel", 12},
        {"hallo", 5},
    };
    unsigned char buf[512];
    memset(buf, 0, sizeof buf);
    uint32_t n = 3, o = 28, t = o + 8 * n, at = t + 8 * n;
    put32(buf, 0x950412de);
    put32(buf + 4, 0);
    put32(buf + 8, n);
    put32(buf + 12, o);
    put32(buf + 16, t);
    put32(buf + 20, 0);
    put32(buf + 24, at);
    for (uint32_t i = 0; i < n; i++) {
        put32(buf + o + 8 * i, orig[i].len);
        put32(buf + o + 8 * i + 4, at);
        memcpy(buf + at, orig[i].s, orig[i].len);
        at += orig[i].len + 1;
    }
    for (uint32_t i = 0; i < n; i++) {
        put32(buf + t + 8 * i, trans[i].len);
        put32(buf + t + 8 * i + 4, at);
        memcpy(buf + at, trans[i].s, trans[i].len);
        at += trans[i].len + 1;
    }
    write_file(path_mo, buf, at);
}

/* A .cat (D.2.2) with plane size 1 and depth 1: message 1 of set 1 (NL_SETD), "hallo cat". */
static void write_cat(void)
{
    static const char text[] = "hallo cat";
    unsigned char buf[64];
    memset(buf, 0, sizeof buf);
    put32(buf, 0x960408de);
    put32(buf + 4, 1);
    put32(buf + 8, 1);
    put32(buf + 12, 2);
    put32(buf + 16, 1);
    put32(buf + 20, 0);
    put32be(buf + 24, 2);
    put32be(buf + 28, 1);
    put32be(buf + 32, 0);
    memcpy(buf + 36, text, sizeof text);
    write_file(path_cat, buf, 36 + sizeof text);
}

static long mq_roundtrip(void)
{
    char m[64];
    unsigned prio;
    if (mq_send(mq, "ping", 4, 0) != 0)
        return 1;
    return mq_receive(mq, m, sizeof m, &prio) == 4 ? 0 : 1;
}

static long shm_cycle(void)
{
    int f = shm_open(shm_name, O_CREAT | O_EXCL | O_RDWR, 0600);
    if (f < 0)
        return 1;
    long bad = 0;
    if (ftruncate(f, 4096) != 0) {
        bad = 2;
    } else {
        void *p = mmap(NULL, 4096, PROT_READ | PROT_WRITE, MAP_SHARED, f, 0);
        if (p == MAP_FAILED)
            bad = 3;
        else if (munmap(p, 4096) != 0)
            bad = 4;
    }
    if (close(f) != 0)
        bad = 5;
    if (shm_unlink(shm_name) != 0)
        bad = 6;
    return bad;
}

static void logv(int pri, const char *fmt, ...)
{
    va_list ap;
    va_start(ap, fmt);
    vsyslog(pri, fmt, ap);
    va_end(ap);
}

static long masked_log(void)
{
    logv(LOG_DEBUG, "cbench %d", 1);
    return 0;
}

static long open_close_log(void)
{
    openlog("cbench", LOG_PID, LOG_USER);
    closelog();
    return 0;
}

static long info(FILE *f)
{
    return malloc_info(0, f) == 0 ? 0 : 1;
}

static long mi2(void)
{
    struct mallinfo2 m = mallinfo2();
    return (long)m.uordblks;
}

int main(void)
{
    int pid = (int)getpid();
    snprintf(mq_name, sizeof mq_name, "/cbench_services_%d", pid);
    snprintf(shm_name, sizeof shm_name, "/cbench_services_%d", pid);
    if (atexit(cleanup) != 0)
        fail("atexit");
    if (!mkdtemp(top))
        fail("mkdtemp");
    have_top = 1;
    snprintf(path_l1, sizeof path_l1, "%s/en_US", top);
    snprintf(path_l2, sizeof path_l2, "%s/en_US/LC_MESSAGES", top);
    snprintf(path_mo, sizeof path_mo, "%s/en_US/LC_MESSAGES/cbench.mo", top);
    snprintf(path_cat, sizeof path_cat, "%s/hello.cat", top);
    if (mkdir(path_l1, 0700) != 0 || mkdir(path_l2, 0700) != 0)
        fail("mkdir");
    write_mo();
    write_cat();

    /* ---- gettext, ngettext */
    unsetenv("LANGUAGE");
    if (!setlocale(LC_ALL, "en_US.UTF-8"))
        fail("setlocale en_US.UTF-8");
    if (!bindtextdomain("cbench", top) || !textdomain("cbench"))
        fail("bindtextdomain");
    if (strcmp(gettext("hello"), "hallo") != 0)
        fail("gettext found");
    if (strcmp(gettext("missing"), "missing") != 0)
        fail("gettext missing");
    if (strcmp(ngettext("apple", "apples", 3), "\xc3\x84pfel") != 0)
        fail("ngettext");
    MEASURE("gettext_found", 200000, gettext("hello"));
    MEASURE("gettext_missing", 200000, gettext("missing"));
    MEASURE("ngettext_found", 200000, ngettext("apple", "apples", 3));

    /* ---- catgets */
    nl_catd cd = catopen(path_cat, 0);
    if (cd == (nl_catd)-1)
        fail("catopen");
    if (strcmp(catgets(cd, 1, 1, "x"), "hallo cat") != 0)
        fail("catgets");
    MEASURE("catgets_found", 1000000, catgets(cd, 1, 1, "x"));
    if (catclose(cd) != 0)
        fail("catclose");

    /* ---- message queue */
    struct mq_attr attr;
    memset(&attr, 0, sizeof attr);
    attr.mq_maxmsg = 8;
    attr.mq_msgsize = 64;
    mq = mq_open(mq_name, O_CREAT | O_EXCL | O_RDWR | O_CLOEXEC, 0600, &attr);
    if (mq == (mqd_t)-1)
        fail("mq_open");
    if (mq_roundtrip() != 0)
        fail("mq_send/mq_receive");
    MEASURE("mq_send_receive_4", 200000, mq_roundtrip());

    /* ---- shared memory */
    if (shm_cycle() != 0)
        fail("shm cycle");
    MEASURE("shm_open_map_unlink", 20000, shm_cycle());

    /* ---- logger: nothing is sent */
    setlogmask(LOG_MASK(LOG_EMERG));
    if (setlogmask(0) != LOG_MASK(LOG_EMERG))
        fail("setlogmask");
    masked_log();
    MEASURE("vsyslog_masked", 1000000, masked_log());
    open_close_log();
    MEASURE("openlog_closelog", 1000000, open_close_log());

    /* ---- allocator introspection */
    void *p = malloc(100);
    if (!p || malloc_usable_size(p) < 100)
        fail("malloc_usable_size");
    MEASURE("malloc_usable_size", 1000000, malloc_usable_size(p));
    if (mi2() < 100)
        fail("mallinfo2");
    MEASURE("mallinfo2", 200000, mi2());
    free(p);
    FILE *null = fopen("/dev/null", "w");
    if (!null || info(null) != 0)
        fail("malloc_info");
    MEASURE("malloc_info_devnull", 2000, info(null));
    fclose(null);
    return 0;
}
