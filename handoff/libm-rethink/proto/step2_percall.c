/* LIBM-RETHINK step 2: representative libm calls per precision class and family, per call.
 * 64 operands in the cbench_libm-style ranges cycled through volatile tables so no call is folded; the sum goes to
 * pc_sink. no spec clause (measurement probe). */
#define __STDC_WANT_IEC_60559_TYPES_EXT__ 1
#include "percall.h"
#include <complex.h>
#include <fenv.h>
#include <math.h>

static volatile float fx[64], fy[64];
static volatile double dx[64], dy[64];
static volatile long double lx[64];
static volatile _Float128 qx[64];

static void fill(double lo, double hi, double lo2, double hi2) {
    unsigned s = 0x2545f491u;
    for (int i = 0; i < 64; i++) {
        s = s * 1103515245u + 12345u;
        double u = (double)(s >> 8) / 16777216.0;
        s = s * 1103515245u + 12345u;
        double v = (double)(s >> 8) / 16777216.0;
        s = s * 1103515245u + 12345u;
        double w = (double)(s >> 8) / 16777216.0 * 0x1p-53;
        double a = lo + (hi - lo) * u, b = lo2 + (hi2 - lo2) * v;
        dx[i] = a; dy[i] = b; fx[i] = (float)a; fy[i] = (float)b;
        lx[i] = (long double)a + (long double)a * (long double)w; /* full 64-bit significands */
        qx[i] = (_Float128)a + (_Float128)a * (_Float128)w * (_Float128)v;
    }
}

#define F1(NAME, FN, LO, HI) CASE(NAME, 20000, 1) { fill(LO, HI, 0, 1); float acc = 0; \
    for (long i = 0; i < n; i++) acc += FN(fx[i & 63]); pc_sink += (long)acc; \
    PC_CHECKING { pc_check("%a", (double)FN(fx[5])); } }
#define D1(NAME, FN, LO, HI) CASE(NAME, 20000, 1) { fill(LO, HI, 0, 1); double acc = 0; \
    for (long i = 0; i < n; i++) acc += FN(dx[i & 63]); pc_sink += (long)acc; \
    PC_CHECKING { pc_check("%a", FN(dx[5])); } }
#define L1(NAME, FN, LO, HI) CASE(NAME, 20000, 1) { fill(LO, HI, 0, 1); long double acc = 0; \
    for (long i = 0; i < n; i++) acc += FN(lx[i & 63]); pc_sink += (long)acc; \
    PC_CHECKING { pc_check("%La", FN(lx[5])); } }
#define Q1(NAME, FN, LO, HI) CASE(NAME, 20000, 1) { fill(LO, HI, 0, 1); _Float128 acc = 0; \
    for (long i = 0; i < n; i++) acc += FN(qx[i & 63]); pc_sink += (long)acc; \
    PC_CHECKING { pc_check("%La", (long double)FN(qx[5])); } }
#define CF(NAME, FN, LO, HI, LO2, HI2) CASE(NAME, 20000, 1) { fill(LO, HI, LO2, HI2); float acc = 0; \
    for (long i = 0; i < n; i++) { float _Complex z = CMPLXF(fx[i & 63], fy[i & 63]); \
        float _Complex r = FN(z); acc += crealf(r) + cimagf(r); } pc_sink += (long)acc; \
    PC_CHECKING { float _Complex r = FN(CMPLXF(fx[5], fy[5])); pc_check("%a %a", (double)crealf(r), (double)cimagf(r)); } }
#define CD(NAME, FN, LO, HI, LO2, HI2) CASE(NAME, 20000, 1) { fill(LO, HI, LO2, HI2); double acc = 0; \
    for (long i = 0; i < n; i++) { double _Complex z = CMPLX(dx[i & 63], dy[i & 63]); \
        double _Complex r = FN(z); acc += creal(r) + cimag(r); } pc_sink += (long)acc; \
    PC_CHECKING { double _Complex r = FN(CMPLX(dx[5], dy[5])); pc_check("%a %a", creal(r), cimag(r)); } }

F1(expf, expf, -10, 10)
F1(logf, logf, 0.01, 100)
F1(sinf, sinf, -10, 10)
CASE(powf, 20000, 1) { fill(0.01, 10, -10, 10); float acc = 0;
    for (long i = 0; i < n; i++) acc += powf(fx[i & 63], fy[i & 63]); pc_sink += (long)acc;
    PC_CHECKING { pc_check("%a", (double)powf(fx[5], fy[5])); } }
D1(exp, exp, -10, 10)
D1(log, log, 0.01, 100)
D1(sin, sin, -10, 10)
D1(atan, atan, -10, 10)
CASE(pow, 20000, 1) { fill(0.01, 10, -10, 10); double acc = 0;
    for (long i = 0; i < n; i++) acc += pow(dx[i & 63], dy[i & 63]); pc_sink += (long)acc;
    PC_CHECKING { pc_check("%a", pow(dx[5], dy[5])); } }
CASE(exp_upward, 20000, 1) { fill(-10, 10, 0, 1); double acc = 0; fesetround(FE_UPWARD);
    for (long i = 0; i < n; i++) acc += exp(dx[i & 63]); fesetround(FE_TONEAREST); pc_sink += (long)acc;
    PC_CHECKING { fesetround(FE_UPWARD); double r = exp(dx[5]); fesetround(FE_TONEAREST); pc_check("%a", r); } }
L1(expl, expl, -10, 10)
L1(logl, logl, 0.01, 100)
L1(sinl, sinl, -10, 10)
L1(atanl, atanl, -10, 10)
L1(sqrtl, sqrtl, 0.01, 100)
L1(cbrtl, cbrtl, 0.01, 100)
Q1(expf128, expf128, -10, 10)
Q1(logf128, logf128, 0.01, 100)
Q1(sinf128, sinf128, -10, 10)
Q1(atanf128, atanf128, -10, 10)
CF(cexpf, cexpf, -2, 2, -2, 2)
CD(cexp, cexp, -2, 2, -2, 2)
CD(clog, clog, 0.01, 10, 0.01, 10)
CD(csqrt, csqrt, -10, 10, -10, 10)
