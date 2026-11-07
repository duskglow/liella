/* LIBM-RETHINK prototype (a measured experiment, never wired into the product): a correctly rounded binary64 exp
 * from a cheap quick phase and Ziv's rounding test, falling back to the library's correctly rounded exp.
 *
 * Method (sources, no library code):
 *   - Tang's table-driven reduction x = (128m + j)·ln2/128 + r, |r| <= ln2/256 (Tang, "Table-driven implementation
 *     of the exponential function in IEEE floating-point arithmetic", ACM TOMS 15(2), 1989);
 *   - two-part Cody-Waite constant ln2/128 = L1 + L2, L1 with 36 significant bits so k·L1 is exact for |k| < 2^17
 *     and x - k·L1 is exact by Sterbenz's lemma (Cody & Waite, "Software Manual for the Elementary Functions", 1980);
 *   - e^r - 1 - r by its Taylor polynomial of degree 6 (truncation < 2^-72 for |r| <= 2^-8.52);
 *   - assembly T·(1 + r + q) with Fast2Sum (Dekker, Numer. Math. 18, 1971); variant D takes T·r exactly by Dekker's
 *     product with Veltkamp's split, variant F by one FMA (2MultFMA; Handbook of Floating-Point Arithmetic, 2018);
 *   - Ziv's strategy (Ziv, ACM TOMS 17(3), 1991): round the quick result only when every value within the proven
 *     bound rounds the same way, else run the next tier (here: the library's exp).
 * Error bounds (relative to zh, derivation in handoff/libm-rethink/DESIGN.md "Prototype"): L 2^-61.50 (the rounding
 * of the plain product T·r dominates), D and F 2^-67.4. The test uses 2^-61.19 and 2^-67, whose slack covers the
 * test's own roundings. Hot path: round-to-nearest with every exception masked (one stmxcsr; FZ/DAZ cannot matter,
 * every value on the path is normal) and 2^-54 <= |x| < 512; everything else is the fallback.
 * no spec clause (prototype). */
#include <stdint.h>
#include <string.h>
#include <math.h>

static inline uint64_t pu64(double d) { uint64_t u; memcpy(&u, &d, 8); return u; }
static inline double pf64(uint64_t u) { double d; memcpy(&d, &u, 8); return d; }

static struct { double h, l; } PT[128]; /* 2^(j/128) = h + l within 2^-105 relative */
static struct { double h, l; } PS[128]; /* h: the top 26 bits of PT[j].h, l = PT[j].h - h (Dekker's split) */
static double PL1, PL2, PINV;           /* ln2/128 = PL1 + PL2 (PL1: 36 bits), 128/ln2 */
static const double PSHIFT = 0x1.8p52;
static const double C3 = 1.0 / 6, C4 = 1.0 / 24, C5 = 1.0 / 120, C6 = 1.0 / 720;
static const double EPS_L = 0x1.cp-62, EPS_X = 0x1p-67;
unsigned long proto_fallbacks;
#ifdef PROTO_CAPTURE
double cap_zh, cap_zl; long cap_m;
#define CAPTURE(zh, zl, m) (cap_zh = (zh), cap_zl = (zl), cap_m = (m))
#else
#define CAPTURE(zh, zl, m) ((void)0)
#endif

/* Tables from the library's own binary128 exp2/log (correctly rounded in Liella; 113 bits are far beyond the 2^-105
 * the tables need). A product would generate them with `cargo xtask tables` from the oracle. */
void proto_exp_init(void) {
    for (int j = 0; j < 128; j++) {
        _Float128 q = exp2f128((_Float128)j / 128);
        double h = (double)q;
        PT[j].h = h;
        PT[j].l = (double)(q - (_Float128)h);
        double s = pf64(pu64(h) & ~((1ull << 27) - 1));
        PS[j].h = s;
        PS[j].l = h - s;
    }
    _Float128 l = logf128(2) / 128;
    PL1 = pf64(pu64((double)l) & ~((1ull << 17) - 1));
    PL2 = (double)(l - (_Float128)PL1);
    PINV = (double)(128 / logf128(2));
}

#define PROTO_FALLBACK { proto_fallbacks++; return exp(x); }

/* UL: the low part of T·r (0 for L); EPS: the test's bound. */
#define PROTO_BODY(UL, EPS)                                                                  \
    uint64_t ix = pu64(x);                                                                   \
    uint32_t top = (uint32_t)(ix >> 52) & 0x7ff;                                             \
    unsigned mx;                                                                             \
    __asm__ volatile("stmxcsr %0" : "=m"(mx));                                               \
    if ((uint32_t)(top - 0x3c9) >= 0x3f || (mx & 0x7f80) != 0x1f80) PROTO_FALLBACK           \
    double z = x * PINV;                                                                     \
    double kd = (z + PSHIFT) - PSHIFT;                                                       \
    int64_t k = (int64_t)kd;                                                                 \
    double r1 = x - kd * PL1;                                                                \
    double kl2 = kd * PL2;                                                                   \
    double rh = r1 - kl2;                                                                    \
    double rl = (r1 - rh) - kl2;                                                             \
    double q = rh * rh * (0.5 + rh * (C3 + rh * (C4 + rh * (C5 + rh * C6))));               \
    int j = (int)(k & 127);                                                                  \
    int64_t m = k >> 7;                                                                      \
    double th = PT[j].h, tl = PT[j].l;                                                       \
    double u = th * rh;                                                                      \
    double yh = th + u;                                                                      \
    double e1 = (th - yh) + u;                                                               \
    double w = th * (rl + q) + tl * (1.0 + rh);                                              \
    double yl = (e1 + (UL)) + w;                                                             \
    double zh = yh + yl;                                                                     \
    double zl = (yh - zh) + yl;                                                              \
    CAPTURE(zh, zl, m);                                                                      \
    double eb = zh * (EPS);                                                                  \
    double lo = zh + (zl - eb), hi = zh + (zl + eb);                                         \
    if (lo != hi) PROTO_FALLBACK                                                             \
    return lo * pf64((uint64_t)(m + 1023) << 52);

__attribute__((noinline)) double proto_exp_L(double x) {
    PROTO_BODY(0.0, EPS_L)
}

#define DEKKER_UL                                                                            \
    ({ double c_ = rh * 134217729.0, a_ = c_ - (c_ - rh), b_ = rh - a_;                      \
       double sh_ = PS[j].h, sl_ = PS[j].l;                                                  \
       (((sh_ * a_ - u) + sh_ * b_) + sl_ * a_) + sl_ * b_; })

__attribute__((noinline)) double proto_exp_D(double x) {
    PROTO_BODY(DEKKER_UL, EPS_X)
}

__attribute__((noinline, target("fma"))) double proto_exp_F(double x) {
    PROTO_BODY(__builtin_fma(th, rh, -u), EPS_X)
}
