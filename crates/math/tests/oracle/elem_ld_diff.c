/* The elementary functions of long double (spec §10.3, D-10.19), end to end on their special and
 * boundary operands (zeros, the smallest subnormal and normal, ±1, ±∞, NaNs quiet and
 * signalling, the domain edges, the overflow and underflow thresholds, huge trigonometric
 * arguments, exact cases, the C Annex F tables of pow and atan2) and on random operands, in
 * every rounding direction. Each line prints the operands' and results' 80-bit encodings (20
 * hex digits), the raised flags and errno. Built against the host C library and statically
 * against Liella's products, the outputs must agree (elem_ld_products.rs) except where Liella is
 * correctly rounded and the host is not. */
#define _GNU_SOURCE
#include <errno.h>
#include <fenv.h>
#include <float.h>
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

static const int MODES[4] = {FE_TONEAREST, FE_UPWARD, FE_DOWNWARD, FE_TOWARDZERO};

static void report(int f, int e) {
    printf(" %c%c%c%c%c %s\n", f & FE_INVALID ? 'I' : '-', f & FE_DIVBYZERO ? 'Z' : '-', f & FE_OVERFLOW ? 'O' : '-',
           f & FE_UNDERFLOW ? 'U' : '-', f & FE_INEXACT ? 'X' : '-', e == 0 ? "0" : e == EDOM ? "EDOM" : e == ERANGE ? "ERANGE" : "E?");
}

static void hex(long double x) {
    uint64_t m;
    uint16_t se;
    memcpy(&m, &x, 8);
    memcpy(&se, (const char *)&x + 8, 2);
    printf(" %04x%016llx", se, (unsigned long long)m);
}

static long double ld(uint16_t se, uint64_t m) {
    long double x = 0;
    memcpy(&x, &m, 8);
    memcpy((char *)&x + 8, &se, 2);
    return x;
}

typedef long double (*l1)(long double);
typedef long double (*l2)(long double, long double);

#define U(n) {#n "l", n##l}
static const struct { const char *name; l1 f; } UN[] = {
    U(exp), U(exp2), U(exp10), U(expm1), U(log), U(log2), U(log10), U(log1p), U(sin), U(cos), U(tan),
    U(asin), U(acos), U(atan), U(sinh), U(cosh), U(tanh), U(asinh), U(acosh), U(atanh), U(cbrt)};
static const struct { const char *name; l2 f; } BI[] = {U(pow), U(atan2)};

static const long double LV[] = {
    0.0L, -0.0L, 0x1p-16445L, -0x1p-16445L, 0x1p-16382L, -0x1p-16382L, 0x1p-34L, -0x1p-35L, 0x1p-66L, -0x1p-68L, 0x1p-100L,
    0.5L, -0.5L, 1.0L, -1.0L, 0x1.0000000000000002p0L, 0x1.fffffffffffffffep-1L, -0x1.fffffffffffffffep-1L, 1.5L, -1.5L,
    2.0L, -2.0L, 3.0L, 10.0L, 100.0L, -100.0L, 11356.0L, 11357.0L, 11356.5234062941439L, -11355.0L, -11399.0L, -11400.0L,
    -11410.0L, 16383.0L, 16384.0L, -16445.0L, -16446.0L, -16447.0L, 4932.0L, 4933.0L, -4950.0L, -4951.0L, 27.0L, 28.0L,
    1e25L, 1e27L, 1e28L, 0x1p63L, -0x1p63L, LDBL_MAX, -LDBL_MAX, 1e300L, 1e4000L, 0x1p16000L, 0x1.921fb54442d18469p0L,
    0x1.921fb54442d18469p1L, 0x1.921fb54442d18469p2L, 0x1p64L, 64.0L, -8.0L, 0.125L, 22.0L, 23.0L, 24.0L, -50.0L, -52.0L,
    40.0L, -40.0L, 22.5L, 0.75L, -0.999L, INFINITY, -INFINITY, NAN, -NAN};
static const long double LP[] = {0.0L, -0.0L, 0x1p-16445L, 0.5L, -0.5L, 1.0L, -1.0L, 2.0L, -2.0L, 3.0L, -3.0L, 0.25L, 1.5L,
                                 9.0L, 16.0L, 0.75L, 1e4000L, -1e4000L, 16383.0L, -16446.0L, INFINITY, -INFINITY, NAN};

/* xorshift64* */
static uint64_t rng = 0x5eedf00dULL;
static uint64_t bits(void) {
    rng ^= rng >> 12;
    rng ^= rng << 25;
    rng ^= rng >> 27;
    return rng * 0x2545f4914f6cdd1dULL;
}
/* A random operand: half moderate (|x| in [2^-8, 2^6)), a quarter near 1, the rest anywhere. */
static long double rand_ld(void) {
    uint64_t r = bits();
    uint64_t m = bits() | (1ULL << 63);
    unsigned e;
    switch (r & 3) {
    case 0: e = 0x3fff - 8 + (unsigned)((r >> 2) % 14); break;
    case 1: e = 0x3fff - 8 + (unsigned)((r >> 2) % 14); break;
    case 2: e = 0x3ffe + (unsigned)((r >> 2) % 2); break;
    default: e = 1 + (unsigned)((r >> 2) % 32766); break;
    }
    return ld((uint16_t)(e | ((r >> 40) & 1 ? 0x8000 : 0)), m);
}

#define NRAND 150

static void one1(const char *name, l1 f, char mc, int mode, long double xv) {
    volatile long double x = xv;
    fesetround(mode);
    feclearexcept(FE_ALL_EXCEPT);
    errno = 0;
    long double r = f(x);
    int fl = fetestexcept(FE_ALL_EXCEPT), e = errno;
    fesetround(FE_TONEAREST);
    printf("%s %c", name, mc);
    hex(x);
    printf(" =");
    hex(r);
    report(fl, e);
}

static void one2(const char *name, l2 f, char mc, int mode, long double xv, long double yv) {
    volatile long double x = xv, y = yv;
    fesetround(mode);
    feclearexcept(FE_ALL_EXCEPT);
    errno = 0;
    long double r = f(x, y);
    int fl = fetestexcept(FE_ALL_EXCEPT), e = errno;
    fesetround(FE_TONEAREST);
    printf("%s %c", name, mc);
    hex(x);
    hex(y);
    printf(" =");
    hex(r);
    report(fl, e);
}

int main(void) {
    const long double snan = __builtin_nansl(""), pnan = __builtin_nanl("0x1234");
    setvbuf(stdout, NULL, _IOFBF, 1 << 16);
    for (int m = 0; m < 4; m++) {
        char mc = "NUDZ"[m];
        for (size_t i = 0; i < sizeof UN / sizeof UN[0]; i++) {
            size_t n = sizeof LV / sizeof LV[0];
            for (size_t j = 0; j < n + 2; j++)
                one1(UN[i].name, UN[i].f, mc, MODES[m], j < n ? LV[j] : j == n ? snan : pnan);
            for (int k = 0; k < NRAND; k++)
                one1(UN[i].name, UN[i].f, mc, MODES[m], rand_ld());
        }
        for (size_t i = 0; i < sizeof BI / sizeof BI[0]; i++) {
            size_t n = sizeof LP / sizeof LP[0];
            for (size_t a = 0; a < n + 1; a++)
                for (size_t b = 0; b < n + 1; b++)
                    one2(BI[i].name, BI[i].f, mc, MODES[m], a < n ? LP[a] : snan, b < n ? LP[b] : snan);
            for (int k = 0; k < NRAND; k++) {
                long double x = rand_ld();
                long double y = rand_ld();
                if (i == 0 && (k & 1)) {
                    /* pow with a positive base and a moderate exponent: finite results. */
                    uint64_t e = bits(), s = bits(), mant = bits();
                    x = fabsl(x);
                    y = ld((uint16_t)((0x3fff - 6 + (unsigned)(e % 12)) | (s & 0x8000)), mant | (1ULL << 63));
                }
                one2(BI[i].name, BI[i].f, mc, MODES[m], x, y);
            }
        }
        /* sincos: both results. */
        for (size_t j = 0; j < sizeof LV / sizeof LV[0]; j++) {
            volatile long double x = LV[j];
            long double s, c;
            fesetround(MODES[m]);
            feclearexcept(FE_ALL_EXCEPT);
            errno = 0;
            sincosl(x, &s, &c);
            int fl = fetestexcept(FE_ALL_EXCEPT), e = errno;
            fesetround(FE_TONEAREST);
            printf("sincosl %c", mc);
            hex(x);
            printf(" =");
            hex(s);
            hex(c);
            report(fl, e);
        }
    }
    return 0;
}
