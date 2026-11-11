/* LIBM-LD-ARC-RETHINK: cycles of x87 asinl, acosl and atan2l per call and per phase, and the latencies of the
 * building blocks a correctly rounded long double arc kernel can be made of. No spec clause (measurement probe).
 *
 * Built three ways by build_probe.sh: gcc -O2 arcprobe.c -lm (glibc), scripts/liella-cc -O2 arcprobe.c -lm
 * (Liella), and the same against Liella products patched by instrument.py, whose phase markers
 * (lfence; rdtsc; lfence) store TSC stamps in the page this program maps at STAMP_ADDR. Unpatched products never
 * write there, so only the per-call figures appear for them.
 *
 *   arcprobe <case> <n> <mode>
 *   case  asinl acosl atan2l_as atan2l_q asinl_fix acosl_fix atan2l_fix asinl_hi asinl_lo acosl_hi ident
 *         mb_imul mb_fsqrt mb_fpatan mb_fyl2x mb_fdiv mb_divsd mb_sqrtsd mb_ldmxcsr mb_fldcw mb_mul64
 *         mb_mulhi128 mb_cvt mb_f80rt
 *   mode  thr: acc += f(t[i]) (independent calls, as the cbench and percall rows)
 *         lat: x = t[i] + r*0, so each call waits for the previous result
 *         tsc: one fenced call at a time: per-call ticks (p10/p50/p90) and per-phase medians
 * Operands: the percall row's generator (validation/percall/libm_ld_arc.c), 64 values cycled. atan2l_as is
 * atan2l(a, sqrtl(1 - a^2)) on asinl's operands (the quotient asinl's kernel sees); atan2l_q is the percall atan2l
 * row (four quadrants); *_fix repeat one operand (t[5]) so every data-dependent branch predicts. */
#define _GNU_SOURCE
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>

#define STAMP_ADDR 0x10000000000UL
#define NSTAMP 7
#define NPT (NSTAMP + 2)
static volatile uint64_t *stamps;
static long double ta[64], tb[64];
static volatile long double sinkld;
static volatile uint64_t sinku;
static volatile double sinkd;

static inline uint64_t tsc_begin(void) {
    uint32_t lo, hi;
    __asm__ volatile("lfence\n\trdtsc\n\tlfence" : "=a"(lo), "=d"(hi) : : "memory");
    return (uint64_t)hi << 32 | lo;
}

static inline uint64_t tsc_end(void) {
    uint32_t lo, hi, aux;
    __asm__ volatile("rdtscp\n\tlfence" : "=a"(lo), "=d"(hi), "=c"(aux) : : "memory");
    return (uint64_t)hi << 32 | lo;
}

static void fill_into(long double *t, double lo, double hi, unsigned s) {
    for (int i = 0; i < 64; i++) {
        s = s * 1103515245u + 12345u;
        double u = (double)(s >> 8) / 16777216.0;
        long double x = (long double)(lo + (hi - lo) * u);
        s = s * 1103515245u + 12345u;
        x += x * ((long double)(s >> 8) / 16777216.0L) * 0x1p-53L;
        t[i] = x;
    }
}

__attribute__((noinline)) static long double ident(long double x) {
    __asm__ volatile("" : : : "memory");
    return x;
}

typedef long double (*f1_t)(long double);
typedef long double (*f2_t)(long double, long double);
struct fcase {
    const char *name;
    f1_t f1;
    f2_t f2;
    int set; /* 0: a in [-1, 1]; 1: [0.5, 1); 2: [0, 0.5); 3: atan2l quadrants on [-10, 10]^2 */
    int as;  /* atan2l(a, sqrt(1 - a^2)) on set's a */
    int fix; /* one operand repeated */
};
static const struct fcase fcases[] = {
    {"asinl", asinl, 0, 0, 0, 0},         {"acosl", acosl, 0, 0, 0, 0},
    {"atan2l_as", 0, atan2l, 0, 1, 0},    {"atan2l_q", 0, atan2l, 3, 0, 0},
    {"asinl_fix", asinl, 0, 0, 0, 1},     {"acosl_fix", acosl, 0, 0, 0, 1},
    {"atan2l_fix", 0, atan2l, 0, 1, 1},   {"asinl_hi", asinl, 0, 1, 0, 0},
    {"asinl_lo", asinl, 0, 2, 0, 0},      {"acosl_hi", acosl, 0, 1, 0, 0},
    {"ident", ident, 0, 0, 0, 0},
};

static void setup(const struct fcase *c) {
    switch (c->set) {
    case 0: fill_into(ta, -1.0, 1.0, 0x2545f491u); break;
    case 1: fill_into(ta, 0.5, 1.0, 0x2545f491u); break;
    case 2: fill_into(ta, 0.0, 0.5, 0x2545f491u); break;
    default:
        fill_into(ta, -10.0, 10.0, 0x2545f491u);
        fill_into(tb, -10.0, 10.0, 0x9e3779b9u);
    }
    if (c->as) /* y = a in tb, x = sqrt(1 - a^2) in ta */
        for (int i = 0; i < 64; i++) {
            tb[i] = ta[i];
            ta[i] = sqrtl((1.0L - tb[i]) * (1.0L + tb[i]));
        }
    if (c->fix)
        for (int i = 0; i < 64; i++) {
            ta[i] = ta[5];
            tb[i] = tb[5];
        }
}

static int off_of(const struct fcase *c) { return c->set == 3 ? 7 : 0; }

static long double loop1(f1_t f, long n, int lat) {
    long double acc = 0, r = 0;
    if (lat)
        for (long i = 0; i < n; i++) {
            r = f(ta[i & 63] + r * 0.0L);
            acc += r;
        }
    else
        for (long i = 0; i < n; i++) acc += f(ta[i & 63]);
    return acc;
}

static long double loop2(f2_t f, long n, int lat, int off) {
    long double acc = 0, r = 0;
    if (lat)
        for (long i = 0; i < n; i++) {
            r = f(tb[i & 63] + r * 0.0L, ta[(i + off) & 63]);
            acc += r;
        }
    else
        for (long i = 0; i < n; i++) acc += f(tb[i & 63], ta[(i + off) & 63]);
    return acc;
}

static int cmpu(const void *a, const void *b) {
    uint32_t x = *(const uint32_t *)a, y = *(const uint32_t *)b;
    return (x > y) - (x < y);
}

static uint32_t pct(uint32_t *v, long m, int p) {
    qsort(v, (size_t)m, sizeof *v, cmpu);
    return m ? v[(m - 1) * p / 100] : 0;
}

/* Fenced calls; the instrumented products stamp: 0 elem entry, 1 after enter, 2 tier entry (operand decoded),
 * 3 after the root (atan2l: right after 2, so 2->3 is one marker's own cost), 4 after the kernel and the
 * composition, 5 after f (rounding and packing), 6 after exit. Phase k is the ticks from the previous present
 * point to point k. */
static void tscrun(const struct fcase *c, long n) {
    static const char *names[NPT - 1] = {"call", "enter", "decode", "root", "kernel", "round", "exit", "ret"};
    int off = off_of(c);
    uint32_t *ph = malloc((size_t)n * (NPT - 1) * sizeof *ph), *tot = malloc((size_t)n * sizeof *tot);
    uint32_t *tmp = malloc((size_t)n * sizeof *tmp);
    long double acc = 0;
    for (long i = 0; i < n; i++) {
        long double x = c->f1 ? ta[i & 63] : tb[i & 63], x2 = ta[(i + off) & 63];
        uint64_t p[NPT];
        for (int k = 0; k < NSTAMP; k++) stamps[k] = 0;
        p[0] = tsc_begin();
        long double r = c->f1 ? c->f1(x) : c->f2(x, x2);
        p[NPT - 1] = tsc_end();
        acc += r;
        for (int k = 0; k < NSTAMP; k++) p[k + 1] = stamps[k];
        tot[i] = (uint32_t)(p[NPT - 1] - p[0]);
        uint64_t prev = p[0];
        for (int k = 1; k < NPT; k++) {
            if (p[k] == 0) {
                ph[i * (NPT - 1) + k - 1] = UINT32_MAX;
                continue;
            }
            ph[i * (NPT - 1) + k - 1] = (uint32_t)(p[k] - prev);
            prev = p[k];
        }
    }
    sinkld = acc;
    long w = n / 10, m = n - w; /* the first tenth warms up */
    uint32_t p10 = pct(tot + w, m, 10), p50 = pct(tot + w, m, 50), p90 = pct(tot + w, m, 90);
    printf("tsc %s n=%ld ticks p10=%u p50=%u p90=%u |", c->name, m, p10, p50, p90);
    for (int k = 0; k < NPT - 1; k++) {
        long q = 0;
        for (long i = w; i < n; i++)
            if (ph[i * (NPT - 1) + k] != UINT32_MAX) tmp[q++] = ph[i * (NPT - 1) + k];
        if (!q) continue;
        if (q == m) printf(" %s=%u", names[k], pct(tmp, q, 50));
        else printf(" %s=%u(%ld%%)", names[k], pct(tmp, q, 50), q * 100 / m);
    }
    printf("\n");
    free(ph);
    free(tot);
    free(tmp);
}

typedef unsigned __int128 u128;
static inline u128 mulhi128(u128 a, u128 b) {
    uint64_t a1 = (uint64_t)(a >> 64), a0 = (uint64_t)a, b1 = (uint64_t)(b >> 64), b0 = (uint64_t)b;
    u128 hh = (u128)a1 * b1, hl = (u128)a1 * b0, lh = (u128)a0 * b1, ll = (u128)a0 * b0;
    u128 mid = (u128)(uint64_t)hl + (uint64_t)lh + (uint64_t)(ll >> 64);
    return hh + (hl >> 64) + (lh >> 64) + (mid >> 64);
}

/* One dependent chain per case; returns the chained operations per iteration (named in the output). */
static int microbench(const char *cs, long n) {
    if (!strcmp(cs, "mb_imul")) { /* 4 imul r64 (3-cycle latency on every class here): calibrates ticks/cycle */
        uint64_t x = 3;
        for (long i = 0; i < n; i++) __asm__ volatile("imul %0, %0\n\timul %0, %0\n\timul %0, %0\n\timul %0, %0" : "+r"(x));
        sinku = x;
        return 4;
    }
    if (!strcmp(cs, "mb_fsqrt")) { /* x87 fsqrt + fadd */
        long double x = 2.5L, c = 1.0L;
        for (long i = 0; i < n; i++) __asm__ volatile("fsqrt\n\tfadd %%st(1), %%st" : "+t"(x) : "u"(c));
        sinkld = x;
        return 1;
    }
    if (!strcmp(cs, "mb_fpatan")) { /* x87 fld1, fpatan + fadd */
        long double x = 0.7L, c = 1.0L;
        for (long i = 0; i < n; i++) __asm__ volatile("fld1\n\tfpatan\n\tfadd %%st(1), %%st" : "+t"(x) : "u"(c));
        sinkld = x;
        return 1;
    }
    if (!strcmp(cs, "mb_fyl2x")) { /* x87 fld1, fxch, fyl2x + fadd */
        long double x = 3.0L, c = 1.5L;
        for (long i = 0; i < n; i++) __asm__ volatile("fld1\n\tfxch\n\tfyl2x\n\tfadd %%st(1), %%st" : "+t"(x) : "u"(c));
        sinkld = x;
        return 1;
    }
    if (!strcmp(cs, "mb_fdiv")) { /* two x87 divisions (x -> c/x -> 1/x) */
        long double x = 1.3L, c = 1.7L;
        for (long i = 0; i < n; i++) __asm__ volatile("fdivr %%st(1), %%st\n\tfdiv %%st(1), %%st" : "+t"(x) : "u"(c));
        sinkld = x;
        return 2;
    }
    if (!strcmp(cs, "mb_divsd")) { /* divsd (x -> c/x) */
        double x = 1.3, c = 1.7;
        for (long i = 0; i < n; i++) {
            x = c / x;
            __asm__ volatile("" : "+x"(x));
        }
        sinkd = x;
        return 1;
    }
    if (!strcmp(cs, "mb_sqrtsd")) { /* sqrtsd + addsd */
        double x = 2.5, c = 1.0;
        for (long i = 0; i < n; i++) __asm__ volatile("sqrtsd %0, %0\n\taddsd %1, %0" : "+x"(x) : "x"(c));
        sinkd = x;
        return 1;
    }
    if (!strcmp(cs, "mb_ldmxcsr")) { /* mb_sqrtsd with an ldmxcsr (same value) per iteration */
        double x = 2.5, c = 1.0;
        uint32_t csr;
        __asm__ volatile("stmxcsr %0" : "=m"(csr));
        for (long i = 0; i < n; i++) __asm__ volatile("ldmxcsr %2\n\tsqrtsd %0, %0\n\taddsd %1, %0" : "+x"(x) : "x"(c), "m"(csr));
        sinkd = x;
        return 1;
    }
    if (!strcmp(cs, "mb_fldcw")) { /* mb_fsqrt with an fldcw (same value) per iteration */
        long double x = 2.5L, c = 1.0L;
        uint16_t cw;
        __asm__ volatile("fnstcw %0" : "=m"(cw));
        for (long i = 0; i < n; i++) __asm__ volatile("fldcw %2\n\tfsqrt\n\tfadd %%st(1), %%st" : "+t"(x) : "u"(c), "m"(cw));
        sinkld = x;
        return 1;
    }
    if (!strcmp(cs, "mb_mul64")) { /* 64x64 -> 128 mul, the high word fed back (xor) */
        uint64_t a = 0x9e3779b97f4a7c15ULL, b = 0xc2b2ae3d27d4eb4fULL;
        for (long i = 0; i < n; i++) {
            u128 p = (u128)a * b;
            a = (uint64_t)(p >> 64) ^ (uint64_t)p;
            __asm__ volatile("" : "+r"(a));
        }
        sinku = a;
        return 1;
    }
    if (!strcmp(cs, "mb_mulhi128")) { /* high half of a 128x128 product (four mul, adds) */
        u128 a = ((u128)0x9e3779b97f4a7c15ULL << 64) | 0x3c6ef372fe94f82bULL;
        u128 b = ((u128)0xc2b2ae3d27d4eb4fULL << 64) | 0x165667b19e3779f9ULL;
        for (long i = 0; i < n; i++) {
            a = mulhi128(a, b) | ((u128)1 << 127);
            uint64_t lo = (uint64_t)a;
            __asm__ volatile("" : "+r"(lo));
            a = (a >> 64 << 64) | lo;
        }
        sinku = (uint64_t)a;
        return 1;
    }
    if (!strcmp(cs, "mb_cvt")) { /* cvtsi2sd + cvttsd2si round trip */
        int64_t x = 123456789;
        for (long i = 0; i < n; i++) {
            double d = (double)x;
            __asm__ volatile("" : "+x"(d));
            x = (int64_t)d + 1;
            __asm__ volatile("" : "+r"(x));
        }
        sinku = (uint64_t)x;
        return 1;
    }
    if (!strcmp(cs, "mb_f80rt")) { /* integer words -> fldt -> fstpt -> integer words (both boundary moves) */
        static uint64_t buf[4] __attribute__((aligned(16)));
        uint64_t m = 0xc90fdaa22168c234ULL, e = 0x3fff;
        for (long i = 0; i < n; i++)
            __asm__ volatile("movq %0, (%2)\n\tmovw %w1, 8(%2)\n\tfldt (%2)\n\tfstpt 16(%2)\n\tmovq 16(%2), %0\n\tmovzwl 24(%2), %k1"
                             : "+r"(m), "+r"(e) : "r"(buf) : "memory");
        sinku = m ^ e;
        return 1;
    }
    return 0;
}

int main(int argc, char **argv) {
    if (argc < 4) {
        fprintf(stderr, "usage: arcprobe <case> <n> <thr|lat|tsc>\n");
        return 2;
    }
    void *pg = mmap((void *)STAMP_ADDR, 4096, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS | MAP_FIXED_NOREPLACE, -1, 0);
    if (pg == MAP_FAILED || (uintptr_t)pg != STAMP_ADDR) {
        perror("arcprobe: mmap of the stamp page");
        return 1;
    }
    stamps = pg;
    const char *cs = argv[1], *mode = argv[3];
    long n = atol(argv[2]);
    if (!strncmp(cs, "mb_", 3)) {
        uint64_t t0 = tsc_begin();
        int ops = microbench(cs, n);
        uint64_t t1 = tsc_end();
        if (!ops) {
            fprintf(stderr, "arcprobe: unknown case %s\n", cs);
            return 2;
        }
        printf("%s n=%ld ops/iter=%d ticks/iter=%.2f\n", cs, n, ops, n ? (double)(t1 - t0) / (double)n : 0.0);
        return 0;
    }
    const struct fcase *c = 0;
    for (size_t i = 0; i < sizeof fcases / sizeof fcases[0]; i++)
        if (!strcmp(fcases[i].name, cs)) c = &fcases[i];
    if (!c) {
        fprintf(stderr, "arcprobe: unknown case %s\n", cs);
        return 2;
    }
    setup(c);
    int off = off_of(c);
    if (!strcmp(mode, "tsc")) {
        tscrun(c, n);
        return 0;
    }
    int lat = !strcmp(mode, "lat");
    uint64_t t0 = tsc_begin();
    long double acc = c->f1 ? loop1(c->f1, n, lat) : loop2(c->f2, n, lat, off);
    uint64_t t1 = tsc_end();
    sinkld = acc;
    long double chk = c->f1 ? c->f1(ta[5]) : c->f2(tb[5], ta[(5 + off) & 63]);
    printf("%s %s n=%ld ticks/call=%.1f check=%.18Lg\n", cs, mode, n, n ? (double)(t1 - t0) / (double)n : 0.0, chk);
    return 0;
}
