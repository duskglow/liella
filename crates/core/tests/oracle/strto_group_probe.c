/* The `group` argument of the compat `__strto*_internal` / `__wcsto*_internal` entry points
 * (spec §8.1-§8.3; STRTO-INTERNAL-GROUP). Black-box probe: built against the host C library and
 * run with LOCPATH naming a private directory holding en_US.UTF-8 and de_DE.UTF-8 made with
 * localedef. Each line: locale, form, group flag, input, value, and the end offset. The record
 * this settled is in crates/boundary/tests/strto_internal_group.rs. */
#define _GNU_SOURCE
#include <locale.h>
#include <stdio.h>
#include <string.h>
#include <wchar.h>
#include <errno.h>

extern long __strtol_internal(const char *, char **, int, int);
extern unsigned long __strtoul_internal(const char *, char **, int, int);
extern double __strtod_internal(const char *, char **, int);
extern float __strtof_internal(const char *, char **, int);
extern long double __strtold_internal(const char *, char **, int);
extern long __wcstol_internal(const wchar_t *, wchar_t **, int, int);
extern double __wcstod_internal(const wchar_t *, wchar_t **, int);

static const char *ints[] = {
    "1,234,567", "1,23,456", ",123", "1234,567", "12,345x", "1,234,", "-1,234", "1,2345",
    "123,456,78", "1,234.567", "1.234.567", "1.23.456", "0x1,234", "01,234", "  +12,345", "1,,234",
    "999,999,999,999,999,999,999", NULL };
static const char *floats[] = {
    "1,234.5", "1,23,456.5", ",5", "1,234,567e3", "1234,567.5", "1,234.5,6", "1.234,5", "1.23.456,5",
    "1,234", "-12,345.25e-1", "0x1,2p0", "1,2345.5", NULL };

static void wide(const char *s, wchar_t *w) { size_t i = 0; for (; s[i]; i++) w[i] = (unsigned char)s[i]; w[i] = 0; }

int main(void) {
    const char *locs[] = { "C", "en_US.UTF-8", "de_DE.UTF-8", NULL };
    for (const char **l = locs; *l; l++) {
        if (!setlocale(LC_ALL, *l)) { printf("== no locale %s\n", *l); continue; }
        printf("== %s\n", *l);
        for (int g = 0; g < 2; g++) {
            for (const char **s = ints; *s; s++) {
                char *e; wchar_t w[64], *we;
                errno = 0; long v = __strtol_internal(*s, &e, 0, g); int er = errno;
                unsigned long u = __strtoul_internal(*s, &e, 10, g); long ue = e - *s;
                e = NULL; v = __strtol_internal(*s, &e, 0, g);
                wide(*s, w); long wv = __wcstol_internal(w, &we, 0, g);
                printf("int g=%d [%s] l=%ld/%ld errno=%d ul10=%lu/%ld wl=%ld/%ld\n", g, *s, v, (long)(e - *s), er, u, ue, wv, (long)(we - w));
            }
            for (const char **s = floats; *s; s++) {
                char *e1, *e2, *e3; wchar_t w[64], *we;
                double d = __strtod_internal(*s, &e1, g);
                float f = __strtof_internal(*s, &e2, g);
                long double ld = __strtold_internal(*s, &e3, g);
                wide(*s, w); double wd = __wcstod_internal(w, &we, g);
                printf("flt g=%d [%s] d=%a/%ld f=%a/%ld ld=%La/%ld wd=%a/%ld\n", g, *s, d, (long)(e1 - *s), (double)f, (long)(e2 - *s), ld, (long)(e3 - *s), wd, (long)(we - w));
            }
        }
    }
    return 0;
}
