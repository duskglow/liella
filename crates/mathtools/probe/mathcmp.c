/* The measurement probe of `mathcmp` (D-10.3): one function of one format evaluated on a
 * stream of operands. Built against the host C library (the reference) and statically against
 * Liella's products (`-DLIELLA_SIDE`, which leaves out the functions Liella does not have yet).
 *
 * Usage: mathcmp_probe FUNC FMT MODE
 *   FMT  f, d, l or q; MODE N, U, D or Z (the rounding direction).
 *   stdin: records of ARITY operands, each 16 bytes (the encoding, little-endian, zero-padded).
 *   stdout: per record 16 bytes of result and one byte of raised flags (FE_* bits).
 * Exit status 2 for an unknown function or format. */
#define _GNU_SOURCE
#define __STDC_WANT_IEC_60559_TYPES_EXT__ 1
#include <fenv.h>
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef unsigned char u8;

/* Every supported function, per arity: X(name). LIELLA_SIDE drops the ones not implemented
 * yet; a later milestone moves each function out of its guard. */
#define UNARY_EXACT(X) X(sqrt) X(rint) X(nearbyint) X(ceil) X(floor) X(trunc) X(round) X(roundeven) X(logb) X(nextup) X(nextdown)
#define BINARY_EXACT(X) X(fmod) X(remainder) X(fdim) X(fmax) X(fmin) X(nextafter) X(copysign)
#define TERNARY_EXACT(X) X(fma)
/* The elementary functions: float and double since milestone 2, long double since 5a; hypot in every format. */
#define UNARY_ELEM(X) X(exp) X(exp2) X(exp10) X(expm1) X(log) X(log2) X(log10) X(log1p) X(sin) X(cos) X(tan) X(asin) X(acos) X(atan) \
    X(sinh) X(cosh) X(tanh) X(asinh) X(acosh) X(atanh) X(cbrt)
#define BINARY_ELEM(X) X(pow) X(atan2)
#define BINARY_ALL(X) X(hypot)
/* The special functions: float and double since milestone 3, long double since 5b. */
#define UNARY_SPECIAL(X) X(erf) X(erfc) X(tgamma) X(lgamma) X(j0) X(j1) X(y0) X(y1)
#ifndef LIELLA_SIDE
#define UNARY_ELEM_WIDE(X) UNARY_ELEM(X)
#define BINARY_ELEM_WIDE(X) BINARY_ELEM(X)
#define UNARY_SPECIAL_WIDE(X) UNARY_SPECIAL(X)
#else
/* _Float128: the exponentials, logarithms, cbrt and pow since milestone 5 (M5-5), the trigonometric and
 * hyperbolic functions and atan2 since M5-6: every elementary function. */
#define UNARY_ELEM_WIDE(X) UNARY_ELEM(X)
#define BINARY_ELEM_WIDE(X) BINARY_ELEM(X)
#define UNARY_SPECIAL_WIDE(X)
#endif


#define CAT_(a, b) a##b
#define CAT(a, b) CAT_(a, b)

static int arity_of(const char *f) {
#define A1(n) if (!strcmp(f, #n)) return 1;
#define A2(n) if (!strcmp(f, #n)) return 2;
#define A3(n) if (!strcmp(f, #n)) return 3;
    UNARY_EXACT(A1) UNARY_ELEM(A1) UNARY_SPECIAL(A1) BINARY_EXACT(A2) BINARY_ELEM(A2) BINARY_ALL(A2) TERNARY_EXACT(A3)
    return 0;
}

#define RUN(T, S, NB)                                                                                 \
    static int run_##S(const char *f, int arity) {                                                     \
        u8 in[48], out[17];                                                                            \
        T a[3];                                                                                        \
        size_t n = (size_t)16 * arity;                                                                 \
        while (fread(in, 1, n, stdin) == n) {                                                          \
            for (int i = 0; i < arity; i++) memcpy(&a[i], in + 16 * i, sizeof(T));                     \
            volatile T x = a[0], y = a[1], z = a[2];                                                   \
            T r = 0;                                                                                   \
            feclearexcept(FE_ALL_EXCEPT);                                                              \
            int found = 0;                                                                             \
            _Pragma("GCC diagnostic push") _Pragma("GCC diagnostic ignored \"-Wunused-value\"")       \
            CALLS_##S                                                                                  \
            _Pragma("GCC diagnostic pop")                                                              \
            if (!found) return 2;                                                                      \
            memset(out, 0, sizeof out);                                                                \
            memcpy(out, &r, NB);                                                                       \
            out[16] = (u8)fetestexcept(FE_ALL_EXCEPT);                                                 \
            fwrite(out, 1, 17, stdout);                                                                \
        }                                                                                              \
        return 0;                                                                                      \
    }

#define C1(S, n) if (!strcmp(f, #n)) { found = 1; r = CAT(n, S)(x); }
#define C2(S, n) if (!strcmp(f, #n)) { found = 1; r = CAT(n, S)(x, y); }
#define C3(S, n) if (!strcmp(f, #n)) { found = 1; r = CAT(n, S)(x, y, z); }
#define C1f(n) C1(f, n)
#define C2f(n) C2(f, n)
#define C3f(n) C3(f, n)
#define C1d(n) C1(, n)
#define C2d(n) C2(, n)
#define C3d(n) C3(, n)
#define C1l(n) C1(l, n)
#define C2l(n) C2(l, n)
#define C3l(n) C3(l, n)
#define C1q(n) C1(f128, n)
#define C2q(n) C2(f128, n)
#define C3q(n) C3(f128, n)
#define CALLS_f UNARY_EXACT(C1f) UNARY_ELEM(C1f) UNARY_SPECIAL(C1f) BINARY_EXACT(C2f) BINARY_ELEM(C2f) BINARY_ALL(C2f) TERNARY_EXACT(C3f)
#define CALLS_d UNARY_EXACT(C1d) UNARY_ELEM(C1d) UNARY_SPECIAL(C1d) BINARY_EXACT(C2d) BINARY_ELEM(C2d) BINARY_ALL(C2d) TERNARY_EXACT(C3d)
#define CALLS_l UNARY_EXACT(C1l) UNARY_ELEM(C1l) UNARY_SPECIAL(C1l) BINARY_EXACT(C2l) BINARY_ELEM(C2l) BINARY_ALL(C2l) TERNARY_EXACT(C3l)
#define CALLS_q UNARY_EXACT(C1q) UNARY_ELEM_WIDE(C1q) UNARY_SPECIAL_WIDE(C1q) BINARY_EXACT(C2q) BINARY_ELEM_WIDE(C2q) BINARY_ALL(C2q) TERNARY_EXACT(C3q)

RUN(float, f, 4)
RUN(double, d, 8)
RUN(long double, l, 10)
RUN(_Float128, q, 16)

int main(int argc, char **argv) {
    if (argc < 4) return 2;
    const char *f = argv[1];
    int arity = arity_of(f);
    if (!arity) return 2;
    static const int modes[] = {FE_TONEAREST, FE_UPWARD, FE_DOWNWARD, FE_TOWARDZERO};
    const char *m = strchr("NUDZ", argv[3][0]);
    if (!m || !argv[3][0]) return 2;
    fesetround(modes[m - "NUDZ"]);
    setvbuf(stdout, NULL, _IOFBF, 1 << 20);
    setvbuf(stdin, NULL, _IOFBF, 1 << 20);
    switch (argv[2][0]) {
    case 'f': return run_f(f, arity);
    case 'd': return run_d(f, arity);
    case 'l': return run_l(f, arity);
    case 'q': return run_q(f, arity);
    }
    return 2;
}
