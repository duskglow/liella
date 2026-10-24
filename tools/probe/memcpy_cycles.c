/* memcpy_cycles: cycles:u per memcpy call, kernels INTERLEAVED in one process (MEMCPY-POLICY-2, D-4.14).
 * Not a probe of behaviour: a timing tool. Build it twice, static (same source, same flags):
 *   glibc:  gcc -std=gnu17 -O2 -fno-builtin -static -o mc_glibc memcpy_cycles.c
 *   Liella: gcc -std=gnu17 -O2 -fno-builtin -static -nostdlib -I <tree>/include \
 *           <tree>/target/liella/liella-crt1.o memcpy_cycles.c <tree>/target/liella/libc.a -lgcc -o mc_liella
 * Run pinned: taskset -c <cpu> ./mc_x [rounds]. Run 1's cross-process numbers drifted up to 2x on the project
 * VMs, so every comparison here is made inside one process: each round measures, for each case, three kernels
 * in a rotating order:
 *   L  the library's memcpy (glibc; or Liella - with GLIBC_TUNABLES=glibc.cpu.x86_rep_movsb_threshold=
 *      999999999 that is the vector path, without it the shipped policy),
 *   R  `rep movsb` (inline; the same instruction sequence as liella_machine_copy_erms),
 *   A  one unaligned 64-byte store of the head, then `rep movsb` from the next 64-byte-aligned destination.
 * Each sample is the mean cycles per call over a batch (cycles:u of this thread; rdtsc if perf is unavailable,
 * marked in the header line). Output per case: median L, R, A and the medians of the per-round paired ratios
 * R/L and A/L with their 10th-90th percentile range; a range that contains 1.0 does not decide.
 * Places ((dst - src) mod 4 KiB and source offset): a0 both aligned, a2k dst 2 KiB further, s7 source +7 (dst
 * aligned), b7 both +7, d600 dst 600 bytes after the source (misaligned dst, a placement the ERMS gate accepts),
 * d7 dst +7 (rejected by the gate: reference only). memmove ov+64 / ov-64 (overlapping) report L only. */
#define _GNU_SOURCE
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/syscall.h>
#include <unistd.h>

typedef void *(*fn_t)(void *, const void *, size_t);
static fn_t volatile f_memcpy = memcpy;
static fn_t volatile f_memmove = memmove;
static int fc = -1;

static void *k_rep(void *d, const void *s, size_t n) {
    void *r = d;
    __asm__ volatile("rep movsb" : "+D"(d), "+S"(s), "+c"(n) : : "memory");
    return r;
}

static void *k_align(void *d, const void *s, size_t n) {
    void *r = d;
    __builtin_memcpy(d, s, 64);
    size_t h = 64 - ((uintptr_t)d & 63);
    d = (char *)d + h;
    s = (const char *)s + h;
    n -= h;
    __asm__ volatile("rep movsb" : "+D"(d), "+S"(s), "+c"(n) : : "memory");
    return r;
}

static void open_ctr(void) {
    uint64_t a[16] = {0};
    a[0] = (uint64_t)128 << 32;   /* type PERF_TYPE_HARDWARE (0), size 128 */
    a[1] = 0;                     /* cycles */
    a[5] = (1u << 5) | (1u << 6); /* exclude_kernel, exclude_hv; enabled at open */
    long fd = syscall(SYS_perf_event_open, a, 0, -1, -1, 0);
    fc = fd < 0 ? -1 : (int)fd;
}

static uint64_t now(void) {
    if (fc < 0) {
        uint32_t lo, hi;
        __asm__ volatile("lfence; rdtsc; lfence" : "=a"(lo), "=d"(hi));
        return ((uint64_t)hi << 32) | lo;
    }
    uint64_t v = 0;
    if (read(fc, &v, sizeof v) != (ssize_t)sizeof v) {
        perror("read counter");
        exit(3);
    }
    return v;
}

static double batch(fn_t f, char *d, const char *s, size_t n) {
    long iters = (long)(2000000 / (n + 256));
    if (iters < 4) iters = 4;
    if (iters > 5000) iters = 5000;
    for (long k = 0; k < iters / 4 + 1; k++) f(d, s, n);
    uint64_t c0 = now();
    for (long k = 0; k < iters; k++) f(d, s, n);
    return (double)(now() - c0) / (double)iters;
}

static int cmpd(const void *a, const void *b) {
    double x = *(const double *)a, y = *(const double *)b;
    return (x > y) - (x < y);
}

/* q-quantile of v[0..m) (sorts v) */
static double qnt(double *v, int m, double q) {
    qsort(v, (size_t)m, sizeof *v, cmpd);
    return v[(int)(q * (m - 1) + 0.5)];
}

#define MAXR 101
enum { NS = 14, NP = 6, NK = 3 };

int main(int argc, char **argv) {
    int rounds = argc > 1 ? atoi(argv[1]) : 21;
    if (rounds < 3 || rounds > MAXR) rounds = 21;
    static const size_t sizes[NS] = {256, 2048, 4096, 6144, 8192, 10240, 12288, 14336, 16384, 20480, 24576, 32768, 65536, 1048576};
    static const struct { const char *name; size_t sa, d; } places[NP] = {
        {"a0", 0, 0}, {"a2k", 0, 2048}, {"s7", 7, 4089}, {"b7", 7, 0}, {"d600", 0, 600}, {"d7", 0, 7}};
    const size_t max = 1048576, page = 4096, span = 2 * max + 4 * page;
    char *lo = mmap(NULL, 2 * span, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (lo == MAP_FAILED) {
        perror("mmap");
        return 2;
    }
    char *hi = lo + span;
    for (size_t i = 0; i < 2 * span; i++) lo[i] = (char)(i * 13 + 5);
    open_ctr();
    static double t[NS][NP][NK][MAXR], mv[NS][2][MAXR];
    fn_t ks[NK] = {f_memcpy, k_rep, k_align};
    int bad = 0;
    for (int r = 0; r < rounds; r++) {
        for (int si = 0; si < NS; si++) {
            size_t n = sizes[si];
            for (int p = 0; p < NP; p++) {
                char *s = lo + places[p].sa, *d = hi + places[p].sa + places[p].d;
                for (int j = 0; j < NK; j++) {
                    int k = (j + r) % NK;
                    memset(d, 0, n);
                    t[si][p][k][r] = batch(ks[k], d, s, n);
                    if (memcmp(d, s, n) != 0) bad = 1;
                }
            }
            mv[si][0][r] = batch(f_memmove, lo + page + 64, lo + page, n);
            mv[si][1][r] = batch(f_memmove, lo + page, lo + page + 64, n);
        }
    }
    printf("# counter %s rounds %d\n# op\tplace\tn\tL\tR\tA\tR/L\tR/L p10-p90\tA/L\tA/L p10-p90\n",
           fc < 0 ? "rdtsc" : "cycles:u", rounds);
    double v[MAXR];
    for (int si = 0; si < NS; si++) {
        for (int p = 0; p < NP; p++) {
            double med[NK], rq[2][3];
            for (int k = 0; k < NK; k++) {
                memcpy(v, t[si][p][k], sizeof(double) * (size_t)rounds);
                med[k] = qnt(v, rounds, 0.5);
            }
            for (int k = 1; k < NK; k++) {
                for (int r = 0; r < rounds; r++) v[r] = t[si][p][k][r] / t[si][p][0][r];
                rq[k - 1][0] = qnt(v, rounds, 0.1);
                rq[k - 1][1] = qnt(v, rounds, 0.5);
                rq[k - 1][2] = qnt(v, rounds, 0.9);
            }
            printf("memcpy\t%s\t%zu\t%.0f\t%.0f\t%.0f\t%.2f\t%.2f-%.2f\t%.2f\t%.2f-%.2f\n", places[p].name, sizes[si],
                   med[0], med[1], med[2], rq[0][1], rq[0][0], rq[0][2], rq[1][1], rq[1][0], rq[1][2]);
        }
        for (int o = 0; o < 2; o++) {
            memcpy(v, mv[si][o], sizeof(double) * (size_t)rounds);
            double m = qnt(v, rounds, 0.5), a = v[(int)(0.1 * (rounds - 1) + 0.5)], b = v[(int)(0.9 * (rounds - 1) + 0.5)];
            printf("memmove\t%s\t%zu\t%.0f\t-\t-\t-\t%.0f-%.0f\t-\t-\n", o ? "ov-64" : "ov+64", sizes[si], m, a, b);
        }
    }
    if (bad) {
        fprintf(stderr, "memcpy_cycles: a copy differs from its source\n");
        return 1;
    }
    return 0;
}
