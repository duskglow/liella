/* Thread-block reads in a process Liella started (phase 6 budget: `gettid` is a load from the
 * thread block, no system call). Prints `name<TAB>ns/op` lines for `cargo xtask bench`; the
 * best of several rounds. stdio arrives in phase 8, so the output is formatted by hand. */
#define _GNU_SOURCE
#include <pthread.h>
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
    write(1, buf, (size_t)n);
}

#include "cbench_rows.h"

#define MEASURE(name, iters, expr)                         \
    do {                                                   \
        if (cb_skip(name))                                 \
            break;                                         \
        double best = 1e300;                               \
        for (int round = 0; round < 5; round++) {          \
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

/* Through volatile pointers: the headers declare pthread_self `const`, which would let the
 * compiler call it once outside the loop. */
static pthread_t (*volatile self_fn)(void) = pthread_self;
static pid_t (*volatile gettid_fn)(void) = gettid;
static pid_t (*volatile getppid_fn)(void) = getppid;

int main(void)
{
    MEASURE("gettid", 10000000L, gettid_fn());
    MEASURE("pthread_self", 10000000L, self_fn());
    /* A system call through the same layer, for the ratio. */
    MEASURE("getppid", 1000000L, getppid_fn());
    return 0;
}
