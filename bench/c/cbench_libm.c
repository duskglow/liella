/* libm speed (Track M milestone 6, story M6-0): ns per call of every libm family in every format, and
 * the envelope probes (the cost of an exceptional operand, of a directed rounding mode). The same file
 * builds against the products (`cargo xtask bench`, linked with Liella's libm.a) and against the host C
 * library (`cc -O2 -fno-builtin bench/c/cbench_libm.c -lm`; `cargo xtask bench --compare cbench_libm`
 * adds glibc static as the third column). Prints `name<TAB>ns/call` lines, the best of 7 rounds.
 *
 * Method (the MATH-M6-PLAN harness): throughput, not latency. N independent operands drawn uniformly
 * from the row's range by a fixed xorshift sequence (the same on both sides), the results summed into
 * a volatile sink. Each row doubles its repeat count until one pass takes >= 1 ms, then times 7 passes
 * and prints the best. Rounding to nearest unless the row name ends in `_upward`.
 * Ranges: exp family, trig, atan, hyperbolic, tgamma/lgamma [-10, 10]; logs [0.01, 100]; log1p [-0.9, 10];
 * asin/acos [-1, 1]; atanh [-0.99, 0.99]; acosh [1, 100]; erf/erfc [-5, 5]; j [-20, 20]; y [0.01, 20];
 * jn/yn order 3; pow x [0.01, 10], y [-10, 10]; hypot [-100, 100]^2; atan2 [-10, 10]^2; cbrt [-100, 100];
 * sqrt [0, 100]; complex z in [-3, 3]^2, cpow exponent 1.5+0.25i. `csinl` is not exported yet (M5-3).
 * The rows `add_baseline`, `mxcsr_pair` and `mxcsr_wrapper_seq` call no libm function (a loop floor,
 * one MXCSR save/load, the MXCSR sequence of Liella's entry wrapper); they print for `--compare` and
 * have no budget row. The GLIBC_2.2.5 compat entries are not measured here: Liella does not export them. */
#define _GNU_SOURCE
#include <complex.h>
#include <fenv.h>
#include <math.h>
#include <time.h>
#include <unistd.h>
#include "cbench_counters.h"

static double now(void)
{
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (double)ts.tv_sec * 1e9 + (double)ts.tv_nsec;
}

static void put(const char *name, double ns)
{
    char buf[64];
    int n = 0;
    while (name[n] && n < 40) {
        buf[n] = name[n];
        n++;
    }
    buf[n++] = '\t';
    unsigned long v = (unsigned long)(ns * 100.0 + 0.5);
    char digits[24];
    int d = 0;
    do {
        digits[d++] = (char)('0' + v % 10);
        v /= 10;
    } while (v || d < 3);
    while (d > 2)
        buf[n++] = digits[--d];
    buf[n++] = '.';
    buf[n++] = digits[1];
    buf[n++] = digits[0];
    buf[n++] = '\n';
    (void)!write(1, buf, (size_t)n);
}

#include "cbench_rows.h"

#define N 256
static float af[N], bf[N];
static double ad[N], bd[N];
static long double al[N], bl[N];
static _Float128 aq[N], bq[N];
static volatile float sf;
static volatile double sd;
static volatile long double sl;
static volatile _Float128 sq;
static volatile unsigned si;

static unsigned long long rng;

static double urand(void)
{
    rng ^= rng << 13;
    rng ^= rng >> 7;
    rng ^= rng << 17;
    return (double)(rng >> 11) / 9007199254740992.0;
}

/* The same operands on every side and in every row (the sequence restarts per row). */
static void fill(double lo, double hi, double lo2, double hi2)
{
    rng = 88172645463325252ULL;
    for (int i = 0; i < N; i++) {
        double x = lo + (hi - lo) * urand(), y = lo2 + (hi2 - lo2) * urand();
        af[i] = (float)x;
        ad[i] = x;
        al[i] = x;
        aq[i] = x;
        bf[i] = (float)y;
        bd[i] = y;
        bl[i] = y;
        bq[i] = y;
    }
}

/* `body` runs `reps` passes over the N operands. */
#define TIMED(name, body)                                                \
    do {                                                                 \
        long reps = 1;                                                   \
        for (;;) {                                                       \
            double t0 = now();                                           \
            body;                                                        \
            if (now() - t0 >= 1e6 || reps >= (1L << 24))                 \
                break;                                                   \
            reps *= 2;                                                   \
        }                                                                \
        double best = 1e300;                                             \
        ctr_begin();                                                     \
        for (int round = 0; round < 7; round++) {                        \
            double t0 = now();                                           \
            body;                                                        \
            double per = (now() - t0) / ((double)reps * N);              \
            if (per < best)                                              \
                best = per;                                              \
        }                                                                \
        ctr_end(name, 7.0 * (double)reps * N);                           \
        put(name, best);                                                 \
    } while (0)

/* One-argument function `f` of type `T` over `a` in [lo, hi], in rounding mode `mode`. */
#define RUM(name, f, T, a, sink, lo, hi, mode)                                    \
    do {                                                                          \
        if (cb_skip(name))                                                        \
            break;                                                                \
        fill(lo, hi, 0, 0);                                                       \
        fesetround(mode);                                                         \
        TIMED(name, { T s_ = 0; for (long r_ = 0; r_ < reps; r_++) for (int i_ = 0; i_ < N; i_++) s_ += f(a[i_]); sink = s_; }); \
        fesetround(FE_TONEAREST);                                                 \
    } while (0)
#define RU(name, f, T, a, sink, lo, hi) RUM(name, f, T, a, sink, lo, hi, FE_TONEAREST)

/* Two-argument function over [lo, hi] x [lo2, hi2]. */
#define RB(name, f, T, a, b, sink, lo, hi, lo2, hi2, mode)                         \
    do {                                                                          \
        if (cb_skip(name))                                                        \
            break;                                                                \
        fill(lo, hi, lo2, hi2);                                                   \
        fesetround(mode);                                                         \
        TIMED(name, { T s_ = 0; for (long r_ = 0; r_ < reps; r_++) for (int i_ = 0; i_ < N; i_++) s_ += f(a[i_], b[i_]); sink = s_; }); \
        fesetround(FE_TONEAREST);                                                 \
    } while (0)

/* jn/yn of order 3. */
#define RN(name, f, T, a, sink, lo, hi)                                           \
    do {                                                                          \
        if (cb_skip(name))                                                        \
            break;                                                                \
        fill(lo, hi, 0, 0);                                                       \
        TIMED(name, { T s_ = 0; for (long r_ = 0; r_ < reps; r_++) for (int i_ = 0; i_ < N; i_++) s_ += f(3, a[i_]); sink = s_; }); \
    } while (0)

/* Complex function of z = a + b i over [-3, 3]^2. */
#define RC(name, f, T, CT, a, b, sink)                                            \
    do {                                                                          \
        if (cb_skip(name))                                                        \
            break;                                                                \
        fill(-3, 3, -3, 3);                                                       \
        TIMED(name, { T s_ = 0; for (long r_ = 0; r_ < reps; r_++) for (int i_ = 0; i_ < N; i_++) { CT z_ = f(a[i_] + b[i_] * (CT)I); s_ += __real__ z_ + __imag__ z_; } sink = s_; }); \
    } while (0)

/* cpow(z, 1.5 + 0.25i). */
#define RCP(name, f, T, CT, a, b, sink)                                           \
    do {                                                                          \
        if (cb_skip(name))                                                        \
            break;                                                                \
        fill(-3, 3, -3, 3);                                                       \
        TIMED(name, { T s_ = 0; for (long r_ = 0; r_ < reps; r_++) for (int i_ = 0; i_ < N; i_++) { CT z_ = f(a[i_] + b[i_] * (CT)I, (CT)1.5 + (CT)0.25 * I); s_ += __real__ z_ + __imag__ z_; } sink = s_; }); \
    } while (0)

/* The floors: no libm call. */
static void floors(void)
{
    if (!cb_skip("add_baseline")) {
        fill(-10, 10, 0, 0);
        TIMED("add_baseline", { double s_ = 0; for (long r_ = 0; r_ < reps; r_++) for (int i_ = 0; i_ < N; i_++) s_ += ad[i_] * 1.0000001; sd = s_; });
    }
    if (!cb_skip("mxcsr_pair"))
        TIMED("mxcsr_pair", { unsigned m_ = 0; for (long r_ = 0; r_ < reps; r_++) for (int i_ = 0; i_ < N; i_++) __asm__ volatile("stmxcsr %0\n\tldmxcsr %0" : "+m"(m_)::"memory"); si = m_; });
    /* Liella's entry wrapper: read the mode; save and load the default; restore; raise inexact. */
    if (!cb_skip("mxcsr_wrapper_seq"))
        TIMED("mxcsr_wrapper_seq", { unsigned m_ = 0; unsigned k_ = 0; for (long r_ = 0; r_ < reps; r_++) for (int i_ = 0; i_ < N; i_++) {
            __asm__ volatile("stmxcsr %0" : "=m"(m_)::"memory");
            __asm__ volatile("stmxcsr %0\n\tmovl $0x1f80, %1\n\tldmxcsr %1" : "=m"(m_), "=m"(k_)::"memory");
            __asm__ volatile("ldmxcsr %0" ::"m"(m_) : "memory");
            __asm__ volatile("stmxcsr %0\n\torl $0x20, %0\n\tldmxcsr %0" : "+m"(m_)::"memory");
        } si = m_; });
}

int main(void)
{
    floors();
    /* The envelope probes. */
    RU("exp_qnan", exp, double, ad, sd, NAN, NAN);
    RU("expf_qnan", expf, float, af, sf, NAN, NAN);
    RU("exp_tiny", exp, double, ad, sd, 1e-300, 2e-300);
    RU("exp_small", exp, double, ad, sd, -0.5, 0.5);
    RU("exp_overflow_errno", exp, double, ad, sd, 1000.0, 1001.0);
    RU("sin_huge", sin, double, ad, sd, 1e6, 1e300);
    RUM("exp_upward", exp, double, ad, sd, -10.0, 10.0, FE_UPWARD);
    RUM("log_upward", log, double, ad, sd, 0.01, 100.0, FE_UPWARD);
    RUM("sin_upward", sin, double, ad, sd, -10.0, 10.0, FE_UPWARD);
    RUM("expf_upward", expf, float, af, sf, -10.0, 10.0, FE_UPWARD);
    RUM("sinf_upward", sinf, float, af, sf, -10.0, 10.0, FE_UPWARD);
    RB("pow_upward", pow, double, ad, bd, sd, 0.01, 10.0, -10.0, 10.0, FE_UPWARD);
    /* Every family in float, double, long double and _Float128. */
    RU("expf", expf, float, af, sf, -10.0, 10.0);
    RU("exp", exp, double, ad, sd, -10.0, 10.0);
    RU("expl", expl, long double, al, sl, -10.0, 10.0);
    RU("expf128", expf128, _Float128, aq, sq, -10.0, 10.0);
    RU("exp2f", exp2f, float, af, sf, -10.0, 10.0);
    RU("exp2", exp2, double, ad, sd, -10.0, 10.0);
    RU("exp2l", exp2l, long double, al, sl, -10.0, 10.0);
    RU("exp2f128", exp2f128, _Float128, aq, sq, -10.0, 10.0);
    RU("exp10f", exp10f, float, af, sf, -10.0, 10.0);
    RU("exp10", exp10, double, ad, sd, -10.0, 10.0);
    RU("exp10l", exp10l, long double, al, sl, -10.0, 10.0);
    RU("exp10f128", exp10f128, _Float128, aq, sq, -10.0, 10.0);
    RU("expm1f", expm1f, float, af, sf, -10.0, 10.0);
    RU("expm1", expm1, double, ad, sd, -10.0, 10.0);
    RU("expm1l", expm1l, long double, al, sl, -10.0, 10.0);
    RU("expm1f128", expm1f128, _Float128, aq, sq, -10.0, 10.0);
    RU("logf", logf, float, af, sf, 0.01, 100.0);
    RU("log", log, double, ad, sd, 0.01, 100.0);
    RU("logl", logl, long double, al, sl, 0.01, 100.0);
    RU("logf128", logf128, _Float128, aq, sq, 0.01, 100.0);
    RU("log2f", log2f, float, af, sf, 0.01, 100.0);
    RU("log2", log2, double, ad, sd, 0.01, 100.0);
    RU("log2l", log2l, long double, al, sl, 0.01, 100.0);
    RU("log2f128", log2f128, _Float128, aq, sq, 0.01, 100.0);
    RU("log10f", log10f, float, af, sf, 0.01, 100.0);
    RU("log10", log10, double, ad, sd, 0.01, 100.0);
    RU("log10l", log10l, long double, al, sl, 0.01, 100.0);
    RU("log10f128", log10f128, _Float128, aq, sq, 0.01, 100.0);
    RU("log1pf", log1pf, float, af, sf, -0.9, 10.0);
    RU("log1p", log1p, double, ad, sd, -0.9, 10.0);
    RU("log1pl", log1pl, long double, al, sl, -0.9, 10.0);
    RU("log1pf128", log1pf128, _Float128, aq, sq, -0.9, 10.0);
    RU("cbrtf", cbrtf, float, af, sf, -100.0, 100.0);
    RU("cbrt", cbrt, double, ad, sd, -100.0, 100.0);
    RU("cbrtl", cbrtl, long double, al, sl, -100.0, 100.0);
    RU("cbrtf128", cbrtf128, _Float128, aq, sq, -100.0, 100.0);
    RU("sqrtf", sqrtf, float, af, sf, 0.0, 100.0);
    RU("sqrt", sqrt, double, ad, sd, 0.0, 100.0);
    RU("sqrtl", sqrtl, long double, al, sl, 0.0, 100.0);
    RU("sqrtf128", sqrtf128, _Float128, aq, sq, 0.0, 100.0);
    RU("sinf", sinf, float, af, sf, -10.0, 10.0);
    RU("sin", sin, double, ad, sd, -10.0, 10.0);
    RU("sinl", sinl, long double, al, sl, -10.0, 10.0);
    RU("sinf128", sinf128, _Float128, aq, sq, -10.0, 10.0);
    RU("cosf", cosf, float, af, sf, -10.0, 10.0);
    RU("cos", cos, double, ad, sd, -10.0, 10.0);
    RU("cosl", cosl, long double, al, sl, -10.0, 10.0);
    RU("cosf128", cosf128, _Float128, aq, sq, -10.0, 10.0);
    RU("tanf", tanf, float, af, sf, -10.0, 10.0);
    RU("tan", tan, double, ad, sd, -10.0, 10.0);
    RU("tanl", tanl, long double, al, sl, -10.0, 10.0);
    RU("tanf128", tanf128, _Float128, aq, sq, -10.0, 10.0);
    RU("asinf", asinf, float, af, sf, -1.0, 1.0);
    RU("asin", asin, double, ad, sd, -1.0, 1.0);
    RU("asinl", asinl, long double, al, sl, -1.0, 1.0);
    RU("asinf128", asinf128, _Float128, aq, sq, -1.0, 1.0);
    RU("acosf", acosf, float, af, sf, -1.0, 1.0);
    RU("acos", acos, double, ad, sd, -1.0, 1.0);
    RU("acosl", acosl, long double, al, sl, -1.0, 1.0);
    RU("acosf128", acosf128, _Float128, aq, sq, -1.0, 1.0);
    RU("atanf", atanf, float, af, sf, -10.0, 10.0);
    RU("atan", atan, double, ad, sd, -10.0, 10.0);
    RU("atanl", atanl, long double, al, sl, -10.0, 10.0);
    RU("atanf128", atanf128, _Float128, aq, sq, -10.0, 10.0);
    RU("sinhf", sinhf, float, af, sf, -10.0, 10.0);
    RU("sinh", sinh, double, ad, sd, -10.0, 10.0);
    RU("sinhl", sinhl, long double, al, sl, -10.0, 10.0);
    RU("sinhf128", sinhf128, _Float128, aq, sq, -10.0, 10.0);
    RU("coshf", coshf, float, af, sf, -10.0, 10.0);
    RU("cosh", cosh, double, ad, sd, -10.0, 10.0);
    RU("coshl", coshl, long double, al, sl, -10.0, 10.0);
    RU("coshf128", coshf128, _Float128, aq, sq, -10.0, 10.0);
    RU("tanhf", tanhf, float, af, sf, -10.0, 10.0);
    RU("tanh", tanh, double, ad, sd, -10.0, 10.0);
    RU("tanhl", tanhl, long double, al, sl, -10.0, 10.0);
    RU("tanhf128", tanhf128, _Float128, aq, sq, -10.0, 10.0);
    RU("asinhf", asinhf, float, af, sf, -10.0, 10.0);
    RU("asinh", asinh, double, ad, sd, -10.0, 10.0);
    RU("asinhl", asinhl, long double, al, sl, -10.0, 10.0);
    RU("asinhf128", asinhf128, _Float128, aq, sq, -10.0, 10.0);
    RU("acoshf", acoshf, float, af, sf, 1.0, 100.0);
    RU("acosh", acosh, double, ad, sd, 1.0, 100.0);
    RU("acoshl", acoshl, long double, al, sl, 1.0, 100.0);
    RU("acoshf128", acoshf128, _Float128, aq, sq, 1.0, 100.0);
    RU("atanhf", atanhf, float, af, sf, -0.99, 0.99);
    RU("atanh", atanh, double, ad, sd, -0.99, 0.99);
    RU("atanhl", atanhl, long double, al, sl, -0.99, 0.99);
    RU("atanhf128", atanhf128, _Float128, aq, sq, -0.99, 0.99);
    RU("erff", erff, float, af, sf, -5.0, 5.0);
    RU("erf", erf, double, ad, sd, -5.0, 5.0);
    RU("erfl", erfl, long double, al, sl, -5.0, 5.0);
    RU("erff128", erff128, _Float128, aq, sq, -5.0, 5.0);
    RU("erfcf", erfcf, float, af, sf, -5.0, 5.0);
    RU("erfc", erfc, double, ad, sd, -5.0, 5.0);
    RU("erfcl", erfcl, long double, al, sl, -5.0, 5.0);
    RU("erfcf128", erfcf128, _Float128, aq, sq, -5.0, 5.0);
    RU("tgammaf", tgammaf, float, af, sf, -10.0, 10.0);
    RU("tgamma", tgamma, double, ad, sd, -10.0, 10.0);
    RU("tgammal", tgammal, long double, al, sl, -10.0, 10.0);
    RU("tgammaf128", tgammaf128, _Float128, aq, sq, -10.0, 10.0);
    RU("lgammaf", lgammaf, float, af, sf, -10.0, 10.0);
    RU("lgamma", lgamma, double, ad, sd, -10.0, 10.0);
    RU("lgammal", lgammal, long double, al, sl, -10.0, 10.0);
    RU("lgammaf128", lgammaf128, _Float128, aq, sq, -10.0, 10.0);
    RU("j0f", j0f, float, af, sf, -20.0, 20.0);
    RU("j0", j0, double, ad, sd, -20.0, 20.0);
    RU("j0l", j0l, long double, al, sl, -20.0, 20.0);
    RU("j0f128", j0f128, _Float128, aq, sq, -20.0, 20.0);
    RU("j1f", j1f, float, af, sf, -20.0, 20.0);
    RU("j1", j1, double, ad, sd, -20.0, 20.0);
    RU("j1l", j1l, long double, al, sl, -20.0, 20.0);
    RU("j1f128", j1f128, _Float128, aq, sq, -20.0, 20.0);
    RU("y0f", y0f, float, af, sf, 0.01, 20.0);
    RU("y0", y0, double, ad, sd, 0.01, 20.0);
    RU("y0l", y0l, long double, al, sl, 0.01, 20.0);
    RU("y0f128", y0f128, _Float128, aq, sq, 0.01, 20.0);
    RU("y1f", y1f, float, af, sf, 0.01, 20.0);
    RU("y1", y1, double, ad, sd, 0.01, 20.0);
    RU("y1l", y1l, long double, al, sl, 0.01, 20.0);
    RU("y1f128", y1f128, _Float128, aq, sq, 0.01, 20.0);
    RB("powf", powf, float, af, bf, sf, 0.01, 10.0, -10.0, 10.0, FE_TONEAREST);
    RB("pow", pow, double, ad, bd, sd, 0.01, 10.0, -10.0, 10.0, FE_TONEAREST);
    RB("powl", powl, long double, al, bl, sl, 0.01, 10.0, -10.0, 10.0, FE_TONEAREST);
    RB("powf128", powf128, _Float128, aq, bq, sq, 0.01, 10.0, -10.0, 10.0, FE_TONEAREST);
    RB("hypotf", hypotf, float, af, bf, sf, -100.0, 100.0, -100.0, 100.0, FE_TONEAREST);
    RB("hypot", hypot, double, ad, bd, sd, -100.0, 100.0, -100.0, 100.0, FE_TONEAREST);
    RB("hypotl", hypotl, long double, al, bl, sl, -100.0, 100.0, -100.0, 100.0, FE_TONEAREST);
    RB("hypotf128", hypotf128, _Float128, aq, bq, sq, -100.0, 100.0, -100.0, 100.0, FE_TONEAREST);
    RB("atan2f", atan2f, float, af, bf, sf, -10.0, 10.0, -10.0, 10.0, FE_TONEAREST);
    RB("atan2", atan2, double, ad, bd, sd, -10.0, 10.0, -10.0, 10.0, FE_TONEAREST);
    RB("atan2l", atan2l, long double, al, bl, sl, -10.0, 10.0, -10.0, 10.0, FE_TONEAREST);
    RB("atan2f128", atan2f128, _Float128, aq, bq, sq, -10.0, 10.0, -10.0, 10.0, FE_TONEAREST);
    RN("jnf", jnf, float, af, sf, -20.0, 20.0);
    RN("jn", jn, double, ad, sd, -20.0, 20.0);
    RN("jnl", jnl, long double, al, sl, -20.0, 20.0);
    RN("jnf128", jnf128, _Float128, aq, sq, -20.0, 20.0);
    RN("ynf", ynf, float, af, sf, 0.01, 20.0);
    RN("yn", yn, double, ad, sd, 0.01, 20.0);
    RN("ynl", ynl, long double, al, sl, 0.01, 20.0);
    RN("ynf128", ynf128, _Float128, aq, sq, 0.01, 20.0);
    RC("cexpf", cexpf, float, float complex, af, bf, sf);
    RC("cexp", cexp, double, double complex, ad, bd, sd);
    RC("cexpl", cexpl, long double, long double complex, al, bl, sl);
    RC("clogf", clogf, float, float complex, af, bf, sf);
    RC("clog", clog, double, double complex, ad, bd, sd);
    RC("clogl", clogl, long double, long double complex, al, bl, sl);
    RC("csinf", csinf, float, float complex, af, bf, sf);
    RC("csin", csin, double, double complex, ad, bd, sd);
    RC("csqrtf", csqrtf, float, float complex, af, bf, sf);
    RC("csqrt", csqrt, double, double complex, ad, bd, sd);
    RC("csqrtl", csqrtl, long double, long double complex, al, bl, sl);
    RCP("cpowf", cpowf, float, float complex, af, bf, sf);
    RCP("cpow", cpow, double, double complex, ad, bd, sd);
    RCP("cpowl", cpowl, long double, long double complex, al, bl, sl);
    return 0;
}
