/* The complex functions of float and double (spec §10.5, Track M milestone 4), end to end on a
 * grid of special and boundary components: zeros, subnormals, ±1, values near π/2 and π, the
 * overflow and underflow thresholds of the exponential, huge values, ±∞ and NaNs (quiet,
 * signalling, with payload) in both the real and the imaginary part (the C Annex G tables), in
 * every rounding direction, and cpow on a smaller grid of bases and exponents; then a random
 * sample of RN operands per function and format in round-to-nearest (mode letter 'n'), the
 * measurement of D-10.3. Each line prints the operands' and the results' bits, the raised flags
 * and errno. Built against the host C library and statically against Liella's products, the
 * outputs must agree (complex_products.rs) apart from recorded differences. */
#define _GNU_SOURCE
#include <complex.h>
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

static uint64_t bd(double x) { uint64_t b; memcpy(&b, &x, 8); return b; }
static uint32_t bf(float x) { uint32_t b; memcpy(&b, &x, 4); return b; }

typedef double complex (*dc)(double complex);
typedef float complex (*fc)(float complex);
typedef double (*dr)(double complex);
typedef float (*fr)(float complex);

#define U(n) {#n, n, n##f}
static const struct { const char *name; dc d; fc f; } CF[] = {
    U(cexp), U(clog), U(clog10), U(csqrt), U(csin), U(ccos), U(ctan), U(csinh), U(ccosh), U(ctanh), U(casin),
    U(cacos), U(catan), U(casinh), U(cacosh), U(catanh), U(cproj), U(conj)};
static const struct { const char *name; dr d; fr f; } RF[] = {U(cabs), U(carg), U(creal), U(cimag)};

/* The components; the last three slots are filled with a signalling NaN, a negative quiet NaN
 * and a NaN with a payload at run time. */
static double DV[] = {
    0.0, -0.0, 0x1p-1074, -0x1p-1074, 0x1p-1022, 0x1p-60, 0.5, -0.5, 1.0, -1.0, 1.5, -2.0, 3.0, 0x1.921fb54442d18p0,
    -0x1.921fb54442d18p1, 10.0, 710.0, -746.0, 1e300, DBL_MAX, -DBL_MAX, INFINITY, -INFINITY, NAN, 0, 0, 0};
static float FV[] = {
    0.0f, -0.0f, 0x1p-149f, -0x1p-149f, 0x1p-126f, 0x1p-30f, 0.5f, -0.5f, 1.0f, -1.0f, 1.5f, -2.0f, 3.0f, 0x1.921fb6p0f,
    -0x1.921fb6p1f, 10.0f, 89.0f, -104.0f, 1e30f, FLT_MAX, -FLT_MAX, INFINITY, -INFINITY, NAN, 0, 0, 0};
#define NV (sizeof DV / sizeof DV[0])

/* cpow: bases and exponents (real, imaginary). */
static const double PZ[][2] = {{0, 0}, {-0.0, 0}, {1, 0}, {-1, 0}, {2, 0}, {-2, 0}, {0, 1}, {1, 1}, {3, 4}, {0.5, -0.5},
                               {-8, 0}, {1e300, 1e300}, {INFINITY, 0}, {0, INFINITY}, {NAN, 0}, {2, NAN}};
static const double PW[][2] = {{0, 0}, {1, 0}, {2, 0}, {3, 0}, {-1, 0}, {0.5, 0}, {0x1.5555555555555p-2, 0}, {0, 1},
                               {1, 1}, {-0.5, 2}, {1e10, 0}, {INFINITY, 0}, {NAN, 0}, {0, NAN}};
#define NZ (sizeof PZ / sizeof PZ[0])
#define NW (sizeof PW / sizeof PW[0])

/* The random sample: a quarter anywhere in the finite range, the rest of magnitude 2^-20..2^20
 * (exponents of the exponent within 2^-4..2^4 for cpow). */
#define RN 2000
static uint64_t rs = 0x9e3779b97f4a7c15u;
static uint64_t rnd(void) { rs ^= rs << 13; rs ^= rs >> 7; rs ^= rs << 17; return rs; }
static double rval(int span) {
    uint64_t r = rnd();
    if (span > 4 && r % 4 == 0) {
        uint64_t b;
        do b = rnd(); while (((b >> 52) & 0x7ff) == 0x7ff);
        double x; memcpy(&x, &b, 8); return x;
    }
    double m = 1.0 + (double)(rnd() >> 11) * 0x1p-53;
    return ldexp(r & 1 ? -m : m, (int)(rnd() % (2 * span + 1)) - span);
}
static float rvalf(int span) {
    uint64_t r = rnd();
    if (span > 4 && r % 4 == 0) {
        uint32_t b;
        do b = (uint32_t)rnd(); while (((b >> 23) & 0xff) == 0xff);
        float x; memcpy(&x, &b, 4); return x;
    }
    float m = 1.0f + (float)(rnd() >> 40) * 0x1p-24f;
    return ldexpf(r & 1 ? -m : m, (int)(rnd() % (2 * span + 1)) - span);
}

int main(void) {
    DV[NV - 3] = __builtin_nans("");
    DV[NV - 2] = -__builtin_nan("");
    DV[NV - 1] = __builtin_nan("0x1234");
    FV[NV - 3] = __builtin_nansf("");
    FV[NV - 2] = -__builtin_nanf("");
    FV[NV - 1] = __builtin_nanf("0x1234");
    setvbuf(stdout, NULL, _IOFBF, 1 << 16);
    for (int m = 0; m < 4; m++) {
        char mc = "NUDZ"[m];
        for (size_t i = 0; i < sizeof CF / sizeof CF[0]; i++)
            for (size_t a = 0; a < NV; a++)
                for (size_t b = 0; b < NV; b++) {
                    volatile double x = DV[a], y = DV[b];
                    fesetround(MODES[m]);
                    feclearexcept(FE_ALL_EXCEPT);
                    errno = 0;
                    double complex r = CF[i].d(CMPLX(x, y));
                    int f = fetestexcept(FE_ALL_EXCEPT), e = errno;
                    fesetround(FE_TONEAREST);
                    printf("%s %c %016llx %016llx = %016llx %016llx", CF[i].name, mc, (unsigned long long)bd(x), (unsigned long long)bd(y),
                           (unsigned long long)bd(creal(r)), (unsigned long long)bd(cimag(r)));
                    report(f, e);
                    volatile float xf = FV[a], yf = FV[b];
                    fesetround(MODES[m]);
                    feclearexcept(FE_ALL_EXCEPT);
                    errno = 0;
                    float complex rf = CF[i].f(CMPLXF(xf, yf));
                    f = fetestexcept(FE_ALL_EXCEPT), e = errno;
                    fesetround(FE_TONEAREST);
                    printf("%sf %c %08x %08x = %08x %08x", CF[i].name, mc, bf(xf), bf(yf), bf(crealf(rf)), bf(cimagf(rf)));
                    report(f, e);
                }
        for (size_t i = 0; i < sizeof RF / sizeof RF[0]; i++)
            for (size_t a = 0; a < NV; a++)
                for (size_t b = 0; b < NV; b++) {
                    volatile double x = DV[a], y = DV[b];
                    fesetround(MODES[m]);
                    feclearexcept(FE_ALL_EXCEPT);
                    errno = 0;
                    double r = RF[i].d(CMPLX(x, y));
                    int f = fetestexcept(FE_ALL_EXCEPT), e = errno;
                    fesetround(FE_TONEAREST);
                    printf("%s %c %016llx %016llx = %016llx", RF[i].name, mc, (unsigned long long)bd(x), (unsigned long long)bd(y), (unsigned long long)bd(r));
                    report(f, e);
                    volatile float xf = FV[a], yf = FV[b];
                    fesetround(MODES[m]);
                    feclearexcept(FE_ALL_EXCEPT);
                    errno = 0;
                    float rf = RF[i].f(CMPLXF(xf, yf));
                    f = fetestexcept(FE_ALL_EXCEPT), e = errno;
                    fesetround(FE_TONEAREST);
                    printf("%sf %c %08x %08x = %08x", RF[i].name, mc, bf(xf), bf(yf), bf(rf));
                    report(f, e);
                }
        for (size_t a = 0; a < NZ; a++)
            for (size_t b = 0; b < NW; b++) {
                volatile double zx = PZ[a][0], zy = PZ[a][1], wx = PW[b][0], wy = PW[b][1];
                fesetround(MODES[m]);
                feclearexcept(FE_ALL_EXCEPT);
                errno = 0;
                double complex r = cpow(CMPLX(zx, zy), CMPLX(wx, wy));
                int f = fetestexcept(FE_ALL_EXCEPT), e = errno;
                fesetround(FE_TONEAREST);
                printf("cpow %c %016llx %016llx %016llx %016llx = %016llx %016llx", mc, (unsigned long long)bd(zx), (unsigned long long)bd(zy),
                       (unsigned long long)bd(wx), (unsigned long long)bd(wy), (unsigned long long)bd(creal(r)), (unsigned long long)bd(cimag(r)));
                report(f, e);
                volatile float zxf = (float)zx, zyf = (float)zy, wxf = (float)wx, wyf = (float)wy;
                fesetround(MODES[m]);
                feclearexcept(FE_ALL_EXCEPT);
                errno = 0;
                float complex rf = cpowf(CMPLXF(zxf, zyf), CMPLXF(wxf, wyf));
                f = fetestexcept(FE_ALL_EXCEPT), e = errno;
                fesetround(FE_TONEAREST);
                printf("cpowf %c %08x %08x %08x %08x = %08x %08x", mc, bf(zxf), bf(zyf), bf(wxf), bf(wyf), bf(crealf(rf)), bf(cimagf(rf)));
                report(f, e);
            }
    }
    for (size_t i = 0; i < sizeof CF / sizeof CF[0] - 2; i++)
        for (int n = 0; n < RN; n++) {
            volatile double x = rval(20), y = rval(20);
            feclearexcept(FE_ALL_EXCEPT);
            errno = 0;
            double complex r = CF[i].d(CMPLX(x, y));
            int f = fetestexcept(FE_ALL_EXCEPT), e = errno;
            printf("%s n %016llx %016llx = %016llx %016llx", CF[i].name, (unsigned long long)bd(x), (unsigned long long)bd(y),
                   (unsigned long long)bd(creal(r)), (unsigned long long)bd(cimag(r)));
            report(f, e);
            volatile float xf = rvalf(20), yf = rvalf(20);
            feclearexcept(FE_ALL_EXCEPT);
            errno = 0;
            float complex rf = CF[i].f(CMPLXF(xf, yf));
            f = fetestexcept(FE_ALL_EXCEPT), e = errno;
            printf("%sf n %08x %08x = %08x %08x", CF[i].name, bf(xf), bf(yf), bf(crealf(rf)), bf(cimagf(rf)));
            report(f, e);
        }
    for (int n = 0; n < RN; n++) {
        volatile double zx = rval(20), zy = rval(20), wx = rval(4), wy = rval(4);
        feclearexcept(FE_ALL_EXCEPT);
        errno = 0;
        double complex r = cpow(CMPLX(zx, zy), CMPLX(wx, wy));
        int f = fetestexcept(FE_ALL_EXCEPT), e = errno;
        printf("cpow n %016llx %016llx %016llx %016llx = %016llx %016llx", (unsigned long long)bd(zx), (unsigned long long)bd(zy),
               (unsigned long long)bd(wx), (unsigned long long)bd(wy), (unsigned long long)bd(creal(r)), (unsigned long long)bd(cimag(r)));
        report(f, e);
        volatile float zxf = rvalf(20), zyf = rvalf(20), wxf = rvalf(4), wyf = rvalf(4);
        feclearexcept(FE_ALL_EXCEPT);
        errno = 0;
        float complex rf = cpowf(CMPLXF(zxf, zyf), CMPLXF(wxf, wyf));
        f = fetestexcept(FE_ALL_EXCEPT), e = errno;
        printf("cpowf n %08x %08x %08x %08x = %08x %08x", bf(zxf), bf(zyf), bf(wxf), bf(wyf), bf(crealf(rf)), bf(cimagf(rf)));
        report(f, e);
    }
    return 0;
}
