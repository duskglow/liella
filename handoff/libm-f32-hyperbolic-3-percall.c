/* Hyperbolic float functions per call (LIBM-F32-HYPERBOLIC-3): typical finite operands on each fast path,
 * plus expf (in-place nearest entry) and sinf (full envelope) as controls for the entry/exit share.
 * 64 operands cycled through a volatile table, so no call is folded; the sum goes to pc_sink. */
#include "percall.h"
#include <math.h>

static volatile float hv[64];

static void hv_fill(float lo, float span) {
    unsigned s = 0x2545f491u;
    for (int i = 0; i < 64; i++) {
        s = s * 1103515245u + 12345u;
        hv[i] = lo + (float)(s >> 8) * 0x1p-24f * span;
    }
}

#define HCASE(fn, lo, span, probe) \
    CASE(fn, 100000, 1) { \
        hv_fill(lo, span); \
        float acc = 0; \
        for (long i = 0; i < n; i++) acc += fn(hv[i & 63]); \
        pc_sink += (long)acc; \
        PC_CHECKING { pc_check("%a", (double)fn(probe)); } \
    }

HCASE(sinhf, -8.0f, 16.0f, 0.75f)
HCASE(coshf, -8.0f, 16.0f, 0.75f)
HCASE(tanhf, -4.0f, 8.0f, 0.75f)
HCASE(asinhf, -8.0f, 16.0f, 0.75f)
HCASE(acoshf, 1.0f, 15.0f, 1.75f)
HCASE(atanhf, -0.95f, 1.9f, 0.75f)
HCASE(expf, -8.0f, 16.0f, 0.75f)
HCASE(sinf, -3.0f, 6.0f, 0.75f)
