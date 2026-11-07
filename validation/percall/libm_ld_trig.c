/* x87 long double sine and cosine per call (LIBM-LD-TRIG-INT, D-10.52). Operands with 64-bit significands (a double
 * in the range, perturbed below its last bit, so the count does not depend on the low 11 bits being zero): [-10, 10]
 * (`sinl`, `cosl`, `sincosl`), and the large arguments of the Payne-Hanek path (`*_1e10`, `*_1e300`). 64 operands
 * cycled through a volatile table so no call is folded; the sum goes to pc_sink. The self-check prints 15 digits
 * (both sides must agree there; the last bits of a reference that is not correctly rounded may differ).
 * no spec clause (measurement probe). */
#include "percall.h"
#include <math.h>

static volatile long double lx[64];

static void fill(double lo, double hi) {
    unsigned s = 0x2545f491u;
    for (int i = 0; i < 64; i++) {
        s = s * 1103515245u + 12345u;
        double u = (double)(s >> 8) / 16777216.0;
        long double x = (long double)(lo + (hi - lo) * u);
        s = s * 1103515245u + 12345u;
        /* perturb below the double's last bit: up to 2^-53 relative */
        x += x * ((long double)(s >> 8) / 16777216.0L) * 0x1p-53L;
        lx[i] = x;
    }
}

#define L1(NAME, FN, LO, HI) \
CASE(NAME, 20000, 1) { \
    fill(LO, HI); long double acc = 0; \
    for (long i = 0; i < n; i++) acc += FN(lx[i & 63]); \
    pc_sink += (long)acc; \
    PC_CHECKING { pc_check("%.15Lg", FN(lx[5])); } \
}

L1(sinl, sinl, -10.0, 10.0)
L1(cosl, cosl, -10.0, 10.0)
L1(sinl_1e10, sinl, 1e10, 2e10)
L1(cosl_1e10, cosl, 1e10, 2e10)
L1(sinl_1e300, sinl, 1e300, 2e300)
L1(cosl_1e300, cosl, 1e300, 2e300)

CASE(sincosl, 20000, 1) {
    fill(-10.0, 10.0);
    long double acc = 0;
    for (long i = 0; i < n; i++) {
        long double s, c;
        sincosl(lx[i & 63], &s, &c);
        acc += s + c;
    }
    pc_sink += (long)acc;
    PC_CHECKING {
        long double s, c;
        sincosl(lx[5], &s, &c);
        pc_check("%.15Lg %.15Lg", s, c);
    }
}
