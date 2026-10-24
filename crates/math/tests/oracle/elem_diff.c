/* The elementary functions of float and double (spec §10.3), end to end on their special and
 * boundary operands: zeros, subnormals, ±1, ±∞, NaNs (quiet, signalling, with payload), the
 * domain edges, the overflow and underflow thresholds, huge trigonometric arguments, and the
 * C Annex F tables of pow and atan2, in every rounding direction. Each line prints the result's
 * bits, the raised flags and errno. Built against the host C library and statically against
 * Liella's products, the outputs must agree (elem_products.rs) apart from recorded
 * differences. Interior accuracy is measured separately (`cargo xtask mathcmp`). */
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

static uint64_t bd(double x) { uint64_t b; memcpy(&b, &x, 8); return b; }
static uint32_t bf(float x) { uint32_t b; memcpy(&b, &x, 4); return b; }

typedef double (*d1)(double);
typedef float (*f1)(float);
typedef double (*d2)(double, double);
typedef float (*f2)(float, float);

#define U(n) {#n, n, n##f}
static const struct { const char *name; d1 d; f1 f; } UN[] = {
    U(exp), U(exp2), U(exp10), U(expm1), U(log), U(log2), U(log10), U(log1p), U(sin), U(cos), U(tan),
    U(asin), U(acos), U(atan), U(sinh), U(cosh), U(tanh), U(asinh), U(acosh), U(atanh), U(cbrt)};
static const struct { const char *name; d2 d; f2 f; } BI[] = {U(pow), U(atan2), U(hypot)};

static const double DV[] = {
    0.0, -0.0, 0x1p-1074, -0x1p-1074, 0x1p-1022, -0x1p-1022, 0x1p-30, -0x1p-30, 0x1p-60, 0.5, -0.5, 1.0, -1.0,
    1.5, -1.5, 2.0, -2.0, 3.0, 10.0, 100.0, -100.0, 0x1.62e42fefa39efp9, 0x1.62e42fefa39f0p9, 709.0, 710.0,
    -708.0, -720.0, -745.0, -746.0, 1022.0, 1023.0, 1024.0, -1074.0, -1075.0, -1076.0, 307.0, 308.0, 309.0,
    -307.0, -323.0, -324.0, 22.0, 23.0, 1e22, 0x1p1023, -0x1p1023, DBL_MAX, -DBL_MAX, 1e300, 0x1.921fb54442d18p0,
    0x1.921fb54442d18p1, 0x1.921fb54442d18p2, 0x1p52, 0x1p53 + 1, 27.0, 8.0, -8.0, 0x1.0000000000001p0,
    0x1.fffffffffffffp-1, 40.0, -40.0, 22.5, INFINITY, -INFINITY, NAN, -NAN};
static const float FV[] = {
    0.0f, -0.0f, 0x1p-149f, -0x1p-149f, 0x1p-126f, -0x1p-126f, 0x1p-30f, -0x1p-30f, 0.5f, -0.5f, 1.0f, -1.0f,
    1.5f, -1.5f, 2.0f, -2.0f, 3.0f, 10.0f, 88.0f, 88.72f, 89.0f, -87.0f, -103.0f, -104.0f, -105.0f, 127.0f,
    128.0f, -149.0f, -150.0f, -151.0f, 38.0f, 39.0f, -45.0f, -46.0f, 10.0f, 11.0f, 1e10f, FLT_MAX, -FLT_MAX,
    0x1.921fb6p0f, 0x1.921fb6p1f, 0x1p23f, 27.0f, 8.0f, -8.0f, 0x1.000002p0f, 0x1.fffffep-1f, 9.0f, -9.0f,
    INFINITY, -INFINITY, NAN, -NAN};
static const double DP[] = {0.0, -0.0, 0x1p-1074, 0.5, -0.5, 1.0, -1.0, 2.0, -2.0, 3.0, -3.0, 0.25, 1e300, -1e300,
                            1023.0, -1075.0, INFINITY, -INFINITY, NAN};
static const float FP[] = {0.0f, -0.0f, 0x1p-149f, 0.5f, -0.5f, 1.0f, -1.0f, 2.0f, -2.0f, 3.0f, -3.0f, 0.25f, 1e30f,
                           -1e30f, 127.0f, -150.0f, INFINITY, -INFINITY, NAN};

int main(void) {
    const double dsnan = __builtin_nans(""), dpnan = __builtin_nan("0x1234");
    const float fsnan = __builtin_nansf(""), fpnan = __builtin_nanf("0x1234");
    setvbuf(stdout, NULL, _IOFBF, 1 << 16);
    for (int m = 0; m < 4; m++) {
        char mc = "NUDZ"[m];
        for (size_t i = 0; i < sizeof UN / sizeof UN[0]; i++) {
            size_t nd = sizeof DV / sizeof DV[0];
            for (size_t j = 0; j < nd + 2; j++) {
                volatile double x = j < nd ? DV[j] : j == nd ? dsnan : dpnan;
                fesetround(MODES[m]);
                feclearexcept(FE_ALL_EXCEPT);
                errno = 0;
                double r = UN[i].d(x);
                int f = fetestexcept(FE_ALL_EXCEPT), e = errno;
                fesetround(FE_TONEAREST);
                printf("%s %c %016llx = %016llx", UN[i].name, mc, (unsigned long long)bd(x), (unsigned long long)bd(r));
                report(f, e);
            }
            size_t nf = sizeof FV / sizeof FV[0];
            for (size_t j = 0; j < nf + 2; j++) {
                volatile float x = j < nf ? FV[j] : j == nf ? fsnan : fpnan;
                fesetround(MODES[m]);
                feclearexcept(FE_ALL_EXCEPT);
                errno = 0;
                float r = UN[i].f(x);
                int f = fetestexcept(FE_ALL_EXCEPT), e = errno;
                fesetround(FE_TONEAREST);
                printf("%sf %c %08x = %08x", UN[i].name, mc, bf(x), bf(r));
                report(f, e);
            }
        }
        for (size_t i = 0; i < sizeof BI / sizeof BI[0]; i++) {
            size_t nd = sizeof DP / sizeof DP[0];
            for (size_t a = 0; a < nd + 1; a++)
                for (size_t b = 0; b < nd + 1; b++) {
                    volatile double x = a < nd ? DP[a] : dsnan, y = b < nd ? DP[b] : dsnan;
                    fesetround(MODES[m]);
                    feclearexcept(FE_ALL_EXCEPT);
                    errno = 0;
                    double r = BI[i].d(x, y);
                    int f = fetestexcept(FE_ALL_EXCEPT), e = errno;
                    fesetround(FE_TONEAREST);
                    printf("%s %c %016llx %016llx = %016llx", BI[i].name, mc, (unsigned long long)bd(x), (unsigned long long)bd(y),
                           (unsigned long long)bd(r));
                    report(f, e);
                }
            size_t nf = sizeof FP / sizeof FP[0];
            for (size_t a = 0; a < nf + 1; a++)
                for (size_t b = 0; b < nf + 1; b++) {
                    volatile float x = a < nf ? FP[a] : fsnan, y = b < nf ? FP[b] : fsnan;
                    fesetround(MODES[m]);
                    feclearexcept(FE_ALL_EXCEPT);
                    errno = 0;
                    float r = BI[i].f(x, y);
                    int f = fetestexcept(FE_ALL_EXCEPT), e = errno;
                    fesetround(FE_TONEAREST);
                    printf("%sf %c %08x %08x = %08x", BI[i].name, mc, bf(x), bf(y), bf(r));
                    report(f, e);
                }
        }
        /* sincos: both results. */
        for (size_t j = 0; j < sizeof DV / sizeof DV[0]; j++) {
            volatile double x = DV[j];
            double s, c;
            fesetround(MODES[m]);
            feclearexcept(FE_ALL_EXCEPT);
            errno = 0;
            sincos(x, &s, &c);
            int f = fetestexcept(FE_ALL_EXCEPT), e = errno;
            fesetround(FE_TONEAREST);
            printf("sincos %c %016llx = %016llx %016llx", mc, (unsigned long long)bd(x), (unsigned long long)bd(s), (unsigned long long)bd(c));
            report(f, e);
        }
    }
    return 0;
}
