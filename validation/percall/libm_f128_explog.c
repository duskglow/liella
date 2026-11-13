/* _Float128 e^x and ln x per call (LIBM-F128-EXPLOG, the three-limb integer tier). Operands with 113-bit significands
 * (a double in the range, perturbed below its last bit, so the count does not depend on the low 60 bits being zero):
 * expf128 on [-20, 20] (typical) and [-11000, 11000] (large k, near the overflow and underflow ends), logf128 on
 * [0.01, 1000] (E != 0 mostly) and [0.9, 1.1] (E = 0 and near 1). 64 operands cycled through a volatile table so no
 * call is folded; the sum goes to pc_sink. The self-check prints 15 digits of the result converted to double.
 * no spec clause (measurement probe). */
#define __STDC_WANT_IEC_60559_TYPES_EXT__ 1
#include "percall.h"
#include <math.h>

static volatile _Float128 qx[64];

static void fill(double lo, double hi) {
    unsigned s = 0x2545f491u;
    for (int i = 0; i < 64; i++) {
        s = s * 1103515245u + 12345u;
        double u = (double)(s >> 8) / 16777216.0;
        _Float128 x = (_Float128)(lo + (hi - lo) * u);
        s = s * 1103515245u + 12345u;
        /* perturb below the double's last bit: up to 2^-53 relative */
        x += x * ((_Float128)(s >> 8) / 16777216.0f128) * 0x1p-53f128;
        qx[i] = x;
    }
}

#define Q1(NAME, FN, LO, HI) \
CASE(NAME, 20000, 1) { \
    fill(LO, HI); _Float128 acc = 0; \
    for (long i = 0; i < n; i++) acc += FN(qx[i & 63]); \
    pc_sink += (long)acc; \
    PC_CHECKING { pc_check("%.15g", (double)FN(qx[5])); } \
}

Q1(expf128, expf128, -20.0, 20.0)
Q1(expf128_wide, expf128, -11000.0, 11000.0)
Q1(logf128, logf128, 0.01, 1000.0)
Q1(logf128_near1, logf128, 0.9, 1.1)
