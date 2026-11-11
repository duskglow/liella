/* Black-box probe (STRTO-FENV-FLAGS, ruling #4639): the floating-point exception flags and errno
 * each strto* / wcsto* / atof / scanf float conversion leaves, per input and rounding mode.
 * Build and run against the reference C library:
 *   cc -O1 -o probe strto_fenv_flags.c -lm && ./probe
 * One line per (input, function): input TAB function TAB four codes (FE_TONEAREST, FE_DOWNWARD,
 * FE_UPWARD, FE_TOWARDZERO), each the raised flags as letters (I inexact, O overflow,
 * U underflow, V invalid, Z divide-by-zero; '-' for none) and 'E' when errno is ERANGE.
 * Record: crates/boundary/tests/data/strto_fenv_flags.txt. */
#define _GNU_SOURCE
#include <errno.h>
#include <fenv.h>
#include <locale.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <wchar.h>

static const char *const inputs[] = {
    "0", "-0", "1", "0.5", "0.1", "-0.1", "1e22", "1e23", "123456789012345678901234567890",
    "1.00000000000000000000000000", "9007199254740993", "18014398509481984", "0x1.8p1",
    "0x1.00000000000001p0", "1e38", "3.4028235e38", "3.40282357e38", "1e308",
    "1.7976931348623157e308", "1.7976931348623158e308", "1.797693134862315808e308",
    "0x1.fffffffffffff8p1023", "1e400", "-1e400", "1e5000", "1e-30", "1e-40", "1e-310",
    "4.9406564584124654e-324", "2.4703282292062328e-324", "1e-400", "-1e-400", "0x1p-1074",
    "0x1.8p-1074", "0x1p-149", "0x1.fffffffffffffp-1023", "2.2250738585072011e-308",
    "2.2250738585072014e-308", "1e-5000", "inf", "nan", "0x1p-16445",
    "1.18973149535723176502e+4932",
};

static const int modes[4] = {FE_TONEAREST, FE_DOWNWARD, FE_UPWARD, FE_TOWARDZERO};

static volatile double sink_d;
static volatile float sink_f;
static volatile long double sink_ld;

static void run(const char *in, int fn) {
    wchar_t w[128];
    mbstowcs(w, in, 128);
    locale_t c = newlocale(LC_ALL_MASK, "C", (locale_t)0);
    for (int m = 0; m < 4; m++) {
        fesetround(modes[m]);
        errno = 0;
        feclearexcept(FE_ALL_EXCEPT);
        switch (fn) {
        case 0: sink_d = strtod(in, NULL); break;
        case 1: sink_f = strtof(in, NULL); break;
        case 2: sink_ld = strtold(in, NULL); break;
        case 3: sink_d = wcstod(w, NULL); break;
        case 4: sink_f = wcstof(w, NULL); break;
        case 5: sink_ld = wcstold(w, NULL); break;
        case 6: sink_d = strtod_l(in, NULL, c); break;
        case 7: sink_d = atof(in); break;
        case 8: { double d; sscanf(in, "%lf", &d); sink_d = d; } break;
        case 9: { float f; sscanf(in, "%f", &f); sink_f = f; } break;
        case 10: { long double l; sscanf(in, "%Lf", &l); sink_ld = l; } break;
        }
        int fl = fetestexcept(FE_ALL_EXCEPT);
        int e = errno;
        fesetround(FE_TONEAREST);
        char code[8];
        int k = 0;
        if (fl & FE_INEXACT) code[k++] = 'I';
        if (fl & FE_OVERFLOW) code[k++] = 'O';
        if (fl & FE_UNDERFLOW) code[k++] = 'U';
        if (fl & FE_INVALID) code[k++] = 'V';
        if (fl & FE_DIVBYZERO) code[k++] = 'Z';
        if (k == 0) code[k++] = '-';
        if (e == ERANGE) code[k++] = 'E';
        code[k] = 0;
        printf("%s%s", m ? " " : "", code);
    }
    freelocale(c);
}

int main(void) {
    static const char *const names[] = {"strtod", "strtof", "strtold", "wcstod", "wcstof", "wcstold", "strtod_l", "atof", "scanf_lf", "scanf_f", "scanf_Lf"};
    for (size_t i = 0; i < sizeof inputs / sizeof *inputs; i++) {
        for (int fn = 0; fn < 11; fn++) {
            printf("%s\t%s\t", inputs[i], names[fn]);
            run(inputs[i], fn);
            printf("\n");
        }
    }
    return 0;
}
