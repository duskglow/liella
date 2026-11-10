/* x87 long double atanl, atan2l, asinl, acosl and tanl per call (LIBM-LD-TRIG-INVERSE-INT, D-10.75). Operands with
 * 64-bit significands (a double in the range, perturbed below its last bit, so the count does not depend on the low
 * 11 bits being zero): atanl on [-10, 10] and [1e3, 1e30]; atan2l over all four quadrants; asinl/acosl on [-1, 1] and
 * on [0.5, 1) (the root of 1 - a^2 near 0); tanl on [-10, 10] and the Payne-Hanek range (`tanl_1e300`). 64 operands
 * cycled through a volatile table so no call is folded; the sum goes to pc_sink. The self-check prints 15 digits.
 * no spec clause (measurement probe). */
#include "percall.h"
#include <math.h>

static volatile long double lx[64], ly[64];

static void fill_into(volatile long double *t, double lo, double hi, unsigned s) {
    for (int i = 0; i < 64; i++) {
        s = s * 1103515245u + 12345u;
        double u = (double)(s >> 8) / 16777216.0;
        long double x = (long double)(lo + (hi - lo) * u);
        s = s * 1103515245u + 12345u;
        /* perturb below the double's last bit: up to 2^-53 relative */
        x += x * ((long double)(s >> 8) / 16777216.0L) * 0x1p-53L;
        t[i] = x;
    }
}

static void fill(double lo, double hi) { fill_into(lx, lo, hi, 0x2545f491u); }

#define L1(NAME, FN, LO, HI) \
CASE(NAME, 20000, 1) { \
    fill(LO, HI); long double acc = 0; \
    for (long i = 0; i < n; i++) acc += FN(lx[i & 63]); \
    pc_sink += (long)acc; \
    PC_CHECKING { pc_check("%.15Lg", FN(lx[5])); } \
}

L1(atanl, atanl, -10.0, 10.0)
L1(atanl_big, atanl, 1e3, 1e30)
L1(asinl, asinl, -1.0, 1.0)
L1(asinl_near1, asinl, 0.5, 1.0)
L1(acosl, acosl, -1.0, 1.0)
L1(acosl_near1, acosl, 0.5, 1.0)
L1(tanl, tanl, -10.0, 10.0)
L1(tanl_1e300, tanl, 1e300, 2e300)

CASE(atan2l, 20000, 1) {
    fill_into(lx, -10.0, 10.0, 0x2545f491u);
    fill_into(ly, -10.0, 10.0, 0x9e3779b9u);
    long double acc = 0;
    for (long i = 0; i < n; i++) acc += atan2l(ly[i & 63], lx[(i + 7) & 63]);
    pc_sink += (long)acc;
    PC_CHECKING { pc_check("%.15Lg", atan2l(ly[5], lx[12])); }
}
