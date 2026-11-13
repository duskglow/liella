/* wl_percall float inverse trigonometric set (LIBM-F32-REST, D-10.82; no spec clause): asinf/acosf in [-0.99, 0.99],
 * atanf in [-10, 10], atan2f with both operands in [-10, 10]. Inputs cycle over 64 values read through a volatile
 * pointer so gcc cannot fold or hoist the call (`wl_percall.sh --spec validation/percall/math_f32_invtrig.c`). Link: -lm. */
#include "percall.h"
#include <math.h>

static float tab[64], tab2[64];
static void fill(float lo, float hi, float lo2, float hi2) {
    for (int k = 0; k < 64; k++) {
        tab[k] = lo + (hi - lo) * (float)((k * 37 + 11) & 63) / 63.0f;
        tab2[k] = lo2 + (hi2 - lo2) * (float)((k * 23 + 5) & 63) / 63.0f;
    }
}
static float *volatile ptab = tab, *volatile ptab2 = tab2;
#define LOOP1(expr) { float *t = ptab; float acc = 0; for (long i = 0; i < n; i++) { float x = t[i & 63]; acc += (expr); } pc_sink += (long)acc; }
#define LOOP2(expr) { float *t = ptab, *u = ptab2; float acc = 0; for (long i = 0; i < n; i++) { float x = t[i & 63], y = u[i & 63]; acc += (expr); } pc_sink += (long)acc; }
#define CHK1(f, a) PC_CHECKING { volatile float v = (a); pc_check("%.9g", (double)f(v)); }

CASE(asinf_typ, 1000000, 1) { fill(-0.99f, 0.99f, 0, 0); LOOP1(asinf(x)); CHK1(asinf, 0.375f); }
CASE(acosf_typ, 1000000, 1) { fill(-0.99f, 0.99f, 0, 0); LOOP1(acosf(x)); CHK1(acosf, -0.375f); }
CASE(atanf_typ, 1000000, 1) { fill(-10, 10, 0, 0); LOOP1(atanf(x)); CHK1(atanf, 2.5f); }
CASE(atan2f_typ, 1000000, 1) { fill(-10, 10, -10, 10); LOOP2(atan2f(x, y)); PC_CHECKING { volatile float a = 3, b = -4; pc_check("%.9g", (double)atan2f(a, b)); } }
