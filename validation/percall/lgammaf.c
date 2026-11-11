/* wl_percall lgammaf set (LIBM-INVALID-CONVERT-SCAN-3; no spec clause): lgammaf and lgammaf_r on positive and
 * negative non-integer operands, whose sign parity reads the operand as an integer (D-10.95). Inputs cycle over 64
 * values read through a volatile pointer so gcc cannot fold or hoist the call
 * (`wl_percall.sh --spec validation/percall/lgammaf.c`). Link: -lm.
 *   pos: x in [0.3, 19.7]; neg: x in [-19.7, -0.3] (the sign is read on every call). */
#include "percall.h"
#include <math.h>
float lgammaf_r(float, int *);
static float tab[64];
static void fill(float lo, float hi) {
    for (int k = 0; k < 64; k++) tab[k] = lo + (hi - lo) * (float)((k * 37 + 11) & 63) / 63.0f;
}
static float *volatile ptab = tab;
#define LOOP1(expr) { float *t = ptab; float acc = 0; for (long i = 0; i < n; i++) { float x = t[i & 63]; acc += (expr); } pc_sink += (long)acc; }
#define CHK1(f, a) PC_CHECKING { volatile float v = (a); pc_check("%.9g", (double)f(v)); }
static float lg_r(float x) { int s; float r = lgammaf_r(x, &s); return r * (float)s; }
CASE(lgammaf_pos, 1000000, 1) { fill(0.3f, 19.7f); LOOP1(lgammaf(x)); CHK1(lgammaf, 2.5f); }
CASE(lgammaf_neg, 1000000, 1) { fill(-19.7f, -0.3f); LOOP1(lgammaf(x)); CHK1(lgammaf, -2.5f); }
CASE(lgammaf_r_neg, 1000000, 1) { fill(-19.7f, -0.3f); LOOP1(lg_r(x)); CHK1(lg_r, -3.5f); }
