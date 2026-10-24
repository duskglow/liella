/* Spec §7.5: strfmon and strfmon_l through real C varargs, in the "C" locale (no currency symbol,
 * no grouping, `.` as the decimal point, two fraction digits). Self-checking: each clause ends
 * with `end <clause>` on standard error, each failed check prints `FAIL <clause>: line N:
 * <check>`; the clause tests in crates/core/tests/s07_locale_clauses.rs read these lines (they
 * also check the flags against conventions with a symbol and grouping, through liella::monetary). */
/* LINK-LIBM: yes */
#define _GNU_SOURCE
#include <errno.h>
#include <locale.h>
#include <monetary.h>
#include <stdio.h>
#include <string.h>

/* fesetround (a libm export: LINK-LIBM above; a glibc reference build needs -lm) and the x86-64
   rounding-mode values (fenv.h is not among the products' headers). */
int fesetround(int);
#define FE_TONEAREST_ 0
#define FE_DOWNWARD_ 0x400
#define FE_UPWARD_ 0x800

static int failures;
static const char *current;

static void begin(const char *clause) { current = clause; }
static void end(void) { fprintf(stderr, "end %s\n", current); }

#define CHECK(cond) do { if (!(cond)) { failures++; fprintf(stderr, "FAIL %s: line %d: %s\n", current, __LINE__, #cond); } } while (0)

/* strfmon into a buffer of `max` bytes gives `want` and returns its length. */
#define FMON(want, max, ...) do { \
        char b_[256]; \
        memset(b_, '#', sizeof b_); \
        ssize_t r_ = strfmon(b_, (max), __VA_ARGS__); \
        int ok_ = r_ == (ssize_t)strlen(want) && strcmp(b_, want) == 0; \
        if (!ok_) \
            fprintf(stderr, "  got  %zd [%.*s]\n  want [%s]\n", r_, r_ > 0 ? (int)r_ : 0, b_, want); \
        CHECK(ok_); \
    } while (0)

int main(void)
{
    char b[64];

    begin("s7_5_c_0");
    FMON("1234.57", 64, "%n", 1234.567);
    FMON("-1234.57", 64, "%i", -1234.567);
    FMON("[1.50] [2.25] [-3.00]", 64, "[%n] [%Ln] [%i]", 1.5, 2.25L, -3.0);
    FMON("a % b", 64, "a %% b");
    /* Flags: fill character and left precision, sign style, field width and justification. */
    FMON(" **12.00", 64, "%=*#4n", 12.0); /* the space aligns with a negative amount's sign */
    FMON("-**12.00", 64, "%=*#4n", -12.0);
    FMON("(5.00)", 64, "%(n", -5.0);
    FMON("5.00", 64, "%(n", 5.0);
    FMON("     12.00", 64, "%10n", 12.0);
    FMON("12.00     |", 64, "%-10n|", 12.0);
    FMON("12.346", 64, "%.3n", 12.3456);
    FMON("12", 64, "%.0n", 12.3);
    /* An invalid conversion. */
    char bad[] = "%q"; /* not a literal: the compiler would reject it */
    errno = 0;
    CHECK(strfmon(b, sizeof b, bad, 1.0) == -1 && errno == EINVAL);
    /* strfmon_l: the given locale object's conventions. */
    locale_t c = newlocale(LC_ALL_MASK, "C", (locale_t)0);
    CHECK(c != (locale_t)0);
    if (c) {
        CHECK(strfmon_l(b, sizeof b, c, "%n", 7.5) == 4 && strcmp(b, "7.50") == 0);
        freelocale(c);
    }
    end();

    begin("s7_5_f_0");
    /* "1234.57" and its terminator need 8 bytes. */
    FMON("1234.57", 8, "%n", 1234.567);
    memset(b, '#', sizeof b);
    errno = 0;
    CHECK(strfmon(b, 7, "%n", 1234.567) == -1 && errno == E2BIG);
    CHECK(b[7] == '#'); /* at most max bytes are written */
    errno = 0;
    CHECK(strfmon(b, 0, "%n", 1.0) == -1 && errno == E2BIG);
    errno = 0;
    CHECK(strfmon(b, 3, "abc") == -1 && errno == E2BIG);
    FMON("abc", 4, "abc");
    end();

    begin("s7_5_c_0");
    /* The amount is rounded in the current rounding mode, as printf rounds (D-7.11), for a
       double and a long double alike. Positive amounts only: for a negative one glibc rounds the
       magnitude (docs/differences-from-glibc.md, D-7.11). */
    CHECK(fesetround(FE_UPWARD_) == 0);
    FMON("2", 8, "%.0n", 1.25);
    FMON("2", 8, "%.0Ln", 1.25L);
    CHECK(fesetround(FE_DOWNWARD_) == 0);
    FMON("1", 8, "%.0n", 1.75);
    FMON("1", 8, "%.0Ln", 1.75L);
    CHECK(fesetround(FE_TONEAREST_) == 0);
    FMON("1", 8, "%.0n", 1.25);
    end();

    return failures ? 1 : 0;
}
