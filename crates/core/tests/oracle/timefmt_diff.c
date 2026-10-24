/* Formatting and parsing time end to end (spec §23.7; D-23.7): built against the host C library
 * and against Liella's products, the outputs must match line for line (timefmt_products.rs).
 * Black-box: every line is a result of a public function. Locales come from LOCPATH; TZ is a
 * fixed rule, so nothing depends on the machine. Takes one argument: a scratch directory. */
#define _GNU_SOURCE
#include <errno.h>
#include <locale.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <time.h>
#include <unistd.h>
#include <wchar.h>

static struct tm mk(int y, int mon, int d, int h, int mi, int s, int wday, int yday, long gmtoff, const char *zone)
{
    struct tm t;
    memset(&t, 0, sizeof t);
    t.tm_year = y - 1900;
    t.tm_mon = mon - 1;
    t.tm_mday = d;
    t.tm_hour = h;
    t.tm_min = mi;
    t.tm_sec = s;
    t.tm_wday = wday;
    t.tm_yday = yday;
    t.tm_gmtoff = gmtoff;
    t.tm_zone = zone;
    return t;
}

static struct tm tms[8];
static const int ntms = 8;

static void init_tms(void)
{
    tms[0] = mk(2024, 1, 1, 0, 0, 0, 1, 0, 0, "UTC");
    tms[1] = mk(2021, 1, 3, 13, 5, 9, 0, 2, 19800, "IST");
    tms[2] = mk(2024, 12, 30, 23, 59, 60, 1, 364, -12600, "-0330");
    tms[3] = mk(1999, 12, 31, 12, 0, 0, 5, 364, 50400, "+14");
    tms[4] = mk(0, 3, 1, 7, 8, 9, 3, 60, 0, "UTC");
    tms[5] = mk(-1, 12, 31, 11, 59, 59, 5, 364, 0, "UTC");
    tms[6] = mk(12345, 6, 15, 18, 30, 0, 4, 165, 3600, NULL);
    tms[7] = mk(1970, 1, 1, 12, 0, 0, 4, 0, 0, "UTC");
    tms[7].tm_isdst = 1;
}

/* Printable: control characters and non-ASCII bytes as \xHH. */
static void shown(const char *s, size_t n)
{
    putchar('[');
    for (size_t i = 0; i < n; i++) {
        unsigned char c = (unsigned char)s[i];
        if (c >= 0x20 && c < 0x7f && c != '\\')
            putchar(c);
        else
            printf("\\x%02X", c);
    }
    putchar(']');
}

static const char *all_directives[] = {
    "%a", "%A", "%b", "%B", "%c", "%C", "%d", "%D", "%e", "%E", "%f", "%F", "%g", "%G", "%h", "%H", "%i", "%I", "%j", "%J", "%k", "%K",
    "%l", "%L", "%m", "%M", "%n", "%N", "%o", "%O", "%p", "%P", "%q", "%Q", "%r", "%R", "%s", "%S", "%t", "%T", "%u", "%U", "%v", "%V",
    "%w", "%W", "%x", "%X", "%y", "%Y", "%z", "%Z", "%%", "%+", "%", "%Ec", "%EC", "%Ex", "%EX", "%Ey", "%EY", "%Ea", "%Ez", "%Od",
    "%Oe", "%OH", "%OI", "%Om", "%OM", "%OS", "%Ou", "%OU", "%OV", "%Ow", "%OW", "%Oy", "%OY", "%Ob", "%OB", "%Oz", "%_d", "%-d", "%0e",
    "%^a", "%^B", "%#a", "%#A", "%#b", "%#Z", "%#p", "%^p", "%#P", "%10Y", "%-10Y", "%_10Y", "%010Y", "%+6Y", "%+Y", "%3d", "%_3S",
    "%-H", "%#x", "%10z", "%_z", "%-z", "%:z", "%5%", "%^#a", "%#^a", "%-_d", "%_-d", "%2Y", "%05e", "%_5j", "%-j", "%10s", "%^10B",
    "%-y", "%_m", "%4C", "%-C", "%_EY", "%-Od", "%^Ob", "%010Ey", "%9999999999d", "%Oq", "a%%b%nc%td",
};

static const char *locale_directives[] = {
    "%a", "%A", "%b", "%B", "%c", "%p", "%P", "%r", "%x", "%X", "%Ec", "%EC", "%Ex", "%EX", "%Ey", "%EY", "%Od", "%Oe", "%OH", "%OI",
    "%Om", "%OM", "%OS", "%Ou", "%OU", "%OV", "%Ow", "%OW", "%Oy", "%Ob", "%OB", "%^a", "%#B", "%10Ey", "%-EY", "%_Od", "%h", "%Z",
};

static void strftime_cases(const char *loc, const char **dirs, size_t ndirs)
{
    printf("== strftime %s\n", loc);
    for (int k = 0; k < ntms; k++) {
        for (size_t i = 0; i < ndirs; i++) {
            char buf[512];
            memset(buf, 0x7f, sizeof buf);
            size_t n = strftime(buf, sizeof buf, dirs[i], &tms[k]);
            printf("%d %s ", k, dirs[i]);
            shown(buf, n);
            printf(" %zu\n", n);
        }
    }
}

static void truncation(void)
{
    printf("== strftime sizes\n");
    struct tm *t = &tms[0];
    static const struct {
        const char *fmt;
        size_t max;
    } c[] = {{"%Y-%m-%d", 11}, {"%Y-%m-%d", 10}, {"%Y-%m-%d", 1}, {"%Y-%m-%d", 0}, {"", 1}, {"", 0}, {"%p", 1}, {"abc", 3}, {"abc", 4},
             {"%P", 64}, {"%10Y", 10}, {"%10Y", 11}};
    for (size_t i = 0; i < sizeof c / sizeof c[0]; i++) {
        char buf[64];
        memset(buf, 'x', sizeof buf);
        size_t n = strftime(buf, c[i].max, c[i].fmt, t);
        printf("strftime(max=%zu, \"%s\") = %zu buf0=%d", c[i].max, c[i].fmt, n, c[i].max ? buf[0] : -1);
        if (n)
            printf(" [%s]", buf);
        printf("\n");
    }
    char buf[64];
    struct tm big = tms[0];
    big.tm_mon = 11;
    big.tm_wday = 6;
    printf("strftime %%s with TZ=UTC0: %zu ", strftime(buf, sizeof buf, "%s", &big));
    shown(buf, strlen(buf));
    printf("\n");
}

static void wide(void)
{
    printf("== wcsftime\n");
    static const wchar_t *f[] = {L"%a %d %b %Y", L"%c", L"%Ey|%EY|%Oy", L"%10Y|%_d|%-m", L"%Z %z", L"%p", L"xéy %A", L"%"};
    for (int k = 0; k < 4; k++) {
        for (size_t i = 0; i < sizeof f / sizeof f[0]; i++) {
            wchar_t buf[256];
            size_t n = wcsftime(buf, 256, f[i], &tms[k]);
            printf("%d %zu:", k, n);
            for (size_t j = 0; j < n; j++)
                printf(buf[j] < 128 && buf[j] >= 32 ? "%c" : "<U+%04X>", (unsigned)buf[j]);
            printf("\n");
        }
    }
    wchar_t small[4];
    printf("wcsftime max=4 \"%%Y-%%m\": %zu\n", wcsftime(small, 4, L"%Y-%m", &tms[0]));
}

/* ------------------------------------------------------------------ strptime */

static void show_parsed(const char *in, const char *end, const struct tm *t)
{
    if (!end) {
        printf(" -> NULL\n");
        return;
    }
    printf(" -> +%ld", (long)(end - in));
    const int *f = &t->tm_sec;
    static const char *names[] = {"sec", "min", "hour", "mday", "mon", "year", "wday", "yday", "isdst"};
    for (int i = 0; i < 9; i++)
        if (f[i] != -77)
            printf(" %s=%d", names[i], f[i]);
    if (t->tm_gmtoff != -77)
        printf(" gmtoff=%ld", t->tm_gmtoff);
    if (!t->tm_zone || strcmp(t->tm_zone, "sentinel") != 0)
        printf(" zone=%s", t->tm_zone ? t->tm_zone : "(null)");
    printf("\n");
}

static void parse(const char *fmt, const char *in)
{
    struct tm t;
    int *f = &t.tm_sec;
    for (int i = 0; i < 9; i++)
        f[i] = -77;
    t.tm_gmtoff = -77;
    t.tm_zone = "sentinel";
    printf("strptime(\"%s\", \"%s\")", in, fmt);
    show_parsed(in, strptime(in, fmt, &t), &t);
}

static void strptime_cases(const char *loc)
{
    printf("== strptime %s\n", loc);
    static const char *pairs[][2] = {
        {"%Y-%m-%d", "2024-02-29"},
        {"%Y-%m-%d", "2023-02-29"},
        {"%Y-%m-%d %H:%M:%S", "2024-07-04 13:05:09"},
        {"%Y-%m-%d", "2024-7-4"},
        {"%Y-%m-%d", " 2024 - 07 - 04"},
        {"%Y %m", "2024    7"},
        {"%d", "32"},
        {"%d", "0"},
        {"%m", "13"},
        {"%H", "24"},
        {"%M", "60"},
        {"%S", "60"},
        {"%S", "61"},
        {"%S", "62"},
        {"%y", "69"},
        {"%y", "68"},
        {"%y", "00"},
        {"%C%y", "1969"},
        {"%C", "20"},
        {"%y %C", "05 19"},
        {"%Y", "-5"},
        {"%Y", "+2024"},
        {"%Y", "123456"},
        {"%G-%V", "2020-53"},
        {"%j", "366"},
        {"%j", "367"},
        {"%Y %j", "2024 060"},
        {"%U %Y", "10 2024"},
        {"%W %w %Y", "10 3 2024"},
        {"%u", "7"},
        {"%w", "7"},
        {"%a", "monday"},
        {"%a", "Mon"},
        {"%a", "MONDAY rest"},
        {"%A", "Tues"},
        {"%b", "september"},
        {"%b", "sept"},
        {"%h", "Dec"},
        {"%B %d %Y", "February 29 2024"},
        {"%I:%M %p", "12:30 am"},
        {"%I:%M %p", "12:30 PM"},
        {"%I %p", "13 PM"},
        {"%p %I", "pm 3"},
        {"%r", "01:02:03 PM"},
        {"%R", "23:59"},
        {"%T", "23:59:59"},
        {"%D", "12/31/99"},
        {"%F", "2024-01-15"},
        {"%c", "Mon Jan  1 00:00:00 2024"},
        {"%x", "01/02/24"},
        {"%X", "10:11:12"},
        {"%e", " 5"},
        {"%k", " 7"},
        {"%l", " 7"},
        {"%n%t", " \t\n  "},
        {"%n", ""},
        {"%%", "%"},
        {"%%", "x"},
        {"%z", "+0530"},
        {"%z", "-03:30"},
        {"%z", "+05"},
        {"%z", "Z"},
        {"%z", "+2400"},
        {"%z", "+1260"},
        {"%Z", "EST"},
        {"%Z %Y", "EST 2024"},
        {"%s", "86400"},
        {"%s", "-1"},
        {"%s", "99999999999999999999"},
        {"%Q", "x"},
        {"%Y", ""},
        {"%Y", "abc"},
        {"abc", "ABC"},
        {"a b", "a     b"},
        {"ab", "a b"},
        {" %Y", "2024"},
        {"%Y ", "2024"},
        {"%EY", "2024"},
        {"%Ey", "24"},
        {"%Oy", "24"},
        {"%Od", "05"},
        {"%Ec", "Mon Jan  1 00:00:00 2024"},
        {"%Ex", "01/02/24"},
        {"%OH:%OM", "07:08"},
        {"%Y%m%d", "20240229"},
        {"%H%M%S", "235960"},
        {"%Y-%m-%d", "2024-02-29trailing"},
    };
    for (size_t i = 0; i < sizeof pairs / sizeof pairs[0]; i++)
        parse(pairs[i][0], pairs[i][1]);
}

static void strptime_local(const char *loc)
{
    printf("== strptime %s (localized)\n", loc);
    /* Round trips: what strftime prints in this locale, strptime reads back. */
    static const char *f[] = {"%a %d %b %Y", "%A %B", "%c", "%x", "%X", "%r", "%p %I", "%Ec", "%Ex", "%EY", "%Oy", "%Od %Om"};
    for (int k = 0; k < 4; k++) {
        for (size_t i = 0; i < sizeof f / sizeof f[0]; i++) {
            char buf[256];
            if (strftime(buf, sizeof buf, f[i], &tms[k]) == 0)
                continue;
            parse(f[i], buf);
        }
    }
}

/* ------------------------------------------------------------------ getdate */

static void getdate_cases(const char *dir)
{
    printf("== getdate\n");
    char tmpl[512], subdir[512];
    snprintf(tmpl, sizeof tmpl, "%s/getdate.tmpl", dir);
    snprintf(subdir, sizeof subdir, "%s/getdate.dir", dir);
    FILE *f = fopen(tmpl, "w");
    fputs("%Y-%m-%d %H:%M:%S\n%d.%m.%Y %H:%M\n%B %d, %Y %I:%M %p\n%m/%d/%Y\n", f);
    fclose(f);
    mkdir(subdir, 0700);

    unsetenv("DATEMSK");
    errno = 0;
    struct tm *r = getdate("2024-01-01 00:00:00");
    printf("DATEMSK unset: %s getdate_err=%d\n", r ? "tm" : "NULL", getdate_err);
    setenv("DATEMSK", "/nonexistent/getdate.tmpl", 1);
    r = getdate("2024-01-01 00:00:00");
    printf("DATEMSK missing: %s getdate_err=%d\n", r ? "tm" : "NULL", getdate_err);
    setenv("DATEMSK", subdir, 1);
    r = getdate("2024-01-01 00:00:00");
    printf("DATEMSK a directory: %s getdate_err=%d\n", r ? "tm" : "NULL", getdate_err);
    setenv("DATEMSK", tmpl, 1);
    static const char *in[] = {"2024-02-29 13:05:09", "29.02.2024 13:05", "February 29, 2024 01:05 PM", "2023-02-29 00:00:00",
                               "31.04.2024 10:00", "2024-13-01 00:00:00", "not a date", "", "  2024-02-29 13:05:09  "};
    for (size_t i = 0; i < sizeof in / sizeof in[0]; i++) {
        r = getdate(in[i]);
        printf("getdate(\"%s\"): ", in[i]);
        if (r)
            printf("%d-%02d-%02d %02d:%02d:%02d wday=%d yday=%d isdst=%d\n", r->tm_year + 1900, r->tm_mon + 1, r->tm_mday, r->tm_hour, r->tm_min,
                   r->tm_sec, r->tm_wday, r->tm_yday, r->tm_isdst);
        else
            printf("NULL getdate_err=%d\n", getdate_err);
        struct tm res;
        memset(&res, 0, sizeof res);
        int e = getdate_r(in[i], &res);
        printf("  getdate_r: %d%s\n", e, e == 0 && r && res.tm_mday == r->tm_mday && res.tm_hour == r->tm_hour ? " same" : "");
    }
    FILE *e = fopen(tmpl, "w");
    fclose(e);
    r = getdate("2024-02-29 13:05:09");
    printf("empty template file: %s getdate_err=%d\n", r ? "tm" : "NULL", getdate_err);
    unlink(tmpl);
    rmdir(subdir);
}

int main(int argc, char **argv)
{
    const char *dir = argc > 1 ? argv[1] : "/tmp";
    setenv("TZ", "UTC0", 1);
    tzset();
    init_tms();
    strftime_cases("C", all_directives, sizeof all_directives / sizeof all_directives[0]);
    truncation();
    wide();
    strptime_cases("C");
    getdate_cases(dir);
    static const char *locs[] = {"en_US.UTF-8", "de_DE.UTF-8", "ja_JP.UTF-8", "th_TH.UTF-8", "fa_IR.UTF-8"};
    for (size_t i = 0; i < sizeof locs / sizeof locs[0]; i++) {
        if (!setlocale(LC_ALL, locs[i])) {
            printf("== %s unavailable\n", locs[i]);
            continue;
        }
        strftime_cases(locs[i], i == 0 ? all_directives : locale_directives,
                       i == 0 ? sizeof all_directives / sizeof all_directives[0] : sizeof locale_directives / sizeof locale_directives[0]);
        wide();
        strptime_local(locs[i]);
        if (i == 0)
            strptime_cases(locs[i]);
    }
    setlocale(LC_ALL, "C");
    printf("done\n");
    return 0;
}
