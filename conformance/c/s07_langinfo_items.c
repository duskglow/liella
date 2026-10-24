/* Spec §7.4 (D-7.10): the one-byte items of the "C" and "POSIX" locales (the monetary sign/precedes/
 * spacing items, the week-calendar items and the measurement item) come back as C strings of length 1:
 * the byte is followed by a NUL, not by the next item's data. Self-checking and glibc-comparable:
 * the same program prints identical lines on glibc 2.39 (tools/probe/remote.sh), and `FAIL` lines
 * and a nonzero exit mark a difference. */
#define _GNU_SOURCE
#include <langinfo.h>
#include <locale.h>
#include <stdio.h>
#include <string.h>

#define ITEM(cat, idx) ((nl_item)(((cat) << 16) | (idx)))

int main(void)
{
    static const struct { const char *name; nl_item item; unsigned char want; } t[] = {
        { "_NL_TIME_WEEK_1STWEEK", ITEM(LC_TIME, 103), 4 },
        { "_NL_TIME_FIRST_WEEKDAY", ITEM(LC_TIME, 104), 1 },
        { "_NL_TIME_FIRST_WORKDAY", ITEM(LC_TIME, 105), 2 },
        { "_NL_TIME_CAL_DIRECTION", ITEM(LC_TIME, 106), 1 },
        { "_NL_MEASUREMENT_MEASUREMENT", ITEM(LC_MEASUREMENT, 0), 1 },
    };
    int failures = 0;
    const char *names[] = { "C", "POSIX" };
    for (int k = 0; k < 2; k++) {
        locale_t l = newlocale(LC_ALL_MASK, names[k], (locale_t)0);
        if (!l) { printf("FAIL %s: newlocale\n", names[k]); failures++; continue; }
        for (unsigned i = 0; i < sizeof t / sizeof t[0]; i++) {
            const char *s = nl_langinfo_l(t[i].item, l);
            size_t n = strlen(s);
            int ok = n == 1 && (unsigned char)s[0] == t[i].want;
            printf("%s %s len=%zu byte=%u %s\n", names[k], t[i].name, n, (unsigned char)s[0], ok ? "ok" : "FAIL");
            failures += !ok;
        }
        /* the monetary one-byte items: items 7..14, 16..21 and the _NL_MONETARY_DUO_* items 24..36 are CHAR_MAX in "C" */
        for (int idx = 7; idx <= 36; idx++) {
            if (idx == 15 || (idx >= 22 && idx <= 23)) continue;   /* CRNCYSTR, and items that are not one-byte */
            const char *s = nl_langinfo_l(ITEM(LC_MONETARY, idx), l);
            size_t n = strlen(s);
            int ok = n == 1 && (unsigned char)s[0] == 0xff;
            printf("%s monetary.%d len=%zu byte=%u %s\n", names[k], idx, n, (unsigned char)s[0], ok ? "ok" : "FAIL");
            failures += !ok;
        }
        freelocale(l);
    }
    printf("failures=%d\n", failures);
    return failures != 0;
}
