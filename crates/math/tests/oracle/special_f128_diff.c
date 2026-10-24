/* The special functions of _Float128 (spec §10.3, Track M milestone 5, M5-7), end to end on their
 * special and boundary operands (zeros, the smallest subnormal and normal values, ±1 and ±2 and
 * their neighbours, ±∞, NaNs quiet and signalling, the poles of the gamma functions and of the
 * Bessel functions of the second kind, the shortcut thresholds of erfc (2^-149, the continued
 * fraction from 6), the overflow and underflow thresholds of the binary128 format, zeros of lgamma
 * and of the Bessel functions, huge arguments up to the largest finite value) and on random
 * operands, in every rounding direction. Each line prints the operands' and results' 128-bit
 * encodings (32 hex digits; for lgammaf128 and lgammaf128_r also the sign of Γ(x) stored:
 * signgam, or the int lgammaf128_r writes), the raised flags and errno. Built against the host C
 * library and statically against Liella's products, the outputs must agree
 * (special_f128_products.rs) except where Liella is correctly rounded and the host is not. */
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

static void hex(_Float128 x) {
    uint64_t w[2];
    memcpy(w, &x, 16);
    printf(" %016llx%016llx", (unsigned long long)w[1], (unsigned long long)w[0]);
}

static _Float128 q(uint64_t hi, uint64_t lo) {
    uint64_t w[2] = {lo, hi};
    _Float128 x;
    memcpy(&x, w, 16);
    return x;
}

typedef _Float128 (*q1)(_Float128);
typedef _Float128 (*qn)(int, _Float128);

#define U(n) {#n "f128", n##f128}
static const struct { const char *name; q1 f; } UN[] = {U(erf), U(erfc), U(tgamma), U(j0), U(j1), U(y0), U(y1)};
static const struct { const char *name; qn f; } NN[] = {U(jn), U(yn)};

static const _Float128 LV[] = {
    0.0f128, -0.0f128, __FLT128_DENORM_MIN__, -__FLT128_DENORM_MIN__, 3 * __FLT128_DENORM_MIN__, __FLT128_MIN__,
    -__FLT128_MIN__, 0x1p-60f128, -0x1p-61f128, 0x1p-100f128, -0x1p-101f128, 0x1.8p-148f128, -0x1p-150f128,
    0x1p-181f128, 0x1p-190f128, 0x1p-300f128, -0x1p-300f128, 0x1.8p-200f128, 0x1p-16383f128, 0x1p-16384f128,
    -0x1p-16384f128, 0.5f128, -0.5f128, 1.0f128, -1.0f128, 0x1.0000000000000000000000000001p0f128,
    0x1.ffffffffffffffffffffffffffffp-1f128, 1.5f128, 2.0f128, -2.0f128, 0x1.0000000000000000000000000001p1f128,
    0x1.ffffffffffffffffffffffffffffp0f128, 2.5f128, -2.5f128, 3.0f128, -3.0f128, 5.0f128, 5.99f128, 6.0f128,
    6.01f128, 9.9f128, 10.0f128, -10.0f128, 12.0f128, -12.0f128, 27.0f128, 28.0f128, 100.0f128, 106.5f128,
    106.9f128, 107.0f128, 107.1f128, 107.2f128, -107.0f128, 1000.0f128, 1754.0f128, 1755.0f128, 1755.5f128,
    1756.0f128, 1800.0f128, -1754.5f128, -1755.5f128, -1766.5f128, -1800.5f128, -100000.5f128,
    -0x1.ffffffffffffffffffffffffffffp111f128, -0x1.ffffffffffffffffffffffffffffp112f128,
    -2.4570247382208005860739114999292f128, -2.7476826467274126919083626496647f128,
    -3.1435808883499798405101834046597f128, -3.9552942848585979085902103817876f128,
    1.4616321449683623412626595423257f128, 2.4048255576957727686216318793265f128,
    3.8317059702075123156144358863082f128, 0.8935769662791675215848871415234f128,
    2.1971413260310170351490335626990f128, 1e10f128, 1e22f128, 0x1p113f128, 0x1p1000f128, 0x1p16000f128,
    __FLT128_MAX__, -__FLT128_MAX__, __builtin_inff128(), -__builtin_inff128(), __builtin_nanf128(""),
    -__builtin_nanf128("")};
static const int NS[] = {-3, -1, 0, 1, 2, 5, 30, 200};

/* xorshift64* */
static uint64_t rng = 0x5eedbe55ULL;
static uint64_t bits(void) {
    rng ^= rng >> 12;
    rng ^= rng << 25;
    rng ^= rng >> 27;
    return rng * 0x2545f4914f6cdd1dULL;
}
/* A random operand with a full 112-bit fraction: half moderate (|x| in [2^-4, 2^6)), a quarter in
 * [2^6, 2^11), the rest anywhere. */
static _Float128 rand_q(void) {
    uint64_t r = bits();
    uint64_t hi = bits() & ((1ULL << 48) - 1), lo = bits();
    unsigned e;
    switch (r & 3) {
    case 0:
    case 1: e = 0x3fff - 4 + (unsigned)((r >> 2) % 10); break;
    case 2: e = 0x3fff + 6 + (unsigned)((r >> 2) % 5); break;
    default: e = 1 + (unsigned)((r >> 2) % 32766); break;
    }
    hi |= (uint64_t)e << 48;
    if ((r >> 40) & 1) hi |= 1ULL << 63;
    return q(hi, lo);
}

#define NRAND 40

static void one1(const char *name, q1 f, char mc, int mode, _Float128 xv) {
    volatile _Float128 x = xv;
    fesetround(mode);
    feclearexcept(FE_ALL_EXCEPT);
    errno = 0;
    _Float128 r = f(x);
    int fl = fetestexcept(FE_ALL_EXCEPT), e = errno;
    fesetround(FE_TONEAREST);
    printf("%s %c", name, mc);
    hex(x);
    printf(" =");
    hex(r);
    report(fl, e);
}

static void onen(const char *name, qn f, char mc, int mode, int n, _Float128 xv) {
    volatile _Float128 x = xv;
    fesetround(mode);
    feclearexcept(FE_ALL_EXCEPT);
    errno = 0;
    _Float128 r = f(n, x);
    int fl = fetestexcept(FE_ALL_EXCEPT), e = errno;
    fesetround(FE_TONEAREST);
    printf("%s %c %08x", name, mc, (unsigned)n);
    hex(x);
    printf(" =");
    hex(r);
    report(fl, e);
}

/* lgammaf128 and lgammaf128_r: the value and the sign stored (signgam for lgammaf128: the
 * reference shows whether it writes it). */
static void onelg(char mc, int mode, _Float128 xv) {
    volatile _Float128 x = xv;
    for (int which = 0; which < 2; which++) {
        int sg = 7;
        signgam = 7;
        fesetround(mode);
        feclearexcept(FE_ALL_EXCEPT);
        errno = 0;
        _Float128 r = which == 0 ? lgammaf128(x) : lgammaf128_r(x, &sg);
        int fl = fetestexcept(FE_ALL_EXCEPT), e = errno;
        fesetround(FE_TONEAREST);
        printf("%s %c", which == 0 ? "lgammaf128" : "lgammaf128_r", mc);
        hex(x);
        printf(" =");
        hex(r);
        printf(" %08x", (unsigned)(which == 1 ? sg : signgam));
        report(fl, e);
    }
}

static int huge_gamma_pole(_Float128 v) {
    _Float128 a = v < 0 ? -v : v;
    return v != 0 && a >= 0x1p-16384f128 && a <= 0x1p-16382f128;
}

int main(void) {
    const _Float128 snan = __builtin_nansf128(""), pnan = __builtin_nanf128("0x1234");
    setvbuf(stdout, NULL, _IOFBF, 1 << 16);
    size_t n = sizeof LV / sizeof LV[0];
    for (int m = 0; m < 4; m++) {
        char mc = "NUDZ"[m];
        rng = 0x5eedbe55ULL; /* the same random operands in every mode */
        for (size_t i = 0; i < sizeof UN / sizeof UN[0]; i++) {
            for (size_t j = 0; j < n + 2; j++) {
                /* Not tgammaf128(±2^-k) for k = 16382..16384: 2^k − γ next to 2^16384 takes the
                 * oracle some 16,500 bits (the same code path is checked at 2^-300). */
                if (UN[i].f == tgammaf128 && j < n && huge_gamma_pole(LV[j])) continue;
                one1(UN[i].name, UN[i].f, mc, MODES[m], j < n ? LV[j] : j == n ? snan : pnan);
            }
            for (int k = 0; k < NRAND; k++)
                one1(UN[i].name, UN[i].f, mc, MODES[m], rand_q());
        }
        for (size_t j = 0; j < n + 2; j++)
            onelg(mc, MODES[m], j < n ? LV[j] : j == n ? snan : pnan);
        for (int k = 0; k < NRAND; k++)
            onelg(mc, MODES[m], rand_q());
        for (size_t i = 0; i < 2; i++)
            for (size_t k = 0; k < sizeof NS / sizeof NS[0]; k++) {
                for (size_t j = 0; j < n + 1; j++) {
                    /* Tiny operands only through the order-0 and order-1 functions: the
                     * general-order oracle needs twice their exponent in bits there. */
                    if (j < n && LV[j] != 0 && (LV[j] < 0 ? -LV[j] : LV[j]) < 0x1p-1000f128) continue;
                    onen(NN[i].name, NN[i].f, mc, MODES[m], NS[k], j < n ? LV[j] : snan);
                }
                for (int r = 0; r < NRAND / 4; r++)
                    onen(NN[i].name, NN[i].f, mc, MODES[m], NS[k], rand_q());
            }
    }
    return 0;
}
