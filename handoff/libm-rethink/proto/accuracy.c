/* LIBM-RETHINK prototype accuracy check. Link against Liella (scripts/liella-cc ... -lm): Liella's exp is correctly
 * rounded in every mode (D-10.13, oracle-checked), so every prototype result must be bit-identical to it.
 *   1. n random operands per distribution (uniform in [-10, 10]; uniform bit patterns over the hot domain
 *      2^-54 <= |x| < 512; uniform in [-745.2, 709.8], which also crosses every fallback boundary), all variants,
 *      in round-to-nearest: count mismatches and fallbacks;
 *   2. the hard operands x = ±k·2^-53, -k·2^-54 (k = 1..48; e^x lies within ~2^-98 of a midpoint or a double);
 *   3. the directed modes: 10^5 operands per mode, the guard must send every call to the fallback;
 *   4. the quick phase's measured relative error against Liella's binary128 exp, which must stay below the bound
 *      the rounding test assumes (2^-61.50 for L, 2^-67.4 for D and F).
 * no spec clause (prototype). */
#define __STDC_WANT_IEC_60559_TYPES_EXT__ 1
#define PROTO_CAPTURE 1
#include <fenv.h>
#include <stdio.h>
#include <stdlib.h>
#include "proto_exp.c"

static uint64_t rs = 0x9e3779b97f4a7c15ull;
static uint64_t rnd(void) { rs ^= rs << 13; rs ^= rs >> 7; rs ^= rs << 17; return rs; }
static double unit(void) { return (double)(rnd() >> 11) * 0x1p-53; }

static double gen(int dist) {
    if (dist == 0) return -10 + 20 * unit();
    if (dist == 1) {
        uint64_t r = rnd();
        uint64_t e = 0x3c9 + (r >> 58) % 0x3f;
        return pf64(((r & 1) << 63) | (e << 52) | (rnd() & ((1ull << 52) - 1)));
    }
    return -745.2 + (709.8 + 745.2) * unit();
}

typedef double (*fn_t)(double);
static const char *names[3] = { "L", "D", "F" };

int main(int argc, char **argv) {
    long n = argc > 1 ? atol(argv[1]) : 10000000;
    proto_exp_init();
    fn_t fn[3] = { proto_exp_L, proto_exp_D, proto_exp_F };
    int nv = __builtin_cpu_supports("fma") ? 3 : 2;
    long total_bad = 0;
    for (int dist = 0; dist < 3; dist++) {
        for (int v = 0; v < nv; v++) {
            long bad = 0;
            unsigned long fb0 = proto_fallbacks;
            rs = 0x9e3779b97f4a7c15ull + (uint64_t)dist;
            for (long i = 0; i < n; i++) {
                double x = gen(dist);
                double r = fn[v](x), ref = exp(x);
                if (pu64(r) != pu64(ref)) {
                    if (bad++ < 5) printf("MISMATCH %s dist %d x=%a proto=%a exp=%a\n", names[v], dist, x, r, ref);
                }
            }
            printf("dist %d variant %s: n %ld mismatches %ld fallbacks %lu (%.4f%%)\n", dist, names[v], n, bad,
                   proto_fallbacks - fb0, 100.0 * (double)(proto_fallbacks - fb0) / (double)n);
            total_bad += bad;
        }
    }
    long hard = 0, hard_bad = 0;
    unsigned long hfb0 = proto_fallbacks;
    for (int k = 1; k <= 48; k++) {
        double d = k * 0x1p-52;
        double xs[3] = { 0.5 * d, -0.5 * d, -0.25 * d };
        for (int t = 0; t < 3; t++)
            for (int v = 0; v < nv; v++) {
                hard++;
                if (pu64(fn[v](xs[t])) != pu64(exp(xs[t]))) { hard_bad++; printf("HARD-MISMATCH %s x=%a\n", names[v], xs[t]); }
            }
    }
    printf("hard operands: %ld checks, mismatches %ld, fallbacks %lu\n", hard, hard_bad, proto_fallbacks - hfb0);
    total_bad += hard_bad;
    int modes[3] = { FE_UPWARD, FE_DOWNWARD, FE_TOWARDZERO };
    long dir_bad = 0;
    unsigned long dfb0 = proto_fallbacks;
    for (int mi = 0; mi < 3; mi++) {
        rs = 0x243f6a8885a308d3ull;
        fesetround(modes[mi]);
        for (long i = 0; i < 100000; i++) {
            double x = gen(i % 3);
            double ref = exp(x);
            for (int v = 0; v < nv; v++) if (pu64(fn[v](x)) != pu64(ref)) dir_bad++;
        }
        fesetround(FE_TONEAREST);
    }
    printf("directed modes: %ld checks, mismatches %ld, fallbacks %lu\n", 300000L * nv, dir_bad, proto_fallbacks - dfb0);
    total_bad += dir_bad;
    double bound[3] = { 0x1.6a09e667f3bcdp-62 /* 2^-61.5 */, 0x1.8406003b2ae5p-68 /* 2^-67.4 */, 0x1.8406003b2ae5p-68 };
    for (int v = 0; v < nv; v++) {
        double worst = 0, wx = 0;
        rs = 0x13198a2e03707344ull;
        long m = 0;
        for (long i = 0; i < 300000; i++) {
            double x = gen(i & 1);
            cap_m = -100000;
            fn[v](x);
            if (cap_m == -100000) continue;
            m++;
            _Float128 f = expf128((_Float128)x);
            _Float128 zq = ((_Float128)cap_zh + (_Float128)cap_zl) * (_Float128)pf64((uint64_t)(cap_m + 1023) << 52);
            _Float128 e = (zq - f) / f;
            double ed = (double)(e < 0 ? -e : e);
            if (ed > worst) { worst = ed; wx = x; }
        }
        printf("variant %s: %ld quick-phase results, max relative error %a (log2 %.3f) at x=%a, bound %a: %s\n",
               names[v], m, worst, log2(worst), wx, bound[v], worst < bound[v] ? "within" : "EXCEEDED");
        if (!(worst < bound[v])) total_bad++;
    }
    printf("RESULT: %s (%ld failures)\n", total_bad ? "FAIL" : "PASS", total_bad);
    return total_bad != 0;
}
