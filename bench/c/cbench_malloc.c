/* Allocator rows (BENCH-HOT, 2026-10-09): malloc/free pairs at the sizes the call-frequency
 * survey saw, a LIFO batch of mixed sizes, free of another thread's block, realloc growth and
 * calloc. The same file builds against the products (`cargo xtask bench`) and against the host
 * C library, so the rows compare identical code. Prints `name<TAB>ns/op` lines, the best of
 * several rounds; every call goes through a volatile pointer so that it is really made.
 * Counters (CBENCH_COUNTERS=1) cover the calling thread only: xthread_free has none (its
 * spin-wait instruction count is not deterministic). */
#undef _FORTIFY_SOURCE
#define _GNU_SOURCE
#include <pthread.h>
#include <stdatomic.h>
#include <stdio.h>
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

static void *(*volatile malloc_fn)(size_t) = malloc;
static void *(*volatile calloc_fn)(size_t, size_t) = calloc;
static void *(*volatile realloc_fn)(void *, size_t) = realloc;
static void (*volatile free_fn)(void *) = free;

/* One malloc + free of `n` bytes; the first byte is written so the block is really used. */
static __attribute__((noinline)) long pair(size_t n)
{
    char *p = malloc_fn(n);
    if (!p)
        return -1;
    p[0] = 1;
    free_fn(p);
    return 1;
}

static __attribute__((noinline)) long calloc_pair(size_t n)
{
    char *p = calloc_fn(1, n);
    if (!p)
        return -1;
    long r = p[n - 1];
    free_fn(p);
    return r + 1;
}

/* 64 allocations of mixed sizes (16..1024, a fixed sequence), freed in reverse order. */
static const unsigned short mixed[16] = {16, 48, 24, 200, 64, 32, 1024, 16, 96, 512, 40, 128, 256, 24, 72, 160};
static __attribute__((noinline)) long lifo64(void)
{
    void *p[64];
    for (int k = 0; k < 64; k++) {
        p[k] = malloc_fn(mixed[k & 15] + (size_t)(k >> 4) * 8);
        if (!p[k])
            return -1;
        *(char *)p[k] = 1;
    }
    for (int k = 63; k >= 0; k--)
        free_fn(p[k]);
    return 64;
}

/* Grow a block from 16 B to 4096 B, doubling, then free it (8 reallocs). */
static __attribute__((noinline)) long grow_x2(void)
{
    size_t n = 16;
    char *p = malloc_fn(n);
    if (!p)
        return -1;
    for (n = 32; n <= 4096; n *= 2) {
        p[0] = 1;
        p = realloc_fn(p, n);
        if (!p)
            return -1;
    }
    free_fn(p);
    return 1;
}

/* Grow in 16-byte steps from 16 B to 1024 B (63 reallocs), the way a string builder does. */
static __attribute__((noinline)) long grow_16step(void)
{
    size_t n = 16;
    char *p = malloc_fn(n);
    if (!p)
        return -1;
    for (n = 32; n <= 1024; n += 16) {
        p[0] = 1;
        p = realloc_fn(p, n);
        if (!p)
            return -1;
    }
    free_fn(p);
    return 1;
}

/* Free of another thread's block: this thread allocates 256 blocks of 64 B, the helper frees
 * them, and the two hand over through an atomic flag (spin). ns per malloc + remote free,
 * handover cost amortised over the batch. */
#define XB 256
static void *xblocks[XB];
static atomic_int xstate; /* 0: owner fills, 1: helper frees, 2: helper exits */

static void *helper(void *arg)
{
    (void)arg;
    for (;;) {
        int s;
        while ((s = atomic_load_explicit(&xstate, memory_order_acquire)) == 0)
            __builtin_ia32_pause();
        if (s == 2)
            return NULL;
        for (int k = 0; k < XB; k++)
            free_fn(xblocks[k]);
        atomic_store_explicit(&xstate, 0, memory_order_release);
    }
}

static __attribute__((noinline)) int xbatch(void)
{
    for (int k = 0; k < XB; k++) {
        xblocks[k] = malloc_fn(64);
        if (!xblocks[k])
            return -1;
        *(char *)xblocks[k] = 1;
    }
    atomic_store_explicit(&xstate, 1, memory_order_release);
    while (atomic_load_explicit(&xstate, memory_order_acquire) != 0)
        __builtin_ia32_pause();
    return 0;
}

/* Realloc rows (the survey: realloc is 10% of heap calls and the costliest). Within usable size:
 * a 17 B block asked for 24 B stays put in both allocators (every 16-byte-granule chunk holds
 * 24 B). Moving: a guard block follows, so growing to 200 B cannot extend in place. */
static __attribute__((noinline)) long realloc_usable(void)
{
    char *p = malloc_fn(17);
    if (!p)
        return -1;
    p = realloc_fn(p, 24);
    if (!p)
        return -1;
    p[23] = 1;
    free_fn(p);
    return 1;
}

static __attribute__((noinline)) long realloc_moving(void)
{
    char *p = malloc_fn(24);
    char *g = malloc_fn(24);
    if (!p || !g)
        return -1;
    p = realloc_fn(p, 200);
    if (!p)
        return -1;
    p[199] = 1;
    free_fn(p);
    free_fn(g);
    return 2;
}

static __attribute__((noinline)) long realloc_null24(void)
{
    char *p = realloc_fn(NULL, 24);
    if (!p)
        return -1;
    p[0] = 1;
    free_fn(p);
    return 1;
}

/* heap_mix_real: the measured call mix of 15 real workloads (ARG-HIST 2026-10-09), replayed from a
 * fixed seed. 128 slots per thread; each step picks a slot and an operation:
 *   22% free(NULL) (the survey's 22% of frees); otherwise
 *   empty slot: malloc 82%, calloc 4%, realloc(NULL, 16..31) 14% (21% of reallocs are realloc(NULL))
 *   full slot:  free 70% (random slot: out of order), realloc 30% (grow by doubling; chains start at
 *               16-31 B; a block that would pass 4 KiB is freed instead)
 * malloc sizes: 37% 1-15 B, 11% 16-31, 11% 32-47, 3% 48-63 (62% <= 63 B), 6% 64-127,
 * 20% 128-255, 7% 256-1023 (95% <= 1 KiB), 4% 1-8 KiB, 1% 8-64 KiB (see mix_size). */
#define SLOTS 128
struct mix {
    unsigned long long rng;
    void *p[SLOTS];
    unsigned sz[SLOTS];
};

static inline unsigned long long next(struct mix *m)
{
    unsigned long long x = m->rng;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    m->rng = x;
    return x * 0x2545F4914F6CDD1DULL;
}

static unsigned mix_size(struct mix *m)
{
    unsigned long long r = next(m);
    unsigned c = (unsigned)((r >> 40) % 100);
    unsigned long long v = r >> 8;
    if (c < 37)
        return 1 + (unsigned)(v % 15);
    if (c < 48)
        return 16 + (unsigned)(v % 16);
    if (c < 59)
        return 32 + (unsigned)(v % 16);
    if (c < 62)
        return 48 + (unsigned)(v % 16);
    if (c < 68)
        return 64 + (unsigned)(v % 64);
    if (c < 88)
        return 128 + (unsigned)(v % 128);
    if (c < 95)
        return 256 + (unsigned)(v % 768);
    if (c < 99)
        return 1024 + (unsigned)(v % 7168);
    return 8192 + (unsigned)(v % 57344);
}

static __attribute__((noinline)) long mix_step(struct mix *m)
{
    unsigned long long r = next(m);
    unsigned slot = (unsigned)(r >> 33) % SLOTS;
    unsigned c = (unsigned)((r >> 20) % 100);
    unsigned c2 = (unsigned)((r >> 12) % 100);
    if (c < 22) {
        free_fn(NULL);
        return 0;
    }
    void *p = m->p[slot];
    if (!p) {
        unsigned n;
        if (c2 < 82) {
            n = mix_size(m);
            p = malloc_fn(n);
        } else if (c2 < 86) {
            n = mix_size(m);
            p = calloc_fn(1, n);
        } else {
            n = 16 + (unsigned)((r >> 44) % 16);
            p = realloc_fn(NULL, n);
        }
        if (!p)
            return -1;
        *(char *)p = 1;
        m->p[slot] = p;
        m->sz[slot] = n;
        return 1;
    }
    if (c2 < 70) {
        free_fn(p);
        m->p[slot] = NULL;
        return 2;
    }
    unsigned n = m->sz[slot] * 2;
    if (n > 4096) {
        free_fn(p);
        m->p[slot] = NULL;
        return 2;
    }
    p = realloc_fn(p, n);
    if (!p)
        return -1;
    *(char *)p = 1;
    m->p[slot] = p;
    m->sz[slot] = n;
    return 3;
}

static void mix_drain(struct mix *m)
{
    for (int k = 0; k < SLOTS; k++) {
        free_fn(m->p[k]);
        m->p[k] = NULL;
    }
}

static struct mix mix1 = {88172645463325252ULL, {0}, {0}};

/* Two-thread variant: both threads run the mix; every 8th step each posts a live block in the
 * other's mailbox (freeing what was there), and each step drains its own mailbox, so blocks are
 * freed by a thread other than the allocating one. The timed thread is the main one; the helper
 * runs the same mix concurrently until the main thread is done. */
static atomic_long mbox[2];
static atomic_int go2, stop2;

static __attribute__((noinline)) long mix_step2(struct mix *m, int me, unsigned long k)
{
    long r = mix_step(m);
    if ((k & 7) == 0) {
        unsigned slot = (unsigned)(next(m) >> 33) % SLOTS;
        void *q = m->p[slot];
        if (q) {
            m->p[slot] = NULL;
            void *old = (void *)atomic_exchange_explicit(&mbox[1 - me], (long)q, memory_order_acq_rel);
            free_fn(old);
        }
    }
    void *mine = (void *)atomic_exchange_explicit(&mbox[me], 0, memory_order_acq_rel);
    free_fn(mine);
    return r;
}

static struct mix mix2[2] = {{0x9E3779B97F4A7C15ULL, {0}, {0}}, {0xD1B54A32D192ED03ULL, {0}, {0}}};
#define MIX2_N 2000000UL
static void *mix2_helper(void *arg)
{
    (void)arg;
    while (!atomic_load_explicit(&go2, memory_order_acquire))
        __builtin_ia32_pause();
    while (!atomic_load_explicit(&stop2, memory_order_acquire))
        for (unsigned long k = 0; k < 1000; k++)
            mix_step2(&mix2[1], 1, k);
    return NULL;
}

int main(void)
{
    MEASURE("malloc_free_16", 10000000L, pair(16));
    MEASURE("malloc_free_32", 10000000L, pair(32));
    MEASURE("malloc_free_64", 10000000L, pair(64));
    MEASURE("malloc_free_128", 10000000L, pair(128));
    MEASURE("malloc_free_256", 10000000L, pair(256));
    MEASURE("malloc_free_1024", 10000000L, pair(1024));
    MEASURE("lifo_batch64", 300000L, lifo64());
    MEASURE("realloc_grow_x2", 1000000L, grow_x2());
    MEASURE("realloc_grow_16step", 200000L, grow_16step());
    MEASURE("calloc_free_64", 5000000L, calloc_pair(64));
    MEASURE("calloc_free_4096", 1000000L, calloc_pair(4096));

    MEASURE("realloc_within_usable", 10000000L, realloc_usable());
    MEASURE("realloc_moving", 5000000L, realloc_moving());
    MEASURE("realloc_null_24", 10000000L, realloc_null24());
    MEASURE("heap_mix_real", 20000000L, mix_step(&mix1));
    mix_drain(&mix1);

    pthread_t t;
    if (pthread_create(&t, NULL, helper, NULL) != 0)
        return 1;
    double best = 1e300;
    for (int round = 0; round < 5; round++) {
        double t0 = now();
        for (int b = 0; b < 2000; b++)
            if (xbatch())
                return 1;
        double per = (now() - t0) / (2000.0 * XB);
        if (per < best)
            best = per;
    }
    atomic_store_explicit(&xstate, 2, memory_order_release);
    pthread_join(t, NULL);
    put("xthread_free_64", best);

    /* heap_mix_real_2t: ns per step of the main thread while the helper runs the same mix. */
    pthread_t t2;
    if (pthread_create(&t2, NULL, mix2_helper, NULL) != 0)
        return 1;
    atomic_store_explicit(&go2, 1, memory_order_release);
    best = 1e300;
    for (int round = 0; round < 5; round++) {
        double t0 = now();
        for (unsigned long k = 0; k < MIX2_N; k++)
            sink = mix_step2(&mix2[0], 0, k);
        double per = (now() - t0) / (double)MIX2_N;
        if (per < best)
            best = per;
    }
    atomic_store_explicit(&stop2, 1, memory_order_release);
    pthread_join(t2, NULL);
    put("heap_mix_real_2t", best);
    return 0;
}
