/* Complex _Float128 fast path per call (MATH-M5-CPLX-F128-ZIV-B2-3, #1940): typical finite operands,
 * 32 argument values cycled through a volatile table so no call is folded. no spec clause (measurement probe). */
#define __STDC_WANT_IEC_60559_TYPES_EXT__ 1
#include "percall.h"
#include <complex.h>
#include <math.h>

static volatile _Float128 zx[32], zy[32];

static void z_fill(void) {
    unsigned s = 0x2545f491u;
    for (int i = 0; i < 32; i++) {
        s = s * 1103515245u + 12345u;
        zx[i] = (_Float128)(s >> 8) * 0x1p-21f128 + 0.25f128;
        s = s * 1103515245u + 12345u;
        zy[i] = (_Float128)(s >> 8) * 0x1p-22f128 + 0.125f128;
    }
}
#define ZROW(name, fn, calls) \
    CASE(name, calls, 1) { \
        z_fill(); \
        _Float128 acc = 0; \
        for (long i = 0; i < n; i++) { \
            _Complex _Float128 z = zx[i & 31] + zy[(i + 5) & 31] * 1.0iF128; \
            _Complex _Float128 r = fn(z); \
            acc += __real__ r + __imag__ r; \
        } \
        pc_sink += (long)acc; \
        PC_CHECKING { _Complex _Float128 z = zx[3] + zy[4] * 1.0iF128; _Complex _Float128 r = fn(z); \
            pc_check("%d %d", PC_SIGN(__real__ r), PC_SIGN(__imag__ r)); } \
    }
ZROW(cexpf128, cexpf128, 0)
ZROW(clogf128, clogf128, 0)
ZROW(csqrtf128, csqrtf128, 0)
ZROW(csinf128, csinf128, 0)
ZROW(ccosf128, ccosf128, 0)
ZROW(ctanf128, ctanf128, 0)
ZROW(csinhf128, csinhf128, 0)
ZROW(casinf128, casinf128, 0)
ZROW(catanf128, catanf128, 0)
CASE(cpowf128, 0, 1) {
    z_fill();
    _Float128 acc = 0;
    for (long i = 0; i < n; i++) {
        _Complex _Float128 z = zx[i & 31] + zy[(i + 5) & 31] * 1.0iF128;
        _Complex _Float128 w = zy[(i + 9) & 31] + zx[(i + 2) & 31] * 1.0iF128;
        _Complex _Float128 r = cpowf128(z, w);
        acc += __real__ r + __imag__ r;
    }
    pc_sink += (long)acc;
}
