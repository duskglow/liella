/* erf/erfc per call at tiny and subnormal operands (LIBM-RETHINK-ERFC; no spec clause, measurement probe): the
 * LIBM-TAIL-PROBE bands, one case each, plus the typical-range rows they must not slow down.
 *   erfcf_tiny   erfcf(x), 0 < |x| < 2.4e-14 (random bit patterns, both signs; subnormals included)
 *   erff_sub     erff(x), |x| <= 1.04e-38 (subnormal x whose erf is subnormal too)
 *   erf_sub      erf(x), x subnormal (random 52-bit fractions, both signs)
 *   erf_tiny     erf(x), 2^-1022 <= |x| < 2^-28 (random bit patterns: the normal binades just above)
 *   erf_typ      erf(x) on [-3, 3] (LIBM-ERF-TINY-F64-2: the typical row the tiny band must not slow down)
 *   erff_typ / erfcf_typ   erff on [-3, 3], erfcf on [-3, 9] (the bench rows' ranges)
 * 64 operands cycled through a volatile table so no call is folded; the sum goes to pc_sink. The self-check prints
 * 4 significant digits: a last-bit difference between two correct roundings is not a different answer. */
#include "percall.h"
#include <math.h>
#include <stdint.h>
#include <string.h>

static volatile float fx[64];
static volatile double dx[64];
static uint64_t st = 0x2545f4914f6cdd1dull;

static uint64_t next(void) {
    st ^= st << 13; st ^= st >> 7; st ^= st << 17;
    return st;
}

static void fill_f32_bits(uint32_t hi) {
    for (int i = 0; i < 64; i++) {
        uint64_t r = next();
        uint32_t b = (uint32_t)(r % hi) + 1u;
        if (r >> 63) b |= 0x80000000u;
        float f; memcpy(&f, &b, 4); fx[i] = f;
    }
}

static void fill_f64_bits(uint64_t lo, uint64_t span) {
    for (int i = 0; i < 64; i++) {
        uint64_t r = next();
        uint64_t b = lo + (r >> 1) % span;
        if (r & 1) b |= 1ull << 63;
        double d; memcpy(&d, &b, 8); dx[i] = d;
    }
}

static void fill_f32_range(float lo, float hi) {
    for (int i = 0; i < 64; i++) fx[i] = lo + (hi - lo) * (float)(next() >> 40) / 16777216.0f;
}

static void fill_f64_range(double lo, double hi) {
    for (int i = 0; i < 64; i++) dx[i] = lo + (hi - lo) * (double)(next() >> 11) / 9007199254740992.0;
}

#define F32(CASE_NAME, NAME, FILL) \
CASE(CASE_NAME, 100000, 1) { \
    FILL; float acc = 0; \
    for (long i = 0; i < n; i++) acc += NAME(fx[i & 63]); \
    pc_sink += (long)acc; \
    PC_CHECKING { pc_check("%.4g %.4g", (double)NAME(fx[5]), (double)NAME(fx[17])); } \
}

#define F64(CASE_NAME, NAME, FILL) \
CASE(CASE_NAME, 100000, 1) { \
    FILL; double acc = 0; \
    for (long i = 0; i < n; i++) acc += NAME(dx[i & 63]); \
    pc_sink += acc > 0; \
    PC_CHECKING { pc_check("%.4g %.4g", NAME(dx[5]), NAME(dx[17])); } \
}

/* 2.4e-14f has the bit pattern 0x28d82edf; 1.04e-38f is 0x00713fe2. */
F32(erfcf_tiny, erfcf, fill_f32_bits(0x28d82edfu))
F32(erff_sub, erff, fill_f32_bits(0x00713fe2u))
F64(erf_sub, erf, fill_f64_bits(1, (1ull << 52) - 1))
F64(erf_tiny, erf, fill_f64_bits(1ull << 52, (994ull << 52)))
F64(erf_typ, erf, fill_f64_range(-3.0, 3.0))
F32(erff_typ, erff, fill_f32_range(-3.0f, 3.0f))
F32(erfcf_typ, erfcf, fill_f32_range(-3.0f, 9.0f))
