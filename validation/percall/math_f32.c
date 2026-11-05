/* wl_percall float math set (MATH-M6-F32-BENCH; no spec clause): every fast32 row, in a typical and, for the trig
 * functions, a reduction-heavy argument range. Inputs cycle over 64 values read through a volatile pointer so gcc
 * cannot fold or hoist the call (`wl_percall.sh --spec validation/percall/math_f32.c`). Link: -lm.
 *   typ:   sinf/cosf/sincosf/tanf x in [-4, 4]; expf [-20, 20]; logf [0.01, 100]; log1pf [-0.5, 10];
 *          expm1f [-5, 5]; cbrtf [0.01, 1000]; hypotf both in [0.1, 10]; powf x in [0.5, 2), y in [-4, 4];
 *          powf_int: y an integer in [-4, 4].
 *   red:   sinf/cosf/sincosf/tanf |x| in [1e3, 1e6] (large n in the reduction).
 *   floor: sqrtf in [0.1, 100], the cheapest correctly rounded call, as the entry/exit reference. */
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

CASE(sinf_typ, 1000000, 1) { fill(-4, 4, 0, 0); LOOP1(sinf(x)); CHK1(sinf, 1.25f); }
CASE(cosf_typ, 1000000, 1) { fill(-4, 4, 0, 0); LOOP1(cosf(x)); CHK1(cosf, 1.25f); }
CASE(sincosf_typ, 1000000, 1) { fill(-4, 4, 0, 0); LOOP1(({ float s, c; sincosf(x, &s, &c); s + c; })); CHK1(sinf, 0.5f); }
CASE(tanf_typ, 1000000, 1) { fill(-4, 4, 0, 0); LOOP1(tanf(x)); CHK1(tanf, 1.25f); }
CASE(sinf_red, 1000000, 1) { fill(1e3f, 1e6f, 0, 0); LOOP1(sinf(x)); CHK1(sinf, 123456.f); }
CASE(cosf_red, 1000000, 1) { fill(1e3f, 1e6f, 0, 0); LOOP1(cosf(x)); CHK1(cosf, 123456.f); }
CASE(sincosf_red, 1000000, 1) { fill(1e3f, 1e6f, 0, 0); LOOP1(({ float s, c; sincosf(x, &s, &c); s + c; })); CHK1(cosf, 654321.f); }
CASE(tanf_red, 1000000, 1) { fill(1e3f, 1e6f, 0, 0); LOOP1(tanf(x)); CHK1(tanf, 123456.f); }
CASE(expf_typ, 1000000, 1) { fill(-20, 20, 0, 0); LOOP1(expf(x)); CHK1(expf, 1.5f); }
CASE(logf_typ, 1000000, 1) { fill(0.01f, 100, 0, 0); LOOP1(logf(x)); CHK1(logf, 1.5f); }
CASE(log1pf_typ, 1000000, 1) { fill(-0.5f, 10, 0, 0); LOOP1(log1pf(x)); CHK1(log1pf, 1.5f); }
CASE(expm1f_typ, 1000000, 1) { fill(-5, 5, 0, 0); LOOP1(expm1f(x)); CHK1(expm1f, 1.5f); }
CASE(cbrtf_typ, 1000000, 1) { fill(0.01f, 1000, 0, 0); LOOP1(cbrtf(x)); CHK1(cbrtf, 10.f); }
CASE(hypotf_typ, 1000000, 1) { fill(0.1f, 10, 0.1f, 10); LOOP2(hypotf(x, y)); PC_CHECKING { volatile float a = 3, b = 4; pc_check("%.9g", (double)hypotf(a, b)); } }
CASE(powf_typ, 1000000, 1) { fill(0.5f, 2, -4, 4); LOOP2(powf(x, y)); PC_CHECKING { volatile float a = 1.5f, b = 2.5f; pc_check("%.9g", (double)powf(a, b)); } }
CASE(powf_int, 1000000, 1) {
    fill(0.5f, 2, 0, 0);
    for (int k = 0; k < 64; k++) tab2[k] = (float)((k % 9) - 4);
    LOOP2(powf(x, y));
    PC_CHECKING { volatile float a = 1.5f, b = 3; pc_check("%.9g", (double)powf(a, b)); }
}
CASE(sqrtf_floor, 1000000, 1) { fill(0.1f, 100, 0, 0); LOOP1(sqrtf(x)); CHK1(sqrtf, 2.f); }
