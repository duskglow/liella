/* libm per call, one representative per parity group (PARITY-LIBM-1105). 64 operands in the bench's ranges cycled through
 * a volatile table so no call is folded; the sum goes to pc_sink. no spec clause (measurement probe). */
#include "percall.h"
#include <math.h>

static volatile double px[64], py[64];
static volatile long double lx[64];

static void fill(double lo, double hi) {
    unsigned s = 0x2545f491u;
    for (int i = 0; i < 64; i++) {
        s = s * 1103515245u + 12345u;
        px[i] = lo + (hi - lo) * (double)(s >> 8) / 16777216.0;
        s = s * 1103515245u + 12345u;
        py[i] = lo + (hi - lo) * (double)(s >> 8) / 16777216.0;
        lx[i] = (long double)px[i];
    }
}

#define F1(NAME, LO, HI) \
CASE(NAME, 100000, 1) { \
    fill(LO, HI); double acc = 0; \
    for (long i = 0; i < n; i++) acc += NAME(px[i & 63]); \
    pc_sink += (long)acc; \
    PC_CHECKING { pc_check("%a", NAME(px[5])); } \
}

F1(expm1, -10, 10)
F1(sin, -10, 10)
F1(tanh, -10, 10)
F1(erf, -5, 5)

CASE(atan2, 100000, 1) {
    fill(-10, 10); double acc = 0;
    for (long i = 0; i < n; i++) acc += atan2(py[i & 63], px[i & 63]);
    pc_sink += (long)acc;
    PC_CHECKING { pc_check("%a", atan2(py[5], px[5])); }
}

CASE(expl, 20000, 1) {
    fill(-10, 10); long double acc = 0;
    for (long i = 0; i < n; i++) acc += expl(lx[i & 63]);
    pc_sink += (long)acc;
    PC_CHECKING { pc_check("%a", (double)expl(lx[5])); }
}
