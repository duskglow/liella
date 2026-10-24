/* The elementary functions of _Float128 (spec §10.3, D-10.19): the exponentials, logarithms, pow
 * and cbrt, the trigonometric and hyperbolic functions and their inverses, atan2 and sincos, end
 * to end on their special and boundary operands (zeros, the smallest subnormal and normal, ±1 and its
 * neighbours, ±∞, NaNs quiet and signalling, the tiny-argument cut-offs, the overflow and
 * underflow thresholds including the subnormal results below e^-11400 that x87 does not have,
 * the exact cases: powers of two and ten, perfect cubes, the dyadic results of pow, the C
 * Annex F tables of pow and atan2, the multiples of π and huge arguments that need the exact
 * argument reduction, the saturation of tanh) and on random operands, in every rounding direction. Each line prints
 * the operands' and results' 128-bit encodings (32 hex digits), the raised flags and errno.
 * Built against the host C library and statically against Liella's products, the outputs must
 * agree (elem_f128_products.rs) except where Liella is correctly rounded and the host is not. */
#define _GNU_SOURCE
#define __STDC_WANT_IEC_60559_TYPES_EXT__ 1
#include <errno.h>
#include <fenv.h>
#include <float.h>
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

typedef _Float128 q;

static const int MODES[4] = {FE_TONEAREST, FE_UPWARD, FE_DOWNWARD, FE_TOWARDZERO};

static void report(int f, int e) {
    printf(" %c%c%c%c%c %s\n", f & FE_INVALID ? 'I' : '-', f & FE_DIVBYZERO ? 'Z' : '-', f & FE_OVERFLOW ? 'O' : '-',
           f & FE_UNDERFLOW ? 'U' : '-', f & FE_INEXACT ? 'X' : '-', e == 0 ? "0" : e == EDOM ? "EDOM" : e == ERANGE ? "ERANGE" : "E?");
}

static void hex(q x) {
    uint64_t w[2];
    memcpy(w, &x, 16);
    printf(" %016llx%016llx", (unsigned long long)w[1], (unsigned long long)w[0]);
}

static q mk(uint64_t hi, uint64_t lo) {
    uint64_t w[2] = {lo, hi};
    q x;
    memcpy(&x, w, 16);
    return x;
}

typedef q (*q1)(q);
typedef q (*q2)(q, q);

#define U(n) {#n "f128", n##f128}
static const struct { const char *name; q1 f; } UN[] = {
    U(exp), U(exp2), U(exp10), U(expm1), U(log), U(log2), U(log10), U(log1p), U(cbrt), U(sin), U(cos), U(tan),
    U(asin), U(acos), U(atan), U(sinh), U(cosh), U(tanh), U(asinh), U(acosh), U(atanh)};
static const struct { const char *name; q2 f; } BI[] = {U(pow), U(atan2)};

static const q LV[] = {
    0.0f128, -0.0f128, 0x1p-16494f128, -0x1p-16494f128, 0x1p-16382f128, -0x1p-16382f128, 0x1p-57f128, -0x1p-58f128, 0x1p-59f128,
    0x1p-115f128, -0x1p-116f128, 0x1p-117f128, -0x1p-118f128, 0x1p-200f128, 0.5f128, -0.5f128, 1.0f128, -1.0f128,
    0x1.0000000000000000000000000001p0f128, 0x1.ffffffffffffffffffffffffffffp-1f128, -0x1.ffffffffffffffffffffffffffffp-1f128,
    1.5f128, -1.5f128, 2.0f128, -2.0f128, 3.0f128, 10.0f128, 100.0f128, -100.0f128, -80.0f128, -85.0f128, -86.0f128, -90.0f128,
    11356.0f128, 11357.0f128, 11356.5234062941439494919310779707648912527f128, -11355.0f128, -11399.0f128, -11400.0f128,
    -11410.0f128, -11420.0f128, -11430.0f128, -11432.5f128, -11433.0f128, -11433.5f128, -11434.0f128, -11440.0f128, 16383.0f128,
    16384.0f128, -16382.5f128, -16445.0f128, -16460.5f128, -16494.0f128, -16494.5f128, -16495.0f128, -16496.0f128, 4932.0f128,
    4933.0f128, -4950.0f128, -4951.0f128, -4960.5f128, -4965.0f128, -4966.0f128, -4967.0f128, 27.0f128, 28.0f128, 48.0f128,
    49.0f128, 55.0f128, 56.0f128, 1e25f128, 1e27f128, 1e28f128, 1e48f128, 1e49f128, 1e55f128, 1e56f128, 0x1p63f128, 0x1p112f128,
    -0x1p112f128, 0x1p113f128, FLT128_MAX, -FLT128_MAX, 1e300f128, 1e4000f128, 0x1p16000f128, 64.0f128, -8.0f128, 0.125f128,
    22.0f128, 23.0f128, 24.0f128, -50.0f128, -52.0f128, 40.0f128, -40.0f128, 22.5f128, 0.75f128, -0.999f128, 3375.0f128,
    -3375.0f128, 0x1p-16491f128, 0x1.8p-16493f128, 1.728f128, 0.001f128, 0x1.921fb54442d18469898cc51701b8p0f128,
    0x1.921fb54442d18469898cc51701b8p1f128, -0x1.921fb54442d18469898cc51701b8p1f128, 0x1.921fb54442d18469898cc51701b8p2f128,
    0x1.921fb54442d18469898cc51701b8p-1f128, 0x1.921fb54442d18469898cc51701b8p16383f128, 0x1.921fb54442d18469898cc51701b8p112f128,
    0x1p1000f128, -0x1p8000f128, 1e22f128, 39.0f128, 39.5f128, 40.5f128, 45.0f128, 11356.5f128, -11357.0f128, 0x1p-56f128,
    -0x1p-56f128, 0x1.0000000000000000000000000002p0f128, 1.0000001f128, 0.99999f128, INFINITY, -INFINITY, NAN, -NAN};
static const q LP[] = {0.0f128, -0.0f128, 0x1p-16494f128, 0.5f128, -0.5f128, 1.0f128, -1.0f128, 2.0f128, -2.0f128, 3.0f128,
                       -3.0f128, 0.25f128, 1.5f128, 9.0f128, 16.0f128, 0.75f128, 1e4000f128, -1e4000f128, 16383.0f128,
                       -16494.0f128, -16446.0f128, 113.0f128, INFINITY, -INFINITY, NAN};
/* pow pairs with exact or nearly exact dyadic results: short odd parts, roots that are exact, odd
 * parts at, just below and just above 113 bits. */
static const q PP[][2] = {
    {9.0f128, 1.5f128}, {16.0f128, 0.75f128}, {2.25f128, 1.5f128}, {6.25f128, 2.5f128}, {-3.0f128, 3.0f128}, {81.0f128, 0.25f128},
    {0.5625f128, 2.5f128}, {3.0f128, 40.0f128}, {3.0f128, 71.0f128}, {3.0f128, 72.0f128}, {3.0f128, 80.0f128}, {-3.0f128, 71.0f128},
    {5.0f128, 48.0f128}, {5.0f128, 49.0f128}, {10.0f128, 48.0f128}, {10.0f128, -3.0f128}, {7.0f128, 40.0f128}, {7.0f128, 41.0f128},
    {0x1.0000000000000000000000000001p0f128, 2.0f128}, {0x1.0000000000000000000000000001p0f128, 3.0f128},
    {0x1.ffffffffffffffffffffffffffffp-1f128, 4.0f128}, {0x1p-8000f128, 2.0f128}, {0x1p-8000f128, 2.0625f128},
    {2.0f128, -16494.0f128}, {2.0f128, -16494.5f128}, {2.0f128, -16460.5f128}, {4.0f128, -8240.25f128}, {10.0f128, -4960.0f128},
    {1e-2000f128, 5.0f128}, {1e-2000f128, 5.71f128}, {3.0f128, 0.5f128}, {4.0f128, 0.5f128}, {6561.0f128, 0.125f128},
    {43046721.0f128, 0.0625f128}, {1853020188851841.0f128, 0.03125f128}, {2.0f128, 16383.0f128}, {2.0f128, 16384.0f128},
    {0x1.8p0f128, 64.0f128}, {0x1.8p0f128, 71.0f128}, {11.0f128, 32.0f128}, {11.0f128, 33.0f128}};

/* xorshift64* */
static uint64_t rng = 0x5eedf128ULL;
static uint64_t bits(void) {
    rng ^= rng >> 12;
    rng ^= rng << 25;
    rng ^= rng >> 27;
    return rng * 0x2545f4914f6cdd1dULL;
}
/* A random operand: half moderate (|x| in [2^-8, 2^6)), a quarter near 1, the rest anywhere. */
static q rand_q(void) {
    uint64_t r = bits();
    uint64_t lo = bits(), hi = bits() & 0xffffffffffffULL;
    unsigned e;
    switch (r & 3) {
    case 0:
    case 1: e = 0x3fff - 8 + (unsigned)((r >> 2) % 14); break;
    case 2: e = 0x3ffe + (unsigned)((r >> 2) % 2); break;
    default: e = 1 + (unsigned)((r >> 2) % 32766); break;
    }
    return mk(((uint64_t)(e | ((r >> 40) & 1 ? 0x8000 : 0)) << 48) | hi, lo);
}

#define NRAND 150

static void one1(const char *name, q1 f, char mc, int mode, q xv) {
    volatile q x = xv;
    fesetround(mode);
    feclearexcept(FE_ALL_EXCEPT);
    errno = 0;
    q r = f(x);
    int fl = fetestexcept(FE_ALL_EXCEPT), e = errno;
    fesetround(FE_TONEAREST);
    printf("%s %c", name, mc);
    hex(x);
    printf(" =");
    hex(r);
    report(fl, e);
}

static void sc(char mc, int mode, q xv) {
    volatile q x = xv;
    q s, c;
    fesetround(mode);
    feclearexcept(FE_ALL_EXCEPT);
    errno = 0;
    sincosf128(x, &s, &c);
    int fl = fetestexcept(FE_ALL_EXCEPT), e = errno;
    fesetround(FE_TONEAREST);
    printf("sincosf128 %c", mc);
    hex(x);
    printf(" =");
    hex(s);
    hex(c);
    report(fl, e);
}

static void one2(const char *name, q2 f, char mc, int mode, q xv, q yv) {
    volatile q x = xv, y = yv;
    fesetround(mode);
    feclearexcept(FE_ALL_EXCEPT);
    errno = 0;
    q r = f(x, y);
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
    const q snan = __builtin_nansf128(""), pnan = __builtin_nanf128("0x1234");
    /* A negative signalling NaN, by its encoding (the sign must survive to the call). */
    const q nsnan = mk(0xffff400000000000ULL, 0x77ULL);
    /* A perfect cube with a 101-bit significand, and its negative. */
    const q big = (q)12345678901.0f128, cube = big * big * big;
    setvbuf(stdout, NULL, _IOFBF, 1 << 16);
    for (int m = 0; m < 4; m++) {
        char mc = "NUDZ"[m];
        for (size_t i = 0; i < sizeof UN / sizeof UN[0]; i++) {
            size_t n = sizeof LV / sizeof LV[0];
            for (size_t j = 0; j < n + 5; j++)
                one1(UN[i].name, UN[i].f, mc, MODES[m],
                     j < n ? LV[j] : j == n ? snan : j == n + 1 ? pnan : j == n + 2 ? cube : j == n + 3 ? -cube : nsnan);
            for (int k = 0; k < NRAND; k++)
                one1(UN[i].name, UN[i].f, mc, MODES[m], rand_q());
        }
        /* exp10 and log10 at the integers and powers of ten (exact while 5^k < 2^113, k <= 48);
         * exp2 and log2 across the subnormal range. The operands are built by arithmetic, not by
         * the library under test, so both builds see the same ones. */
        q ten = 1.0f128;
        for (int k = 0; k <= 60; k++, ten *= 10.0f128) {
            one1("exp10f128", exp10f128, mc, MODES[m], (q)k);
            one1("exp10f128", exp10f128, mc, MODES[m], -(q)k);
            one1("log10f128", log10f128, mc, MODES[m], ten);
        }
        q two = 0x1p-16494f128;
        for (int k = -16494; k <= -16370; k += 3, two *= 8.0f128) {
            one1("exp2f128", exp2f128, mc, MODES[m], (q)k);
            one1("exp2f128", exp2f128, mc, MODES[m], (q)k + 0.25f128);
            one1("log2f128", log2f128, mc, MODES[m], two);
        }
        size_t n = sizeof LP / sizeof LP[0];
        for (size_t i = 0; i < sizeof BI / sizeof BI[0]; i++)
            for (size_t a = 0; a < n + 2; a++)
                for (size_t b = 0; b < n + 2; b++)
                    one2(BI[i].name, BI[i].f, mc, MODES[m], a < n ? LP[a] : a == n ? snan : nsnan, b < n ? LP[b] : b == n ? snan : nsnan);
        for (size_t p = 0; p < sizeof PP / sizeof PP[0]; p++)
            one2("powf128", powf128, mc, MODES[m], PP[p][0], PP[p][1]);
        for (int k = 0; k < NRAND; k++)
            one2("atan2f128", atan2f128, mc, MODES[m], rand_q(), rand_q());
        /* sincos: both results. */
        for (size_t j = 0; j < sizeof LV / sizeof LV[0] + 2; j++)
            sc(mc, MODES[m], j < sizeof LV / sizeof LV[0] ? LV[j] : j == sizeof LV / sizeof LV[0] ? snan : nsnan);
        for (int k = 0; k < NRAND / 3; k++)
            sc(mc, MODES[m], rand_q());
        for (int k = 0; k < NRAND; k++) {
            q x = rand_q();
            q y = rand_q();
            if (k & 1) {
                /* A positive base and a moderate exponent: finite results. */
                uint64_t e = bits(), s = bits(), lo = bits(), hi = bits() & 0xffffffffffffULL;
                x = fabsf128(x);
                y = mk(((uint64_t)((0x3fff - 6 + (unsigned)(e % 12)) | (s & 0x8000)) << 48) | hi, lo);
            }
            one2("powf128", powf128, mc, MODES[m], x, y);
        }
    }
    return 0;
}
