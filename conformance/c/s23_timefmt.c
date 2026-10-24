/* Spec §23.7: formatting and parsing time as compiled C code sees it: strftime and wcsftime
 * through the real headers, strptime filling a caller's struct tm, getdate's result and the
 * `getdate_err` data object. Each failing CHECK exits with its line number. */
/* RUN-ENV: TZ=UTC0 */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <locale.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>
#include <wchar.h>

/* getdate's template file, once made; a failing check removes it before it exits. */
static char path[64];

#define CHECK(clause, cond) do { if (!(cond)) { if (path[0]) unlink(path); return __LINE__; } } while (0)

int main(void)
{
    struct tm t;
    memset(&t, 0, sizeof t);
    t.tm_year = 124;
    t.tm_mon = 1;
    t.tm_mday = 29;
    t.tm_hour = 13;
    t.tm_min = 5;
    t.tm_sec = 9;
    t.tm_wday = 4;
    t.tm_yday = 59;
    t.tm_zone = "UTC";
    char buf[64];
    CHECK("23.7", strftime(buf, sizeof buf, "%a %d %b %Y %H:%M:%S %Z", &t) == 28);
    CHECK("23.7", strcmp(buf, "Thu 29 Feb 2024 13:05:09 UTC") == 0);
    CHECK("23.7", strftime(buf, 5, "%Y-%m", &t) == 0);
    locale_t c = newlocale(LC_ALL_MASK, "C", (locale_t)0);
    CHECK("23.7", c && strftime_l(buf, sizeof buf, "%^A", &t, c) == 8 && strcmp(buf, "THURSDAY") == 0);
    wchar_t w[32];
    CHECK("23.7", wcsftime(w, 32, L"%F %T", &t) == 19 && wcscmp(w, L"2024-02-29 13:05:09") == 0);
    CHECK("23.7", wcsftime_l(w, 32, L"%j", &t, c) == 3 && wcscmp(w, L"060") == 0);
    freelocale(c);

    struct tm p;
    memset(&p, 0, sizeof p);
    p.tm_isdst = 77;
    const char *in = "2024-02-29 13:05:09 rest";
    char *end = strptime(in, "%Y-%m-%d %H:%M:%S", &p);
    CHECK("23.7", end == in + 19);
    CHECK("23.7", p.tm_year == 124 && p.tm_mon == 1 && p.tm_mday == 29 && p.tm_hour == 13 && p.tm_wday == 4 && p.tm_yday == 59);
    CHECK("23.7", p.tm_isdst == 77);
    CHECK("23.7", strptime("x", "%Y", &p) == NULL);

    /* getdate: DATEMSK unset, then a template file. */
    unsetenv("DATEMSK");
    getdate_err = 0;
    CHECK("23.7", getdate("2024-02-29") == NULL && getdate_err == 1);
    snprintf(path, sizeof path, "/tmp/liella-s23-timefmt-%d", (int)getpid());
    int fd = open(path, O_WRONLY | O_CREAT | O_TRUNC, 0600);
    CHECK("23.7", fd >= 0);
    const char *tmpl = "%Y-%m-%d %H:%M\n";
    CHECK("23.7", write(fd, tmpl, strlen(tmpl)) == (ssize_t)strlen(tmpl));
    close(fd);
    setenv("DATEMSK", path, 1);
    struct tm *g = getdate("2024-02-29 13:05");
    CHECK("23.7", g && g->tm_year == 124 && g->tm_mday == 29 && g->tm_hour == 13 && g->tm_sec == 0 && g->tm_wday == 4);
    CHECK("23.7", getdate("2023-02-29 13:05") == NULL && getdate_err == 8);
    CHECK("23.7", getdate("nothing") == NULL && getdate_err == 7);
    struct tm r;
    CHECK("23.7", getdate_r("2024-02-29 13:05", &r) == 0 && r.tm_min == 5);
    getdate_err = 0;
    CHECK("23.7", getdate_r("nothing", &r) == 7 && getdate_err == 0);
    unlink(path);
    CHECK("23.7", getdate("2024-02-29 13:05") == NULL && getdate_err == 2);
    return 0;
}
