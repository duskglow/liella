/* erff/erfcf per call (LIBM-F32-REST-NEXT-3): the bench's rows ([-5, 5], as cbench_libm) and one case per branch
 * of the fast32 kernels (crates/math/src/fast32/erf.rs): |x| < 1 (erf_small), 1 <= |x| < 4 (1 - erfc_big),
 * erfc of 1 <= x < 9.1875 (erfc_big) and erfc of x <= -1 (2 - erfc_big). 64 operands cycled through a volatile
 * table so no call is folded; the sum goes to pc_sink. no spec clause (measurement probe). */
#include "percall.h"
#include <math.h>

static volatile float ef[64];

static void fillf(float lo, float hi) {
    unsigned s = 0x2545f491u;
    for (int i = 0; i < 64; i++) {
        s = s * 1103515245u + 12345u;
        ef[i] = lo + (hi - lo) * (float)(s >> 8) / 16777216.0f;
    }
}

#define F1(CASE_NAME, NAME, LO, HI) \
CASE(CASE_NAME, 100000, 1) { \
    fillf(LO, HI); float acc = 0; \
    for (long i = 0; i < n; i++) acc += NAME(ef[i & 63]); \
    pc_sink += (long)acc; \
    PC_CHECKING { pc_check("%a %a", (double)NAME(ef[5]), (double)NAME(ef[17])); } \
}

F1(erff, erff, -5.0f, 5.0f)
F1(erfcf, erfcf, -5.0f, 5.0f)
F1(erff_small, erff, -0.99f, 0.99f)
F1(erff_big, erff, 1.0f, 3.99f)
F1(erfcf_small, erfcf, -0.99f, 0.99f)
F1(erfcf_pos, erfcf, 1.0f, 9.0f)
F1(erfcf_neg, erfcf, -3.99f, -1.0f)
