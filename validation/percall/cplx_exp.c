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
