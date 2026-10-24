/* The complex forwarders of long double, _Float64x and _Float128 (spec §10.5, D-10.21): creal,
 * cimag, conj, cproj and cabs in every one of those formats, carg in long double and _Float64x.
 * First the ABI lines: the parts of a long double _Complex travel in memory and come back in
 * st(0) (real) and st(1) (imaginary); a _Float128 _Complex travels and comes back in memory. A
 * sentinel with distinct parts catches swapped parts, and the x87 tag word after a run of calls
 * catches a register left on the stack. Each prints "abi ok" or "abi BAD <what>". Then every
 * function on every pair of a grid of special and boundary parts (zeros, subnormals, ±1, the
 * largest finite, ±∞, NaNs quiet and signalling, and for long double the encodings the x87 does
 * not generate), and on random pairs, in every rounding direction. Each line prints the
 * operands' and results' encodings (20 hex digits for long double, 32 for _Float128), the raised
 * flags and errno. Built against the host C library and statically against Liella's products,
 * the outputs must agree (complex_ld_products.rs) except where Liella is correctly rounded and
 * the host is not. */
#define _GNU_SOURCE
#include <complex.h>
#include <errno.h>
#include <fenv.h>
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

static const int MODES[4] = {FE_TONEAREST, FE_UPWARD, FE_DOWNWARD, FE_TOWARDZERO};

static void report(int f, int e) {
    printf(" %c%c%c%c%c %s\n", f & FE_INVALID ? 'I' : '-', f & FE_DIVBYZERO ? 'Z' : '-', f & FE_OVERFLOW ? 'O' : '-',
           f & FE_UNDERFLOW ? 'U' : '-', f & FE_INEXACT ? 'X' : '-', e == 0 ? "0" : e == EDOM ? "EDOM" : e == ERANGE ? "ERANGE" : "E?");
}

static long double ld(uint16_t se, uint64_t m) {
    long double x = 0;
    memcpy(&x, &m, 8);
    memcpy((char *)&x + 8, &se, 2);
    return x;
}

static _Float128 q(uint64_t hi, uint64_t lo) {
    _Float128 x;
    memcpy(&x, &lo, 8);
    memcpy((char *)&x + 8, &hi, 8);
    return x;
}

static void hl(long double x) {
    uint64_t m;
    uint16_t se;
    memcpy(&m, &x, 8);
    memcpy(&se, (const char *)&x + 8, 2);
    printf(" %04x%016llx", se, (unsigned long long)m);
}

static void hq(_Float128 x) {
    uint64_t lo, hi;
    memcpy(&lo, &x, 8);
    memcpy(&hi, (const char *)&x + 8, 8);
    printf(" %016llx%016llx", (unsigned long long)hi, (unsigned long long)lo);
}

/* Bit equality (NaNs and signed zeros included). */
static int eql(long double a, long double b) { return memcmp(&a, &b, 10) == 0; }
static int eqq(_Float128 a, _Float128 b) { return memcmp(&a, &b, 16) == 0; }

static unsigned x87_tags(void) {
    unsigned short env[14];
    __asm__ volatile("fnstenv %0" : "=m"(env));
    __asm__ volatile("fldenv %0" : : "m"(env));
    return env[4];
}

typedef long double complex (*lcf)(long double complex);
typedef long double (*lrf)(long double complex);
typedef _Float128 complex (*qcf)(_Float128 complex);
typedef _Float128 (*qrf)(_Float128 complex);

static void abi(void) {
    const char *bad = NULL;
    volatile lcf conj_l = conjl;
    volatile lrf creal_l = creall, cimag_l = cimagl, cabs_l = cabsl;
    volatile qcf conj_q = conjf128;
    volatile qrf creal_q = crealf128, cimag_q = cimagf128, cabs_q = cabsf128;
    long double complex z = CMPLXL(1.25L, -3.5L);
    _Float128 complex w = CMPLXF128(1.25f128, -3.5f128);
    if (creal_l(z) != 1.25L) bad = "creall";
    else if (cimag_l(z) != -3.5L) bad = "cimagl";
    else if (creall(conj_l(z)) != 1.25L || cimagl(conj_l(z)) != 3.5L) bad = "conjl parts";
    else if (cabs_l(CMPLXL(3.0L, 4.0L)) != 5.0L) bad = "cabsl";
    else if (creal_q(w) != 1.25f128) bad = "crealf128";
    else if (cimag_q(w) != -3.5f128) bad = "cimagf128";
    else if (crealf128(conj_q(w)) != 1.25f128 || cimagf128(conj_q(w)) != 3.5f128) bad = "conjf128 parts";
    else if (cabs_q(CMPLXF128(3.0f128, 4.0f128)) != 5.0f128) bad = "cabsf128";
    if (bad == NULL) {
        /* Twenty returns through st(0)/st(1) with live x87 values around them: an unbalanced
         * stack would overflow (invalid, NaN) and leave tags set. */
        feclearexcept(FE_ALL_EXCEPT);
        long double sr = 0, si = 0;
        for (int i = 0; i < 20; i++) {
            long double complex r = conj_l(CMPLXL((long double)i, (long double)(2 * i)));
            sr += creall(r);
            si += cimagl(r);
            sr += cimag_l(CMPLXL(0.0L, 1.0L));
        }
        if (sr != 210.0L || si != -380.0L) bad = "x87 sums";
        else if (fetestexcept(FE_INVALID)) bad = "x87 invalid";
        else if (x87_tags() != 0xffff) bad = "x87 tags";
    }
    if (bad) printf("abi BAD %s\n", bad);
    else printf("abi ok\n");
}

static const struct { uint16_t se; uint64_t m; } LV[] = {
    {0x0000, 0}, {0x8000, 0}, {0x0000, 1}, {0x8000, 0x7fffffffffffffffull}, {0x0001, 1ull << 63},
    {0x3fff, 1ull << 63}, {0xbfff, 1ull << 63}, {0x3fff, 3ull << 62}, {0x4000, 3ull << 62}, {0xc001, 1ull << 63},
    {0x7ffe, ~0ull}, {0xfffe, ~0ull}, {0x7fff, 1ull << 63}, {0xffff, 1ull << 63}, {0x7fff, 3ull << 62},
    {0xffff, 3ull << 62}, {0x7fff, (1ull << 63) | 1}, {0xffff, (3ull << 62) | 0x1234},
    /* The encodings the x87 does not generate: a pseudo-denormal, an unnormal, a pseudo-infinity. */
    {0x0000, 1ull << 63}, {0x3fff, 1ull << 62}, {0x7fff, 0},
};
#define NL (sizeof LV / sizeof LV[0])

static const struct { uint64_t hi, lo; } QV[] = {
    {0, 0}, {1ull << 63, 0}, {0, 1}, {0x8000ffffffffffffull, ~0ull}, {0x0001000000000000ull, 0},
    {0x3fff000000000000ull, 0}, {0xbfff000000000000ull, 0}, {0x3fff800000000000ull, 0}, {0x4000800000000000ull, 0},
    {0xc001000000000000ull, 0}, {0x7ffeffffffffffffull, ~0ull}, {0xfffeffffffffffffull, ~0ull},
    {0x7fff000000000000ull, 0}, {0xffff000000000000ull, 0}, {0x7fff800000000000ull, 0}, {0xffff800000000000ull, 0},
    {0x7fff000000000000ull, 1}, {0xffff400000000000ull, 0x1234},
};
#define NQ (sizeof QV / sizeof QV[0])

#define FL(n) {#n, n}
static const struct { const char *name; lcf f; } LC[] = {FL(conjl), FL(cprojl), FL(conjf64x), FL(cprojf64x)};
static const struct { const char *name; lrf f; } LR[] = {FL(creall), FL(cimagl), FL(cabsl), FL(cargl),
                                                         FL(crealf64x), FL(cimagf64x), FL(cabsf64x), FL(cargf64x)};
static const struct { const char *name; qcf f; } QC[] = {FL(conjf128), FL(cprojf128)};
static const struct { const char *name; qrf f; } QR[] = {FL(crealf128), FL(cimagf128), FL(cabsf128)};
#define N(a) (sizeof a / sizeof a[0])

static void run_l(long double re, long double im, char mc) {
    long double complex z;
    __real__ z = re;
    __imag__ z = im;
    for (size_t i = 0; i < N(LC); i++) {
        feclearexcept(FE_ALL_EXCEPT);
        errno = 0;
        long double complex r = LC[i].f(z);
        int f = fetestexcept(FE_ALL_EXCEPT), e = errno;
        printf("%s %c", LC[i].name, mc);
        hl(re), hl(im);
        printf(" =");
        hl(__real__ r), hl(__imag__ r);
        report(f, e);
    }
    for (size_t i = 0; i < N(LR); i++) {
        feclearexcept(FE_ALL_EXCEPT);
        errno = 0;
        long double r = LR[i].f(z);
        int f = fetestexcept(FE_ALL_EXCEPT), e = errno;
        printf("%s %c", LR[i].name, mc);
        hl(re), hl(im);
        printf(" =");
        hl(r);
        report(f, e);
    }
}

static void run_q(_Float128 re, _Float128 im, char mc) {
    _Float128 complex z;
    __real__ z = re;
    __imag__ z = im;
    for (size_t i = 0; i < N(QC); i++) {
        feclearexcept(FE_ALL_EXCEPT);
        errno = 0;
        _Float128 complex r = QC[i].f(z);
        int f = fetestexcept(FE_ALL_EXCEPT), e = errno;
        printf("%s %c", QC[i].name, mc);
        hq(re), hq(im);
        printf(" =");
        hq(__real__ r), hq(__imag__ r);
        report(f, e);
    }
    for (size_t i = 0; i < N(QR); i++) {
        feclearexcept(FE_ALL_EXCEPT);
        errno = 0;
        _Float128 r = QR[i].f(z);
        int f = fetestexcept(FE_ALL_EXCEPT), e = errno;
        printf("%s %c", QR[i].name, mc);
        hq(re), hq(im);
        printf(" =");
        hq(r);
        report(f, e);
    }
}

static uint64_t rs = 0x9e3779b97f4a7c15u;
static uint64_t rnd(void) { rs ^= rs << 13; rs ^= rs >> 7; rs ^= rs << 17; return rs; }

/* A random finite part: the whole exponent range, or near the other part's exponent. */
static long double rl(int near, uint16_t e0) {
    uint64_t r = rnd();
    uint16_t e = near ? (uint16_t)((e0 & 0x7fff) + (r % 129) - 64) : (uint16_t)(r % 0x7fff);
    if (e >= 0x7fff) e = 0x7ffe;
    uint64_t m = rnd();
    m = e == 0 ? m >> 1 : m | (1ull << 63);
    return ld((uint16_t)(e | (rnd() & 1 ? 0x8000 : 0)), m);
}

static _Float128 rq(int near, uint64_t e0) {
    uint64_t r = rnd();
    uint64_t e = near ? ((e0 & 0x7fff) + (r % 229) - 114) : r % 0x7fff;
    if (e >= 0x7fff) e = 0x7ffe;
    uint64_t hi = (rnd() & 0xffffffffffffull) | (e << 48) | (rnd() & 1 ? 1ull << 63 : 0);
    return q(hi, rnd());
}

int main(void) {
    abi();
    for (int mi = 0; mi < 4; mi++) {
        char mc = "NUDZ"[mi];
        fesetround(MODES[mi]);
        for (size_t i = 0; i < NL; i++)
            for (size_t j = 0; j < NL; j++) run_l(ld(LV[i].se, LV[i].m), ld(LV[j].se, LV[j].m), mc);
        for (size_t i = 0; i < NQ; i++)
            for (size_t j = 0; j < NQ; j++) run_q(q(QV[i].hi, QV[i].lo), q(QV[j].hi, QV[j].lo), mc);
        rs = 0x9e3779b97f4a7c15u;
        for (int k = 0; k < 400; k++) {
            long double a = rl(0, 0);
            uint16_t ea;
            memcpy(&ea, (const char *)&a + 8, 2);
            run_l(a, rl(k & 1, ea), mc);
            _Float128 b = rq(0, 0);
            uint64_t eb;
            memcpy(&eb, (const char *)&b + 8, 8);
            run_q(b, rq(k & 1, eb >> 48), mc);
        }
        fesetround(FE_TONEAREST);
    }
    /* Sanity: the results above must not depend on a mode left behind. */
    if (!eql(creall(CMPLXL(1.0L, 2.0L)), 1.0L) || !eqq(cimagf128(CMPLXF128(1.0f128, 2.0f128)), 2.0f128)) printf("abi BAD final\n");
    return 0;
}
