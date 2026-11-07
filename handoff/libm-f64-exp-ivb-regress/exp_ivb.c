/* exp per call over the cbench_libm row ranges (LIBM-F64-EXP-IVB-REGRESS): exp [-10, 10], exp_small [-0.5, 0.5],
 * plus the two halves of the wide range (|x| > 0.5 only) to split the paths. 256 operands cycled through a volatile
 * table so no call is folded. no spec clause (measurement probe). */
#include "percall.h"
#include <math.h>

static volatile double px[256];

static void fill(double lo, double hi, int skip_small) {
    unsigned long s = 88172645463325252UL;
    int i = 0;
    while (i < 256) {
        s ^= s << 13; s ^= s >> 7; s ^= s << 17;
        double x = lo + (hi - lo) * (double)(s >> 11) / 9007199254740992.0;
        if (skip_small && x > -0.5 && x < 0.5)
            continue;
        px[i++] = x;
    }
}

#define E1(NAME, LO, HI, SKIP) \
CASE(NAME, 100000, 1) { \
    fill(LO, HI, SKIP); double acc = 0; \
    for (long i = 0; i < n; i++) acc += exp(px[i & 255]); \
    pc_sink += (long)acc; \
    PC_CHECKING { pc_check("%a %a", exp(px[5]), exp(px[77])); } \
}

E1(exp_wide, -10.0, 10.0, 0)
E1(exp_small, -0.5, 0.5, 0)
E1(exp_big, -10.0, 10.0, 1)
