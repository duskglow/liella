/* Broken-down time and time zones end to end (spec §23.5-23.6; D-23.4 to D-23.6): built against
 * the host C library and against Liella's products, the outputs must match line for line
 * (zones_products.rs). Black-box: every line is a result of a public function. The zone files are
 * the system's (/usr/share/zoneinfo), the same for both builds. Each TZ value is examined in a
 * fresh child process, so that no result depends on the zones examined before it. */
#define _GNU_SOURCE
#include <errno.h>
#include <limits.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/syscall.h>
#include <time.h>
#include <unistd.h>

static void show_tm(const char *what, const struct tm *t)
{
    if (!t) {
        printf("%s: NULL errno=%d\n", what, errno);
        return;
    }
    printf("%s: %ld-%02d-%02d %02d:%02d:%02d wday=%d yday=%d isdst=%d gmtoff=%ld zone=%s\n", what, (long)t->tm_year + 1900, t->tm_mon + 1, t->tm_mday,
           t->tm_hour, t->tm_min, t->tm_sec, t->tm_wday, t->tm_yday, t->tm_isdst, t->tm_gmtoff, t->tm_zone ? t->tm_zone : "(null)");
}

/* A text without its trailing newline. */
static const char *line_of(const char *s)
{
    static char b[128];
    if (!s)
        return "NULL";
    snprintf(b, sizeof b, "%s", s);
    char *nl = strchr(b, '\n');
    if (nl)
        *nl = 0;
    return b;
}

static struct tm mk(int y, int mon, int d, int h, int mi, int s, int isdst)
{
    struct tm t;
    memset(&t, 0, sizeof t);
    t.tm_year = y - 1900;
    t.tm_mon = mon - 1;
    t.tm_mday = d;
    t.tm_hour = h;
    t.tm_min = mi;
    t.tm_sec = s;
    t.tm_isdst = isdst;
    t.tm_wday = 99;
    t.tm_yday = 999;
    return t;
}

/* ------------------------------------------------------------------ UTC */

static void utc(void)
{
    printf("== gmtime\n");
    static const int64_t ts[] = {0, -1, 951782400, 951868799, 2147483647LL, 2147483648LL, -2147483648LL, -2147483649LL, 253402300799LL,
                                 253402300800LL, -62135596800LL, -62135596801LL, 67767976233532799LL, 67767976233532800LL, -67768040609740800LL,
                                 -67768040609740801LL, INT64_MAX, INT64_MIN};
    for (size_t i = 0; i < sizeof ts / sizeof ts[0]; i++) {
        time_t t = (time_t)ts[i];
        struct tm r;
        char name[64];
        snprintf(name, sizeof name, "gmtime_r(%lld)", (long long)ts[i]);
        errno = 0;
        show_tm(name, gmtime_r(&t, &r));
    }

    printf("== timegm\n");
    struct {
        const char *what;
        struct tm t;
    } cases[] = {
        {"2000-02-29", mk(2000, 2, 29, 12, 0, 0, 0)},
        {"mday 0", mk(2024, 3, 0, 0, 0, 0, 0)},
        {"mday 32", mk(2024, 1, 32, 0, 0, 0, 0)},
        {"mon 13", mk(2023, 13, 1, 0, 0, 0, 0)},
        {"mon -1", mk(2023, -1, 1, 0, 0, 0, 0)},
        {"hour -1", mk(2024, 1, 1, -1, 0, 0, 0)},
        {"sec 60", mk(2016, 12, 31, 23, 59, 60, 0)},
        {"sec -1", mk(1970, 1, 1, 0, 0, -1, 0)},
        {"min 1000000", mk(2024, 1, 1, 0, 1000000, 0, 0)},
        {"sec INT_MAX", mk(2024, 1, 1, 0, 0, INT_MAX, 0)},
        {"sec INT_MIN", mk(2024, 1, 1, 0, 0, INT_MIN, 0)},
        {"isdst 1 ignored", mk(2024, 7, 1, 0, 0, 0, 1)},
        {"year INT_MAX", mk(0, 1, 1, 0, 0, 0, 0)},
        {"year INT_MIN", mk(0, 1, 1, 0, 0, 0, 0)},
        {"year INT_MAX mon 12", mk(0, 12, 1, 0, 0, 0, 0)},
    };
    cases[12].t.tm_year = INT_MAX;
    cases[13].t.tm_year = INT_MIN;
    cases[14].t.tm_year = INT_MAX;
    cases[14].t.tm_mon = 12;
    for (size_t i = 0; i < sizeof cases / sizeof cases[0]; i++) {
        struct tm t = cases[i].t;
        errno = 0;
        time_t r = timegm(&t);
        printf("timegm %s: %lld errno=%d\n", cases[i].what, (long long)r, errno);
        show_tm("  normalized", &t);
    }

    printf("== asctime\n");
    static const int years[] = {-1900, 0, 999, 1000, 1973, 9999, 10000, 2147481747};
    for (size_t i = 0; i < sizeof years / sizeof years[0]; i++) {
        struct tm t = mk(1973, 9, 16, 1, 3, 52, 0);
        t.tm_year = years[i] - 1900;
        t.tm_wday = 0;
        char buf[64];
        memset(buf, 'x', sizeof buf);
        errno = 0;
        char *r = asctime_r(&t, buf);
        printf("asctime_r year %d: [%s] newline=%d errno=%d\n", years[i], line_of(r), r && strchr(r, '\n') != NULL, errno);
    }
    struct tm odd = mk(1973, 9, 16, 1, 3, 52, 0);
    odd.tm_wday = 7;
    odd.tm_mon = 12;
    errno = 0;
    char *r = asctime(&odd);
    printf("asctime wday 7 mon 12: %s errno=%d\n", r ? "text" : "NULL", errno);
    odd = mk(1973, 9, 16, 1, 3, 52, 0);
    odd.tm_mday = 123;
    odd.tm_hour = 100;
    odd.tm_wday = 1;
    errno = 0;
    r = asctime(&odd);
    printf("asctime mday 123 hour 100: [%s] errno=%d\n", line_of(r), errno);

    printf("== misc\n");
    static const int dy[] = {1900, 2000, 2023, 2024, 0, -4, 2100};
    for (size_t i = 0; i < sizeof dy / sizeof dy[0]; i++)
        printf("dysize(%d) = %d\n", dy[i], dysize(dy[i]));
    printf("difftime(10, 3) = %.1f\n", difftime(10, 3));
    printf("difftime(0, INT64_MAX) = %.6e\n", difftime(0, (time_t)INT64_MAX));
    printf("difftime(2^53+1, 0) = %.1f\n", difftime((time_t)9007199254740993LL, 0));
}

/* ------------------------------------------------------------------ zones */

static void zone(const char *tz)
{
    printf("== TZ=%s\n", tz ? tz : "(unset)");
    if (tz)
        setenv("TZ", tz, 1);
    else
        unsetenv("TZ");
    tzset();
    printf("tzname=[%s][%s] timezone=%ld daylight=%d\n", tzname[0], tzname[1], timezone, daylight);

    /* Every change of offset or abbreviation in 2024, found hour by hour. */
    struct tm start = mk(2024, 1, 1, 0, 0, 0, 0);
    time_t t0 = timegm(&start), end = t0 + 366L * 86400;
    struct tm prev;
    time_t pt = t0;
    localtime_r(&pt, &prev);
    show_tm("2024-01-01T00Z", &prev);
    time_t trans[8];
    int nt = 0;
    for (time_t t = t0 + 900; t <= end; t += 900) {
        struct tm cur;
        localtime_r(&t, &cur);
        if (cur.tm_gmtoff != prev.tm_gmtoff || cur.tm_isdst != prev.tm_isdst || strcmp(cur.tm_zone, prev.tm_zone) != 0) {
            /* Find the exact second. */
            time_t lo = t - 900, hi = t;
            while (hi - lo > 1) {
                time_t mid = lo + (hi - lo) / 2;
                struct tm m;
                localtime_r(&mid, &m);
                if (m.tm_gmtoff == prev.tm_gmtoff && m.tm_isdst == prev.tm_isdst && strcmp(m.tm_zone, prev.tm_zone) == 0)
                    lo = mid;
                else
                    hi = mid;
            }
            char name[64];
            snprintf(name, sizeof name, "transition at %lld", (long long)hi);
            struct tm a;
            localtime_r(&hi, &a);
            show_tm(name, &a);
            if (nt < 8)
                trans[nt++] = hi;
            prev = cur;
        }
    }

    /* Sample instants in other years (the zone file's range and the rule beyond it). */
    static const int years[] = {1850, 1900, 1950, 1970, 2000, 2037, 2038, 2100, 2500};
    for (size_t i = 0; i < sizeof years / sizeof years[0]; i++) {
        for (int m = 1; m <= 7; m += 6) {
            struct tm u = mk(years[i], m, 15, 12, 0, 0, 0);
            time_t t = timegm(&u);
            struct tm l;
            char name[32];
            snprintf(name, sizeof name, "%d-%02d-15T12Z", years[i], m);
            errno = 0;
            show_tm(name, localtime_r(&t, &l));
        }
    }

    /* mktime around each transition: the local times just before, in the gap or overlap, and
     * after, with each tm_isdst. */
    for (int k = 0; k < nt; k++) {
        struct tm before, after;
        time_t b = trans[k] - 1;
        localtime_r(&b, &before);
        localtime_r(&trans[k], &after);
        long jump = after.tm_gmtoff - before.tm_gmtoff;
        static const int offsets[] = {-1, 0, 1, 2};
        for (size_t o = 0; o < sizeof offsets / sizeof offsets[0]; o++) {
            /* Local wall times relative to the wall time just before the transition. */
            long step = jump > 0 ? jump / 2 * offsets[o] : (jump < 0 ? -jump / 2 * offsets[o] : 1800L * offsets[o]);
            for (int isdst = -1; isdst <= 1; isdst++) {
                struct tm w = before;
                w.tm_sec += 1 + (int)step;
                w.tm_isdst = isdst;
                w.tm_wday = 99;
                w.tm_yday = 999;
                errno = 0;
                time_t r = mktime(&w);
                printf("transition %d wall+%lds isdst=%d: t-trans=%lld errno=%d\n", k, 1 + step, isdst, (long long)(r - trans[k]), errno);
                show_tm("  normalized", &w);
            }
        }
    }

    /* mktime normalization in this zone. */
    struct tm n = mk(2024, 1, 32, 25, 61, 61, -1);
    errno = 0;
    time_t r = mktime(&n);
    printf("mktime 2024-01-32 25:61:61 isdst=-1: %lld errno=%d\n", (long long)(r - t0), errno);
    show_tm("  normalized", &n);
    n = mk(2024, 1, 1, 0, 0, 0, 0);
    n.tm_year = INT_MAX;
    errno = 0;
    r = mktime(&n);
    printf("mktime year INT_MAX: %lld errno=%d\n", (long long)r, errno);
    time_t big = (time_t)INT64_MAX;
    struct tm l;
    errno = 0;
    show_tm("localtime_r(INT64_MAX)", localtime_r(&big, &l));
}

static void implicit(void)
{
    printf("== implicit tzset\n");
    setenv("TZ", "UTC0", 1);
    tzset();
    time_t t = 1720000000;
    setenv("TZ", "JST-9", 1);
    struct tm l;
    show_tm("localtime_r after setenv, no tzset", localtime_r(&t, &l));
    printf("tzname after localtime_r=[%s][%s] timezone=%ld\n", tzname[0], tzname[1], timezone);
    setenv("TZ", "EST5EDT,M3.2.0,M11.1.0", 1);
    show_tm("localtime after setenv", localtime(&t));
    printf("tzname after localtime=[%s][%s] timezone=%ld daylight=%d\n", tzname[0], tzname[1], timezone, daylight);
    setenv("TZ", "NZST-12NZDT,M9.5.0,M4.1.0/3", 1);
    struct tm m = mk(2024, 7, 3, 12, 0, 0, -1);
    time_t r = mktime(&m);
    printf("mktime after setenv: %lld zone=%s\n", (long long)(r - t), m.tm_zone);
    setenv("TZ", "CET-1CEST,M3.5.0,M10.5.0/3", 1);
    struct tm *lt = localtime(&t);
    printf("localtime after another setenv: zone=%s gmtoff=%ld\n", lt->tm_zone, lt->tm_gmtoff);
    struct tm *g = gmtime(&t), *lo = localtime(&t);
    printf("gmtime and localtime share their result: %d\n", g == lo);
    char *c1 = ctime(&t), *a1 = asctime(lo);
    printf("ctime and asctime share their result: %d\n", c1 == a1);
}

/* `zone(tz)` in a child process (with `TZDIR` set when given), so that it starts from the state a
 * new process has. */
static void fresh(const char *tz, const char *tzdir)
{
    fflush(stdout);
    pid_t pid = fork();
    if (pid == 0) {
        if (tzdir)
            setenv("TZDIR", tzdir, 1);
        zone(tz);
        fflush(stdout);
        _exit(0);
    }
    int st;
    syscall(SYS_wait4, pid, &st, 0, NULL);
}

int main(void)
{
    utc();
    static const char *zones[] = {
        NULL,
        "UTC",
        "",
        ":UTC",
        "UTC0",
        "GMT+5",
        "America/New_York",
        "Europe/Berlin",
        "Europe/Dublin",
        "Europe/London",
        ":Europe/London",
        "Australia/Lord_Howe",
        "Asia/Kolkata",
        "Pacific/Kiritimati",
        "America/Sao_Paulo",
        "Africa/Casablanca",
        "Antarctica/Troll",
        "/usr/share/zoneinfo/Asia/Tokyo",
        ":/usr/share/zoneinfo/Asia/Tokyo",
        "EST",
        "EST5EDT",
        "EST5EDT,M3.2.0,M11.1.0",
        "CET-1CEST,M3.5.0,M10.5.0/3",
        "NZST-12NZDT,M9.5.0,M4.1.0/3",
        "<+0330>-3:30",
        "<-03>3<-02>,M3.5.0/-2,M10.5.0/-1",
        "XXX3:30",
        "JST-9",
        "AAA3BBB,J60/2,J300/2",
        "AAA3BBB,59/2,299/2",
        "AAA3BBB,M3.2.0/-1,M11.1.0/26",
        "AAA3BBB,M3.2.0/167,M11.1.0/-167",
        "AAA3BBB2,M3.2.0,M11.1.0",
        "AAA3BBB-1:30,M3.2.0,M11.1.0",
        "AAA-25",
        "AAA25",
        "AAA24:59:59",
        "AB1",
        "<AB>1",
        "AAA",
        "AAA3BBB,M13.1.0,M11.1.0",
        "AAA3BBB,M3.6.0,M11.1.0",
        "AAA3BBB,J0/2,J300/2",
        "AAA3BBB,366/2,J300/2",
        "AAA3BBB,M3.2.0",
        "Invalid//Zone",
        "Nonexistent/Zone",
        "../../../etc/passwd",
    };
    for (size_t i = 0; i < sizeof zones / sizeof zones[0]; i++)
        fresh(zones[i], NULL);
    printf("== TZDIR=/usr/share/zoneinfo/America\n");
    fresh("New_York", "/usr/share/zoneinfo/America");
    printf("== TZDIR=/nonexistent\n");
    fresh("Europe/Berlin", "/nonexistent");
    fflush(stdout);
    if (fork() == 0) {
        implicit();
        fflush(stdout);
        _exit(0);
    }
    int st;
    syscall(SYS_wait4, -1, &st, 0, NULL);
    printf("done\n");
    return 0;
}
