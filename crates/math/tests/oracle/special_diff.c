/* The special functions of float and double (spec §10.3, Track M milestone 3), end to end on
 * their special and boundary operands: zeros, subnormals, ±∞, NaNs, poles (the non-positive
 * integers of the gamma functions, zero for the Bessel functions of the second kind), the
 * overflow and underflow thresholds, zeros of the Bessel functions and of lgamma, huge
 * arguments, in every rounding direction. Each line prints the result's bits (and for lgamma,
 * gamma and lgamma_r the sign of Γ(x) stored), the raised flags and errno. Built against the host
 * C library and statically against Liella's products, the outputs must agree
 * (special_products.rs) apart from recorded differences. */
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
typedef double (*dn)(int, double);
typedef float (*fn)(int, float);

#define U(n) {#n, n, n##f}
static const struct { const char *name; d1 d; f1 f; } UN[] = {U(erf), U(erfc), U(tgamma), U(j0), U(j1), U(y0), U(y1)};
static const struct { const char *name; dn d; fn f; } NN[] = {U(jn), U(yn)};

static const double DV[] = {
    0.0, -0.0, 0x1p-1074, -0x1p-1074, 0x1p-1022, 0x1p-60, -0x1p-60, 0x1p-30, 1e-300, 0.5, -0.5, 1.0, -1.0, 1.5, 2.0, -2.0,
    2.5, -2.5, 3.0, -3.0, 5.0, 10.0, 12.0, -12.0, 26.5, 27.0, 27.3, 28.0, -28.0, 100.0, 170.5, 171.0, 171.7, 172.0, -170.5,
    -171.5, -180.5, -190.5, -1e5 - 0.5, 1e-310, 0x1.0000000000001p0, 0x1.fffffffffffffp0, -2.4570247382208005,
    2.404825557695773, 3.8317059702075125, 0.8935769662791675, 1e10, 1e22, 0x1p1000, DBL_MAX, -DBL_MAX, 1e300, 1e306,
    4503599627370496.5, -4503599627370495.5, INFINITY, -INFINITY, NAN, -NAN};
static const float FV[] = {
    0.0f, -0.0f, 0x1p-149f, -0x1p-149f, 0x1p-126f, 0x1p-30f, -0x1p-30f, 0.5f, -0.5f, 1.0f, -1.0f, 1.5f, 2.0f, -2.0f, 2.5f, -2.5f,
    3.0f, -3.0f, 5.0f, 9.0f, 10.0f, -10.0f, 10.5f, 34.0f, 35.0f, 35.1f, 36.0f, -36.0f, -40.5f, -41.5f, -45.5f, 100.0f,
    2.4048256f, 3.8317060f, 0.8935770f, 1e10f, 1e30f, FLT_MAX, -FLT_MAX, 4e36f, 8388608.5f, -8388607.5f, INFINITY, -INFINITY,
    NAN, -NAN};
static const int NS[] = {-3, -1, 0, 1, 2, 5, 30, 200};

int main(void) {
    const double dsnan = __builtin_nans(""), dpnan = __builtin_nan("0x1234");
    const float fsnan = __builtin_nansf(""), fpnan = __builtin_nanf("0x1234");
    setvbuf(stdout, NULL, _IOFBF, 1 << 16);
    size_t nd = sizeof DV / sizeof DV[0], nf = sizeof FV / sizeof FV[0];
    for (int m = 0; m < 4; m++) {
        char mc = "NUDZ"[m];
        for (size_t i = 0; i < sizeof UN / sizeof UN[0]; i++) {
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
        /* lgamma, gamma and lgamma_r: the value and the sign stored. */
        for (size_t j = 0; j < nd + 2; j++) {
            volatile double x = j < nd ? DV[j] : j == nd ? dsnan : dpnan;
            for (int which = 0; which < 3; which++) {
                int sg = 7;
                signgam = 7;
                fesetround(MODES[m]);
                feclearexcept(FE_ALL_EXCEPT);
                errno = 0;
                double r = which == 0 ? lgamma(x) : which == 1 ? gamma(x) : lgamma_r(x, &sg);
                int f = fetestexcept(FE_ALL_EXCEPT), e = errno;
                fesetround(FE_TONEAREST);
                printf("%s %c %016llx = %016llx %08x", which == 0 ? "lgamma" : which == 1 ? "gamma" : "lgamma_r", mc,
                       (unsigned long long)bd(x), (unsigned long long)bd(r), (unsigned)(which == 2 ? sg : signgam));
                report(f, e);
            }
        }
        for (size_t j = 0; j < nf + 2; j++) {
            volatile float x = j < nf ? FV[j] : j == nf ? fsnan : fpnan;
            for (int which = 0; which < 3; which++) {
                int sg = 7;
                signgam = 7;
                fesetround(MODES[m]);
                feclearexcept(FE_ALL_EXCEPT);
                errno = 0;
                float r = which == 0 ? lgammaf(x) : which == 1 ? gammaf(x) : lgammaf_r(x, &sg);
                int f = fetestexcept(FE_ALL_EXCEPT), e = errno;
                fesetround(FE_TONEAREST);
                printf("%s %c %08x = %08x %08x", which == 0 ? "lgammaf" : which == 1 ? "gammaf" : "lgammaf_r", mc, bf(x), bf(r),
                       (unsigned)(which == 2 ? sg : signgam));
                report(f, e);
            }
        }
        /* jn, yn: integer orders. */
        for (size_t i = 0; i < 2; i++)
            for (size_t k = 0; k < sizeof NS / sizeof NS[0]; k++) {
                for (size_t j = 0; j < nd + 1; j++) {
                    volatile double x = j < nd ? DV[j] : dsnan;
                    fesetround(MODES[m]);
                    feclearexcept(FE_ALL_EXCEPT);
                    errno = 0;
                    double r = NN[i].d(NS[k], x);
                    int f = fetestexcept(FE_ALL_EXCEPT), e = errno;
                    fesetround(FE_TONEAREST);
                    printf("%s %c %08x %016llx = %016llx", NN[i].name, mc, (unsigned)NS[k], (unsigned long long)bd(x), (unsigned long long)bd(r));
                    report(f, e);
                }
                for (size_t j = 0; j < nf + 1; j++) {
                    volatile float x = j < nf ? FV[j] : fsnan;
                    fesetround(MODES[m]);
                    feclearexcept(FE_ALL_EXCEPT);
                    errno = 0;
                    float r = NN[i].f(NS[k], x);
                    int f = fetestexcept(FE_ALL_EXCEPT), e = errno;
                    fesetround(FE_TONEAREST);
                    printf("%sf %c %08x %08x = %08x", NN[i].name, mc, (unsigned)NS[k], bf(x), bf(r));
                    report(f, e);
                }
            }
    }
    return 0;
}
