/* double erf/erfc per call (LIBM-SPECIAL-FAST-TIER-3): the bench's row ([-5, 5], as cbench_libm) and one case per
 * branch of the fast tier (crates/math/src/fast_special.rs): erf's series (|x| < 2^-5) and table (2^-5 <= |x| < 6),
 * erfc through 1 - erf (-6 < x < 1) and through e^(-x^2)·Y (1 <= x < 27.25). 64 operands cycled through a volatile
 * table so no call is folded; the sum goes to pc_sink. no spec clause (measurement probe).
 * The self-check prints 15 significant digits, not every bit: Liella's erf/erfc are correctly rounded and the
 * reference may differ in the last bit (erfc(0x1.b12e88p+0), the erfc row's ex[17]: correctly rounded
 * 0x1.11c9655380463p-6, a quad-precision oracle puts the exact value 0.69 ulp above ...462p-6; pinned by
 * crates/boundary/tests/erf_quick.rs), which is not a different answer. */
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

F1(erf, erf, -5, 5)
F1(erfc, erfc, -5, 5)
F1(erf_series, erf, -0x1p-5, 0x1p-5)
F1(erf_table, erf, 0.03125, 5.9)
F1(erfc_below1, erfc, -5.9, 0.99)
F1(erfc_above1, erfc, 1, 26)
