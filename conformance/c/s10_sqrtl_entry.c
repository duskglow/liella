/* `sqrtl`'s entry (spec §10, D-10.71: csys/src/machine/sqrtl.S) through the real C calling
 * convention: the operand in the caller's stack slot, the result in st(0). The Rust tests reach
 * the software and in-place paths through `liella_sqrtl_bits`, never this entry, so its own
 * tests (control word, operand class) are checked here.
 * Clauses: s10_0_f_0 (the root is correctly rounded in the caller's direction with its flags and
 * errno). Self-checking: each failed check prints `FAIL line N: <check>`; exit 1 on any failure.
 * The function is called through a volatile pointer: a compiler may expand a direct `sqrtl`
 * call inline (`fsqrt`) and never reach the entry. */
/* LINK-LIBM: yes */
#include <errno.h>
#include <fenv.h>
#include <fpu_control.h>
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

static long double (*volatile root)(long double) = sqrtl;
static int failed;

#define CHECK(c) do { if (!(c)) { fprintf(stderr, "FAIL line %d: %s\n", __LINE__, #c); failed = 1; } } while (0)

/* The 64-bit significand and the sign-and-exponent word of an x87 extended value. */
static uint64_t mant(long double v)
{
    uint64_t m;
    memcpy(&m, &v, sizeof m);
    return m;
}

static unsigned se(long double v)
{
    uint16_t s;
    memcpy(&s, (const char *)&v + 8, sizeof s);
    return s;
}

/* One call with errno 0 and the flags clear; the raised flags in *flags. */
static long double call(long double x, int *flags)
{
    errno = 0;
    feclearexcept(FE_ALL_EXCEPT);
    long double r = root(x);
    *flags = fetestexcept(FE_ALL_EXCEPT);
    return r;
}

int main(void)
{
    int fl;
    long double r;

    /* The default word, a positive normal: the in-place `fsqrt`. sqrt(2) to 64 bits is
     * 1.0110101000001001111001100110011111110011101111001100100100001000|0101... (round down). */
    r = call(4.0L, &fl);
    CHECK(r == 2.0L && fl == 0 && errno == 0);
    r = call(2.0L, &fl);
    CHECK(mant(r) == 0xb504f333f9de6484ULL && se(r) == 0x3fff);
    CHECK(fl == FE_INEXACT && errno == 0);

    /* Upward: the in-place root follows the caller's direction. */
    CHECK(fesetround(FE_UPWARD) == 0);
    r = call(2.0L, &fl);
    CHECK(fesetround(FE_TONEAREST) == 0);
    CHECK(mant(r) == 0xb504f333f9de6485ULL && se(r) == 0x3fff && fl == FE_INEXACT);

    /* A negative normal is a domain error: NaN, invalid and EDOM (never the in-place path,
     * which would give the NaN and the flag but no errno). */
    r = call(-4.0L, &fl);
    CHECK(isnan(r) && fl == FE_INVALID && errno == EDOM);
    /* -0 is its own root, with no flag and no errno. */
    r = call(-0.0L, &fl);
    CHECK(r == 0.0L && signbit(r) && fl == 0 && errno == 0);

    /* 53-bit precision control is not the default word: the root keeps all 64 bits (the
     * software path, D-10.20); `fsqrt` in place would round to 53. The word is restored before
     * anything else touches the result. */
    fpu_control_t saved, pc53 = 0x027f;
    _FPU_GETCW(saved);
    _FPU_SETCW(pc53);
    errno = 0;
    long double v = root(2.0L);
    int e = errno;
    _FPU_SETCW(saved);
    CHECK(mant(v) == 0xb504f333f9de6484ULL && se(v) == 0x3fff && e == 0);

    if (failed)
        return 1;
    puts("s10_sqrtl_entry: ok");
    return 0;
}
