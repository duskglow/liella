/* LIBM-RETHINK-2 prototype timing (Ivy Bridge and N100 timing hosts; no compiler there, so it is built on the build
 * host by build_time.sh): nanoseconds per call of the library's exp and log and of the prototype's exp variants
 * (proto_exp.c: L plain products, LV the same code in VEX encoding without FMA, D Dekker's product, F one FMA).
 * Operands: 4096 uniform in [-10, 10] (the cbench_libm exp range and the seed of proto_percall.c); log operands
 * 10·|x| + 2^-10. Two modes: tput (independent calls, the cbench shape) and lat (each argument depends on the previous
 * result through `+ acc·0`, which adds one multiply and one add to every call's chain). Every call goes through the
 * same function pointer, so all variants pay the same call cost; `ident` measures that cost. Rounds are interleaved
 * across variants and modes; each line prints the median and the minimum of the rounds.
 *   Liella: LIELLA_PRODUCTS=<products> scripts/liella-cc -O2 -o t_liella proto_time.c -lm   (static)
 *   glibc:  gcc -O2 -Wl,-z,now -o t_glibc proto_time.c -lm                                   (dynamic, as wl_percall)
 * no spec clause (measurement probe). */
#define _GNU_SOURCE
#define __STDC_WANT_IEC_60559_TYPES_EXT__ 1
#include <cpuid.h>
#include <stdio.h>
#include <stdlib.h>
#include <time.h>
#include "proto_exp.c"

typedef double (*fn_t)(double);

__attribute__((noinline, target("avx"))) double proto_exp_LV(double x) {
    PROTO_BODY(0.0, EPS_L)
}

/* Latency variants (LIBM-RETHINK-2): the same method, table and test bounds as PROTO_BODY with a shorter dependency
 * chain: k's integer from the bits of the shifted sum t = 1.5·2^52 + k (no cvttsd2si), Estrin's scheme for the
 * polynomial (Estrin 1960; Knuth, TAOCP vol. 2, sec. 4.6.4), and the result taken as zh·2^m, which equals the tested
 * value whenever the test passes (lo == hi implies lo == zh = RN(yh + yl), because |zl| is at most half an ulp of zh
 * on its side), so the test runs beside the result instead of in front of it. MA(a, b, c) = a·b + c: one FMA (E) or a
 * product and a sum (LE; LEV is LE in VEX encoding). Timing experiment: the bound is not re-derived for Estrin's
 * order of operations (the CHECK line compares with the library's exp over the operands). */
#define MA_PLAIN(a, b, c) ((a) * (b) + (c))
#define MA_FMA(a, b, c) __builtin_fma((a), (b), (c))
#define PROTO_BODY_E(MA, UL, EPS)                                                            \
    uint64_t ix = pu64(x);                                                                   \
    uint32_t top = (uint32_t)(ix >> 52) & 0x7ff;                                             \
    unsigned mx;                                                                             \
    __asm__ volatile("stmxcsr %0" : "=m"(mx));                                               \
    if ((uint32_t)(top - 0x3c9) >= 0x3f || (mx & 0x7f80) != 0x1f80) PROTO_FALLBACK           \
    double t = MA(x, PINV, PSHIFT);                                                          \
    double kd = t - PSHIFT;                                                                  \
    int64_t k = (int64_t)(pu64(t) - pu64(PSHIFT));                                           \
    double r1 = MA(-kd, PL1, x);                                                             \
    double rh = MA(-kd, PL2, r1);                                                            \
    double rl = MA(-kd, PL2, r1 - rh);                                                       \
    double r2 = rh * rh;                                                                     \
    double pa = MA(rh, C3, 0.5);                                                             \
    double pb = MA(rh, C5, C4);                                                              \
    double pc = MA(r2, C6, pb);                                                              \
    double q = r2 * MA(r2, pc, pa);                                                          \
    int j = (int)(k & 127);                                                                  \
    int64_t m = k >> 7;                                                                      \
    double th = PT[j].h, tl = PT[j].l;                                                       \
    double u = th * rh;                                                                      \
    double yh = th + u;                                                                      \
    double e1 = (th - yh) + u;                                                               \
    double w = MA(th, rl + q, tl * (1.0 + rh));                                              \
    double yl = (e1 + (UL)) + w;                                                             \
    double zh = yh + yl;                                                                     \
    double zl = (yh - zh) + yl;                                                              \
    double eb = zh * (EPS);                                                                  \
    double sc = pf64((uint64_t)(m + 1023) << 52);                                            \
    if (zh + (zl - eb) != zh + (zl + eb)) PROTO_FALLBACK                                     \
    return zh * sc;

__attribute__((noinline)) double proto_exp_LE(double x) {
    PROTO_BODY_E(MA_PLAIN, 0.0, EPS_L)
}

__attribute__((noinline, target("avx"))) double proto_exp_LEV(double x) {
    PROTO_BODY_E(MA_PLAIN, 0.0, EPS_L)
}

__attribute__((noinline, target("fma"))) double proto_exp_E(double x) {
    PROTO_BODY_E(MA_FMA, MA_FMA(th, rh, -u), EPS_X)
}

__attribute__((noinline)) static double ident(double x) {
    __asm__("" : "+x"(x));
    return x;
}

static double ex[4096], lx[4096];
static volatile double sink;

static void fill(void) {
    uint64_t s = 0x9e3779b97f4a7c15ull;
    for (int i = 0; i < 4096; i++) {
        s ^= s << 13; s ^= s >> 7; s ^= s << 17;
        ex[i] = -10 + 20 * ((double)(s >> 11) * 0x1p-53);
        lx[i] = 10 * (ex[i] < 0 ? -ex[i] : ex[i]) + 0x1p-10;
    }
}

static int has_avx, has_fma;

static void detect(void) {
    unsigned a, b, c, d;
    if (!__get_cpuid(1, &a, &b, &c, &d)) return;
    if ((c & bit_OSXSAVE) && (c & bit_AVX)) {
        unsigned lo, hi;
        __asm__ volatile("xgetbv" : "=a"(lo), "=d"(hi) : "c"(0));
        if ((lo & 6) == 6) {
            has_avx = 1;
            has_fma = (c & bit_FMA) != 0;
        }
    }
}

__attribute__((noinline)) static double run_tput(fn_t f, const double *a, long n) {
    double acc = 0;
    for (long i = 0; i < n; i++) acc += f(a[i & 4095]);
    return acc;
}

__attribute__((noinline)) static double run_lat(fn_t f, const double *a, long n) {
    double acc = 0;
    for (long i = 0; i < n; i++) acc = f(a[i & 4095] + acc * 0.0);
    return acc;
}

static double now_ns(void) {
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return (double)t.tv_sec * 1e9 + (double)t.tv_nsec;
}

static int cmpd(const void *p, const void *q) {
    double a = *(const double *)p, b = *(const double *)q;
    return (a > b) - (a < b);
}

struct variant { const char *name; fn_t f; const double *ops; int on; };

#define MAXR 64

int main(int argc, char **argv) {
    long n = argc > 1 ? atol(argv[1]) : 1000000;
    int rounds = argc > 2 ? atoi(argv[2]) : 7;
    const char *tag = argc > 3 ? argv[3] : "run";
    if (rounds < 1 || rounds > MAXR || n < 4096) { fprintf(stderr, "usage: n>=4096 rounds 1..%d tag\n", MAXR); return 2; }
    detect();
    fill();
    proto_exp_init();
    struct variant v[] = {
        {"ident", ident, ex, 1},
        {"exp_lib", exp, ex, 1},
        {"proto_L", proto_exp_L, ex, 1},
        {"proto_LV", proto_exp_LV, ex, has_avx},
        {"proto_D", proto_exp_D, ex, 1},
        {"proto_F", proto_exp_F, ex, has_fma},
        {"proto_LE", proto_exp_LE, ex, 1},
        {"proto_LEV", proto_exp_LEV, ex, has_avx},
        {"proto_E", proto_exp_E, ex, has_fma},
        {"log_lib", log, lx, 1},
    };
    enum { NV = sizeof v / sizeof v[0] };
    /* Agreement of every prototype variant with the library's exp over the operands (0 expected when the library's
     * exp is correctly rounded; a non-CR library may differ in rare cases) and the fallback rate. */
    for (int k = 2; k < NV - 1; k++) {
        if (!v[k].on) continue;
        long diff = 0, fb0 = (long)proto_fallbacks;
        for (int i = 0; i < 4096; i++) diff += v[k].f(ex[i]) != exp(ex[i]);
        printf("CHECK %s %s mismatches_vs_lib=%ld fallback=%.3f%%\n", tag, v[k].name, diff,
               100.0 * (double)((long)proto_fallbacks - fb0) / 4096);
    }
    static double t[NV][2][MAXR];
    for (int k = 0; k < NV; k++)  /* warm-up */
        if (v[k].on) { sink = run_tput(v[k].f, v[k].ops, 4096); sink = run_lat(v[k].f, v[k].ops, 4096); }
    for (int r = 0; r < rounds; r++)
        for (int k = 0; k < NV; k++) {
            if (!v[k].on) continue;
            double t0 = now_ns();
            sink = run_tput(v[k].f, v[k].ops, n);
            double t1 = now_ns();
            sink = run_lat(v[k].f, v[k].ops, n);
            double t2 = now_ns();
            t[k][0][r] = (t1 - t0) / (double)n;
            t[k][1][r] = (t2 - t1) / (double)n;
        }
    printf("# %s n=%ld rounds=%d avx=%d fma=%d (ns per call: median min)\n", tag, n, rounds, has_avx, has_fma);
    for (int k = 0; k < NV; k++) {
        if (!v[k].on) continue;
        for (int m = 0; m < 2; m++) {
            qsort(t[k][m], (size_t)rounds, sizeof(double), cmpd);
            printf("TIME %s %s %s %.3f %.3f\n", tag, v[k].name, m ? "lat" : "tput", t[k][m][rounds / 2], t[k][m][0]);
        }
    }
    return 0;
}
