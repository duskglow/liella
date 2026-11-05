/* x87 long double exp/log family per call (LIBM-WIDE-DD-TIER). The `*` cases use the cbench_libm operands (a double in
 * the bench's range, widened to long double); the `*_full` cases use 64-bit significands in the same range, so the
 * count does not depend on the low 11 bits being zero. 64 operands cycled through a volatile table so no call is
 * folded; the sum goes to pc_sink. no spec clause (measurement probe). */
#include "percall.h"
#include <math.h>

static volatile long double lx[64];

static void fill(double lo, double hi, int full) {
    unsigned s = 0x2545f491u;
    for (int i = 0; i < 64; i++) {
        s = s * 1103515245u + 12345u;
        double u = (double)(s >> 8) / 16777216.0;
        long double x = (long double)(lo + (hi - lo) * u);
        if (full) {
            s = s * 1103515245u + 12345u;
            /* perturb below the double's last bit: up to 2^-60 relative */
            x += x * ((long double)(s >> 8) / 16777216.0L) * 0x1p-53L;
        }
        lx[i] = x;
    }
}

#define L1(NAME, FN, LO, HI, FULL) \
CASE(NAME, 20000, 1) { \
    fill(LO, HI, FULL); long double acc = 0; \
    for (long i = 0; i < n; i++) acc += FN(lx[i & 63]); \
    pc_sink += (long)acc; \
    PC_CHECKING { pc_check("%La", FN(lx[5])); } \
}

L1(expl, expl, -10.0, 10.0, 0)
L1(exp2l, exp2l, -10.0, 10.0, 0)
L1(exp10l, exp10l, -10.0, 10.0, 0)
L1(expm1l, expm1l, -10.0, 10.0, 0)
L1(logl, logl, 0.01, 100.0, 0)
L1(log2l, log2l, 0.01, 100.0, 0)
L1(log10l, log10l, 0.01, 100.0, 0)
L1(log1pl, log1pl, -0.9, 10.0, 0)
L1(expl_full, expl, -10.0, 10.0, 1)
L1(logl_full, logl, 0.01, 100.0, 1)
L1(expm1l_full, expm1l, -10.0, 10.0, 1)
L1(log1pl_full, log1pl, -0.9, 10.0, 1)
