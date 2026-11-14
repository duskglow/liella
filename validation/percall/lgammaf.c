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
/* zero: the 64 encodings from 32 below the cbench_libm row's x = -3.14382577 (bits 0xc0493471, beside lnΓ's zero
 * near -3.1435, where |lgammaf| < 2^-8) up (LIBM-F32-REST-NEXT): the path such operands take. */
static void fill_bits(unsigned b) {
    for (int k = 0; k < 64; k++) { union { unsigned u; float f; } c = { b + (unsigned)((k * 37 + 11) & 63) }; tab[k] = c.f; }
}
CASE(lgammaf_zero, 100000, 1) { fill_bits(0xc0493471u - 32u); LOOP1(lgammaf(x)); CHK1(lgammaf, -3.14382577f); }
CASE(lgammaf_pos, 1000000, 1) { fill(0.3f, 19.7f); LOOP1(lgammaf(x)); CHK1(lgammaf, 2.5f); }
CASE(lgammaf_neg, 1000000, 1) { fill(-19.7f, -0.3f); LOOP1(lgammaf(x)); CHK_M25; }
CASE(lgammaf_r_neg, 1000000, 1) { fill(-19.7f, -0.3f); LOOP1(lg_r(x)); CHK1(lg_r, -3.5f); }
/* LIBM-LGAMMAF-EDGE-2 (no spec clause):
 *   big: x = -42.5 - 15j, j < 64 (half-integers in [-987.5, -42.5]: the reflection below -42);
 *   tab: the 18 operands of the exception table (crates/math/src/fast32/lgamma_tab.rs), cycled;
 *   retry: 49 operands in [-2.6, -2.46] with 2^-8 <= |y| < 1 that the first rounding test refuses (the second try;
 *   printed by lgammaf_small_refusals_take_the_second_try), cycled. */
static const unsigned lg_tab_bits[18] = {
    0xc236ca27u, 0xc2683537u, 0xc298aeecu, 0xc33139a3u, 0xc33807b8u, 0xc36c073cu, 0xc4af5953u, 0xc6460e10u,
    0xc6975a8cu, 0xc6e4d0dau, 0xc6f7e151u, 0xc6fbee85u, 0xc7567ae3u, 0xc76bc3c3u, 0xc797bfc6u, 0xc8691f7fu,
    0xc9aeb5a5u, 0xca46d773u,
};
static const unsigned lg_retry_bits[49] = {
    0xc01d7c17u, 0xc01d96f3u, 0xc01de2fdu, 0xc01de8b1u, 0xc01e0893u, 0xc01e63a3u, 0xc01f0155u, 0xc01f0799u,
    0xc01f15eeu, 0xc01f2a19u, 0xc01f2d7fu, 0xc01f961bu, 0xc01f9f9bu, 0xc01fa43eu, 0xc01fdcf8u, 0xc0205e13u,
    0xc020843au, 0xc020a185u, 0xc020a23eu, 0xc020cddcu, 0xc020de02u, 0xc0213014u, 0xc021949eu, 0xc021b07du,
    0xc02236afu, 0xc0224f19u, 0xc0229ccbu, 0xc022b176u, 0xc022e5d8u, 0xc02304c5u, 0xc0231569u, 0xc0234953u,
    0xc023889cu, 0xc0238919u, 0xc0239216u, 0xc023a7d3u, 0xc023bfd7u, 0xc0242d3fu, 0xc0243144u, 0xc0245644u,
    0xc0248452u, 0xc024958fu, 0xc02503bfu, 0xc02510c8u, 0xc0255076u, 0xc0257f01u, 0xc0258edfu, 0xc025c3f3u,
    0xc0260cc2u,
};
static void fill_list(const unsigned *b, int len) {
    for (int k = 0; k < 64; k++) { union { unsigned u; float f; } c = { b[((k * 37 + 11) & 63) % len] }; tab[k] = c.f; }
}
CASE(lgammaf_big, 1000000, 1) { for (int k = 0; k < 64; k++) tab[k] = -42.5f - 15.0f * (float)((k * 37 + 11) & 63); LOOP1(lgammaf(x)); CHK1(lgammaf, -100.5f); }
CASE(lgammaf_tab, 1000000, 1) { fill_list(lg_tab_bits, 18); LOOP1(lgammaf(x)); CHK1(lgammaf, -45.697414f); }
CASE(lgammaf_retry, 100000, 1) { fill_list(lg_retry_bits, 49); LOOP1(lgammaf(x)); CHK1(lgammaf, -2.46069884f); }
