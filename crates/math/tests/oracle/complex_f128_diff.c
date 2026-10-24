/* cexp, clog, clog10, csqrt, cpow, csin, ccos, ctan, csinh, ccosh, ctanh, casin, cacos, catan,
 * casinh, cacosh, catanh and carg of _Float128 (spec §10.5, Track M milestone 5, M5-4), end to end
 * on a grid of special and boundary components: zeros, subnormals, 2^-160 (below the tiny
 * cut-off 2^-150), 2^-140 (between it and 2^-113), the near-unit clog operand (2^112 + 1)·2^-200,
 * ±1, values near π/2 and π, the overflow threshold of the binary128 exponential, -11400 (a
 * subnormal exponential) and -11440 (beyond the smallest subnormal),
 * 2^113 - 1, huge values, ±∞ and NaNs (quiet, signalling, with payload) in both parts (the C
 * Annex G tables, the branch cuts on both signs of zero), in every rounding direction; cpow on a
 * smaller grid of bases and exponents; then a random sample of operands per function in
 * round-to-nearest (mode letter 'n'). Each line prints the operands' and the results' bits (the
 * binary128 encoding, 32 hex digits), the raised flags and errno. Built against the host C library
 * and statically against Liella's products, the outputs must agree (complex_f128_products.rs)
 * apart from recorded differences. The values are built from their bits, so the program needs no
 * binary128 arithmetic of its own. */
#define _GNU_SOURCE
#include <complex.h>
#include <errno.h>
#include <fenv.h>
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

typedef _Float128 Q;
typedef _Float128 _Complex QC;

static const int MODES[4] = {FE_TONEAREST, FE_UPWARD, FE_DOWNWARD, FE_TOWARDZERO};

static void report(int f, int e) {
    printf(" %c%c%c%c%c %s\n", f & FE_INVALID ? 'I' : '-', f & FE_DIVBYZERO ? 'Z' : '-', f & FE_OVERFLOW ? 'O' : '-',
           f & FE_UNDERFLOW ? 'U' : '-', f & FE_INEXACT ? 'X' : '-', e == 0 ? "0" : e == EDOM ? "EDOM" : e == ERANGE ? "ERANGE" : "E?");
}

static void hq(Q x) {
    uint64_t w[2];
    memcpy(w, &x, 16);
    printf(" %016llx%016llx", (unsigned long long)w[1], (unsigned long long)w[0]);
}

static Q mkq(int neg, unsigned e, uint64_t hi, uint64_t lo) {
    uint64_t w[2] = {lo, (hi & 0xffffffffffffull) | ((uint64_t)e << 48) | (neg ? 1ull << 63 : 0)};
    Q x;
    memcpy(&x, w, 16);
    return x;
}

typedef QC (*qc)(QC);
typedef Q (*qr)(QC);

static const struct { const char *name; qc f; } CF[] = {
    {"cexpf128", cexpf128}, {"clogf128", clogf128}, {"clog10f128", clog10f128}, {"csqrtf128", csqrtf128},
    {"csinf128", csinf128}, {"ccosf128", ccosf128}, {"ctanf128", ctanf128}, {"csinhf128", csinhf128},
    {"ccoshf128", ccoshf128}, {"ctanhf128", ctanhf128}, {"casinf128", casinf128}, {"cacosf128", cacosf128},
    {"catanf128", catanf128}, {"casinhf128", casinhf128}, {"cacoshf128", cacoshf128}, {"catanhf128", catanhf128}};
#define NF (sizeof CF / sizeof CF[0])

/* The components; the last three slots are filled with a signalling NaN, a negative quiet NaN
 * and a NaN with a payload at run time. */
static Q LV[] = {
    0.0f128, -0.0f128, 0x1p-16494f128, -0x1p-16494f128, 0x1p-16382f128, 0x1p-160f128, 0x1p-140f128,
    0x1.0000000000000000000000000001p-88f128, 0.5f128, -0.5f128, 1.0f128, -1.0f128, 1.5f128, -2.0f128, 3.0f128,
    0x1.921fb54442d18469898cc51701b8p0f128, -0x1.921fb54442d18469898cc51701b8p1f128, 10.0f128, 11357.0f128,
    -11400.0f128, -11440.0f128, 0x1.ffffffffffffffffffffffffffffp112f128, 1e4000f128, 0x1.ffffffffffffffffffffffffffffp16383f128,
    -0x1.ffffffffffffffffffffffffffffp16383f128, 0, 0, 0, 0, 0, 0};
#define NV (sizeof LV / sizeof LV[0])

/* cpow: bases and exponents (real, imaginary); the infinities and NaNs are set at run time. */
static Q PZ[][2] = {{0, 0}, {-0.0f128, 0}, {1, 0}, {-1, 0}, {2, 0}, {-2, 0}, {0, 1}, {1, 1}, {3, 4},
                    {0.5f128, -0.5f128}, {-8, 0}, {1e4000f128, 1e4000f128}, {0, 0}, {0, 0}, {0, 0}, {2, 0}};
static Q PW[][2] = {{0, 0}, {1, 0}, {2, 0}, {3, 0}, {-1, 0}, {0.5f128, 0}, {0x1.5555555555555555555555555555p-2f128, 0},
                    {0, 1}, {1, 1}, {-0.5f128, 2}, {1e10f128, 0}, {0x1.0000000000000000000000000001p112f128, 0}, {0, 0}, {0, 0}, {0, 0}};
#define NZ (sizeof PZ / sizeof PZ[0])
#define NW (sizeof PW / sizeof PW[0])

/* The random sample: a quarter anywhere in the finite range, the rest of magnitude 2^-20..2^20
 * (exponents of the exponent within 2^-4..2^4 for cpow). */
#define RN 1000
static uint64_t rs = 0x9e3779b97f4a7c15u;
static uint64_t rnd(void) { rs ^= rs << 13; rs ^= rs >> 7; rs ^= rs << 17; return rs; }
static Q rval(int span) {
    uint64_t r = rnd();
    uint64_t hi = rnd(), lo = rnd();
    if (span > 4 && r % 4 == 0) {
        unsigned e;
        do e = (unsigned)(rnd() & 0x7fff); while (e == 0x7fff);
        return mkq((int)(r & 2), e, hi, lo);
    }
    int k = (int)(rnd() % (2 * (unsigned)span + 1)) - span;
    return mkq((int)(r & 1), (unsigned)(16383 + k), hi, lo);
}

static void unary(size_t i, char mc, int mode, Q x0, Q y0) {
    volatile Q x = x0, y = y0;
    fesetround(mode);
    feclearexcept(FE_ALL_EXCEPT);
    errno = 0;
    QC r = CF[i].f(__builtin_complex((Q)x, (Q)y));
    int f = fetestexcept(FE_ALL_EXCEPT), e = errno;
    fesetround(FE_TONEAREST);
    printf("%s %c", CF[i].name, mc);
    hq(x), hq(y), printf(" ="), hq(__real__ r), hq(__imag__ r);
    report(f, e);
}

static void arg(char mc, int mode, Q x0, Q y0) {
    volatile Q x = x0, y = y0;
    volatile qr f = cargf128;
    fesetround(mode);
    feclearexcept(FE_ALL_EXCEPT);
    errno = 0;
    Q r = f(__builtin_complex((Q)x, (Q)y));
    int fl = fetestexcept(FE_ALL_EXCEPT), e = errno;
    fesetround(FE_TONEAREST);
    printf("cargf128 %c", mc);
    hq(x), hq(y), printf(" ="), hq(r);
    report(fl, e);
}

static void power(char mc, int mode, Q a, Q b, Q c, Q d) {
    volatile Q zx = a, zy = b, wx = c, wy = d;
    fesetround(mode);
    feclearexcept(FE_ALL_EXCEPT);
    errno = 0;
    QC r = cpowf128(__builtin_complex((Q)zx, (Q)zy), __builtin_complex((Q)wx, (Q)wy));
    int f = fetestexcept(FE_ALL_EXCEPT), e = errno;
    fesetround(FE_TONEAREST);
    printf("cpowf128 %c", mc);
    hq(zx), hq(zy), hq(wx), hq(wy), printf(" ="), hq(__real__ r), hq(__imag__ r);
    report(f, e);
}

int main(void) {
    Q inf = __builtin_inff128(), nan = __builtin_nanf128("");
    LV[NV - 6] = inf;
    LV[NV - 5] = -inf;
    LV[NV - 4] = nan;
    LV[NV - 3] = __builtin_nansf128("");
    LV[NV - 2] = -nan;
    LV[NV - 1] = __builtin_nanf128("0x1234");
    PZ[12][0] = inf, PZ[13][1] = inf, PZ[14][0] = nan, PZ[15][1] = nan;
    PW[12][0] = inf, PW[13][0] = nan, PW[14][1] = nan;
    setvbuf(stdout, NULL, _IOFBF, 1 << 16);
    for (int m = 0; m < 4; m++) {
        char mc = "NUDZ"[m];
        for (size_t i = 0; i < NF; i++)
            for (size_t a = 0; a < NV; a++)
                for (size_t b = 0; b < NV; b++) unary(i, mc, MODES[m], LV[a], LV[b]);
        for (size_t a = 0; a < NV; a++)
            for (size_t b = 0; b < NV; b++) arg(mc, MODES[m], LV[a], LV[b]);
        for (size_t a = 0; a < NZ; a++)
            for (size_t b = 0; b < NW; b++) power(mc, MODES[m], PZ[a][0], PZ[a][1], PW[b][0], PW[b][1]);
    }
    for (size_t i = 0; i < NF; i++)
        for (int n = 0; n < RN; n++) {
            Q x = rval(20), y = rval(20);
            unary(i, 'n', FE_TONEAREST, x, y);
        }
    for (int n = 0; n < RN; n++) {
        Q x = rval(20), y = rval(20);
        arg('n', FE_TONEAREST, x, y);
    }
    for (int n = 0; n < RN; n++) {
        Q a = rval(20), b = rval(20), c = rval(4), d = rval(4);
        power('n', FE_TONEAREST, a, b, c, d);
    }
    return 0;
}
