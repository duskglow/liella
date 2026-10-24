/* hypotf per call (MATH-M6-F32-HYPOT, #1541): the fast path (sqrtsd + tail test) on typical finite operands.
 * 64 operand pairs cycled through a volatile table, so the call is never folded; the sum goes to pc_sink. */
#include "percall.h"
#include <math.h>

static volatile float hx[64], hy[64];

static void hyp_fill(void) {
    unsigned s = 0x2545f491u;
    for (int i = 0; i < 64; i++) {
        s = s * 1103515245u + 12345u;
        hx[i] = (float)(s >> 8) * 0x1p-12f;
        s = s * 1103515245u + 12345u;
        hy[i] = (float)(s >> 8) * 0x1p-14f;
    }
}

CASE(hypotf, 100000, 1) {
    hyp_fill();
    float acc = 0;
    for (long i = 0; i < n; i++) acc += hypotf(hx[i & 63], hy[i & 63]);
    pc_sink += (long)acc;
    PC_CHECKING { float r = hypotf(hx[5], hy[7]); pc_check("%a %a", (double)r, (double)hypotf(3.0f, 4.0f)); }
}
