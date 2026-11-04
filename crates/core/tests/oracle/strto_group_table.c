/* The `group` argument of all fifteen compat `__strto*_internal` / `__wcsto*_internal` entry
 * points (spec §8.1-§8.3; STRTO-INTERNAL-GROUP-3). Black-box probe: built against the host C
 * library and run with LOCPATH naming a directory of the test's OWN LC_NUMERIC-only locales
 * (crates/boundary/tests/strto_internal_group.rs writes their sources and compiles them with
 * `localedef -c`): grp3 (`,` `.` 3;3), grp32 (3;2), grpstop (3;CHAR_MAX), grpde (`.` `,` 3;3),
 * grpsame (thousands_sep == decimal_point == `.`). One line per locale, flag and input, in the
 * exact form the test prints for Liella; the record is in that test's RECORD. */
#define _GNU_SOURCE
#include <errno.h>
#include <locale.h>
#include <stdio.h>
#include <string.h>
#include <wchar.h>

extern long __strtol_internal(const char *, char **, int, int);
extern long long __strtoll_internal(const char *, char **, int, int);
extern unsigned long __strtoul_internal(const char *, char **, int, int);
extern unsigned long long __strtoull_internal(const char *, char **, int, int);
extern double __strtod_internal(const char *, char **, int);
extern float __strtof_internal(const char *, char **, int);
extern long double __strtold_internal(const char *, char **, int);
extern long __wcstol_internal(const wchar_t *, wchar_t **, int, int);
extern long long __wcstoll_internal(const wchar_t *, wchar_t **, int, int);
extern unsigned long __wcstoul_internal(const wchar_t *, wchar_t **, int, int);
extern unsigned long long __wcstoull_internal(const wchar_t *, wchar_t **, int, int);
extern double __wcstod_internal(const wchar_t *, wchar_t **, int);
extern float __wcstof_internal(const wchar_t *, wchar_t **, int);
extern long double __wcstold_internal(const wchar_t *, wchar_t **, int);

static const char *in[] = {
    "1,234,567", "1,23,456", ",123", "1234,567", "12,345x", "1,234,", "-1,234", "1,2345", "123,456,78",
    "1,234.567", "1.234.567", "1.23.456", "01,234", "  +12,345", "1,,234", "999,999,999,999,999,999,999",
    "1,234.5", "1,23,456.5", "1,234,567e3", "1234,567.5", "1,234.5,6", "1.234,5", "1.23.456,5",
    "-12,345.25e-1", "1,2345.5", "12,34,567", "1,234.5.6", "1.234.5", "1..5", "12345,678", NULL };

static void wide(const char *s, wchar_t *w) { size_t i = 0; for (; s[i]; i++) w[i] = (unsigned char)s[i]; w[i] = 0; }
static void ldbits(long double v, unsigned long long *m, unsigned *se) { unsigned char b[16] = {0}; memcpy(b, &v, 10); memcpy(m, b, 8); *se = b[8] | (b[9] << 8); }

int main(void) {
    const char *locs[] = { "C", "grp3", "grp32", "grpstop", "grpde", "grpsame", NULL };
    for (const char **l = locs; *l; l++) {
        if (!setlocale(LC_NUMERIC, *l)) { printf("no locale %s\n", *l); continue; }
        for (int g = 0; g < 2; g++) {
            for (const char **p = in; *p; p++) {
                const char *s = *p; char *e; wchar_t w[64], *we; unsigned long long m, wm; unsigned se, wse;
                wide(s, w);
                errno = 0; long l10 = __strtol_internal(s, &e, 10, g); int el = errno; long ol = e - s;
                long l0 = __strtol_internal(s, &e, 0, g); long ol0 = e - s;
                long long ll = __strtoll_internal(s, &e, 10, g); long oll = e - s;
                errno = 0; unsigned long ul = __strtoul_internal(s, &e, 10, g); int eul = errno; long oul = e - s;
                unsigned long long ull = __strtoull_internal(s, &e, 10, g); long oull = e - s;
                long wl = __wcstol_internal(w, &we, 10, g); long owl = we - w;
                long long wll = __wcstoll_internal(w, &we, 10, g); long owll = we - w;
                unsigned long wul = __wcstoul_internal(w, &we, 10, g); long owul = we - w;
                unsigned long long wull = __wcstoull_internal(w, &we, 10, g); long owull = we - w;
                errno = 0; double d = __strtod_internal(s, &e, g); int ed = errno; long od = e - s;
                float f = __strtof_internal(s, &e, g); long of = e - s;
                ldbits(__strtold_internal(s, &e, g), &m, &se); long old = e - s;
                double wd = __wcstod_internal(w, &we, g); long owd = we - w;
                float wf = __wcstof_internal(w, &we, g); long owf = we - w;
                ldbits(__wcstold_internal(w, &we, g), &wm, &wse); long owld = we - w;
                unsigned long long db, wdb; unsigned fb, wfb;
                memcpy(&db, &d, 8); memcpy(&wdb, &wd, 8); memcpy(&fb, &f, 4); memcpy(&wfb, &wf, 4);
                printf("%s g=%d [%s] l=%ld/%ld/%d l0=%ld/%ld ll=%lld/%ld ul=%lu/%ld/%d ull=%llu/%ld wl=%ld/%ld wll=%lld/%ld"
                       " wul=%lu/%ld wull=%llu/%ld d=%016llx/%ld/%d f=%08x/%ld ld=%016llx:%04x/%ld wd=%016llx/%ld"
                       " wf=%08x/%ld wld=%016llx:%04x/%ld\n",
                       *l, g, s, l10, ol, el, l0, ol0, ll, oll, ul, oul, eul, ull, oull, wl, owl, wll, owll, wul, owul,
                       wull, owull, db, od, ed, fb, of, m, se, old, wdb, owd, wfb, owf, wm, wse, owld);
            }
        }
    }
    return 0;
}
