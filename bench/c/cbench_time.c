/* Time conversion and zones (phase 9): broken-down time, zones and formatting. The same file
 * builds against the products (`cargo xtask bench`) and against the host C library
 * (`cc -O2 bench/c/cbench_time.c`), so benchmarks/progress.md compares identical code. Prints
 * `name<TAB>ns/op` lines, the best of several rounds; the lines are formatted by hand so that the
 * measured functions do not format their own results. */
#define _GNU_SOURCE
#include <stdlib.h>
#include <string.h>
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

static struct tm *(*volatile gmtime_r_fn)(const time_t *, struct tm *) = gmtime_r;
static struct tm *(*volatile localtime_r_fn)(const time_t *, struct tm *) = localtime_r;
static time_t (*volatile mktime_fn)(struct tm *) = mktime;
static time_t (*volatile timegm_fn)(struct tm *) = timegm;
static size_t (*volatile strftime_fn)(char *, size_t, const char *, const struct tm *) = strftime;

static long lt(time_t t)
{
    struct tm r;
    localtime_r_fn(&t, &r);
    return r.tm_hour;
}

static long mk(long i)
{
    struct tm t;
    memset(&t, 0, sizeof t);
    t.tm_year = 124;
    t.tm_mon = (int)(i % 12);
    t.tm_mday = 15;
    t.tm_hour = 12;
    t.tm_isdst = -1;
    return (long)mktime_fn(&t);
}

static long tg(long i)
{
    struct tm t;
    memset(&t, 0, sizeof t);
    t.tm_year = 124;
    t.tm_mon = (int)(i % 12);
    t.tm_mday = 15;
    return (long)timegm_fn(&t);
}

int main(void)
{
    struct tm r;
    char buf[128];
    time_t base = 1720000000;

    MEASURE("gmtime_r", 2000000, (gmtime_r_fn(&(time_t){base + i * 3607}, &r), r.tm_mday));
    MEASURE("timegm", 2000000, tg(i));
    setenv("TZ", "UTC0", 1);
    tzset();
    MEASURE("localtime_r_utc", 2000000, lt(base + i * 3607));
    setenv("TZ", "America/New_York", 1);
    tzset();
    MEASURE("localtime_r_new_york", 2000000, lt(base + i * 3607));
    MEASURE("localtime_r_new_york_2100", 500000, lt(4102444800 + i * 3607));
    MEASURE("mktime_new_york", 500000, mk(i));
    setenv("TZ", "EST5EDT,M3.2.0,M11.1.0", 1);
    tzset();
    MEASURE("localtime_r_posix_rule", 1000000, lt(base + i * 3607));
    gmtime_r(&base, &r);
    MEASURE("strftime_iso", 1000000, strftime_fn(buf, sizeof buf, "%Y-%m-%dT%H:%M:%S%z", &r));
    MEASURE("strftime_c", 1000000, strftime_fn(buf, sizeof buf, "%c", &r));
    return 0;
}
