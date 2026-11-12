/* wl_percall Bessel band set (LIBM-RETHINK-BESSEL, D-10.106; no spec clause): j0/j1/y0/y1 and the float forms, one
 * case per input band of the LIBM-TAIL-PROBE ranking: tiny (|x| < 2^-20 float, < 2^-27 double), the typical range,
 * large (|x| >= 2^19 float, >= 32 double) and the double zeros. Every call goes through a volatile function pointer.
 * Each case cycles over 64 inputs spread over its band (geometric for tiny and large). Link: -lm.
 * `wl_percall.sh --spec validation/percall/libm_bessel.c .`; one call: `wl_trace.sh --spec ... . bes_j0f_large j0f`. */
#include "percall.h"
#include <math.h>

static double dt[64];
static float ft[64];
static double (*volatile dj0)(double) = j0;
static double (*volatile dj1)(double) = j1;
static double (*volatile dy0)(double) = y0;
static double (*volatile dy1)(double) = y1;
static float (*volatile fj0)(float) = j0f;
static float (*volatile fj1)(float) = j1f;
static float (*volatile fy0)(float) = y0f;
static float (*volatile fy1)(float) = y1f;

/* float bands: tiny 2^-21 .. 2^-147 (geometric), mid [0.25, 30], large 2^19 .. 2^127 (geometric) */
static void ftiny(void) { for (int k = 0; k < 64; k++) ft[k] = ldexpf(1.0f + (float)((k * 37) & 63) / 64.0f, -21 - 2 * k); }
static void fmid(void) { for (int k = 0; k < 64; k++) ft[k] = 0.25f + 29.75f * (float)((k * 37 + 11) & 63) / 63.0f; }
static void flarge(void) { for (int k = 0; k < 64; k++) ft[k] = ldexpf(1.0f + (float)((k * 37) & 63) / 64.0f, 19 + (k * 13) % 109); }
/* double bands: tiny 2^-28 .. 2^-1070, mid [0.25, 30], large 32 .. 2^1000, and +-k ulps around zeros */
static void dtiny(void) { for (int k = 0; k < 64; k++) dt[k] = ldexp(1.0 + (double)((k * 37) & 63) / 64.0, -28 - 16 * k); }
static void dmid(void) { for (int k = 0; k < 64; k++) dt[k] = 0.25 + 29.75 * (double)((k * 37 + 11) & 63) / 63.0; }
static void dlarge(void) { for (int k = 0; k < 64; k++) dt[k] = ldexp(1.0 + (double)((k * 37) & 63) / 64.0, 5 + (k * 13) % 997); }
static void dzero(double z) {
    for (int k = 0; k < 64; k++) {
        double v = z;
        for (int j = 0; j < (k & 31) + 1; j++) v = nextafter(v, k & 32 ? 0.0 : 100.0);
        dt[k] = v;
    }
}
#define LOOPD(fp) { double (*f)(double) = fp; double acc = 0; for (long i = 0; i < n; i++) acc += f(dt[i & 63]); pc_sink += (long)(acc * 1e3); }
#define LOOPF(fp) { float (*f)(float) = fp; double acc = 0; for (long i = 0; i < n; i++) acc += f(ft[i & 63]); pc_sink += (long)(acc * 1e3); }

CASE(bes_j0f_tiny, 1000000, 1) { ftiny(); LOOPF(fj0); PC_CHECKING { pc_check("%.9g", (double)fj0(1e-10f)); } }
CASE(bes_j1f_tiny, 1000000, 1) { ftiny(); LOOPF(fj1); PC_CHECKING { pc_check("%.9g", (double)fj1(1e-10f)); } }
CASE(bes_y0f_tiny, 1000000, 1) { ftiny(); LOOPF(fy0); PC_CHECKING { pc_check("%.9g", (double)fy0(1e-10f)); } }
CASE(bes_y1f_tiny, 1000000, 1) { ftiny(); LOOPF(fy1); PC_CHECKING { pc_check("%.9g", (double)fy1(1e-10f)); } }
CASE(bes_j0f_mid, 1000000, 1) { fmid(); LOOPF(fj0); PC_CHECKING { pc_check("%.9g", (double)fj0(1.5f)); } }
CASE(bes_y1f_mid, 1000000, 1) { fmid(); LOOPF(fy1); PC_CHECKING { pc_check("%.9g", (double)fy1(1.5f)); } }
CASE(bes_j0f_large, 1000000, 1) { flarge(); LOOPF(fj0); PC_CHECKING { pc_check("%.9g", (double)fj0(1e6f)); } }
CASE(bes_j1f_large, 1000000, 1) { flarge(); LOOPF(fj1); PC_CHECKING { pc_check("%.9g", (double)fj1(1e6f)); } }
CASE(bes_y0f_large, 1000000, 1) { flarge(); LOOPF(fy0); PC_CHECKING { pc_check("%.9g", (double)fy0(1e6f)); } }
CASE(bes_y1f_large, 1000000, 1) { flarge(); LOOPF(fy1); PC_CHECKING { pc_check("%.9g", (double)fy1(1e6f)); } }
CASE(bes_j0_tiny, 1000000, 1) { dtiny(); LOOPD(dj0); PC_CHECKING { pc_check("%.17g", dj0(1e-10)); } }
CASE(bes_j1_tiny, 1000000, 1) { dtiny(); LOOPD(dj1); PC_CHECKING { pc_check("%.17g", dj1(1e-10)); } }
CASE(bes_y0_tiny, 1000000, 1) { dtiny(); LOOPD(dy0); PC_CHECKING { pc_check("%.17g", dy0(1e-10)); } }
CASE(bes_y1_tiny, 1000000, 1) { dtiny(); LOOPD(dy1); PC_CHECKING { pc_check("%.17g", dy1(1e-10)); } }
CASE(bes_j0_mid, 1000000, 1) { dmid(); LOOPD(dj0); PC_CHECKING { pc_check("%.17g", dj0(1.5)); } }
CASE(bes_j0_large, 1000000, 1) { dlarge(); LOOPD(dj0); PC_CHECKING { pc_check("%.17g", dj0(1e6)); } }
CASE(bes_y1_large, 1000000, 1) { dlarge(); LOOPD(dy1); PC_CHECKING { pc_check("%.17g", dy1(1e6)); } }
CASE(bes_j0_zero, 100000, 1) { dzero(11.791534439014281); LOOPD(dj0); PC_CHECKING { pc_check("%.17g", dj0(2.0)); } }
CASE(bes_y1_zero, 100000, 1) { dzero(8.5960058683311689); LOOPD(dy1); PC_CHECKING { pc_check("%.17g", dy1(2.0)); } }
