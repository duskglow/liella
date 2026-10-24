/* The exact and correctly rounded libm functions (spec §10.1, §10.2, §10.4, §10.6), end to end:
 * every function of those families in every format, on edge inputs, under every rounding mode
 * where the result depends on it. Each line prints the result's bits, the raised exception flags
 * and errno. Built against the host C library it records the reference behaviour
 * (spec/behaviour/libm-exact.glibc.txt.gz); built statically against Liella's products it must
 * print the same lines (math_products.rs), apart from recorded differences.
 *
 * Operands are volatile and functions are called through pointers, so the compiler folds
 * nothing. An optional argument selects one section (fenv, nan, f, d, l, q, narrow, legacy). */
#define _GNU_SOURCE
#define __STDC_WANT_IEC_60559_TYPES_EXT__ 1
#include <errno.h>
#include <fenv.h>
#include <float.h>
#include <math.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>
#include "../../../../conformance/c/expect_crash.h" /* the expected-crash helper (CORE-STORM) */

static const int MODES[4] = {FE_TONEAREST, FE_UPWARD, FE_DOWNWARD, FE_TOWARDZERO};
static const char MODE_NAME[4] = {'N', 'U', 'D', 'Z'};

/* Side results of a call (frexp's exponent, modf's integral part, ...), printed after it. */
static char side[96];

static void hexstr(char *out, const void *p, size_t n) {
    const unsigned char *b = p;
    for (size_t i = n; i-- > 0;) out += sprintf(out, "%02x", b[i]);
}

static void hexmem(const void *p, size_t n) {
    char buf[40];
    hexstr(buf, p, n);
    fputs(buf, stdout);
}

/* The flags raised by a call and errno after it, then the side results. */
static void report(int f, int e) {
    printf(" %c%c%c%c%c %s%s\n", f & FE_INVALID ? 'I' : '-', f & FE_DIVBYZERO ? 'Z' : '-', f & FE_OVERFLOW ? 'O' : '-',
           f & FE_UNDERFLOW ? 'U' : '-', f & FE_INEXACT ? 'X' : '-', e == 0 ? "0" : e == EDOM ? "EDOM" : e == ERANGE ? "ERANGE" : "E?",
           side);
    side[0] = 0;
}

static void tail(void) { report(fetestexcept(FE_ALL_EXCEPT), errno); }

static void begin(void) {
    feclearexcept(FE_ALL_EXCEPT);
    errno = 0;
}

/* The significant bytes of each format. */
#define NB_f 4
#define NB_d 8
#define NB_l 10
#define NB_q 16
#define PB_f(x) do { float v_ = (x); hexmem(&v_, 4); } while (0)
#define PB_d(x) do { double v_ = (x); hexmem(&v_, 8); } while (0)
#define PB_l(x) do { long double v_ = (x); hexmem(&v_, 10); } while (0)
#define PB_q(x) do { _Float128 v_ = (x); hexmem(&v_, 16); } while (0)

#define CAT_(a, b) a##b
#define CAT(a, b) CAT_(a, b)

/* One block per format: T the type, K the printer key, then the format's constants (BIG_ is
 * 2^(MANT_DIG-1), the start of the integers). */
#define FORMAT(T, K, BIG_, TRUE_MIN_, MIN_, MAX_, INF_, NAN_, SNAN_, PNAN_, LIT)                                     \
    static T CAT(in_, K)[] = {                                                                                     \
        0, -0.0, TRUE_MIN_, -(TRUE_MIN_), (MIN_) / 2, (MIN_), -(MIN_), (MIN_) * (1 - LIT(0x1p-20)), 0.25, 0.5, -0.5,   \
        0.75, 1, -1, 1.5, -1.5, 2.5, -2.5, 3, 4.5, -3.5, (T)1 / 3, LIT(0x1p30) + 0.5, LIT(0x1p31) - 0.5, LIT(0x1p31),  \
        -LIT(0x1p31), -LIT(0x1p31) - 0.5, LIT(0x1p32), LIT(0x1p63), -LIT(0x1p63), LIT(0x1p64), LIT(0x1p100),          \
        -LIT(1e30), (BIG_) - 0.5, -((BIG_) - 0.5), (BIG_) * 2, (BIG_) * 2 - 1, (MAX_), -(MAX_), (INF_), -(INF_),       \
        (NAN_), -(NAN_), (PNAN_), (SNAN_), -(SNAN_)};                                                              \
    static T CAT(pair_, K)[] = {0, -0.0, TRUE_MIN_, -(TRUE_MIN_), (MIN_), 1, -1, 1.5, -2.5, 3, LIT(0x1p31), (MAX_),   \
                                -(MAX_), (INF_), -(INF_), (NAN_), (SNAN_), (PNAN_)};                               \
    static void CAT(unary_, K)(const char *name, T (*fn)(T), int modes) {                                         \
        for (int m = 0; m < (modes ? 4 : 1); m++) {                                                              \
            fesetround(MODES[m]);                                                                                \
            for (size_t i = 0; i < sizeof CAT(in_, K) / sizeof(T); i++) {                                        \
                volatile T x = CAT(in_, K)[i];                                                                   \
                begin();                                                                                         \
                T r = fn(x);                                                                                     \
                int f = fetestexcept(FE_ALL_EXCEPT), e = errno;                                                  \
                printf("%s%s %c ", name, #K, MODE_NAME[m]);                                                      \
                CAT(PB_, K)(x);                                                                                  \
                printf(" = ");                                                                                   \
                CAT(PB_, K)(r);                                                                                  \
                report(f, e);                                                                                    \
            }                                                                                                    \
        }                                                                                                        \
        fesetround(FE_TONEAREST);                                                                                \
    }                                                                                                            \
    static void CAT(binary_, K)(const char *name, T (*fn)(T, T), int modes) {                                    \
        for (int m = 0; m < (modes ? 4 : 1); m++) {                                                              \
            fesetround(MODES[m]);                                                                                \
            for (size_t i = 0; i < sizeof CAT(pair_, K) / sizeof(T); i++)                                        \
                for (size_t j = 0; j < sizeof CAT(pair_, K) / sizeof(T); j++) {                                  \
                    volatile T x = CAT(pair_, K)[i], y = CAT(pair_, K)[j];                                       \
                    begin();                                                                                     \
                    T r = fn(x, y);                                                                              \
                    int f = fetestexcept(FE_ALL_EXCEPT), e = errno;                                              \
                    printf("%s%s %c ", name, #K, MODE_NAME[m]);                                                  \
                    CAT(PB_, K)(x);                                                                              \
                    printf(" ");                                                                                 \
                    CAT(PB_, K)(y);                                                                              \
                    printf(" = ");                                                                               \
                    CAT(PB_, K)(r);                                                                              \
                    report(f, e);                                                                                \
                }                                                                                                \
        }                                                                                                        \
        fesetround(FE_TONEAREST);                                                                                \
    }                                                                                                            \
    /* Integer-valued results (classification, ilogb, lrint, ...), as long long. */                           \
    static void CAT(toint_, K)(const char *name, long long (*fn)(T), int modes) {                                \
        for (int m = 0; m < (modes ? 4 : 1); m++) {                                                              \
            fesetround(MODES[m]);                                                                                \
            for (size_t i = 0; i < sizeof CAT(in_, K) / sizeof(T); i++) {                                        \
                volatile T x = CAT(in_, K)[i];                                                                   \
                begin();                                                                                         \
                long long r = fn(x);                                                                             \
                int f = fetestexcept(FE_ALL_EXCEPT), e = errno;                                                  \
                printf("%s%s %c ", name, #K, MODE_NAME[m]);                                                      \
                CAT(PB_, K)(x);                                                                                  \
                printf(" = %lld", r);                                                                            \
                report(f, e);                                                                                    \
            }                                                                                                    \
        }                                                                                                        \
        fesetround(FE_TONEAREST);                                                                                \
    }                                                                                                            \
    static void CAT(pairint_, K)(const char *name, long long (*fn)(T, T)) {                                      \
        for (size_t i = 0; i < sizeof CAT(pair_, K) / sizeof(T); i++)                                            \
            for (size_t j = 0; j < sizeof CAT(pair_, K) / sizeof(T); j++) {                                      \
                volatile T x = CAT(pair_, K)[i], y = CAT(pair_, K)[j];                                           \
                begin();                                                                                         \
                long long r = fn(x, y);                                                                          \
                int f = fetestexcept(FE_ALL_EXCEPT), e = errno;                                                  \
                printf("%s%s N ", name, #K);                                                                     \
                CAT(PB_, K)(x);                                                                                  \
                printf(" ");                                                                                     \
                CAT(PB_, K)(y);                                                                                  \
                printf(" = %lld", r);                                                                            \
                report(f, e);                                                                                    \
            }                                                                                                    \
    }

#define LIT_f(x) x##f
#define LIT_d(x) x
#define LIT_l(x) x##L
#define LIT_q(x) x##f128

FORMAT(float, f, 0x1p23f, FLT_TRUE_MIN, FLT_MIN, FLT_MAX, __builtin_inff(), __builtin_nanf(""), __builtin_nansf(""),
       __builtin_nanf("0x12345"), LIT_f)
FORMAT(double, d, 0x1p52, DBL_TRUE_MIN, DBL_MIN, DBL_MAX, __builtin_inf(), __builtin_nan(""), __builtin_nans(""),
       __builtin_nan("0x123456789"), LIT_d)
FORMAT(long double, l, 0x1p63L, LDBL_TRUE_MIN, LDBL_MIN, LDBL_MAX, __builtin_infl(), __builtin_nanl(""), __builtin_nansl(""),
       __builtin_nanl("0x123456789abc"), LIT_l)
FORMAT(_Float128, q, 0x1p112f128, FLT128_TRUE_MIN, FLT128_MIN, FLT128_MAX, __builtin_inff128(), __builtin_nanf128(""),
       __builtin_nansf128(""), __builtin_nanf128("0x123456789abcdef0123"), LIT_q)

/* Adapters from the C signatures to the drivers' shapes. Each outer macro expands its function
 * name argument before the inner one pastes it into the adapter's name. */
#define WRAP_INT(K, T, fn) WRAP_INT_(K, T, fn)
#define WRAP_INT_(K, T, fn) static long long w_##K##_##fn(T x) { return fn(x); }
#define WRAP_PINT(K, T, fn) WRAP_PINT_(K, T, fn)
#define WRAP_PINT_(K, T, fn) static long long w_##K##_##fn(T x, T y) { return fn(&x, &y); }
#define WRAP_INT2(K, T, fn) WRAP_INT2_(K, T, fn)
#define WRAP_INT2_(K, T, fn) static long long w_##K##_##fn(T x, T y) { return fn(x, y); }
#define WRAP_EXP(K, T, fn) WRAP_EXP_(K, T, fn)
#define WRAP_EXP_(K, T, fn) static T w_##K##_##fn(T x) { int e = 12345; T r = fn(x, &e); sprintf(side, " [%d]", e); return r; }
#define WRAP_MODF(K, T, fn) WRAP_MODF_(K, T, fn)
#define WRAP_MODF_(K, T, fn) static T w_##K##_##fn(T x) { T i; T r = fn(x, &i); side[0] = ' '; side[1] = '['; hexstr(side + 2, &i, NB_##K); strcat(side, "]"); return r; }
#define WRAP_REMQUO(K, T, fn) WRAP_REMQUO_(K, T, fn)
#define WRAP_REMQUO_(K, T, fn) static T w_##K##_##fn(T x, T y) { int q = 12345; T r = fn(x, y, &q); sprintf(side, " [%d]", q); return r; }
#define WRAP_CANON(K, T, fn) WRAP_CANON_(K, T, fn)
#define WRAP_CANON_(K, T, fn) static T w_##K##_##fn(T x) { T r = 0; int s = fn(&r, &x); sprintf(side, " [%d]", s); return r; }
#define WRAP_GETP(K, T, fn) WRAP_GETP_(K, T, fn)
#define WRAP_GETP_(K, T, fn) static T w_##K##_##fn(T x) { return fn(&x); }
#define WRAP_SETP(K, T, fn) WRAP_SETP_(K, T, fn)
#define WRAP_SETP_(K, T, fn) static T w_##K##_##fn(T x) { T r = 0; int s = fn(&r, x); sprintf(side, " [%d]", s); return r; }
/* Fixed second operands: name suffix, value. */
#define WRAP_ARG(K, T, fn, tag, IT, v) WRAP_ARG_(K, T, fn, tag, IT, v)
#define WRAP_ARG_(K, T, fn, tag, IT, v) static T w_##K##_##fn##_##tag(T x) { return fn(x, (IT)(v)); }
#define WRAP_FMA(K, T, fn, tag, z) WRAP_FMA_(K, T, fn, tag, z)
#define WRAP_FMA_(K, T, fn, tag, z) static T w_##K##_##fn##_##tag(T x, T y) { return fn(x, y, (T)(z)); }

#define PER_FORMAT(K, T, S)                                                                                     \
    WRAP_INT(K, T, CAT(__fpclassify, S)) WRAP_INT(K, T, CAT(__isinf, S)) WRAP_INT(K, T, CAT(__isnan, S))      \
    WRAP_INT(K, T, CAT(__finite, S)) WRAP_INT(K, T, CAT(__signbit, S)) WRAP_INT(K, T, CAT(__issignaling, S))   \
    WRAP_INT(K, T, CAT(ilogb, S)) WRAP_INT(K, T, CAT(llogb, S)) WRAP_INT(K, T, CAT(lrint, S))                   \
    WRAP_INT(K, T, CAT(llrint, S)) WRAP_INT(K, T, CAT(lround, S)) WRAP_INT(K, T, CAT(llround, S))               \
    WRAP_PINT(K, T, CAT(totalorder, S)) WRAP_PINT(K, T, CAT(totalordermag, S)) WRAP_INT2(K, T, CAT(__iseqsig, S)) \
    WRAP_EXP(K, T, CAT(frexp, S)) WRAP_MODF(K, T, CAT(modf, S)) WRAP_REMQUO(K, T, CAT(remquo, S))              \
    WRAP_ARG(K, T, CAT(ldexp, S), p3, int, 3) WRAP_ARG(K, T, CAT(ldexp, S), m3, int, -3)                        \
    WRAP_ARG(K, T, CAT(ldexp, S), p40000, int, 40000) WRAP_ARG(K, T, CAT(ldexp, S), m40000, int, -40000)        \
    WRAP_ARG(K, T, CAT(ldexp, S), m1074, int, -1074) WRAP_ARG(K, T, CAT(scalbn, S), m1, int, -1)                \
    WRAP_ARG(K, T, CAT(scalbln, S), p5e9, long, 5000000000) WRAP_ARG(K, T, CAT(scalbln, S), m5e9, long, -5000000000) \
    WRAP_CANON(K, T, CAT(canonicalize, S)) WRAP_GETP(K, T, CAT(getpayload, S)) WRAP_SETP(K, T, CAT(setpayload, S)) \
    WRAP_SETP(K, T, CAT(setpayloadsig, S)) WRAP_FMA(K, T, CAT(fma, S), z0, 0) WRAP_FMA(K, T, CAT(fma, S), z1, 1)   \
    WRAP_FMA(K, T, CAT(fma, S), zm1, -1) WRAP_FMA(K, T, CAT(fma, S), znan, NAN)

/* With an empty suffix the pasted pointer `wrapper(__fpclassify)` names are the same shape. */
PER_FORMAT(f, float, f)
PER_FORMAT(d, double, )
PER_FORMAT(l, long double, l)
PER_FORMAT(q, _Float128, f128)

/* The fromfp family: every rounding direction and a few widths. */
#define FROMFP(K, T, S)                                                                                           \
    static void fromfp_##K(void) {                                                                               \
        static const unsigned W[] = {0, 1, 2, 8, 31, 32, 63, 64, 65};                                             \
        for (int rnd = 0; rnd <= 4; rnd++)                                                                        \
            for (size_t w = 0; w < sizeof W / sizeof W[0]; w++)                                                   \
                for (size_t i = 0; i < sizeof in_##K / sizeof in_##K[0]; i++) {                                    \
                    volatile T x = in_##K[i];                                                                      \
                    begin(); long long a = CAT(fromfp, S)(x, rnd, W[w]); printf("fromfp%s %d %u ", #K, rnd, W[w]);  \
                    PB_##K(x); printf(" = %lld", a); tail();                                                        \
                    begin(); unsigned long long b = CAT(ufromfp, S)(x, rnd, W[w]); printf("ufromfp%s %d %u ", #K, rnd, W[w]); \
                    PB_##K(x); printf(" = %llu", b); tail();                                                        \
                    begin(); a = CAT(fromfpx, S)(x, rnd, W[w]); printf("fromfpx%s %d %u ", #K, rnd, W[w]);           \
                    PB_##K(x); printf(" = %lld", a); tail();                                                        \
                    begin(); b = CAT(ufromfpx, S)(x, rnd, W[w]); printf("ufromfpx%s %d %u ", #K, rnd, W[w]);         \
                    PB_##K(x); printf(" = %llu", b); tail();                                                        \
                }                                                                                                  \
    }
FROMFP(f, float, f)
FROMFP(d, double, )
FROMFP(l, long double, l)
FROMFP(q, _Float128, f128)

#define RUN_FORMAT(K, S)                                                                                         \
    do {                                                                                                         \
        toint_##K("fpclassify", CAT(w_##K##___fpclassify, S), 0);                                               \
        toint_##K("isinf", CAT(w_##K##___isinf, S), 0);                                                         \
        toint_##K("isnan", CAT(w_##K##___isnan, S), 0);                                                         \
        toint_##K("finite", CAT(w_##K##___finite, S), 0);                                                       \
        toint_##K("signbit", CAT(w_##K##___signbit, S), 0);                                                     \
        toint_##K("issignaling", CAT(w_##K##___issignaling, S), 0);                                             \
        toint_##K("ilogb", CAT(w_##K##_ilogb, S), 0);                                                           \
        toint_##K("llogb", CAT(w_##K##_llogb, S), 0);                                                           \
        toint_##K("lrint", CAT(w_##K##_lrint, S), 1);                                                           \
        toint_##K("llrint", CAT(w_##K##_llrint, S), 1);                                                         \
        toint_##K("lround", CAT(w_##K##_lround, S), 0);                                                         \
        toint_##K("llround", CAT(w_##K##_llround, S), 0);                                                       \
        pairint_##K("totalorder", CAT(w_##K##_totalorder, S));                                                  \
        pairint_##K("totalordermag", CAT(w_##K##_totalordermag, S));                                            \
        pairint_##K("iseqsig", CAT(w_##K##___iseqsig, S));                                                      \
        unary_##K("ceil", CAT(ceil, S), 0);                                                                      \
        unary_##K("floor", CAT(floor, S), 0);                                                                    \
        unary_##K("trunc", CAT(trunc, S), 0);                                                                    \
        unary_##K("round", CAT(round, S), 0);                                                                    \
        unary_##K("roundeven", CAT(roundeven, S), 0);                                                            \
        unary_##K("rint", CAT(rint, S), 1);                                                                      \
        unary_##K("nearbyint", CAT(nearbyint, S), 1);                                                            \
        unary_##K("logb", CAT(logb, S), 0);                                                                      \
        unary_##K("nextup", CAT(nextup, S), 0);                                                                  \
        unary_##K("nextdown", CAT(nextdown, S), 0);                                                              \
        unary_##K("fabs", CAT(fabs, S), 0);                                                                      \
        unary_##K("sqrt", CAT(sqrt, S), 1);                                                                      \
        unary_##K("frexp", CAT(w_##K##_frexp, S), 0);                                                           \
        unary_##K("modf", CAT(w_##K##_modf, S), 0);                                                             \
        unary_##K("ldexp3", CAT(CAT(w_##K##_ldexp, S), _p3), 1);                                                \
        unary_##K("ldexp-3", CAT(CAT(w_##K##_ldexp, S), _m3), 1);                                               \
        unary_##K("ldexp40000", CAT(CAT(w_##K##_ldexp, S), _p40000), 1);                                        \
        unary_##K("ldexp-40000", CAT(CAT(w_##K##_ldexp, S), _m40000), 1);                                       \
        unary_##K("ldexp-1074", CAT(CAT(w_##K##_ldexp, S), _m1074), 1);                                         \
        unary_##K("scalbn-1", CAT(CAT(w_##K##_scalbn, S), _m1), 1);                                             \
        unary_##K("scalbln5e9", CAT(CAT(w_##K##_scalbln, S), _p5e9), 1);                                        \
        unary_##K("scalbln-5e9", CAT(CAT(w_##K##_scalbln, S), _m5e9), 1);                                       \
        unary_##K("canonicalize", CAT(w_##K##_canonicalize, S), 0);                                            \
        unary_##K("getpayload", CAT(w_##K##_getpayload, S), 0);                                                \
        unary_##K("setpayload", CAT(w_##K##_setpayload, S), 0);                                                \
        unary_##K("setpayloadsig", CAT(w_##K##_setpayloadsig, S), 0);                                          \
        binary_##K("copysign", CAT(copysign, S), 0);                                                             \
        binary_##K("fmin", CAT(fmin, S), 0);                                                                     \
        binary_##K("fmax", CAT(fmax, S), 0);                                                                     \
        binary_##K("fminimum", CAT(fminimum, S), 0);                                                             \
        binary_##K("fmaximum", CAT(fmaximum, S), 0);                                                             \
        binary_##K("fminimum_num", CAT(fminimum_num, S), 0);                                                     \
        binary_##K("fmaximum_num", CAT(fmaximum_num, S), 0);                                                     \
        binary_##K("fminimum_mag", CAT(fminimum_mag, S), 0);                                                     \
        binary_##K("fmaximum_mag", CAT(fmaximum_mag, S), 0);                                                     \
        binary_##K("fminimum_mag_num", CAT(fminimum_mag_num, S), 0);                                             \
        binary_##K("fmaximum_mag_num", CAT(fmaximum_mag_num, S), 0);                                             \
        binary_##K("fminmag", CAT(fminmag, S), 0);                                                               \
        binary_##K("fmaxmag", CAT(fmaxmag, S), 0);                                                               \
        binary_##K("fdim", CAT(fdim, S), 1);                                                                     \
        binary_##K("fmod", CAT(fmod, S), 0);                                                                     \
        binary_##K("remainder", CAT(remainder, S), 0);                                                           \
        binary_##K("remquo", CAT(w_##K##_remquo, S), 0);                                                        \
        binary_##K("nextafter", CAT(nextafter, S), 0);                                                           \
        binary_##K("fma0", CAT(CAT(w_##K##_fma, S), _z0), 1);                                                   \
        binary_##K("fma1", CAT(CAT(w_##K##_fma, S), _z1), 1);                                                   \
        binary_##K("fma-1", CAT(CAT(w_##K##_fma, S), _zm1), 1);                                                 \
        binary_##K("fma-nan", CAT(CAT(w_##K##_fma, S), _znan), 0);                                              \
        fromfp_##K();                                                                                            \
    } while (0)

/* The BSD and XSI names that exist only for float, double and long double. */
static float w_f_nexttoward(float x) { return nexttowardf(x, 0x1p-16000L); }
static double w_d_nexttoward(double x) { return nexttoward(x, 0x1p-16000L); }
static long double w_l_nexttoward(long double x) { return nexttowardl(x, 0x1p-16000L); }
static void legacy(void) {
    unary_f("significand", significandf, 0);
    unary_d("significand", significand, 0);
    unary_l("significand", significandl, 0);
    unary_f("nexttoward", w_f_nexttoward, 0);
    unary_d("nexttoward", w_d_nexttoward, 0);
    unary_l("nexttoward", w_l_nexttoward, 0);
    binary_f("scalb", scalbf, 1);
    binary_d("scalb", scalb, 1);
    binary_l("scalb", scalbl, 1);
    binary_f("drem", dremf, 0);
    binary_d("drem", drem, 0);
    binary_l("drem", dreml, 0);
}

/* Narrowing arithmetic (§10.4): the double-rounding-sensitive cases. */
static void narrowing(void) {
    static const double D[] = {1, 0x1p-24, 0x1.000001p0, 0x1.0000010000001p0, 0x1.fffffep127, 0x1.ffffffp127, 0x1p-149,
                               0x1p-150, 0x1.8p-150, 0x1.fffffep-127, 0x1p-126, -0x1p-200, 3, 0, -0.0, __builtin_inf(),
                               __builtin_nan(""), __builtin_nans(""), 1e300};
    static const long double L[] = {1, 0x1p-24L, 0x1.000001p0L, 0x1.00000000000008p0L, 0x1.0000000000000801p0L,
                                    0x1p-1074L, 0x1p-1075L, 0x1.8p-1075L, 0x1p-1022L, 1e4000L, 0, -0.0L, __builtin_nansl("")};
    static const _Float128 Q[] = {1, 0x1p-24f128, 0x1.000001p0f128, 0x1.00000000000008p0f128, 0x1.0000000000000000001p0f128,
                                  0x1p-1075f128, 0x1p-16446f128, 0x1.8p-16446f128, 1e4000f128, 0, __builtin_nansf128("")};
    for (int m = 0; m < 4; m++) {
        fesetround(MODES[m]);
        for (size_t i = 0; i < sizeof D / sizeof D[0]; i++)
            for (size_t j = 0; j < sizeof D / sizeof D[0]; j++) {
                volatile double x = D[i], y = D[j];
#define N2(name, call, K) begin(); { __typeof__(call) r_ = call; int f_ = fetestexcept(FE_ALL_EXCEPT), e_ = errno; printf("%s %c ", name, MODE_NAME[m]); PB_d(x); printf(" "); PB_d(y); printf(" = "); PB_##K(r_); report(f_, e_); }
                N2("fadd", fadd(x, y), f) N2("fsub", fsub(x, y), f) N2("fmul", fmul(x, y), f) N2("fdiv", fdiv(x, y), f)
                N2("ffma", ffma(x, y, 0x1p-30), f) N2("f32xaddf64", f32xaddf64(x, y), d)
            }
        for (size_t i = 0; i < sizeof D / sizeof D[0]; i++) {
            volatile double x = D[i];
            begin(); { float r = fsqrt(x); int f_ = fetestexcept(FE_ALL_EXCEPT), e_ = errno; printf("fsqrt %c ", MODE_NAME[m]); PB_d(x); printf(" = "); PB_f(r); report(f_, e_); }
        }
        for (size_t i = 0; i < sizeof L / sizeof L[0]; i++)
            for (size_t j = 0; j < sizeof L / sizeof L[0]; j++) {
                volatile long double x = L[i], y = L[j];
#define N3(name, call, K) begin(); { __typeof__(call) r_ = call; int f_ = fetestexcept(FE_ALL_EXCEPT), e_ = errno; printf("%s %c ", name, MODE_NAME[m]); PB_l(x); printf(" "); PB_l(y); printf(" = "); PB_##K(r_); report(f_, e_); }
                N3("faddl", faddl(x, y), f) N3("daddl", daddl(x, y), d) N3("dsubl", dsubl(x, y), d) N3("dmull", dmull(x, y), d)
                N3("ddivl", ddivl(x, y), d) N3("dfmal", dfmal(x, y, 0x1p-60L), d)
            }
        for (size_t i = 0; i < sizeof L / sizeof L[0]; i++) {
            volatile long double x = L[i];
            begin(); { double r = dsqrtl(x); int f_ = fetestexcept(FE_ALL_EXCEPT), e_ = errno; printf("dsqrtl %c ", MODE_NAME[m]); PB_l(x); printf(" = "); PB_d(r); report(f_, e_); }
        }
        for (size_t i = 0; i < sizeof Q / sizeof Q[0]; i++)
            for (size_t j = 0; j < sizeof Q / sizeof Q[0]; j++) {
                volatile _Float128 x = Q[i], y = Q[j];
#define N4(name, call, K) begin(); { __typeof__(call) r_ = call; int f_ = fetestexcept(FE_ALL_EXCEPT), e_ = errno; printf("%s %c ", name, MODE_NAME[m]); PB_q(x); printf(" "); PB_q(y); printf(" = "); PB_##K(r_); report(f_, e_); }
                N4("f32addf128", f32addf128(x, y), f) N4("f64mulf128", f64mulf128(x, y), d) N4("f64xdivf128", f64xdivf128(x, y), l)
                N4("f64xfmaf128", f64xfmaf128(x, y, 0x1p-70f128), l)
            }
    }
    fesetround(FE_TONEAREST);
}

/* nan(tag) (§10.2). */
static void nans(void) {
    static const char *T[] = {"", "0", "0x123", "123", "0123", "abc", "0x7fffff", "0x3fffffffffffff", "99999999999999999999999999",
                              "0x1ffffffffffffffffffffffffffff", "(", "1e3", "-1", "0x"};
    for (size_t i = 0; i < sizeof T / sizeof T[0]; i++) {
        printf("nan \"%s\" = ", T[i]); PB_f(nanf(T[i])); printf(" "); PB_d(nan(T[i])); printf(" "); PB_l(nanl(T[i]));
        printf(" "); PB_q(nanf128(T[i])); printf("\n");
    }
}

/* The floating-point environment (§10.6). */
static void fenv(void) {
    printf("math_errhandling %d\n", math_errhandling);
    printf("fegetround %d\n", fegetround());
    for (int m = 0; m < 4; m++) {
        int r = fesetround(MODES[m]);
        printf("fesetround %d -> %d, now %d\n", MODES[m], r, fegetround());
    }
    int r = fesetround(0x1234);
    printf("fesetround 0x1234 -> %d, now %d\n", r, fegetround());
    r = fesetround(-1);
    printf("fesetround -1 -> %d, now %d\n", r, fegetround());
    fesetround(FE_TONEAREST);
    static const int F[] = {FE_INVALID, FE_DIVBYZERO, FE_OVERFLOW, FE_UNDERFLOW, FE_INEXACT, FE_ALL_EXCEPT, 0, 0x40, 0x1000};
    for (size_t i = 0; i < sizeof F / sizeof F[0]; i++) {
        feclearexcept(FE_ALL_EXCEPT);
        r = feraiseexcept(F[i]);
        printf("feraiseexcept %#x -> %d, flags %#x\n", F[i], r, fetestexcept(FE_ALL_EXCEPT));
        feclearexcept(FE_ALL_EXCEPT);
        r = fesetexcept(F[i]);
        printf("fesetexcept %#x -> %d, flags %#x\n", F[i], r, fetestexcept(FE_ALL_EXCEPT));
        r = feclearexcept(F[i]);
        printf("feclearexcept %#x -> %d, flags %#x\n", F[i], r, fetestexcept(FE_ALL_EXCEPT));
    }
    /* Flags raised by x87 arithmetic are seen too. */
    feclearexcept(FE_ALL_EXCEPT);
    volatile long double z = 0;
    z = z / z;
    printf("x87 0/0: flags %#x\n", fetestexcept(FE_ALL_EXCEPT));
    feclearexcept(FE_ALL_EXCEPT);
    printf("after clear: flags %#x\n", fetestexcept(FE_ALL_EXCEPT));
    volatile double dz = 0;
    dz = 1 / dz;
    fexcept_t saved;
    r = fegetexceptflag(&saved, FE_ALL_EXCEPT);
    printf("fegetexceptflag -> %d, flags %#x\n", r, fetestexcept(FE_ALL_EXCEPT));
    printf("fexcept_t %#x\n", (unsigned)saved);
    printf("fetestexceptflag %#x\n", fetestexceptflag(&saved, FE_ALL_EXCEPT));
    feclearexcept(FE_ALL_EXCEPT);
    r = fesetexceptflag(&saved, FE_DIVBYZERO | FE_INVALID);
    printf("fesetexceptflag -> %d, flags %#x\n", r, fetestexcept(FE_ALL_EXCEPT));
    fexcept_t forged = FE_INVALID | FE_INEXACT;
    feclearexcept(FE_ALL_EXCEPT);
    r = fesetexceptflag(&forged, FE_INEXACT);
    printf("fesetexceptflag forged -> %d, flags %#x\n", r, fetestexcept(FE_ALL_EXCEPT));
    feclearexcept(FE_ALL_EXCEPT);
    printf("fegetexcept %#x\n", fegetexcept());
    r = feenableexcept(FE_DIVBYZERO);
    printf("feenableexcept -> %#x\n", r);
    r = feenableexcept(FE_INVALID);
    printf("feenableexcept -> %#x\n", r);
    printf("fegetexcept %#x\n", fegetexcept());
    r = feenableexcept(0x40);
    printf("feenableexcept 0x40 -> %#x\n", r);
    r = fedisableexcept(FE_ALL_EXCEPT);
    printf("fedisableexcept -> %#x\n", r);
    printf("fegetexcept %#x\n", fegetexcept());
    fenv_t env;
    fesetround(FE_UPWARD);
    feraiseexcept(FE_OVERFLOW);
    r = fegetenv(&env);
    printf("fegetenv -> %d\n", r);
    printf("env cw %#x sw %#x mxcsr %#x\n", env.__control_word, env.__status_word & 0x3f, env.__mxcsr);
    r = feholdexcept(&env);
    printf("feholdexcept -> %d, flags %#x, round %d\n", r, fetestexcept(FE_ALL_EXCEPT), fegetround());
    printf("held cw %#x sw %#x mxcsr %#x\n", env.__control_word, env.__status_word & 0x3f, env.__mxcsr);
    feraiseexcept(FE_INEXACT);
    r = feupdateenv(&env);
    printf("feupdateenv -> %d, flags %#x, round %d\n", r, fetestexcept(FE_ALL_EXCEPT), fegetround());
    r = fesetenv(FE_DFL_ENV);
    printf("fesetenv dfl -> %d, flags %#x, round %d, except %#x\n", r, fetestexcept(FE_ALL_EXCEPT), fegetround(), fegetexcept());
    femode_t mode;
    fesetround(FE_DOWNWARD);
    r = fegetmode(&mode);
    printf("fegetmode -> %d\n", r);
    printf("mode cw %#x mxcsr %#x\n", mode.__control_word, mode.__mxcsr);
    feraiseexcept(FE_INVALID);
    r = fesetmode(FE_DFL_MODE);
    printf("fesetmode dfl -> %d, round %d, flags %#x\n", r, fegetround(), fetestexcept(FE_ALL_EXCEPT));
    r = fesetmode(&mode);
    printf("fesetmode -> %d, round %d\n", r, fegetround());
    fesetenv(FE_DFL_ENV);
    r = fesetenv(FE_NOMASK_ENV);
    printf("fesetenv nomask -> %d, except %#x\n", r, fegetexcept());
    fesetenv(FE_DFL_ENV);
    fegetenv(&env);
    printf("dfl cw %#x sw %#x mxcsr %#x\n", env.__control_word, env.__status_word, env.__mxcsr);
    /* Which unit holds the flags each function sets (visible through fegetenv). */
    static const struct { const char *name; int which; } UNIT[] = {{"feraiseexcept", 0}, {"fesetexcept", 1}, {"fesetexceptflag", 2}};
    for (size_t i = 0; i < 3; i++) {
        fesetenv(FE_DFL_ENV);
        fexcept_t all = FE_ALL_EXCEPT;
        if (UNIT[i].which == 0) feraiseexcept(FE_ALL_EXCEPT);
        if (UNIT[i].which == 1) fesetexcept(FE_ALL_EXCEPT);
        if (UNIT[i].which == 2) fesetexceptflag(&all, FE_ALL_EXCEPT);
        fegetenv(&env);
        printf("%s units: sw %#x mxcsr %#x\n", UNIT[i].name, env.__status_word & 0x3f, env.__mxcsr & 0x3f);
    }
    fesetenv(FE_DFL_ENV);
    feraiseexcept(FE_INVALID);
    fexcept_t div = FE_DIVBYZERO;
    r = fesetexceptflag(&div, FE_DIVBYZERO | FE_INVALID);
    printf("fesetexceptflag clears -> %d, flags %#x\n", r, fetestexcept(FE_ALL_EXCEPT));
    fesetenv(FE_DFL_ENV);
    /* Traps: an enabled exception raised by feraiseexcept delivers SIGFPE (in a child). */
    static const int TRAP[] = {FE_INVALID, FE_DIVBYZERO, FE_OVERFLOW, FE_UNDERFLOW, FE_INEXACT};
    for (size_t i = 0; i < 5; i++) {
        fflush(stdout);
        pid_t p = fork();
        if (p == 0) {
            feenableexcept(TRAP[i]);
            expect_crash();
            feraiseexcept(TRAP[i]);
            _exit(0);
        }
        int st = 0;
        waitpid(p, &st, 0);
        printf("trap %#x: %s\n", TRAP[i], WIFSIGNALED(st) ? (WTERMSIG(st) == SIGFPE ? "SIGFPE" : "other signal") : "no trap");
        fflush(stdout);
        p = fork();
        if (p == 0) {
            feenableexcept(TRAP[i]);
            expect_crash();
            fesetexcept(TRAP[i]);
            _exit(0);
        }
        waitpid(p, &st, 0);
        printf("fesetexcept trap %#x: %s\n", TRAP[i], WIFSIGNALED(st) ? (WTERMSIG(st) == SIGFPE ? "SIGFPE" : "other signal") : "no trap");
    }
}

/* x87 encodings IEEE does not define: pseudo-denormal, unnormal, pseudo-infinity, pseudo-NaN. */
static void x87(void) {
    static const struct { uint64_t m; uint16_t se; } X[] = {
        {0x8000000000000001ull, 0}, {0x0000000000000001ull, 1}, {0x4000000000000000ull, 0x3fff}, {0, 0x7fff},
        {0x4000000000000000ull, 0x7fff}, {0x8000000000000000ull, 0x7fff}, {0xc000000000000000ull, 0x7fff},
        {0x8000000000000000ull, 0x8000}, {0, 0x8001}};
    for (size_t i = 0; i < sizeof X / sizeof X[0]; i++) {
        long double v;
        memset(&v, 0, sizeof v);
        memcpy(&v, &X[i].m, 8);
        memcpy((char *)&v + 8, &X[i].se, 2);
        volatile long double x = v;
        long double xv = x, one = 1;
        begin(); { long double r = 0; int s = canonicalizel(&r, &xv); printf("canonicalize-x87 "); PB_l(xv); printf(" = [%d] ", s); PB_l(r); } tail();
        begin(); { long long r = __iscanonicall(x); printf("iscanonical-x87 "); PB_l(xv); printf(" = %lld", r); } tail();
        begin(); { long long r = __fpclassifyl(x); printf("fpclassify-x87 "); PB_l(xv); printf(" = %lld", r); } tail();
        begin(); { long long r = __isnanl(x); printf("isnan-x87 "); PB_l(xv); printf(" = %lld", r); } tail();
        begin(); { long long r = __isinfl(x); printf("isinf-x87 "); PB_l(xv); printf(" = %lld", r); } tail();
        begin(); { long long r = __finitel(x); printf("finite-x87 "); PB_l(xv); printf(" = %lld", r); } tail();
        begin(); { long long r = __signbitl(x); printf("signbit-x87 "); PB_l(xv); printf(" = %lld", r); } tail();
        begin(); { long double r = fmal(x, 1, 0); printf("fma-x87 "); PB_l(xv); printf(" = "); PB_l(r); } tail();
        begin(); { long long r = __issignalingl(x); printf("issignaling-x87 "); PB_l(xv); printf(" = %lld", r); } tail();
        begin(); { long double r = fabsl(x); printf("fabs-x87 "); PB_l(xv); printf(" = "); PB_l(r); } tail();
        begin(); { long double r = rintl(x); printf("rint-x87 "); PB_l(xv); printf(" = "); PB_l(r); } tail();
        begin(); { long double r = sqrtl(x); printf("sqrt-x87 "); PB_l(xv); printf(" = "); PB_l(r); } tail();
        begin(); { long long r = totalorderl(&xv, &one); printf("totalorder-x87 "); PB_l(xv); printf(" = %lld", r); } tail();
    }
}

int main(int argc, char **argv) {
    const char *only = argc > 1 ? argv[1] : "";
    setvbuf(stdout, NULL, _IOFBF, 1 << 16);
    if (!*only || !strcmp(only, "fenv")) fenv();
    if (!*only || !strcmp(only, "nan")) nans();
    if (!*only || !strcmp(only, "f")) RUN_FORMAT(f, f);
    if (!*only || !strcmp(only, "d")) RUN_FORMAT(d, );
    if (!*only || !strcmp(only, "l")) RUN_FORMAT(l, l);
    if (!*only || !strcmp(only, "q")) RUN_FORMAT(q, f128);
    if (!*only || !strcmp(only, "legacy")) legacy();
    if (!*only || !strcmp(only, "narrow")) narrowing();
    if (!*only || !strcmp(only, "x87")) x87();
    return 0;
}
