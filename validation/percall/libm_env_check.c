/* wl_percall libm entry-check set (LIBM-ENV-CHECK-COST, D-10.101; no spec clause): sqrt and sqrtf, whose entries read no
 * MXCSR, and three entries that keep the read as controls (exp, expf, sinf). Every call goes through a volatile function
 * pointer so the compiler cannot expand sqrt/sqrtf inline as the instruction and never reach the entry. Inputs cycle
 * over 64 values in [0.1, 100] (`wl_percall.sh --spec validation/percall/libm_env_check.c`). Link: -lm. */
#include "percall.h"
#include <math.h>

static double dt[64];
static float ft[64];
static double (*volatile dsq)(double) = sqrt;
static float (*volatile fsq)(float) = sqrtf;
static double (*volatile dexp)(double) = exp;
static float (*volatile fexp)(float) = expf;
static float (*volatile fsin)(float) = sinf;
static void fill(void) {
    for (int k = 0; k < 64; k++) {
        dt[k] = 0.1 + 99.9 * (double)((k * 37 + 11) & 63) / 63.0;
        ft[k] = (float)dt[k] * 0.04f;
    }
}
#define LOOPD(fp) { double (*f)(double) = fp; double acc = 0; for (long i = 0; i < n; i++) acc += f(dt[i & 63]); pc_sink += (long)acc; }
#define LOOPF(fp) { float (*f)(float) = fp; float acc = 0; for (long i = 0; i < n; i++) acc += f(ft[i & 63]); pc_sink += (long)acc; }

CASE(env_sqrt, 1000000, 1) { fill(); LOOPD(dsq); PC_CHECKING { pc_check("%.17g", dsq(2.0)); } }
CASE(env_sqrtf, 1000000, 1) { fill(); LOOPF(fsq); PC_CHECKING { pc_check("%.9g", (double)fsq(2.0f)); } }
CASE(env_exp, 1000000, 1) { fill(); for (int k = 0; k < 64; k++) dt[k] *= 0.1; LOOPD(dexp); PC_CHECKING { pc_check("%.17g", dexp(1.5)); } }
CASE(env_expf, 1000000, 1) { fill(); LOOPF(fexp); PC_CHECKING { pc_check("%.9g", (double)fexp(1.5f)); } }
CASE(env_sinf, 1000000, 1) { fill(); LOOPF(fsin); PC_CHECKING { pc_check("%.9g", (double)fsin(1.25f)); } }
