/* cpow / cpowf per call (LIBM-CPLX-POW-5). Typical rows: z and w uniform in [-3, 3]^2.
 * Band rows: |w log z| log-uniform in the three bands of cpow_decline_rates (2^-6..1, 1..16,
 * 16..512), where the quick tier declines 2^-9.1, 2^-6.4 and 2^-2.4 of the parts and the middle
 * tier decides them; w is chosen as m e^{it} / clog(z) during setup (setup is outside the count).
 * 64 operand pairs cycled through volatile tables so no call is folded.
 * no spec clause (measurement probe). */
#include "percall.h"
#include <complex.h>
#include <math.h>

static volatile double zx[64], zy[64], wx[64], wy[64];

static double p_urand(unsigned long long *r) {
    *r ^= *r << 13;
    *r ^= *r >> 7;
    *r ^= *r << 17;
    return (double)(*r >> 11) / 9007199254740992.0;
}

/* lo2, hi2: the band of log2 |w log z|; lo2 > hi2 selects the typical operands. */
static void p_fill(double lo2, double hi2) {
    unsigned long long r = 0x9e3779b97f4a7c15ULL;
    for (int i = 0; i < 64; i++) {
        double x = -3 + 6 * p_urand(&r), y = -3 + 6 * p_urand(&r);
        double u = p_urand(&r), t = p_urand(&r);
        double a = -3 + 6 * u, b = -3 + 6 * t;
        if (lo2 <= hi2) {
            double complex l = clog(x + y * I);
            double m = exp2(lo2 + (hi2 - lo2) * u);
            double complex w = m * cexp(6.283185307179586 * t * I) / l;
            a = creal(w);
            b = cimag(w);
        }
        zx[i] = x;
        zy[i] = y;
        wx[i] = a;
        wy[i] = b;
    }
}
#define PROW(name, fn, T, CT, lo2, hi2) \
    CASE(name, 0, 1) { \
        p_fill(lo2, hi2); \
        T acc = 0; \
        for (long i = 0; i < n; i++) { \
            CT z = (T)zx[i & 63] + (T)zy[i & 63] * (CT)I; \
            CT w = (T)wx[i & 63] + (T)wy[i & 63] * (CT)I; \
            CT r = fn(z, w); \
            acc += __real__ r + __imag__ r; \
        } \
        pc_sink += (long)acc; \
        PC_CHECKING { CT z = (T)zx[3] + (T)zy[3] * (CT)I; CT w = (T)wx[3] + (T)wy[3] * (CT)I; \
            CT r = fn(z, w); pc_check("%d %d", PC_SIGN(__real__ r), PC_SIGN(__imag__ r)); } \
    }
PROW(cpow, cpow, double, double complex, 1, 0)
PROW(cpow_band_small, cpow, double, double complex, -6, 0)
PROW(cpow_band_mid, cpow, double, double complex, 0, 4)
PROW(cpow_band_large, cpow, double, double complex, 4, 9)
PROW(cpowf, cpowf, float, float complex, 1, 0)
/* float: |w log z| in 4..64, so e^E stays inside the float range. */
PROW(cpowf_band_large, cpowf, float, float complex, 2, 6)
