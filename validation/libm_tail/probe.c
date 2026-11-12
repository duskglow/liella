/* LIBM-TAIL-PROBE (no spec clause; measurement tool, LAB-ONLY): instruction cost of a libm function per input.
 *
 * Cost = user-mode retired instructions (perf_event_open; read through the perf userpage: offset + rdpmc under its
 * seqlock, so a context switch between two readings is accounted for), minus the same reading around an identity
 * function with the same signature through the same code (calibrated at start, minimum of many readings). Built
 * twice from this one file: against the installed glibc (gcc -lm, dynamic, -z now) and against Liella
 * (scripts/liella-cc -lm, static), so both libraries see the same harness and inputs. glibc is only a black box.
 *
 * A counter reading costs ~35 us on the build VMs (rdpmc traps to the hypervisor), so exhaustive f32 runs measure
 * BATCHES of consecutive inputs (exact sums, so per-band costs are exact at batch resolution) and percentiles come
 * from per-input runs over a sample.
 *
 *   probe <func> <mode> <out-prefix> <part> <nparts> [S]      (blocks with key = part mod nparts)
 *   u32  f32 unary, per input: block = bits >> 16; stride S inside a block (one pseudo-random input per run of S).
 *   x32  f32 unary, every input: block = bits >> 16, measured in batches of S consecutive inputs (S | 65536).
 *   b32 / B32  f32 binary: block = (x >> 23) << 9 | (y >> 23), S random inputs; b = per input, B = one batch.
 *   u64 / U64  f64 unary: block = bits >> 46 (64 per binade), S random inputs; u = per input, U = one batch.
 *   b64 / B64  f64 binary: block = (x >> 55) << 9 | (y >> 55), S random inputs; b = per input, B = one batch.
 *   n64  f64 unary per input near the points in env TAIL_POINTS (comma list): block = point index; each point
 *        +-k ulps (k = 1..S) and +-2^j ulps (j = 0..51).
 * Output: <prefix>.blocks.<part> `key n sum max arg arg2 n_over2000` (batch modes: max = largest batch mean,
 * n_over2000 = inputs in batches whose mean is over 2000), <prefix>.hot.<part> `first_input_bits K cost` for
 * every batch with mean over 2000, <prefix>.hist.<part> `binade bucket count` (counts inputs; the value is the
 * per-input cost or, in batch modes, the batch mean; buckets exact below 256, then 16 per octave),
 * <prefix>.meta.<part>. Analysis: validation/libm_tail/analyze.py. Driver: validation/libm_tail/run.sh. */
#define _GNU_SOURCE
#include <linux/perf_event.h>
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/syscall.h>
#include <unistd.h>

typedef float (*f1)(float);
typedef float (*f2)(float, float);
typedef double (*d1)(double);
typedef double (*d2)(double, double);

static float w_sincosf(float x) { float s, c; sincosf(x, &s, &c); return s + c; }
static double w_sincos(double x) { double s, c; sincos(x, &s, &c); return s + c; }
static float w_lgammaf_r(float x) { int s; return lgammaf_r(x, &s); }

struct ent { const char *name; void *fn; };
#define U(n) {#n, (void *)n}
static const struct ent T1F[] = {
    U(acosf), U(acoshf), U(asinf), U(asinhf), U(atanf), U(atanhf), U(cbrtf), U(cosf), U(coshf), U(erff),
    U(erfcf), U(expf), U(exp10f), U(exp2f), U(expm1f), U(j0f), U(j1f), U(lgammaf), U(logf), U(log10f),
    U(log1pf), U(log2f), U(sinf), U(sinhf), U(tanf), U(tanhf), U(tgammaf), U(y0f), U(y1f), U(sqrtf),
    {"sincosf", (void *)w_sincosf}, {"lgammaf_r", (void *)w_lgammaf_r}, {0, 0}};
static const struct ent T2F[] = {U(atan2f), U(powf), U(hypotf), U(fmodf), U(remainderf), {0, 0}};
static const struct ent T1D[] = {
    U(acos), U(acosh), U(asin), U(asinh), U(atan), U(atanh), U(cbrt), U(cos), U(cosh), U(erf),
    U(erfc), U(exp), U(exp10), U(exp2), U(expm1), U(j0), U(j1), U(lgamma), U(log), U(log10),
    U(log1p), U(log2), U(sin), U(sinh), U(tan), U(tanh), U(tgamma), U(y0), U(y1), U(sqrt),
    {"sincos", (void *)w_sincos}, {0, 0}};
static const struct ent T2D[] = {U(atan2), U(pow), U(hypot), U(fmod), U(remainder), {0, 0}};

__attribute__((noinline)) static float id1f(float x) { __asm__ volatile("" : "+x"(x)); return x; }
__attribute__((noinline)) static float id2f(float x, float y) { __asm__ volatile("" : "+x"(x), "+x"(y)); return x; }
__attribute__((noinline)) static double id1d(double x) { __asm__ volatile("" : "+x"(x)); return x; }
__attribute__((noinline)) static double id2d(double x, double y) { __asm__ volatile("" : "+x"(x), "+x"(y)); return x; }

static volatile struct perf_event_mmap_page *pc;
static int pwidth;

static inline uint64_t rd(uint32_t ecx) {
    uint32_t lo, hi;
    __asm__ volatile("lfence; rdpmc; lfence" : "=a"(lo), "=d"(hi) : "c"(ecx) : "memory");
    return ((uint64_t)hi << 32) | lo;
}

/* The task's user instruction count (perf userpage protocol: offset + sign-extended counter under the seqlock). */
static inline int64_t cnt(void) {
    for (;;) {
        uint32_t seq = pc->lock;
        __asm__ volatile("" ::: "memory");
        uint32_t idx = pc->index;
        int64_t off = pc->offset, v = 0;
        if (idx) v = (int64_t)(rd(idx - 1) << (64 - pwidth)) >> (64 - pwidth);
        __asm__ volatile("" ::: "memory");
        if (pc->lock == seq) return off + v;
    }
}

static void pmu_open(void) {
    struct perf_event_attr a;
    memset(&a, 0, sizeof a);
    a.type = PERF_TYPE_HARDWARE;
    a.size = sizeof a;
    a.config = PERF_COUNT_HW_INSTRUCTIONS;
    a.exclude_kernel = 1;
    a.exclude_hv = 1;
    int fd = (int)syscall(SYS_perf_event_open, &a, 0, -1, -1, 0);
    if (fd < 0) { perror("perf_event_open"); exit(3); }
    void *m = mmap(0, (size_t)sysconf(_SC_PAGESIZE), PROT_READ, MAP_SHARED, fd, 0);
    if (m == MAP_FAILED) { perror("mmap perf page"); exit(3); }
    pc = m;
    if (!pc->cap_user_rdpmc || !pc->index || !pc->pmc_width) {
        fprintf(stderr, "probe: rdpmc not available (cap_user_rdpmc=%d index=%u)\n", (int)pc->cap_user_rdpmc, pc->index);
        exit(3);
    }
    pwidth = pc->pmc_width;
}

static uint64_t sinkv;

#define COMMA ,
/* One call, or one batch of n calls, between two readings. Kept out of line so the measured and the calibration
 * runs execute the same code. */
#define ONE(NAME, FT, T, ARGS, CALL)                                                                     \
    __attribute__((noinline)) static int64_t NAME(FT g, ARGS) {                                         \
        int64_t t0 = cnt();                                                                             \
        T v = CALL;                                                                                     \
        int64_t t1 = cnt();                                                                             \
        uint64_t u = 0; memcpy(&u, &v, sizeof v); sinkv += u;                                           \
        return t1 - t0;                                                                                 \
    }
ONE(one_f1, f1, float, float x, g(x))
ONE(one_f2, f2, float, float x COMMA float y, g(x, y))
ONE(one_d1, d1, double, double x, g(x))
ONE(one_d2, d2, double, double x COMMA double y, g(x, y))
#define BAT(NAME, FT, T, CALL)                                                                           \
    __attribute__((noinline)) static int64_t NAME(FT g, const T *xs, const T *ys, int n) {              \
        (void)ys;                                                                                       \
        uint64_t acc = 0;                                                                               \
        int64_t t0 = cnt();                                                                             \
        for (int i = 0; i < n; i++) { T v = CALL; uint64_t u = 0; memcpy(&u, &v, sizeof v); acc += u; } \
        int64_t t1 = cnt();                                                                             \
        sinkv += acc;                                                                                   \
        return t1 - t0;                                                                                 \
    }
BAT(bat_f1, f1, float, g(xs[i]))
BAT(bat_f2, f2, float, g(xs[i], ys[i]))
BAT(bat_d1, d1, double, g(xs[i]))
BAT(bat_d2, d2, double, g(xs[i], ys[i]))

#define NB 768
static uint64_t *hist; /* [nbinades][NB], counts inputs */
static inline int bucket(uint64_t c) {
    if (c < 256) return (int)c;
    int e = 63 - __builtin_clzll(c);
    int b = 256 + (e - 8) * 16 + (int)((c >> (e - 4)) & 15);
    return b < NB ? b : NB - 1;
}

static struct { uint64_t n, sum, max, arg, arg2, over; } B;
static FILE *fb, *fhot;
static void blk_flush(uint64_t key) {
    if (B.n)
        fprintf(fb, "%llu %llu %llu %llu %llx %llx %llu\n", (unsigned long long)key, (unsigned long long)B.n,
                (unsigned long long)B.sum, (unsigned long long)B.max, (unsigned long long)B.arg,
                (unsigned long long)B.arg2, (unsigned long long)B.over);
    memset(&B, 0, sizeof B);
}
/* k inputs costing c in total (k = 1: one input). */
static void rec(int64_t c, uint64_t k, int binade, uint64_t arg, uint64_t arg2) {
    if (c < 0) c = 0;
    uint64_t mean = (uint64_t)c / k;
    if (B.n == 0 || mean > B.max) { B.max = mean; B.arg = arg; B.arg2 = arg2; }
    B.n += k;
    B.sum += (uint64_t)c;
    if (mean > 2000) {
        B.over += k;
        if (k > 1) fprintf(fhot, "%llx %llu %llu\n", (unsigned long long)arg, (unsigned long long)k, (unsigned long long)c);
    }
    hist[(size_t)binade * NB + (size_t)bucket(mean)] += k;
}

static uint64_t sm(uint64_t *s) {
    uint64_t z = (*s += 0x9e3779b97f4a7c15ull);
    z = (z ^ (z >> 30)) * 0xbf58476d1ce4e5b9ull;
    z = (z ^ (z >> 27)) * 0x94d049bb133111ebull;
    return z ^ (z >> 31);
}

static int cmp64(const void *a, const void *b) {
    int64_t x = *(const int64_t *)a, y = *(const int64_t *)b;
    return (x > y) - (x < y);
}

static void *find(const char *name, const struct ent *t) {
    for (int i = 0; t[i].name; i++)
        if (!strcmp(t[i].name, name)) return t[i].fn;
    return 0;
}

#define MAXN 65536
static uint64_t xb[MAXN], yb[MAXN];
static float xf[MAXN], yf[MAXN];
static double xd[MAXN], yd[MAXN];

int main(int argc, char **argv) {
    if (argc < 6) { fprintf(stderr, "usage: probe func u32|x32|b32|B32|u64|U64|b64|B64|n64 prefix part nparts [S]\n"); return 2; }
    const char *fname = argv[1], *mode = argv[2], *pre = argv[3];
    uint64_t part = strtoull(argv[4], 0, 10), np = strtoull(argv[5], 0, 10);
    uint64_t S = argc > 6 ? strtoull(argv[6], 0, 10) : 1;
    /* kind: 0 f32 unary, 1 f32 binary, 2 f64 unary, 3 f64 binary; batch: one reading per K inputs */
    int kind, batch = 0;
    uint64_t nkeys;
    if (!strcmp(mode, "u32")) kind = 0, nkeys = 65536;
    else if (!strcmp(mode, "x32")) kind = 0, batch = 1, nkeys = 65536;
    else if (!strcmp(mode, "b32") || !strcmp(mode, "B32")) kind = 1, batch = mode[0] == 'B', nkeys = 512 * 512;
    else if (!strcmp(mode, "u64") || !strcmp(mode, "U64")) kind = 2, batch = mode[0] == 'U', nkeys = 1u << 18;
    else if (!strcmp(mode, "b64") || !strcmp(mode, "B64")) kind = 3, batch = mode[0] == 'B', nkeys = 512 * 512;
    else if (!strcmp(mode, "n64")) kind = 2, nkeys = 4096;
    else { fprintf(stderr, "probe: bad mode\n"); return 2; }
    if (!np || part >= np || !S || S > MAXN || (mode[0] == 'x' && 65536 % S) || (mode[0] == 'n' && S + 52 > MAXN / 2)) {
        fprintf(stderr, "probe: bad arguments\n");
        return 2;
    }
    void *fn = find(fname, kind == 0 ? T1F : kind == 1 ? T2F : kind == 2 ? T1D : T2D);
    if (!fn) { fprintf(stderr, "probe: unknown function %s for mode %s\n", fname, mode); return 2; }
    uint64_t K = S; /* batch size (batch modes) */
    pmu_open();
    int nbin = kind <= 1 ? 512 : 4096;
    hist = calloc((size_t)nbin * NB, sizeof *hist);
    char path[4096];
    snprintf(path, sizeof path, "%s.blocks.%llu", pre, (unsigned long long)part);
    fb = fopen(path, "w");
    snprintf(path, sizeof path, "%s.hot.%llu", pre, (unsigned long long)part);
    fhot = fopen(path, "w");
    if (!fb || !fhot || !hist) { perror("probe: output"); return 2; }

    /* Calibration: the identity function through the same code; minimum of the readings. */
    int ncal = batch ? 33 : 2001;
    int64_t *cal = malloc(sizeof(int64_t) * (size_t)ncal);
    for (int i = 0; i < ncal; i++) {
        xf[0] = yf[0] = (float)i; xd[0] = yd[0] = i;
        if (batch) {
            cal[i] = kind == 0 ? bat_f1(id1f, xf, yf, (int)K) : kind == 1 ? bat_f2(id2f, xf, yf, (int)K)
                   : kind == 2 ? bat_d1(id1d, xd, yd, (int)K) : bat_d2(id2d, xd, yd, (int)K);
        } else {
            cal[i] = kind == 0 ? one_f1(id1f, xf[0]) : kind == 1 ? one_f2(id2f, xf[0], yf[0])
                   : kind == 2 ? one_d1(id1d, xd[0]) : one_d2(id2d, xd[0], yd[0]);
        }
    }
    qsort(cal, (size_t)ncal, sizeof cal[0], cmp64);
    int64_t overhead = cal[0], cal_med = cal[ncal / 2], cal_max = cal[ncal - 1];

    char *pts = 0, *save = 0, *tok = 0;
    if (mode[0] == 'n') {
        const char *e = getenv("TAIL_POINTS");
        if (!e) { fprintf(stderr, "probe: n64 needs TAIL_POINTS\n"); return 2; }
        pts = strdup(e);
        tok = strtok_r(pts, ",", &save);
    }
    for (uint64_t key = mode[0] == 'n' ? 0 : part; key < nkeys; key += mode[0] == 'n' ? 1 : np) {
        uint64_t n = 0, st = key * 0x9876543ull + 7;
        if (mode[0] == 'n') {
            if (!tok) break;
            double p = strtod(tok, 0);
            tok = strtok_r(0, ",", &save);
            if (key % np != part) continue;
            uint64_t pb; memcpy(&pb, &p, 8);
            for (uint64_t k = 1; k <= S + 52; k++) {
                uint64_t d = k <= S ? k : 1ull << (k - S - 1);
                xb[n++] = pb - d;
                xb[n++] = pb + d;
            }
        } else if (kind == 0) {
            uint64_t step = batch ? 1 : S;
            for (uint64_t lo = 0; lo < 65536; lo += step) xb[n++] = key << 16 | (step > 1 ? lo + sm(&st) % step : lo);
        } else {
            for (uint64_t i = 0; i < S; i++) {
                uint64_t z = sm(&st), w = sm(&st);
                if (kind == 1) xb[n] = (key >> 9) << 23 | (z & 0x7fffff), yb[n] = (key & 511) << 23 | (w & 0x7fffff);
                else if (kind == 2) xb[n] = key << 46 | (z & ((1ull << 46) - 1));
                else xb[n] = (key >> 9) << 55 | (z & ((1ull << 55) - 1)), yb[n] = (key & 511) << 55 | (w & ((1ull << 55) - 1));
                n++;
            }
        }
        for (uint64_t i = 0; i < n; i++) {
            if (kind <= 1) { uint32_t a = (uint32_t)xb[i], b = (uint32_t)yb[i]; memcpy(&xf[i], &a, 4); memcpy(&yf[i], &b, 4); }
            else { memcpy(&xd[i], &xb[i], 8); memcpy(&yd[i], &yb[i], 8); }
        }
        int sh = kind <= 1 ? 23 : 52;
        if (batch) {
            for (uint64_t j = 0; j < n; j += K) {
                int64_t c = kind == 0 ? bat_f1((f1)fn, xf + j, yf + j, (int)K) : kind == 1 ? bat_f2((f2)fn, xf + j, yf + j, (int)K)
                          : kind == 2 ? bat_d1((d1)fn, xd + j, yd + j, (int)K) : bat_d2((d2)fn, xd + j, yd + j, (int)K);
                rec(c - overhead, K, (int)(xb[j] >> sh), xb[j], yb[j]);
            }
        } else {
            for (uint64_t i = 0; i < n; i++) {
                int64_t c = kind == 0 ? one_f1((f1)fn, xf[i]) : kind == 1 ? one_f2((f2)fn, xf[i], yf[i])
                          : kind == 2 ? one_d1((d1)fn, xd[i]) : one_d2((d2)fn, xd[i], yd[i]);
                rec(c - overhead, 1, mode[0] == 'n' ? (int)(key & 4095) : (int)(xb[i] >> sh), xb[i], yb[i]);
            }
        }
        blk_flush(key);
    }
    fclose(fb);
    fclose(fhot);
    snprintf(path, sizeof path, "%s.hist.%llu", pre, (unsigned long long)part);
    FILE *fh = fopen(path, "w");
    if (!fh) { perror("probe: hist"); return 2; }
    for (int b = 0; b < nbin; b++)
        for (int k = 0; k < NB; k++)
            if (hist[(size_t)b * NB + (size_t)k])
                fprintf(fh, "%d %d %llu\n", b, k, (unsigned long long)hist[(size_t)b * NB + (size_t)k]);
    fclose(fh);
    snprintf(path, sizeof path, "%s.meta.%llu", pre, (unsigned long long)part);
    FILE *fm = fopen(path, "w");
    if (!fm) { perror("probe: meta"); return 2; }
    fprintf(fm, "func %s\nmode %s\nS %llu\nbatch %llu\noverhead %lld\ncal_median %lld\ncal_max %lld\nsink %llx\n", fname, mode,
            (unsigned long long)S, (unsigned long long)(batch ? K : 1), (long long)overhead, (long long)cal_med,
            (long long)cal_max, (unsigned long long)sinkv);
    fclose(fm);
    return 0;
}
