/* hypot (f64) per call (LIBM-RETHINK-HYPOT): three operand distributions, 64 pairs each cycled through volatile
 * tables so no call is folded; the sum goes to pc_sink.
 *   hypot_typ:  x, y uniform in [-100, 100] (the cbench_libm row's range).
 *   hypot_bits: x, y uniform over the bit patterns of finite doubles (the libm tail probe's sampling).
 *   hypot_near: x over every binade, y = x * u with u in [1/4, 4] (no exponent gap, any magnitude).
 *   hypot_near_up: hypot_near's pairs rounded upward (a directed mode; LIBM-RETHINK-HYPOT-3). */
#include "percall.h"
#include <fenv.h>
#include <math.h>
#include <stdint.h>
#include <string.h>

static volatile double tx[64], ty[64], bx[64], by[64], nx[64], ny[64];

static uint64_t hyp_rng(uint64_t *s) {
    *s ^= *s << 13; *s ^= *s >> 7; *s ^= *s << 17;
    return *s;
}

static double hyp_bits_finite(uint64_t *s) {
    for (;;) {
        uint64_t b = hyp_rng(s);
        if (((b >> 52) & 0x7ff) == 0x7ff) continue;
        double d; memcpy(&d, &b, 8);
        return d;
    }
}

static void hyp_fill(void) {
    uint64_t s = 0x9e3779b97f4a7c15ull;
    for (int i = 0; i < 64; i++) {
        tx[i] = ((double)(hyp_rng(&s) >> 11) * 0x1p-53 - 0.5) * 200.0;
        ty[i] = ((double)(hyp_rng(&s) >> 11) * 0x1p-53 - 0.5) * 200.0;
        bx[i] = hyp_bits_finite(&s);
        by[i] = hyp_bits_finite(&s);
        double x = ldexp(1.0 + (double)(hyp_rng(&s) >> 11) * 0x1p-53, (int)(hyp_rng(&s) % 2040) - 1020);
        nx[i] = x;
        ny[i] = x * ldexp(1.0 + (double)(hyp_rng(&s) >> 11) * 0x1p-53, (int)(hyp_rng(&s) % 4) - 2);
    }
}

CASE(hypot_typ, 100000, 1) {
    hyp_fill();
    double acc = 0;
    for (long i = 0; i < n; i++) acc += hypot(tx[i & 63], ty[i & 63]);
    pc_sink += (long)acc;
    PC_CHECKING { pc_check("%a %a", hypot(tx[5], ty[7]), hypot(3.0, 4.0)); }
}

CASE(hypot_bits, 100000, 1) {
    hyp_fill();
    double acc = 0;
    for (long i = 0; i < n; i++) acc += hypot(bx[i & 63], by[i & 63]);
    pc_sink += (long)(acc != 0);
    PC_CHECKING { pc_check("%a %a", hypot(bx[5], by[7]), hypot(bx[9], by[11])); }
}

CASE(hypot_near, 100000, 1) {
    hyp_fill();
    double acc = 0;
    for (long i = 0; i < n; i++) acc += hypot(nx[i & 63], ny[i & 63]);
    pc_sink += (long)(acc != 0);
    PC_CHECKING { pc_check("%a %a", hypot(nx[5], ny[7]), hypot(nx[9], ny[11])); }
}

CASE(hypot_near_up, 0, 1) {
    hyp_fill();
    double acc = 0;
    fesetround(FE_UPWARD);
    for (long i = 0; i < n; i++) acc += hypot(nx[i & 63], ny[i & 63]);
    double c1 = hypot(nx[5], ny[7]), c2 = hypot(nx[9], ny[11]);
    fesetround(FE_TONEAREST);
    pc_sink += (long)(acc != 0);
    PC_CHECKING { pc_check("%a %a", c1, c2); }
}

/* hypot_near_down, hypot_near_zero: hypot_near's pairs in the other two directed modes (LIBM-HYPOT-DIRECTED-FAST). */
CASE(hypot_near_down, 0, 1) {
    hyp_fill();
    double acc = 0;
    fesetround(FE_DOWNWARD);
    for (long i = 0; i < n; i++) acc += hypot(nx[i & 63], ny[i & 63]);
    double c1 = hypot(nx[5], ny[7]), c2 = hypot(nx[9], ny[11]);
    fesetround(FE_TONEAREST);
    pc_sink += (long)(acc != 0);
    PC_CHECKING { pc_check("%a %a", c1, c2); }
}

CASE(hypot_near_zero, 0, 1) {
    hyp_fill();
    double acc = 0;
    fesetround(FE_TOWARDZERO);
    for (long i = 0; i < n; i++) acc += hypot(nx[i & 63], ny[i & 63]);
    double c1 = hypot(nx[5], ny[7]), c2 = hypot(nx[9], ny[11]);
    fesetround(FE_TONEAREST);
    pc_sink += (long)(acc != 0);
    PC_CHECKING { pc_check("%a %a", c1, c2); }
}
