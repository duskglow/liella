/* clog / clogf / csqrt / csqrtf per call (LIBM-CPLX-LOG-SQRT): cbench_libm's own 256 operands (its
 * xorshift and seed, [-3, 3]^2), so the counts tie to the bench rows; plus the real axis (y = +0,
 * x in (0, 3)), which the quick tiers leave to the 384-bit route, to show what those operands cost.
 * no spec clause (measurement probe). */
#include "percall.h"
#include <complex.h>
#include <math.h>

static volatile double bx[256], by[256];

static double b_urand(unsigned long long *r) {
    *r ^= *r << 13;
    *r ^= *r >> 7;
    *r ^= *r << 17;
    return (double)(*r >> 11) / 9007199254740992.0;
}

static void b_fill(void) {
    unsigned long long r = 88172645463325252ULL;
    for (int i = 0; i < 256; i++) {
        double x = -3 + 6 * b_urand(&r), y = -3 + 6 * b_urand(&r);
        bx[i] = x;
        by[i] = y;
    }
}
#define BROW(name, fn, T, CT) \
    CASE(name, 0, 1) { \
        b_fill(); \
        T acc = 0; \
        for (long i = 0; i < n; i++) { \
            CT z = (T)bx[i & 255] + (T)by[i & 255] * (CT)I; \
            CT r = fn(z); \
            acc += __real__ r + __imag__ r; \
        } \
        pc_sink += (long)acc; \
        PC_CHECKING { CT z = (T)bx[3] + (T)by[3] * (CT)I; CT r = fn(z); \
            pc_check("%d %d", PC_SIGN(__real__ r), PC_SIGN(__imag__ r)); } \
    }
BROW(clog_bench, clog, double, double complex)
BROW(clogf_bench, clogf, float, float complex)
BROW(csqrt_bench, csqrt, double, double complex)
BROW(csqrtf_bench, csqrtf, float, float complex)

/* The positive real axis: x = |bx| (never 0 for this seed), y = +0. */
#define AROW(name, fn, T, CT) \
    CASE(name, 0, 1) { \
        b_fill(); \
        T acc = 0; \
        for (long i = 0; i < n; i++) { \
            CT z = CMPLX(fabs(bx[i & 255]), 0.0); \
            CT r = fn(z); \
            acc += __real__ r + __imag__ r; \
        } \
        pc_sink += (long)acc; \
        PC_CHECKING { CT z = CMPLX(fabs(bx[3]), 0.0); CT r = fn(z); \
            pc_check("%d %d", PC_SIGN(__real__ r), PC_SIGN(__imag__ r)); } \
    }
AROW(clog_axis, clog, double, double complex)
AROW(csqrt_axis, csqrt, double, double complex)
