/* _Float128 sine and cosine per call (LIBM-F128-TRIG-INT, D-10.85). Operands with 113-bit significands (a double in
 * the range, perturbed below its last bit, so the count does not depend on the low 60 bits being zero): [-10, 10]
 * (`sinf128`, `cosf128`) and the large arguments of the Payne-Hanek reduction (`*_1e10`, `*_1e300`). 64 operands cycled
 * through a volatile table so no call is folded; the sum goes to pc_sink. The self-check prints 15 digits of the
 * result converted to double. no spec clause (measurement probe). */
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

Q1(sinf128, sinf128, -10.0, 10.0)
Q1(cosf128, cosf128, -10.0, 10.0)
Q1(sinf128_1e10, sinf128, 1e10, 2e10)
Q1(cosf128_1e10, cosf128, 1e10, 2e10)
Q1(sinf128_1e300, sinf128, 1e300, 2e300)
Q1(cosf128_1e300, cosf128, 1e300, 2e300)

/* tanf128 and sincosf128 on the same reduction (LIBM-F128-TRIG-INT-2, D-10.91). */
Q1(tanf128, tanf128, -10.0, 10.0)
Q1(tanf128_1e300, tanf128, 1e300, 2e300)

CASE(sincosf128, 20000, 1) {
    fill(-10.0, 10.0); _Float128 acc = 0, s, c;
    for (long i = 0; i < n; i++) { sincosf128(qx[i & 63], &s, &c); acc += s + c; }
    pc_sink += (long)acc;
    PC_CHECKING { sincosf128(qx[5], &s, &c); pc_check("%.15g %.15g", (double)s, (double)c); }
}
