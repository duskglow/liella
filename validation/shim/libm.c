/* Validation stand-ins for the maths functions gawk's language needs (validation/README.md,
 * "Workarounds", milestone 6): NOT part of Liella.
 *
 * Track M's libm is not a product yet (T3). gawk calls sin, cos, atan2, exp, log, sqrt, pow and
 * fmod for its built-in functions and its `^`/`%` operators; linking fails without them. These
 * thin stand-ins are written from C17 7.12 and Annex F (special cases) and use the x87 and SSE2
 * instructions documented in the Intel SDM for the arithmetic itself. They are accurate to
 * about one unit in the last place for ordinary arguments, not correctly rounded. Every result
 * of a build that links this file is PROVISIONAL (owner ruling, 2026-10-04) and is re-run when
 * Track M's libm is a product.
 */
#include "missing.c" /* modf */

static int is_nan(double x) { return x != x; }
static int is_inf(double x) { return x == __builtin_inf() || x == -__builtin_inf(); }

/* sqrt (C17 7.12.7.5): SQRTSD, correctly rounded; NaN for negative arguments. */
double sqrt(double x)
{
    double r;
    __asm__("sqrtsd %1, %0" : "=x"(r) : "x"(x));
    return r;
}

/* sin or cos in extended precision; FSIN/FCOS take |x| < 2^63, so larger arguments are first
 * reduced modulo 2*pi with FPREM1 (exact remainder, of the 64-bit pi the FPU holds). */
static long double trig(long double x, int cosine)
{
    long double r = x;
    if (x >= 0x1p62L || x <= -0x1p62L) {
        long double pi, twopi;
        __asm__("fldpi" : "=t"(pi));
        twopi = pi * 2;
        __asm__("1:\tfprem1\n\t"
                "fnstsw %%ax\n\t"
                "testw $0x400, %%ax\n\t"
                "jnz 1b"
                : "=t"(r) : "0"(x), "u"(twopi) : "ax", "cc");
    }
    if (cosine)
        __asm__("fcos" : "=t"(r) : "0"(r));
    else
        __asm__("fsin" : "=t"(r) : "0"(r));
    return r;
}

/* sin, cos (C17 7.12.4.6, 7.12.4.5; F.10.1.6, F.10.1.5): NaN for infinities. */
double sin(double x)
{
    if (is_nan(x) || is_inf(x))
        return x - x;
    if (x == 0)
        return x;
    return (double)trig(x, 0);
}

double cos(double x)
{
    if (is_nan(x) || is_inf(x))
        return x - x;
    return (double)trig(x, 1);
}

/* atan2 (C17 7.12.4.4): FPATAN computes arctan(st(1)/st(0)) in the quadrant of (st(0), st(1)),
 * including the signed-zero and infinity cases of F.10.1.4. */
double atan2(double y, double x)
{
    long double r;
    __asm__("fpatan" : "=t"(r) : "0"((long double)x), "u"((long double)y) : "st(1)");
    return (double)r;
}

/* log (C17 7.12.6.7; F.10.3.7): FYL2X with ln 2. */
double log(double x)
{
    if (is_nan(x))
        return x;
    if (x < 0)
        return (x - x) / (x - x);
    if (x == 0)
        return -__builtin_inf();
    if (is_inf(x))
        return x;
    long double c, r;
    __asm__("fldln2" : "=t"(c));
    __asm__("fyl2x" : "=t"(r) : "0"((long double)x), "u"(c) : "st(1)");
    return (double)r;
}

double log10(double x)
{
    if (is_nan(x))
        return x;
    if (x < 0)
        return (x - x) / (x - x);
    if (x == 0)
        return -__builtin_inf();
    if (is_inf(x))
        return x;
    long double c, r;
    __asm__("fldlg2" : "=t"(c));
    __asm__("fyl2x" : "=t"(r) : "0"((long double)x), "u"(c) : "st(1)");
    return (double)r;
}

/* 2^t for finite t, in extended precision: F2XM1 on the fraction, FSCALE by the integer. */
static long double exp2l_finite(long double t)
{
    long double n, f, r;
    __asm__("frndint" : "=t"(n) : "0"(t));
    __asm__("f2xm1" : "=t"(f) : "0"(t - n));
    __asm__("fscale" : "=t"(r) : "0"(f + 1), "u"(n));
    return r;
}

/* exp (C17 7.12.6.1; F.10.3.1). */
double exp(double x)
{
    if (is_nan(x))
        return x;
    if (x == __builtin_inf())
        return x;
    if (x == -__builtin_inf())
        return 0;
    if (x > 710)
        return __builtin_inf();
    if (x < -746)
        return 0;
    long double c;
    __asm__("fldl2e" : "=t"(c));
    return (double)exp2l_finite((long double)x * c);
}

/* log2 of |x| > 0 in extended precision. */
static long double log2l_pos(long double x)
{
    long double r;
    __asm__("fyl2x" : "=t"(r) : "0"(x), "u"(1.0L) : "st(1)");
    return r;
}

/* Whether y is an integer, and whether it is odd (|y| < 2^53 for odd). */
static int is_integer(double y) { return !is_inf(y) && (y >= 0x1p52 || y <= -0x1p52 || y == (double)(long long)y); }
static int is_odd(double y) { return y > -0x1p53 && y < 0x1p53 && is_integer(y) && ((long long)y & 1); }

/* pow (C17 7.12.7.4; the special cases of F.10.4.4 in their order). */
double pow(double x, double y)
{
    if (y == 0 || x == 1)
        return 1;
    if (is_nan(x) || is_nan(y))
        return x + y;
    if (x == 0) {
        if (y < 0)
            return is_odd(y) ? 1 / x : __builtin_inf();
        return is_odd(y) ? x : 0;
    }
    if (x == -1 && is_inf(y))
        return 1;
    if (y == -__builtin_inf())
        return (x > -1 && x < 1) ? __builtin_inf() : 0;
    if (y == __builtin_inf())
        return (x > -1 && x < 1) ? 0 : __builtin_inf();
    if (x == -__builtin_inf()) {
        if (y < 0)
            return is_odd(y) ? -0.0 : 0;
        return is_odd(y) ? -__builtin_inf() : __builtin_inf();
    }
    if (x == __builtin_inf())
        return y < 0 ? 0 : __builtin_inf();
    int negate = 0;
    if (x < 0) {
        if (!is_integer(y))
            return (x - x) / (x - x);
        negate = is_odd(y);
        x = -x;
    }
    long double t = (long double)y * log2l_pos(x);
    double r;
    if (t > 1100)
        r = __builtin_inf();
    else if (t < -1100)
        r = 0;
    else
        r = (double)exp2l_finite(t);
    return negate ? -r : r;
}

/* fmod (C17 7.12.10.1; F.10.7.1): FPREM is exact. */
double fmod(double x, double y)
{
    if (is_nan(x) || is_nan(y) || is_inf(x) || y == 0)
        return (x * y) / (x * y);
    if (is_inf(y))
        return x;
    long double r;
    __asm__("1:\tfprem\n\t"
            "fnstsw %%ax\n\t"
            "testw $0x400, %%ax\n\t"
            "jnz 1b"
            : "=t"(r) : "0"((long double)x), "u"((long double)y) : "ax", "cc");
    return (double)r;
}

/* floor, ceil (C17 7.12.9.2, 7.12.9.1). */
double floor(double x)
{
    if (is_nan(x) || x >= 0x1p52 || x <= -0x1p52 || x == 0)
        return x;
    double i = (double)(long long)x;
    if (i > x)
        i -= 1;
    return (i == 0 && x < 0) ? -0.0 : i;
}

double ceil(double x)
{
    if (is_nan(x) || x >= 0x1p52 || x <= -0x1p52 || x == 0)
        return x;
    double i = (double)(long long)x;
    if (i < x)
        i += 1;
    return (i == 0 && x < 0) ? -0.0 : i;
}
