/* cexp, clog, clog10, csqrt and cpow (M5-2) and csin, ccos, ctan, csinh, ccosh, ctanh, casin, cacos,
 * catan, casinh, cacosh and catanh (M5-3) of long double and _Float64x, plus __clog10l (spec §10.5,
 * Track M milestone 5), end to end on a grid of special and boundary components: zeros,
 * subnormals, 2^-61 (both parts between the tiny cut-off and 2^-60), ±1, values near π/2 and π,
 * the overflow and underflow thresholds of the x87 exponential, 2^64 - 1, huge values, ±∞ and NaNs (quiet, signalling, with payload) in both
 * parts (the C Annex G tables, the branch cuts on both signs of zero), in every rounding
 * direction; cpow on a smaller grid of bases and exponents; then a random sample of RN operands
 * per function in round-to-nearest (mode letter 'n'). Each line prints the operands' and the
 * results' bits (sign/exponent then significand, 20 hex digits), the raised flags and errno.
 * Built against the host C library and statically against Liella's products, the outputs must
 * agree (complex_ld_cr_products.rs) apart from recorded differences. */
#define _GNU_SOURCE
#include <complex.h>
#include <errno.h>
#include <fenv.h>
#include <float.h>
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

extern long double complex __clog10l(long double complex);

static const int MODES[4] = {FE_TONEAREST, FE_UPWARD, FE_DOWNWARD, FE_TOWARDZERO};

static void report(int f, int e) {
    printf(" %c%c%c%c%c %s\n", f & FE_INVALID ? 'I' : '-', f & FE_DIVBYZERO ? 'Z' : '-', f & FE_OVERFLOW ? 'O' : '-',
           f & FE_UNDERFLOW ? 'U' : '-', f & FE_INEXACT ? 'X' : '-', e == 0 ? "0" : e == EDOM ? "EDOM" : e == ERANGE ? "ERANGE" : "E?");
}

static void hl(long double x) {
    uint64_t m;
    uint16_t se;
    memcpy(&m, &x, 8);
    memcpy(&se, (const char *)&x + 8, 2);
    printf(" %04x%016llx", se, (unsigned long long)m);
}

typedef long double complex (*lc)(long double complex);

static const struct { const char *name; lc f; int random; } CF[] = {
    {"cexpl", cexpl, 1}, {"clogl", clogl, 1}, {"clog10l", clog10l, 1}, {"csqrtl", csqrtl, 1},
    {"cexpf64x", cexpf64x, 0}, {"clogf64x", clogf64x, 0}, {"clog10f64x", clog10f64x, 0}, {"csqrtf64x", csqrtf64x, 0},
    {"__clog10l", __clog10l, 0},
    {"csinl", csinl, 1}, {"ccosl", ccosl, 1}, {"ctanl", ctanl, 1}, {"csinhl", csinhl, 1}, {"ccoshl", ccoshl, 1}, {"ctanhl", ctanhl, 1}, {"casinl", casinl, 1}, {"cacosl", cacosl, 1}, {"catanl", catanl, 1}, {"casinhl", casinhl, 1}, {"cacoshl", cacoshl, 1}, {"catanhl", catanhl, 1},
    {"csinf64x", csinf64x, 0}, {"ccosf64x", ccosf64x, 0}, {"ctanf64x", ctanf64x, 0}, {"csinhf64x", csinhf64x, 0}, {"ccoshf64x", ccoshf64x, 0}, {"ctanhf64x", ctanhf64x, 0}, {"casinf64x", casinf64x, 0}, {"cacosf64x", cacosf64x, 0}, {"catanf64x", catanf64x, 0}, {"casinhf64x", casinhf64x, 0}, {"cacoshf64x", cacoshf64x, 0}, {"catanhf64x", catanhf64x, 0}};
#define NF (sizeof CF / sizeof CF[0])

/* The components; the last three slots are filled with a signalling NaN, a negative quiet NaN
 * and a NaN with a payload at run time. */
static long double LV[] = {
    0.0L, -0.0L, 0x1p-16445L, -0x1p-16445L, 0x1p-16382L, 0x1p-70L, 0x1p-61L, 0.5L, -0.5L, 1.0L, -1.0L, 1.5L, -2.0L, 3.0L,
    0x1.921fb54442d1846ap0L, -0x1.921fb54442d1846ap1L, 10.0L, 11357.0L, -11400.0L, 0x1.fffffffffffffffep63L, 1e4000L,
    LDBL_MAX, -LDBL_MAX, INFINITY, -INFINITY, NAN, 0, 0, 0};
#define NV (sizeof LV / sizeof LV[0])

/* cpow: bases and exponents (real, imaginary). */
static const long double PZ[][2] = {{0, 0}, {-0.0L, 0}, {1, 0}, {-1, 0}, {2, 0}, {-2, 0}, {0, 1}, {1, 1}, {3, 4},
                                    {0.5L, -0.5L}, {-8, 0}, {1e4000L, 1e4000L}, {INFINITY, 0}, {0, INFINITY}, {NAN, 0}, {2, NAN}};
static const long double PW[][2] = {{0, 0}, {1, 0}, {2, 0}, {3, 0}, {-1, 0}, {0.5L, 0}, {0x1.5555555555555556p-2L, 0},
                                    {0, 1}, {1, 1}, {-0.5L, 2}, {1e10L, 0}, {0x1p62L + 1, 0}, {INFINITY, 0}, {NAN, 0}, {0, NAN}};
#define NZ (sizeof PZ / sizeof PZ[0])
#define NW (sizeof PW / sizeof PW[0])

/* The random sample: a quarter anywhere in the finite range, the rest of magnitude 2^-20..2^20
 * (exponents of the exponent within 2^-4..2^4 for cpow). */
#define RN 2000
static uint64_t rs = 0x9e3779b97f4a7c15u;
static uint64_t rnd(void) { rs ^= rs << 13; rs ^= rs >> 7; rs ^= rs << 17; return rs; }
static long double rval(int span) {
    uint64_t r = rnd();
    if (span > 4 && r % 4 == 0) {
        uint16_t e;
        do e = (uint16_t)(rnd() & 0x7fff); while (e == 0x7fff);
        uint64_t m = rnd();
        m = e == 0 ? m & ~(1ull << 63) : m | (1ull << 63);
        uint16_t se = (uint16_t)(e | (r & 2 ? 0x8000 : 0));
        long double x = 0;
        memcpy(&x, &m, 8);
        memcpy((char *)&x + 8, &se, 2);
        return x;
    }
    long double m = 1.0L + (long double)(rnd() >> 1) * 0x1p-63L;
    return ldexpl(r & 1 ? -m : m, (int)(rnd() % (2 * (unsigned)span + 1)) - span);
}

static void unary(size_t i, char mc, int mode, long double x0, long double y0) {
    volatile long double x = x0, y = y0;
    fesetround(mode);
    feclearexcept(FE_ALL_EXCEPT);
    errno = 0;
    long double complex r = CF[i].f(CMPLXL(x, y));
    int f = fetestexcept(FE_ALL_EXCEPT), e = errno;
    fesetround(FE_TONEAREST);
    printf("%s %c", CF[i].name, mc);
    hl(x), hl(y), printf(" ="), hl(creall(r)), hl(cimagl(r));
    report(f, e);
}

static void power(int f64x, char mc, int mode, long double a, long double b, long double c, long double d) {
    volatile long double zx = a, zy = b, wx = c, wy = d;
    fesetround(mode);
    feclearexcept(FE_ALL_EXCEPT);
    errno = 0;
    long double complex r = f64x ? cpowf64x(CMPLXL(zx, zy), CMPLXL(wx, wy)) : cpowl(CMPLXL(zx, zy), CMPLXL(wx, wy));
    int f = fetestexcept(FE_ALL_EXCEPT), e = errno;
    fesetround(FE_TONEAREST);
    printf("%s %c", f64x ? "cpowf64x" : "cpowl", mc);
    hl(zx), hl(zy), hl(wx), hl(wy), printf(" ="), hl(creall(r)), hl(cimagl(r));
    report(f, e);
}

int main(void) {
    LV[NV - 3] = __builtin_nansl("");
    LV[NV - 2] = -__builtin_nanl("");
    LV[NV - 1] = __builtin_nanl("0x1234");
    setvbuf(stdout, NULL, _IOFBF, 1 << 16);
    for (int m = 0; m < 4; m++) {
        char mc = "NUDZ"[m];
        for (size_t i = 0; i < NF; i++)
            for (size_t a = 0; a < NV; a++)
                for (size_t b = 0; b < NV; b++) unary(i, mc, MODES[m], LV[a], LV[b]);
        for (size_t a = 0; a < NZ; a++)
            for (size_t b = 0; b < NW; b++)
                for (int f64x = 0; f64x < 2; f64x++) power(f64x, mc, MODES[m], PZ[a][0], PZ[a][1], PW[b][0], PW[b][1]);
    }
    for (size_t i = 0; i < NF; i++)
        if (CF[i].random)
            for (int n = 0; n < RN; n++) {
                long double x = rval(20), y = rval(20);
                unary(i, 'n', FE_TONEAREST, x, y);
            }
    for (int n = 0; n < RN; n++) {
        long double a = rval(20), b = rval(20), c = rval(4), d = rval(4);
        power(0, 'n', FE_TONEAREST, a, b, c, d);
    }
    return 0;
}
