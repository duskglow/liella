/* The special functions of long double (spec §10.3, Track M milestone 5b), end to end on their
 * special and boundary operands (zeros, the smallest subnormal and normal values, ±1 and its
 * neighbours, ±∞, NaNs quiet and signalling, the poles of the gamma functions and of the Bessel
 * functions of the second kind, the overflow and underflow thresholds of the 80-bit format,
 * zeros of lgamma and of the Bessel functions, huge arguments up to the largest finite value)
 * and on random operands, in every rounding direction. Each line prints the operands' and
 * results' 80-bit encodings (20 hex digits; for lgammal, gammal and lgammal_r also the sign of
 * Γ(x) stored), the raised flags and errno. Built against the host C library and statically
 * against Liella's products, the outputs must agree (special_ld_products.rs) except where Liella
 * is correctly rounded and the host is not. */
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
typedef long double (*ln)(int, long double);

#define U(n) {#n "l", n##l}
static const struct { const char *name; l1 f; } UN[] = {U(erf), U(erfc), U(tgamma), U(j0), U(j1), U(y0), U(y1)};
static const struct { const char *name; ln f; } NN[] = {U(jn), U(yn)};

static const long double LV[] = {
    0.0L, -0.0L, 0x1p-16445L, -0x1p-16445L, 0x3p-16445L, 0x1p-16382L, -0x1p-16382L, 0x1p-60L, -0x1p-61L, 0x1p-100L,
    -0x1p-101L, 0x1p-190L, 0x1p-300L, -0x1p-300L, 0x1.8p-200L, 0x1p-16383L, 0x1p-16384L, -0x1p-16384L, 0.5L, -0.5L, 1.0L,
    -1.0L, 0x1.0000000000000002p0L, 0x1.fffffffffffffffep-1L, 1.5L, 2.0L, -2.0L, 0x1.0000000000000002p1L,
    0x1.fffffffffffffffep0L, 2.5L, -2.5L, 3.0L, -3.0L, 5.0L, 10.0L, -10.0L, 12.0L, -12.0L, 27.0L, 28.0L, 100.0L,
    106.5L, 106.75L, 106.8L, 107.0L, -106.8L, 1000.0L, 1754.0L, 1755.0L, 1755.5L, 1756.0L, 1800.0L, -1754.5L,
    -1755.5L, -1766.5L, -1800.5L, -100000.5L, -0x1.fffffffffffffffep61L, -0x1.ffffffffffffffffp62L,
    -2.457024738220800L, -2.747682646727L, -3.1435808883499798L, -3.9552942848585L, 1.4616321449683623L,
    2.404825557695772768L, 3.831705970207512316L, 0.893576966279167522L, 2.197141326031017035L, 1e10L, 1e22L,
    0x1p63L, 0x1p1000L, 0x1p16000L, LDBL_MAX, -LDBL_MAX, INFINITY, -INFINITY, NAN, -NAN};
static const int NS[] = {-3, -1, 0, 1, 2, 5, 30, 200};

/* xorshift64* */
static uint64_t rng = 0x5eedbe55ULL;
static uint64_t bits(void) {
    rng ^= rng >> 12;
    rng ^= rng << 25;
    rng ^= rng >> 27;
    return rng * 0x2545f4914f6cdd1dULL;
}
/* A random operand: half moderate (|x| in [2^-4, 2^6)), a quarter in [2^6, 2^11), the rest
 * anywhere. */
static long double rand_ld(void) {
    uint64_t r = bits();
    uint64_t m = bits() | (1ULL << 63);
    unsigned e;
    switch (r & 3) {
    case 0:
    case 1: e = 0x3fff - 4 + (unsigned)((r >> 2) % 10); break;
    case 2: e = 0x3fff + 6 + (unsigned)((r >> 2) % 5); break;
    default: e = 1 + (unsigned)((r >> 2) % 32766); break;
    }
    return ld((uint16_t)(e | ((r >> 40) & 1 ? 0x8000 : 0)), m);
}

#define NRAND 40

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

static void onen(const char *name, ln f, char mc, int mode, int n, long double xv) {
    volatile long double x = xv;
    fesetround(mode);
    feclearexcept(FE_ALL_EXCEPT);
    errno = 0;
    long double r = f(n, x);
    int fl = fetestexcept(FE_ALL_EXCEPT), e = errno;
    fesetround(FE_TONEAREST);
    printf("%s %c %08x", name, mc, (unsigned)n);
    hex(x);
    printf(" =");
    hex(r);
    report(fl, e);
}

/* lgammal, gammal and lgammal_r: the value and the sign stored. */
static void onelg(char mc, int mode, long double xv) {
    volatile long double x = xv;
    for (int which = 0; which < 3; which++) {
        int sg = 7;
        signgam = 7;
        fesetround(mode);
        feclearexcept(FE_ALL_EXCEPT);
        errno = 0;
        long double r = which == 0 ? lgammal(x) : which == 1 ? gammal(x) : lgammal_r(x, &sg);
        int fl = fetestexcept(FE_ALL_EXCEPT), e = errno;
        fesetround(FE_TONEAREST);
        printf("%s %c", which == 0 ? "lgammal" : which == 1 ? "gammal" : "lgammal_r", mc);
        hex(x);
        printf(" =");
        hex(r);
        printf(" %08x", (unsigned)(which == 2 ? sg : signgam));
        report(fl, e);
    }
}

int main(void) {
    const long double snan = __builtin_nansl(""), pnan = __builtin_nanl("0x1234");
    setvbuf(stdout, NULL, _IOFBF, 1 << 16);
    size_t n = sizeof LV / sizeof LV[0];
    for (int m = 0; m < 4; m++) {
        char mc = "NUDZ"[m];
        rng = 0x5eedbe55ULL; /* the same random operands in every mode */
        for (size_t i = 0; i < sizeof UN / sizeof UN[0]; i++) {
            for (size_t j = 0; j < n + 2; j++) {
                /* Not tgammal(±2^-k) for k = 16382..16384: 2^k − γ next to the largest finite value
                 * takes the oracle some 16,400 bits (the same code path is checked at 2^-300). */
                if (UN[i].f == tgammal && j < n && LV[j] != 0 && fabsl(LV[j]) >= 0x1p-16384L && fabsl(LV[j]) <= 0x1p-16382L) continue;
                one1(UN[i].name, UN[i].f, mc, MODES[m], j < n ? LV[j] : j == n ? snan : pnan);
            }
            for (int k = 0; k < NRAND; k++)
                one1(UN[i].name, UN[i].f, mc, MODES[m], rand_ld());
        }
        for (size_t j = 0; j < n + 2; j++)
            onelg(mc, MODES[m], j < n ? LV[j] : j == n ? snan : pnan);
        for (int k = 0; k < NRAND; k++)
            onelg(mc, MODES[m], rand_ld());
        for (size_t i = 0; i < 2; i++)
            for (size_t k = 0; k < sizeof NS / sizeof NS[0]; k++) {
                for (size_t j = 0; j < n + 1; j++) {
                    /* Tiny operands only through j0l, j1l, y0l, y1l: the general-order oracle needs
                     * twice their exponent in bits there. */
                    if (j < n && LV[j] != 0 && fabsl(LV[j]) < 0x1p-1000L) continue;
                    onen(NN[i].name, NN[i].f, mc, MODES[m], NS[k], j < n ? LV[j] : snan);
                }
                for (int r = 0; r < NRAND / 4; r++)
                    onen(NN[i].name, NN[i].f, mc, MODES[m], NS[k], rand_ld());
            }
    }
    return 0;
}
