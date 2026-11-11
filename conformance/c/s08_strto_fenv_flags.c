/* §8.2, §8.3, §12.7 (C Annex F.5; STRTO-FENV-FLAGS, D-8.12): every float conversion raises the
 * IEEE exceptions its correctly rounded result implies - inexact when it is not exact, overflow
 * when it rounds beyond the largest finite value, underflow when it is tiny and inexact - and
 * nothing for an exact result; errno is ERANGE exactly with overflow or underflow.
 *
 * The expected table is the reference's, settled by the black-box probe
 * crates/core/tests/oracle/strto_fenv_flags.c against glibc 2.39 (record:
 * crates/boundary/tests/data/strto_fenv_flags.txt; every function of one format agreed there).
 * Codes: I inexact, O overflow, U underflow, '-' none, then E for ERANGE; columns
 * FE_TONEAREST, FE_DOWNWARD, FE_UPWARD, FE_TOWARDZERO. */
/* LINK-LIBM: yes */
#define _GNU_SOURCE
#include <errno.h>
#include <fenv.h>
#include <locale.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <wchar.h>

struct row {
    const char *in;
    const char *want[3][4]; /* float, double, long double */
};

static const struct row rows[] = {
    {"0", {{"-", "-", "-", "-"}, {"-", "-", "-", "-"}, {"-", "-", "-", "-"}}},
    {"-0", {{"-", "-", "-", "-"}, {"-", "-", "-", "-"}, {"-", "-", "-", "-"}}},
    {"1", {{"-", "-", "-", "-"}, {"-", "-", "-", "-"}, {"-", "-", "-", "-"}}},
    {"0.5", {{"-", "-", "-", "-"}, {"-", "-", "-", "-"}, {"-", "-", "-", "-"}}},
    {"0.1", {{"I", "I", "I", "I"}, {"I", "I", "I", "I"}, {"I", "I", "I", "I"}}},
    {"-0.1", {{"I", "I", "I", "I"}, {"I", "I", "I", "I"}, {"I", "I", "I", "I"}}},
    {"1e22", {{"I", "I", "I", "I"}, {"-", "-", "-", "-"}, {"-", "-", "-", "-"}}},
    {"1e23", {{"I", "I", "I", "I"}, {"I", "I", "I", "I"}, {"-", "-", "-", "-"}}},
    {"123456789012345678901234567890", {{"I", "I", "I", "I"}, {"I", "I", "I", "I"}, {"I", "I", "I", "I"}}},
    {"1.00000000000000000000000000", {{"-", "-", "-", "-"}, {"-", "-", "-", "-"}, {"-", "-", "-", "-"}}},
    {"9007199254740993", {{"I", "I", "I", "I"}, {"I", "I", "I", "I"}, {"-", "-", "-", "-"}}},
    {"18014398509481984", {{"-", "-", "-", "-"}, {"-", "-", "-", "-"}, {"-", "-", "-", "-"}}},
    {"0x1.8p1", {{"-", "-", "-", "-"}, {"-", "-", "-", "-"}, {"-", "-", "-", "-"}}},
    {"0x1.00000000000001p0", {{"I", "I", "I", "I"}, {"I", "I", "I", "I"}, {"-", "-", "-", "-"}}},
    {"1e38", {{"I", "I", "I", "I"}, {"I", "I", "I", "I"}, {"I", "I", "I", "I"}}},
    {"3.4028235e38", {{"I", "I", "IOE", "I"}, {"I", "I", "I", "I"}, {"I", "I", "I", "I"}}},
    {"3.40282357e38", {{"IOE", "I", "IOE", "I"}, {"I", "I", "I", "I"}, {"I", "I", "I", "I"}}},
    {"1e308", {{"IOE", "IOE", "IOE", "IOE"}, {"I", "I", "I", "I"}, {"I", "I", "I", "I"}}},
    {"1.7976931348623157e308", {{"IOE", "IOE", "IOE", "IOE"}, {"I", "I", "I", "I"}, {"I", "I", "I", "I"}}},
    {"1.7976931348623158e308", {{"IOE", "IOE", "IOE", "IOE"}, {"I", "I", "IOE", "I"}, {"I", "I", "I", "I"}}},
    {"1.797693134862315808e308", {{"IOE", "IOE", "IOE", "IOE"}, {"IOE", "I", "IOE", "I"}, {"I", "I", "I", "I"}}},
    {"0x1.fffffffffffff8p1023", {{"IOE", "IOE", "IOE", "IOE"}, {"IOE", "I", "IOE", "I"}, {"-", "-", "-", "-"}}},
    {"1e400", {{"IOE", "IOE", "IOE", "IOE"}, {"IOE", "IOE", "IOE", "IOE"}, {"I", "I", "I", "I"}}},
    {"-1e400", {{"IOE", "IOE", "IOE", "IOE"}, {"IOE", "IOE", "IOE", "IOE"}, {"I", "I", "I", "I"}}},
    {"1e5000", {{"IOE", "IOE", "IOE", "IOE"}, {"IOE", "IOE", "IOE", "IOE"}, {"IOE", "IOE", "IOE", "IOE"}}},
    {"1e-30", {{"I", "I", "I", "I"}, {"I", "I", "I", "I"}, {"I", "I", "I", "I"}}},
    {"1e-40", {{"IUE", "IUE", "IUE", "IUE"}, {"I", "I", "I", "I"}, {"I", "I", "I", "I"}}},
    {"1e-310", {{"IUE", "IUE", "IUE", "IUE"}, {"IUE", "IUE", "IUE", "IUE"}, {"I", "I", "I", "I"}}},
    {"4.9406564584124654e-324", {{"IUE", "IUE", "IUE", "IUE"}, {"IUE", "IUE", "IUE", "IUE"}, {"I", "I", "I", "I"}}},
    {"2.4703282292062328e-324", {{"IUE", "IUE", "IUE", "IUE"}, {"IUE", "IUE", "IUE", "IUE"}, {"I", "I", "I", "I"}}},
    {"1e-400", {{"IUE", "IUE", "IUE", "IUE"}, {"IUE", "IUE", "IUE", "IUE"}, {"I", "I", "I", "I"}}},
    {"-1e-400", {{"IUE", "IUE", "IUE", "IUE"}, {"IUE", "IUE", "IUE", "IUE"}, {"I", "I", "I", "I"}}},
    {"0x1p-1074", {{"IUE", "IUE", "IUE", "IUE"}, {"-", "-", "-", "-"}, {"-", "-", "-", "-"}}},
    {"0x1.8p-1074", {{"IUE", "IUE", "IUE", "IUE"}, {"IUE", "IUE", "IUE", "IUE"}, {"-", "-", "-", "-"}}},
    {"0x1p-149", {{"-", "-", "-", "-"}, {"-", "-", "-", "-"}, {"-", "-", "-", "-"}}},
    {"0x1.fffffffffffffp-1023", {{"IUE", "IUE", "IUE", "IUE"}, {"IUE", "IUE", "IUE", "IUE"}, {"-", "-", "-", "-"}}},
    {"2.2250738585072011e-308", {{"IUE", "IUE", "IUE", "IUE"}, {"IUE", "IUE", "IUE", "IUE"}, {"I", "I", "I", "I"}}},
    {"2.2250738585072014e-308", {{"IUE", "IUE", "IUE", "IUE"}, {"I", "I", "I", "I"}, {"I", "I", "I", "I"}}},
    {"1e-5000", {{"IUE", "IUE", "IUE", "IUE"}, {"IUE", "IUE", "IUE", "IUE"}, {"IUE", "IUE", "IUE", "IUE"}}},
    {"inf", {{"-", "-", "-", "-"}, {"-", "-", "-", "-"}, {"-", "-", "-", "-"}}},
    {"nan", {{"-", "-", "-", "-"}, {"-", "-", "-", "-"}, {"-", "-", "-", "-"}}},
    {"0x1p-16445", {{"IUE", "IUE", "IUE", "IUE"}, {"IUE", "IUE", "IUE", "IUE"}, {"-", "-", "-", "-"}}},
    {"1.18973149535723176502e+4932", {{"IOE", "IOE", "IOE", "IOE"}, {"IOE", "IOE", "IOE", "IOE"}, {"I", "I", "I", "I"}}},
};

static const int modes[4] = {FE_TONEAREST, FE_DOWNWARD, FE_UPWARD, FE_TOWARDZERO};
static const char *const mode_names[4] = {"nearest", "down", "up", "zero"};

static volatile double sink_d;
static volatile float sink_f;
static volatile long double sink_ld;

/* fn: 0 strtof 1 wcstof 2 sscanf %f | 3 strtod 4 wcstod 5 strtod_l 6 atof 7 sscanf %lf
 *     | 8 strtold 9 wcstold 10 sscanf %Lf */
static const char *const names[] = {"strtof", "wcstof", "sscanf %f", "strtod", "wcstod", "strtod_l", "atof", "sscanf %lf", "strtold", "wcstold", "sscanf %Lf"};
static const int format_of[] = {0, 0, 0, 1, 1, 1, 1, 1, 2, 2, 2};

static void code_of(char *code, int fl, int e, int with_errno) {
    int k = 0;
    if (fl & FE_INEXACT) code[k++] = 'I';
    if (fl & FE_OVERFLOW) code[k++] = 'O';
    if (fl & FE_UNDERFLOW) code[k++] = 'U';
    if (fl & FE_INVALID) code[k++] = 'V';
    if (fl & FE_DIVBYZERO) code[k++] = 'Z';
    if (k == 0) code[k++] = '-';
    if (with_errno && e == ERANGE) code[k++] = 'E';
    code[k] = 0;
}

int main(void) {
    int bad = 0;
    locale_t c = newlocale(LC_ALL_MASK, "C", (locale_t)0);
    for (size_t i = 0; i < sizeof rows / sizeof *rows; i++) {
        const char *in = rows[i].in;
        wchar_t w[128];
        mbstowcs(w, in, 128);
        for (int fn = 0; fn < 11; fn++) {
            for (int m = 0; m < 4; m++) {
                fesetround(modes[m]);
                errno = 0;
                feclearexcept(FE_ALL_EXCEPT);
                switch (fn) {
                case 0: sink_f = strtof(in, NULL); break;
                case 1: sink_f = wcstof(w, NULL); break;
                case 2: { float f; sscanf(in, "%f", &f); sink_f = f; } break;
                case 3: sink_d = strtod(in, NULL); break;
                case 4: sink_d = wcstod(w, NULL); break;
                case 5: sink_d = strtod_l(in, NULL, c); break;
                case 6: sink_d = atof(in); break;
                case 7: { double d; sscanf(in, "%lf", &d); sink_d = d; } break;
                case 8: sink_ld = strtold(in, NULL); break;
                case 9: sink_ld = wcstold(w, NULL); break;
                case 10: { long double l; sscanf(in, "%Lf", &l); sink_ld = l; } break;
                }
                int fl = fetestexcept(FE_ALL_EXCEPT);
                int e = errno;
                fesetround(FE_TONEAREST);
                char got[8];
                code_of(got, fl, e, 1);
                if (strcmp(got, rows[i].want[format_of[fn]][m]) != 0) {
                    printf("FAIL flags %s(\"%s\") %s: got %s want %s\n", names[fn], in, mode_names[m], got, rows[i].want[format_of[fn]][m]);
                    bad++;
                }
            }
        }
    }
    freelocale(c);
    if (bad) {
        printf("FAIL %d flag mismatches\n", bad);
        return 1;
    }
    printf("PASS strto_fenv_flags\n");
    return 0;
}
