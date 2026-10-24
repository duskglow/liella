/* Translation tests for spec §7.5 (crates/boundary/tests/s07_locale_symbols.rs): strfmon,
 * strfmon_l and __strfmon_l through real C varargs (a `double` and a `long double` amount, a
 * string-free format), in the "C" locale. Self-checking: each symbol's checks end with
 * `end <symbol>` on standard error, each failed check prints `FAIL <symbol>: line N: <check>`; the
 * exit status is 1 when any check failed. */
#define _GNU_SOURCE
#include <errno.h>
#include <locale.h>
#include <monetary.h>
#include <stdio.h>
#include <string.h>
#include <sys/types.h>

/* Exported, not declared by the installed headers. */
extern ssize_t __strfmon_l(char *s, size_t max, locale_t loc, const char *fmt, ...);

static int failures;
static const char *current;

static void begin(const char *symbol) { current = symbol; }
static void end(void) { fprintf(stderr, "end %s\n", current); }

#define CHECK(cond) do { if (!(cond)) { failures++; fprintf(stderr, "FAIL %s: line %d: %s\n", current, __LINE__, #cond); } } while (0)

/* `r` is the length of `want`, and `b` holds it. */
static int got(ssize_t r, const char *b, const char *want)
{
    if (r == (ssize_t)strlen(want) && strcmp(b, want) == 0)
        return 1;
    fprintf(stderr, "  got  %zd [%s]\n  want [%s]\n", r, r >= 0 ? b : "", want);
    return 0;
}

int main(void)
{
    char b[64];
    locale_t c = newlocale(LC_ALL_MASK, "C", (locale_t)0);

    begin("strfmon");
    CHECK(got(strfmon(b, sizeof b, "%n|%Li", 1234.567, -2.5L), b, "1234.57|-2.50"));
    memset(b, '#', sizeof b);
    errno = 0;
    CHECK(strfmon(b, 4, "%n", 12.0) == -1 && errno == E2BIG && b[4] == '#');
    end();

    begin("strfmon_l");
    CHECK(c != (locale_t)0);
    CHECK(got(strfmon_l(b, sizeof b, c, "[%=*#4n]", -7.5), b, "[-***7.50]"));
    errno = 0;
    CHECK(strfmon_l(b, 2, c, "%n", 1.0) == -1 && errno == E2BIG);
    end();

    begin("__strfmon_l");
    CHECK(got(__strfmon_l(b, sizeof b, c, "%(n %Ln", -3.0, 4.25L), b, "(3.00) 4.25"));
    errno = 0;
    CHECK(__strfmon_l(b, 3, c, "%n", 1.0) == -1 && errno == E2BIG);
    end();

    if (c != (locale_t)0)
        freelocale(c);
    return failures ? 1 : 0;
}
