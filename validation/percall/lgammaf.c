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
/* lgammaf(-2.5) = ln(8 sqrt(pi) / 15) = -0.0562437164976740506725945... (exact; LIBM-INVALID-CONVERT-SCAN-4).
 * Rounded to nearest (and down) it is -0x1.ccbfap-5; rounded up, -0x1.ccbf9ep-5. The reference returns the second
 * (faithful, not correctly rounded), Liella the first (pinned in crates/math/tests/s10_math_clauses.rs). Only at
 * this operand the line reads "faithful" for either neighbour of the exact value; any other result prints its
 * value, so a wrong answer on either side still reads MISMATCH. */
#define LG_M25_DN (-0x1.ccbfap-5f)
#define LG_M25_UP (-0x1.ccbf9ep-5f)
#define CHK_M25 PC_CHECKING { volatile float v = -2.5f; float r = lgammaf(v); \
    if (r == LG_M25_DN || r == LG_M25_UP) pc_check("lgammaf(-2.5) faithful to -0.0562437164976740507"); \
    else pc_check("%.9g", (double)r); }
CASE(lgammaf_pos, 1000000, 1) { fill(0.3f, 19.7f); LOOP1(lgammaf(x)); CHK1(lgammaf, 2.5f); }
CASE(lgammaf_neg, 1000000, 1) { fill(-19.7f, -0.3f); LOOP1(lgammaf(x)); CHK_M25; }
CASE(lgammaf_r_neg, 1000000, 1) { fill(-19.7f, -0.3f); LOOP1(lg_r(x)); CHK1(lg_r, -3.5f); }
