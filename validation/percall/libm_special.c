/* double lgamma/tgamma/j0/j1/y0/y1 per call (LIBM-F64-SPECIAL-QUICK-REST): the bench's rows (cbench_libm's
 * ranges: gamma [-10, 10], j0/j1 [-20, 20], y0/y1 [0.01, 20]) and, for the gamma pair, the positive and negative
 * halves (crates/math/src/fast_gamma.rs: the table range x >= 2^-5 and the reflection product x <= -1/2), so a
 * quick phase can be aimed at the branch that holds the surplus. 64 operands cycled through a volatile table so no
 * call is folded; the sum goes to pc_sink. The self-check prints 15 significant digits (correctly rounded results
 * may differ from the reference in the last bit). no spec clause (measurement probe). */
#include "percall.h"
#include <math.h>

static volatile double ex[64];

static void fill(double lo, double hi) {
    unsigned s = 0x2545f491u;
    for (int i = 0; i < 64; i++) {
        s = s * 1103515245u + 12345u;
        ex[i] = lo + (hi - lo) * (double)(s >> 8) / 16777216.0;
    }
}

#define F1(CASE_NAME, NAME, LO, HI) \
CASE(CASE_NAME, 100000, 1) { \
    fill(LO, HI); double acc = 0; \
    for (long i = 0; i < n; i++) acc += NAME(ex[i & 63]); \
    pc_sink += (long)acc; \
    PC_CHECKING { pc_check("%.15g %.15g", NAME(ex[5]), NAME(ex[17])); } \
}

F1(lgamma, lgamma, -10, 10)
F1(lgamma_pos, lgamma, 0.03125, 10)
F1(lgamma_neg, lgamma, -10, -0.5)
F1(tgamma, tgamma, -10, 10)
F1(tgamma_pos, tgamma, 0.03125, 10)
F1(tgamma_neg, tgamma, -10, -0.5)
F1(j0, j0, -20, 20)
F1(j1, j1, -20, 20)
F1(y0, y0, 0.01, 20)
F1(y1, y1, 0.01, 20)
