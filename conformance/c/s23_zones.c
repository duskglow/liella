/* Spec §23.5, §23.6: the time-zone data objects and struct tm results as compiled C code sees
 * them: `tzname`/`__tzname`, `timezone`/`__timezone` and `daylight`/`__daylight` are one object
 * each, "GMT" before the first tzset, written by tzset; the static results of gmtime/localtime
 * and asctime/ctime. Each failing CHECK exits with its line number. */
/* RUN-ENV: TZ=EST5EDT,M3.2.0,M11.1.0 */
#define _GNU_SOURCE
#include <errno.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#define CHECK(clause, cond) do { if (!(cond)) return __LINE__; } while (0)

extern char *__tzname[2];
extern long __timezone;
extern int __daylight;

int main(void)
{
    /* One object behind each pair of names. */
    CHECK("23.6", (void *)tzname == (void *)__tzname);
    CHECK("23.6", &timezone == &__timezone);
    CHECK("23.6", &daylight == &__daylight);
    /* Before the first tzset (as glibc). */
    CHECK("23.6", strcmp(tzname[0], "GMT") == 0 && strcmp(tzname[1], "GMT") == 0);
    CHECK("23.6", timezone == 0 && daylight == 0);

    tzset();
    CHECK("23.6", strcmp(tzname[0], "EST") == 0 && strcmp(tzname[1], "EDT") == 0);
    CHECK("23.6", timezone == 18000 && daylight == 1);
    CHECK("23.6", __tzname[0] == tzname[0] && __timezone == 18000 && __daylight == 1);

    /* localtime: tm_gmtoff east, tm_zone the abbreviation, valid after TZ changes. */
    time_t t = 1720000000; /* 2024-07-03 09:46:40 UTC */
    struct tm *l = localtime(&t);
    CHECK("23.5", l && l->tm_hour == 5 && l->tm_isdst == 1 && l->tm_gmtoff == -14400);
    const char *zone = l->tm_zone;
    CHECK("23.5", strcmp(zone, "EDT") == 0);
    setenv("TZ", "JST-9", 1);
    tzset();
    CHECK("23.6", strcmp(tzname[0], "JST") == 0 && strcmp(tzname[1], "JST") == 0);
    CHECK("23.6", timezone == -32400 && daylight == 0);
    CHECK("23.5", strcmp(zone, "EDT") == 0);

    /* The program may write the objects; both names see it. */
    static char mine[] = "MINE";
    tzname[0] = mine;
    CHECK("23.6", __tzname[0] == mine);

    /* gmtime and localtime share one result, ctime and asctime another. */
    struct tm *g = gmtime(&t);
    CHECK("23.5", g && g->tm_hour == 9 && g->tm_gmtoff == 0 && strcmp(g->tm_zone, "GMT") == 0);
    CHECK("23.5", localtime(&t) == g && g->tm_hour == 18);
    char *c = ctime(&t);
    CHECK("23.5", c && strcmp(c, "Wed Jul  3 18:46:40 2024\n") == 0);
    CHECK("23.5", asctime(g) == c);
    char buf[26];
    CHECK("23.5", ctime_r(&t, buf) == buf && strcmp(buf, c) == 0);

    /* mktime and timegm through the C structure. */
    struct tm m = {0};
    m.tm_year = 124;
    m.tm_mon = 0;
    m.tm_mday = 32;
    m.tm_isdst = -1;
    CHECK("23.5", mktime(&m) == 1706713200 && m.tm_mon == 1 && m.tm_mday == 1 && m.tm_yday == 31);
    m.tm_mday = 1;
    m.tm_hour = 0;
    CHECK("23.5", timegm(&m) == 1706745600 && strcmp(m.tm_zone, "GMT") == 0);
    CHECK("23.5", timelocal(&m) == 1706713200);
    return 0;
}
