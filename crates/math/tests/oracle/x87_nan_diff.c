/* Which NaN the two-operand long double functions return (x87_nan_products.rs): every ordered
 * pair of a set of NaNs (both signs, quiet and signalling, small and large payloads, equal
 * payloads), invalid x87 encodings (pseudo-NaN, pseudo-infinity, unnormal), a pseudo-denormal
 * and a few numbers, through each function. One line per call: function, operand indices, the
 * result's bits, and whether invalid was raised. Built against the host C library and against
 * Liella's products; the two outputs must be identical. */
#define _GNU_SOURCE 1
#include <complex.h>
#include <fenv.h>
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

typedef union { long double v; struct { uint64_t m; uint16_t se; } b; } ld;

static const struct { uint16_t se; uint64_t m; } ops[] = {
    {0x7fff, 0xc000000000000001ull}, /*  0 +qNaN small */
    {0x7fff, 0xc000000000001000ull}, /*  1 +qNaN large */
    {0xffff, 0xc000000000000001ull}, /*  2 -qNaN small (equal payload to 0) */
    {0xffff, 0xc000000000001000ull}, /*  3 -qNaN large (equal payload to 1) */
    {0x7fff, 0x8000000000000005ull}, /*  4 +sNaN small */
    {0x7fff, 0x8000000000002000ull}, /*  5 +sNaN large */
    {0xffff, 0x8000000000000005ull}, /*  6 -sNaN small (equal payload to 4) */
    {0xffff, 0xc000000000000000ull}, /*  7 -qNaN default */
    {0x7fff, 0x4000000000000001ull}, /*  8 pseudo-NaN (invalid) */
    {0x7fff, 0x0000000000000000ull}, /*  9 pseudo-infinity (invalid) */
    {0x4000, 0x4000000000000000ull}, /* 10 unnormal (invalid) */
    {0x0000, 0x8000000000000001ull}, /* 11 pseudo-denormal */
    {0x3fff, 0xc000000000000000ull}, /* 12 1.5 */
    {0x7fff, 0x8000000000000000ull}, /* 13 +inf */
    {0x8000, 0x0000000000000000ull}, /* 14 -0 */
};
#define N (sizeof ops / sizeof ops[0])

static long double val(unsigned i) {
    volatile ld u;
    memset((void *)&u, 0, sizeof u);
    u.b.m = ops[i].m;
    u.b.se = ops[i].se;
    return u.v;
}

static void pld(long double r) {
    ld u;
    memset(&u, 0, sizeof u);
    u.v = r;
    printf(" %04x%016llx", u.b.se, (unsigned long long)u.b.m);
}

static void end(void) { printf(" %c\n", fetestexcept(FE_INVALID) ? 'i' : '-'); }

typedef long double (*f2)(long double, long double);
typedef float (*n2f)(long double, long double);
typedef double (*n2d)(long double, long double);

static long double remquo_(long double a, long double b) { int q; return remquol(a, b, &q); }

int main(void) {
    static const struct { const char *name; f2 f; } fs[] = {
        {"fmodl", fmodl}, {"remainderl", remainderl}, {"dreml", dreml}, {"remquol", remquo_},
        {"powl", powl}, {"copysignl", copysignl}, {"fdiml", fdiml}, {"fmaxl", fmaxl}, {"fminl", fminl},
        {"fmaxmagl", fmaxmagl}, {"fminmagl", fminmagl}, {"fmaximuml", fmaximuml}, {"fminimuml", fminimuml},
        {"fmaximum_numl", fmaximum_numl}, {"fminimum_numl", fminimum_numl}, {"fmaximum_magl", fmaximum_magl},
        {"fminimum_magl", fminimum_magl}, {"fmaximum_mag_numl", fmaximum_mag_numl},
        {"fminimum_mag_numl", fminimum_mag_numl}, {"nextafterl", nextafterl}, {"nexttowardl", nexttowardl},
        {"scalbl", scalbl}, {"hypotl", hypotl}, {"atan2l", atan2l},
    };
    static const struct { const char *name; n2f f; } nf[] = {
        {"faddl", faddl}, {"fsubl", fsubl}, {"fmull", fmull}, {"fdivl", fdivl},
    };
    static const struct { const char *name; n2d f; } nd[] = {
        {"daddl", daddl}, {"dsubl", dsubl}, {"dmull", dmull}, {"ddivl", ddivl},
    };
    for (unsigned a = 0; a < N; a++)
        for (unsigned b = 0; b < N; b++) {
            long double x = val(a), y = val(b);
            for (unsigned k = 0; k < sizeof fs / sizeof fs[0]; k++) {
                feclearexcept(FE_ALL_EXCEPT);
                long double r = fs[k].f(x, y);
                printf("%s %u %u =", fs[k].name, a, b);
                pld(r);
                end();
            }
            for (unsigned k = 0; k < sizeof nf / sizeof nf[0]; k++) {
                feclearexcept(FE_ALL_EXCEPT);
                float r = nf[k].f(x, y);
                uint32_t u;
                memcpy(&u, &r, sizeof u);
                printf("%s %u %u = %08x", nf[k].name, a, b, (unsigned)u);
                end();
            }
            /* The complex long double functions (X87-NAN-RULE-AUDIT-2): z = a + bi; cpowl
             * raises z to b + ai. */
            long double _Complex z = CMPLXL(x, y), w = CMPLXL(y, x);
            static const struct { const char *name; long double _Complex (*f)(long double _Complex); } cf[] = {
                {"cexpl", cexpl}, {"clogl", clogl}, {"clog10l", clog10l}, {"csqrtl", csqrtl},
            };
            for (unsigned k = 0; k < sizeof cf / sizeof cf[0]; k++) {
                feclearexcept(FE_ALL_EXCEPT);
                long double _Complex r = cf[k].f(z);
                printf("%s %u %u =", cf[k].name, a, b);
                pld(creall(r));
                pld(cimagl(r));
                end();
            }
            feclearexcept(FE_ALL_EXCEPT);
            long double _Complex p = cpowl(z, w);
            printf("cpowl %u %u =", a, b);
            pld(creall(p));
            pld(cimagl(p));
            end();
            feclearexcept(FE_ALL_EXCEPT);
            long double s = cabsl(z);
            printf("cabsl %u %u =", a, b);
            pld(s);
            end();
            feclearexcept(FE_ALL_EXCEPT);
            s = cargl(z);
            printf("cargl %u %u =", a, b);
            pld(s);
            end();
            for (unsigned k = 0; k < sizeof nd / sizeof nd[0]; k++) {
                feclearexcept(FE_ALL_EXCEPT);
                double r = nd[k].f(x, y);
                uint64_t u;
                memcpy(&u, &r, sizeof u);
                printf("%s %u %u = %016llx", nd[k].name, a, b, (unsigned long long)u);
                end();
            }
        }
    return 0;
}
