/* cexp / cexpf per call (LIBM-CPLX-EXP-TIER-2): operands uniform in [-3, 3]^2 as cbench_libm's
 * complex rows, 32 values cycled through a volatile table so no call is folded.
 * no spec clause (measurement probe). */
#include "percall.h"
#include <complex.h>
#include <math.h>

static volatile double zx[32], zy[32];

static void z_fill(void) {
    unsigned s = 0x2545f491u;
    for (int i = 0; i < 32; i++) {
        s = s * 1103515245u + 12345u;
        zx[i] = (double)(s >> 8) * 0x1p-24 * 6.0 - 3.0;
        s = s * 1103515245u + 12345u;
        zy[i] = (double)(s >> 8) * 0x1p-24 * 6.0 - 3.0;
    }
}
#define ZROW(name, fn, T, CT, calls) \
    CASE(name, calls, 1) { \
        z_fill(); \
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
ZROW(cexp, cexp, double, double complex, 0)
ZROW(cexpf, cexpf, float, float complex, 0)

/* cbench_libm's own 256 operands (its xorshift and seed, [-3, 3]^2), to tie this probe to the
 * bench row. */
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
BROW(cexp_bench, cexp, double, double complex)
BROW(cexpf_bench, cexpf, float, float complex)
