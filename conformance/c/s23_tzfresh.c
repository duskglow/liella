/* Spec §23.6: an implicit tzset (localtime, mktime) reads TZ from the live `environ` array on every
 * call and sees any change to it (TZ-FRESH): a raw write into a slot of the array, an edit of the
 * string in place (value, truncation, rename of the matched entry, an earlier entry edited into a
 * match, an absent name edited into presence), a replaced array, swapped duplicates (the first
 * wins), and the setenv family. Each case primes the zone, mutates, and checks localtime's hour,
 * mktime's result and strftime's %Z against the explicit expectation and against an explicit
 * tzset. */
#define _GNU_SOURCE
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

extern char **environ;

#define CHECK(cond) do { if (!(cond)) return __LINE__; } while (0)

static int hr(void)
{
    time_t t = 0;
    return localtime(&t)->tm_hour;
}

static long mk(void)
{
    struct tm tm;
    memset(&tm, 0, sizeof tm);
    tm.tm_year = 70;
    tm.tm_mday = 1;
    return (long)mktime(&tm);
}

static int zn(char *out, size_t n)
{
    time_t t = 0;
    struct tm *tm = localtime(&t);
    return (int)strftime(out, n, "%Z", tm);
}

/* The hour, mktime's value and the zone name now, all equal to a fresh explicit tzset's. */
static int fresh(int hour, long secs, const char *name)
{
    char a[32], b[32];
    int h1 = hr();
    long m1 = mk();
    zn(a, sizeof a);
    tzset();
    int h2 = hr();
    long m2 = mk();
    zn(b, sizeof b);
    if (h1 != h2 || m1 != m2 || strcmp(a, b) != 0) return 1;
    if (hour >= 0 && (h1 != hour || m1 != secs)) return 2;
    if (name && strcmp(a, name) != 0) return 3;
    return 0;
}

static char buf1[32], buf2[32], buf3[32];
static char *arr[8];

static void prime(void)
{
    hr();
    mk();
}

int main(void)
{
    /* 1: a raw slot write replaces the TZ entry with another string. */
    strcpy(buf1, "TZ=EST5");
    arr[0] = "A=1"; arr[1] = buf1; arr[2] = NULL;
    environ = arr;
    tzset();
    CHECK(hr() == 19);
    arr[1] = "TZ=PST8";
    CHECK(hr() == 16);
    CHECK(mk() == 28800);
    arr[1] = "TZ=JST-9";
    CHECK(fresh(9, -32400, "JST") == 0);

    /* 2: the string edited in place, same length, and longer. */
    strcpy(buf1, "TZ=EST5");
    arr[1] = buf1;
    tzset();
    prime();
    memcpy(buf1, "TZ=PST8", 8);
    CHECK(fresh(16, 28800, "PST") == 0);
    strcpy(buf1, "TZ=CET-1");
    CHECK(fresh(1, -3600, "CET") == 0);

    /* 3: in-place truncation of the value (TZ becomes empty: UTC). */
    strcpy(buf1, "TZ=EST5");
    tzset();
    prime();
    buf1[3] = 0;
    CHECK(fresh(0, 0, NULL) == 0);

    /* 4: the matched entry renamed in place (TZ becomes unset: the default zone). */
    strcpy(buf1, "TZ=PST8");
    tzset();
    prime();
    buf1[0] = 'X';
    CHECK(fresh(-1, 0, NULL) == 0);
    buf1[0] = 'T';
    CHECK(fresh(16, 28800, "PST") == 0);

    /* 5: an earlier entry edited in place into a match (the first entry wins). */
    strcpy(buf2, "TX=PST8");
    strcpy(buf1, "TZ=EST5");
    arr[0] = buf2; arr[1] = buf1; arr[2] = NULL;
    tzset();
    prime();
    CHECK(hr() == 19);
    buf2[1] = 'Z';
    CHECK(fresh(16, 28800, "PST") == 0);

    /* 6: an absent name edited in place into presence. */
    strcpy(buf2, "TY=PST8");
    arr[0] = "A=1"; arr[1] = buf2; arr[2] = NULL;
    tzset();
    prime();
    buf2[1] = 'Z';
    CHECK(fresh(16, 28800, "PST") == 0);

    /* 7: a slot write making an unset name set, and one removing it. */
    arr[0] = "A=1"; arr[1] = "B=2"; arr[2] = NULL;
    tzset();
    prime();
    arr[1] = "TZ=PST8";
    CHECK(fresh(16, 28800, "PST") == 0);
    arr[1] = "B=2";
    CHECK(fresh(-1, 0, NULL) == 0);

    /* 8: duplicates swapped by raw slot writes (the first wins). */
    arr[0] = "TZ=EST5"; arr[1] = "TZ=PST8"; arr[2] = NULL;
    tzset();
    prime();
    CHECK(hr() == 19);
    arr[0] = "TZ=PST8"; arr[1] = "TZ=EST5";
    CHECK(fresh(16, 28800, "PST") == 0);

    /* 9: a replaced array (assigning environ). */
    strcpy(buf3, "TZ=JST-9");
    char *b[3] = { "A=1", buf3, NULL };
    environ = b;
    CHECK(fresh(9, -32400, "JST") == 0);
    char *c[2] = { "TZ=EST5", NULL };
    environ = c;
    CHECK(fresh(19, 18000, "EST") == 0);

    /* 10: the setenv family. */
    strcpy(buf1, "TZ=EST5");
    arr[0] = "A=1"; arr[1] = buf1; arr[2] = NULL;
    environ = arr;
    tzset();
    prime();
    setenv("TZ", "PST8", 1);
    CHECK(fresh(16, 28800, "PST") == 0);
    unsetenv("TZ");
    CHECK(fresh(-1, 0, NULL) == 0);
    static char put[] = "TZ=CET-1";
    putenv(put);
    CHECK(fresh(1, -3600, "CET") == 0);
    put[3] = 'J'; put[4] = 'S'; put[5] = 'T'; put[6] = '-'; put[7] = '9';
    CHECK(fresh(9, -32400, "JST") == 0);
    clearenv();
    CHECK(fresh(-1, 0, NULL) == 0);
    setenv("TZ", "EST5", 1);
    CHECK(fresh(19, 18000, "EST") == 0);
    return 0;
}
