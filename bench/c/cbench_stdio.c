/* Streams and formatting (phase 8): printf("%d") into a buffer and into a stream, fputs and
 * putc, through fully buffered streams on /dev/null so that the system call is a small part of
 * the cost. The same file builds against the products (`cargo xtask bench`) and against the
 * host C library (`cc -O2 bench/c/cbench_stdio.c`), so benchmarks/progress.md compares
 * identical code. Prints `name<TAB>ns/op` lines, the best of several rounds; the lines are
 * formatted by hand so that the measured functions do not format their own results. */
#undef _FORTIFY_SOURCE
#define _GNU_SOURCE
#include <stdio.h>
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

/* Through volatile pointers, so that each call is made (no header macros or inlining). */
static int (*volatile snprintf_fn)(char *, size_t, const char *, ...) = snprintf;
static int (*volatile fprintf_fn)(FILE *, const char *, ...) = fprintf;
static int (*volatile fputs_fn)(const char *, FILE *) = fputs;
static int (*volatile putc_fn)(int, FILE *) = putc;
static size_t (*volatile fwrite_fn)(const void *, size_t, size_t, FILE *) = fwrite;

static char line[33] = "the quick brown fox jumps over\n";
static char block[4096];

int main(void)
{
    char buf[64];
    FILE *f = fopen("/dev/null", "w");
    if (!f)
        return 1;
    static char fbuf[8192];
    setvbuf(f, fbuf, _IOFBF, sizeof fbuf);
    memset(block, 'x', sizeof block);

    MEASURE("snprintf_d", 2000000, snprintf_fn(buf, sizeof buf, "%d", (int)(i & 0xffff) * 7919));
    MEASURE("snprintf_mixed", 500000, snprintf_fn(buf, sizeof buf, "%s=%d (%5.2f) %x", "key", (int)i, (double)i * 0.25, (unsigned)i));
    MEASURE("fprintf_d", 2000000, fprintf_fn(f, "%d\n", (int)(i & 0xffff) * 7919));
    MEASURE("fputs_32", 2000000, fputs_fn(line, f));
    MEASURE("putc", 10000000, putc_fn('x', f));
    MEASURE("fwrite_4k", 200000, fwrite_fn(block, 1, sizeof block, f));
    fclose(f);
    return 0;
}
