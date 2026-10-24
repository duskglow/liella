/* The glibc side of benchmarks/progress.md: the host C library's own functions, timed with the
 * method of crates/boundary/examples/bench_mem.rs and bench_heap.rs (fastest of 41 batches of
 * 2000 calls after 2000 warm-up calls; threaded rows: wall time per operation, best of 5).
 * Prints `name<TAB>ns/op`, with the names the Rust benchmarks use.
 *
 * Build and run: benchmarks/run_glibc_ref.sh (cc -O2; links the host glibc dynamically, so its
 * IFUNCs pick the same AVX2/ERMS paths a normal program gets). */
#define _GNU_SOURCE
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <fcntl.h>
#include <signal.h>
#include <sys/stat.h>
#include <sys/time.h>
#include <unistd.h>

/* Keeps the compiler from removing or merging calls whose results are unused. */
#define KEEP(x) __asm__ volatile("" : : "g"(x) : "memory")

static double now_ns(void)
{
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return (double)t.tv_sec * 1e9 + (double)t.tv_nsec;
}

typedef void (*op_fn)(void *ctx);

static double best_of(op_fn f, void *ctx)
{
    for (int i = 0; i < 2000; i++)
        f(ctx);
    double best = 1e300;
    for (int b = 0; b < 41; b++) {
        double t = now_ns();
        for (int i = 0; i < 2000; i++)
            f(ctx);
        double per = (now_ns() - t) / 2000.0;
        if (per < best)
            best = per;
    }
    return best;
}

struct buf { unsigned char *src, *dst, *other; char *text; size_t n; };

static void do_memcpy(void *c) { struct buf *b = c; KEEP(memcpy(b->dst, b->src, b->n)); }
static void do_memset(void *c) { struct buf *b = c; KEEP(memset(b->dst, 7, b->n)); }
static void do_memcmp(void *c) { struct buf *b = c; KEEP(memcmp(b->src, b->other, b->n)); }
static void do_memchr(void *c) { struct buf *b = c; KEEP(memchr(b->src, 0, b->n)); }
static void do_strlen(void *c) { struct buf *b = c; KEEP(strlen(b->text)); }
static void do_strnlen(void *c) { struct buf *b = c; KEEP(strnlen(b->text, b->n + 1)); }
static void do_rawmemchr(void *c) { struct buf *b = c; KEEP(rawmemchr(b->text, 0)); }

/* Phase 5: the thin kernel-surface calls (crates/boundary/examples/bench_calls.rs). */
static int zero_fd;
static void do_getpid(void *c) { (void)c; KEEP(getpid()); }
static void do_getppid(void *c) { (void)c; KEEP(getppid()); }
static void do_gettid(void *c) { (void)c; KEEP(gettid()); }
static void do_self(void *c) { (void)c; KEEP(pthread_self()); }
static void do_clock_mono(void *c) { (void)c; struct timespec t; KEEP(clock_gettime(CLOCK_MONOTONIC, &t)); KEEP(t.tv_nsec); }
static void do_time(void *c) { (void)c; KEEP(time(NULL)); }
static void do_gettimeofday(void *c) { (void)c; struct timeval t; KEEP(gettimeofday(&t, NULL)); KEEP(t.tv_usec); }
static void do_pagesize(void *c) { (void)c; KEEP(sysconf(_SC_PAGESIZE)); }
static void do_nprocs(void *c) { (void)c; KEEP(sysconf(_SC_NPROCESSORS_ONLN)); }
static void do_read1(void *c) { (void)c; char b; KEEP(read(zero_fd, &b, 1)); }
static void do_sigmask(void *c) { (void)c; sigset_t s; KEEP(sigprocmask(SIG_BLOCK, NULL, &s)); }
static void do_stat(void *c) { (void)c; struct stat st; KEEP(stat("/", &st)); }
static void do_openclose(void *c) { (void)c; int fd = open("/dev/null", O_RDONLY); KEEP(close(fd)); }

struct alloc { size_t n; void *ptrs[1000]; };

static void do_malloc_free(void *c)
{
    struct alloc *a = c;
    void *p = malloc(a->n);
    KEEP(p);
    free(p);
}

static void do_batch(void *c)
{
    struct alloc *a = c;
    for (int i = 0; i < 1000; i++)
        a->ptrs[i] = malloc(64);
    KEEP(a->ptrs);
    for (int i = 0; i < 1000; i++)
        free(a->ptrs[i]);
}

struct thr { pthread_barrier_t *start; long per; };

static void *thread_body(void *arg)
{
    struct thr *t = arg;
    pthread_barrier_wait(t->start);
    for (long i = 0; i < t->per; i++) {
        void *p = malloc(64);
        KEEP(p);
        free(p);
    }
    return NULL;
}

static double threaded(int threads, long per)
{
    double best = 1e300;
    for (int run = 0; run < 5; run++) {
        pthread_barrier_t start;
        pthread_barrier_init(&start, NULL, (unsigned)threads + 1);
        pthread_t tid[64];
        struct thr arg = { &start, per };
        for (int i = 0; i < threads; i++)
            pthread_create(&tid[i], NULL, thread_body, &arg);
        pthread_barrier_wait(&start);
        double t = now_ns();
        for (int i = 0; i < threads; i++)
            pthread_join(tid[i], NULL);
        double per_op = (now_ns() - t) / ((double)threads * (double)per);
        if (per_op < best)
            best = per_op;
        pthread_barrier_destroy(&start);
    }
    return best;
}

int main(void)
{
    static const struct { size_t n; const char *tag; } sizes[] = { { 64, "64" }, { 4096, "4k" }, { 65536, "64k" } };
    for (size_t k = 0; k < 3; k++) {
        size_t n = sizes[k].n;
        struct buf b = { malloc(n), malloc(n), malloc(n), malloc(n + 1), n };
        memset(b.src, 0x5A, n);
        memset(b.other, 0x5A, n);
        memset(b.text, 'x', n);
        b.text[n] = 0;
        printf("glibc_memcpy_%s\t%.2f\n", sizes[k].tag, best_of(do_memcpy, &b));
        printf("glibc_memset_%s\t%.2f\n", sizes[k].tag, best_of(do_memset, &b));
        printf("glibc_memcmp_%s\t%.2f\n", sizes[k].tag, best_of(do_memcmp, &b));
        printf("glibc_memchr_%s\t%.2f\n", sizes[k].tag, best_of(do_memchr, &b));
        printf("glibc_strlen_%s\t%.2f\n", sizes[k].tag, best_of(do_strlen, &b));
        printf("glibc_strnlen_%s\t%.2f\n", sizes[k].tag, best_of(do_strnlen, &b));
        printf("glibc_rawmemchr_%s\t%.2f\n", sizes[k].tag, best_of(do_rawmemchr, &b));
    }
    static const struct { size_t n; const char *tag; } heap[] = { { 64, "64" }, { 4096, "4k" }, { 65536, "64k" }, { 1 << 20, "1m" } };
    static struct alloc a;
    for (size_t k = 0; k < 4; k++) {
        a.n = heap[k].n;
        printf("glibc_malloc_free_%s\t%.2f\n", heap[k].tag, best_of(do_malloc_free, &a));
    }
    printf("glibc_batch_1000x64\t%.2f\n", best_of(do_batch, &a) / 1000.0);
    zero_fd = open("/dev/zero", O_RDONLY);
    printf("glibc_getpid\t%.2f\n", best_of(do_getpid, NULL));
    printf("glibc_getppid\t%.2f\n", best_of(do_getppid, NULL));
    printf("glibc_gettid\t%.2f\n", best_of(do_gettid, NULL));
    printf("glibc_pthread_self\t%.2f\n", best_of(do_self, NULL));
    printf("glibc_clock_gettime_mono\t%.2f\n", best_of(do_clock_mono, NULL));
    printf("glibc_time\t%.2f\n", best_of(do_time, NULL));
    printf("glibc_gettimeofday\t%.2f\n", best_of(do_gettimeofday, NULL));
    printf("glibc_sysconf_pagesize\t%.2f\n", best_of(do_pagesize, NULL));
    printf("glibc_sysconf_nprocs_onln\t%.2f\n", best_of(do_nprocs, NULL));
    printf("glibc_read_devzero_1\t%.2f\n", best_of(do_read1, NULL));
    printf("glibc_sigprocmask_query\t%.2f\n", best_of(do_sigmask, NULL));
    printf("glibc_stat_root\t%.2f\n", best_of(do_stat, NULL));
    printf("glibc_open_close_devnull\t%.2f\n", best_of(do_openclose, NULL));
    int counts[] = { 1, 4, 16 };
    for (int k = 0; k < 3; k++)
        printf("glibc_threads%d_64\t%.2f\n", counts[k], threaded(counts[k], 200000));
    return 0;
}
