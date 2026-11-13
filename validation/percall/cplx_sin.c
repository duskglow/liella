/* csin / csinh / csinf / csinhf per call (LIBM-CPLX-SINCOS): typical operands uniform in [-3, 3]^2 as
 * cbench_libm's complex rows, a small-|y| set (csin's hyperbolic argument in the series band
 * 2^-20 <= |y| < 2^-4) and a large-|y| set (40 <= |y| < 700, e^-|y| dropped, the result still
 * finite); 32 values cycled through a volatile table so no call is folded.
 * no spec clause (measurement probe). */
#include "percall.h"
#include <complex.h>
#include <math.h>

static volatile double zx[32], zy[32];

static double z_next(unsigned *s) {
    *s = *s * 1103515245u + 12345u;
    return (double)(*s >> 8) * 0x1p-24;
}

/* kind 0: [-3, 3]^2; 1: |y| log-uniform in [2^-20, 2^-4); 2: |y| uniform in [40, 700). */
static void z_fill(int kind) {
    unsigned s = 0x2545f491u;
    for (int i = 0; i < 32; i++) {
        zx[i] = z_next(&s) * 6.0 - 3.0;
        double u = z_next(&s), sg = (i & 1) ? -1.0 : 1.0;
        zy[i] = kind == 0 ? u * 6.0 - 3.0 : kind == 1 ? sg * exp2(-20.0 + 16.0 * u) : sg * (40.0 + 660.0 * u);
    }
}
#define ZROW(name, fn, T, CT, kind) \
    CASE(name, 0, 1) { \
        z_fill(kind); \
        T acc = 0; \
        for (long i = 0; i < n; i++) { \
            CT z = (T)zx[i & 31] + (T)zy[(i + 5) & 31] * (CT)I; \
            CT r = fn(z); \
            acc += __real__ r + __imag__ r; \
        } \
        pc_sink += (long)acc; \
        PC_CHECKING { CT z = (T)zx[3] + (T)zy[4] * (CT)I; CT r = fn(z); \
            pc_check("%d %d", PC_SIGN(__real__ r), PC_SIGN(__imag__ r)); } \
    }
ZROW(csin, csin, double, double complex, 0)
ZROW(csinh, csinh, double, double complex, 0)
ZROW(csinf, csinf, float, float complex, 0)
ZROW(csinhf, csinhf, float, float complex, 0)
ZROW(csin_small_y, csin, double, double complex, 1)
ZROW(csin_large_y, csin, double, double complex, 2)
ZROW(csinf_small_y, csinf, float, float complex, 1)

/* cbench_libm's own 256 operands (its xorshift and seed, [-3, 3]^2), to tie this probe to the
 * bench rows. */
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
BROW(csin_bench, csin, double, double complex)
BROW(csinf_bench, csinf, float, float complex)
