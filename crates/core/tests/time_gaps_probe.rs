//! The phase 9 time spec gaps, settled by black-box probe (TIME-SPEC-GAPS-SETTLE; record
//! `spec/behaviour/time-gaps.md`): `mktime` across skipped and repeated hours with `tm_isdst`
//! -1/0/1, POSIX `TZ` rules before 1970, `getdate` with `%Z` and with partial times, a signed `%y`
//! in `strptime`, and `TZ` unset with `/etc/localtime` unreadable. The probe [`PROBE`] is built
//! against the host C library and statically against Liella's products; the outputs must agree
//! line by line except the recorded differences in [`KNOWN`]. Every zone comes from a POSIX rule
//! string (no zone files; `TZDIR` names an empty directory of the test's own), and the
//! `/etc/localtime` cases run in a private user and mount namespace (`unshare -r -m`) over a tmpfs.
//! Needs the products (`cargo xtask so`) and a C compiler; without a compiler, or without
//! unprivileged namespaces for the last test, the test says so and passes.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects, missing_docs)]
#[path = "support/skip_note.rs"]
mod skip_note;

#[path = "support/timeout.rs"]
mod timeout;
#[path = "support/oracle.rs"]
mod oracle;
use std::path::{Path, PathBuf};
use std::process::Command;

const PROBE: &str = r##"/* time_gaps_probe.c: black-box probe of the phase 9 time spec gaps (TIME-SPEC-GAPS-SETTLE).
   Usage: time_gaps_probe <scratch-dir> [unset]. Prints one observation per line. */
#define _GNU_SOURCE
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <errno.h>

static void show_tm(const struct tm *t) {
    printf("%04d-%02d-%02d %02d:%02d:%02d wday=%d yday=%d isdst=%d off=%ld zone=%s", t->tm_year + 1900, t->tm_mon + 1, t->tm_mday,
           t->tm_hour, t->tm_min, t->tm_sec, t->tm_wday, t->tm_yday, t->tm_isdst, t->tm_gmtoff, t->tm_zone ? t->tm_zone : "(null)");
}

static void globals(void) { printf("tzname=[%s,%s] timezone=%ld daylight=%d\n", tzname[0], tzname[1], timezone, daylight); }

static void mk(int y, int mo, int d, int h, int mi, int isdst) {
    struct tm t;
    memset(&t, 0, sizeof t);
    t.tm_year = y - 1900; t.tm_mon = mo - 1; t.tm_mday = d; t.tm_hour = h; t.tm_min = mi; t.tm_isdst = isdst;
    errno = 0;
    time_t r = mktime(&t);
    printf("mktime %04d-%02d-%02d %02d:%02d isdst=%d -> %lld errno=%d ", y, mo, d, h, mi, isdst, (long long)r, r == (time_t)-1 ? errno : 0);
    show_tm(&t);
    printf("\n");
}

static void loc(int y, int mo, int d) {
    struct tm u;
    memset(&u, 0, sizeof u);
    u.tm_year = y - 1900; u.tm_mon = mo - 1; u.tm_mday = d; u.tm_hour = 12;
    time_t t = timegm(&u);
    struct tm r;
    if (!localtime_r(&t, &r)) { printf("localtime %04d-%02d-%02d 12:00Z -> NULL\n", y, mo, d); return; }
    printf("localtime %04d-%02d-%02d 12:00Z -> ", y, mo, d);
    show_tm(&r);
    printf("\n");
}

static void settz(const char *tz) {
    if (tz) setenv("TZ", tz, 1); else unsetenv("TZ");
    tzset();
    printf("== TZ %s\n", tz ? tz : "(unset)");
    globals();
}

static void gaps(void) {
    static const int iso[] = {-1, 0, 1};
    settz("EST5EDT,M3.2.0,M11.1.0");
    static const int spring[][2] = {{1, 30}, {2, 0}, {2, 30}, {2, 59}, {3, 0}, {3, 30}};
    static const int fall[][2] = {{0, 30}, {1, 0}, {1, 30}, {1, 59}, {2, 0}, {2, 30}};
    for (int i = 0; i < 6; i++) for (int k = 0; k < 3; k++) mk(2024, 3, 10, spring[i][0], spring[i][1], iso[k]);
    for (int i = 0; i < 6; i++) for (int k = 0; k < 3; k++) mk(2024, 11, 3, fall[i][0], fall[i][1], iso[k]);
    /* a repeated hour with tm_isdst -1 right after a summer and right after a winter conversion */
    mk(2024, 7, 1, 12, 0, -1); mk(2024, 11, 3, 1, 30, -1); mk(2024, 1, 15, 12, 0, -1); mk(2024, 11, 3, 1, 30, -1);
    settz("AEST-10AEDT,M10.1.0,M4.1.0/3");
    static const int au_on[][2] = {{1, 30}, {2, 30}, {3, 30}};
    static const int au_off[][2] = {{1, 30}, {2, 30}, {3, 30}};
    for (int i = 0; i < 3; i++) for (int k = 0; k < 3; k++) mk(2024, 10, 6, au_on[i][0], au_on[i][1], iso[k]);
    for (int i = 0; i < 3; i++) for (int k = 0; k < 3; k++) mk(2024, 4, 7, au_off[i][0], au_off[i][1], iso[k]);
    settz("<+1030>-10:30<+11>-11,M10.1.0,M4.1.0");
    static const int lh_on[][2] = {{1, 45}, {2, 15}, {2, 45}};
    static const int lh_off[][2] = {{1, 15}, {1, 45}, {2, 15}};
    for (int i = 0; i < 3; i++) for (int k = 0; k < 3; k++) mk(2024, 10, 6, lh_on[i][0], lh_on[i][1], iso[k]);
    for (int i = 0; i < 3; i++) for (int k = 0; k < 3; k++) mk(2024, 4, 7, lh_off[i][0], lh_off[i][1], iso[k]);
}

static void pre1970(const char *scratch) {
    char empty[4096];
    snprintf(empty, sizeof empty, "%s/emptyzones", scratch);
    setenv("TZDIR", empty, 1); /* a rule string without dates must not read the host's zone files */
    static const char *const tzs[] = {"EST5EDT,M3.2.0,M11.1.0", "CET-1CEST,M3.5.0,M10.5.0/3", "AAA3BBB,J60/2,J300/2", "AAA3BBB,59,299",
                                      "AAA-2BBB-3,M4.1.0/-1,M9.5.0/26", "EST5EDT"};
    static const int years[] = {1600, 1800, 1899, 1900, 1901, 1930, 1960, 1969, 1970, 2024, 2100};
    for (unsigned i = 0; i < sizeof tzs / sizeof *tzs; i++) {
        settz(tzs[i]);
        for (unsigned y = 0; y < sizeof years / sizeof *years; y++) { loc(years[y], 1, 15); loc(years[y], 7, 15); }
        mk(1960, 7, 15, 12, 0, -1);
        mk(1960, 1, 15, 12, 0, -1);
        mk(1900, 7, 15, 12, 0, -1);
    }
    unsetenv("TZDIR");
}

/* The bracket of a getdate call: the earliest second any clock reads just before it and the latest
   just after. time() and CLOCK_REALTIME_COARSE follow the clock tick and can trail CLOCK_REALTIME by
   up to a tick, so a call reading any of them reads a second in [lo() before, hi() after]. */
static long long clocks(int last) {
    struct timespec c, f;
    long long t = (long long)time(NULL);
    clock_gettime(CLOCK_REALTIME_COARSE, &c);
    clock_gettime(CLOCK_REALTIME, &f);
    long long m = t;
    if (last ? c.tv_sec > m : c.tv_sec < m) m = c.tv_sec;
    if (last ? f.tv_sec > m : f.tv_sec < m) m = f.tv_sec;
    return m;
}
static long long lo(void) { return clocks(0); }
static long long hi(void) { return clocks(1); }

/* Returns once every clock reads a second after `s`. */
static void past(long long s) {
    while (lo() <= s) {
        struct timespec ts = {0, 1000000};
        nanosleep(&ts, NULL);
    }
}

/* The test's instant (argv "@<epoch seconds>"), captured once by the test and given to both programs;
   -1 when absent. The TZ offsets of the getdate cases derive from it alone, never from the program's
   own clock, so both programs get the same rule string and a run straddling UTC midnight cannot give
   them two dates (#2012). */
static long long inst = -1;

/* The clock the getdate cases see, set again before every call (pin()): `std`<offset>`rest` with the
   offset chosen so CLOCK_REALTIME's current second reads `pin_tod` seconds after midnight on the date of
   the test's instant (pin_mode 1; 86400 and on: the next date), or the UTC instant `pin_at` (pin_mode 2, no DST correction). A
   call therefore sees its pinned second or the next, however long the run before it took. */
static const char *pin_std = "UTC", *pin_rest = "";
static int pin_mode;
static long pin_tod;
static long long pin_at;

static void settz_off(long w, struct tm *c, time_t now) {
    char tz[96];
    long a = w < 0 ? -w : w; /* the POSIX offset: west of UTC is positive */
    snprintf(tz, sizeof tz, "%s%s%ld:%02ld:%02ld%s", pin_std, w < 0 ? "-" : "", a / 3600, a / 60 % 60, a % 60, pin_rest);
    setenv("TZ", tz, 1);
    tzset();
    localtime_r(&now, c);
}

/* Pins the clock (see pin_mode) at CLOCK_REALTIME's current second, which it returns; `c` gets the
   local time pinned. */
static long long pin(struct tm *c) {
    struct timespec f;
    clock_gettime(CLOCK_REALTIME, &f);
    time_t now = f.tv_sec;
    if (pin_mode == 2) { settz_off((long)((long long)now - pin_at), c, now); return now; }
    long w = (long)((inst >= 0 ? inst : (long long)now) % 86400) - pin_tod;
    for (int i = 0; i < 3; i++) {
        settz_off(w, c, now);
        long got = c->tm_hour * 3600L + c->tm_min * 60L + c->tm_sec;
        if (got == pin_tod % 86400) break;
        /* DST in force (one more hour west), or the seconds since the test's instant, taken modulo a day */
        w += ((got - pin_tod % 86400) % 86400 + 86400 + 43200) % 86400 - 43200;
    }
    return now;
}

/* How a call is placed against a second boundary (the forced-edge test, argv "!<instant>,<edge>"):
   0 as it falls; 1 within the pinned second; 2 every clock passes the boundary after the bracket opens
   and before the call; 3 after the call and before the bracket closes. */
static int edge;

/* Prints the result's own fields and the local time of every second of the call's bracket; the test
   classifies each field as current or not against that bracket, per program. */
static void gd(const char *scratch, const char *tmpl, const char *in) {
    char path[4096];
    snprintf(path, sizeof path, "%s/datemsk", scratch);
    FILE *f = fopen(path, "w");
    if (!f) { printf("getdate: cannot write %s\n", path); exit(1); }
    fprintf(f, "%s\n", tmpl);
    fclose(f);
    setenv("DATEMSK", path, 1);
    struct tm c, *r;
    long long t0, t1;
    int placed, tries = 0;
    do { /* a forced edge is arranged again when the run was descheduled across the planned second */
        long long s = pin(&c);
        if (edge == 1) past(s - 1); /* the bracket is then the pinned second alone */
        t0 = lo();
        if (edge == 2) past(s);
        r = getdate(in);
        long long mid = hi();
        if (edge == 3) past(s);
        t1 = hi();
        placed = edge == 0 || (edge == 1 && t1 == s) || (edge == 2 && mid == s + 1) || (edge == 3 && mid == s);
    } while (!placed && ++tries < 20);
    printf("getdate(\"%s\", \"%s\") ->", in, tmpl);
    if (!placed) { printf(" edge not arranged\n"); return; }
    if (!r) { printf(" NULL getdate_err=%d\n", getdate_err); return; }
    printf(" at=%04d-%02d-%02d %02d:%02d:%02d w%d isdst=%d zone=%s now=", r->tm_year + 1900, r->tm_mon + 1, r->tm_mday,
           r->tm_hour, r->tm_min, r->tm_sec, r->tm_wday, r->tm_isdst, r->tm_zone ? r->tm_zone : "(null)");
    for (long long T = t0; T <= t1; T++) {
        time_t x = (time_t)T;
        struct tm q;
        localtime_r(&x, &q);
        printf("%s%04d-%02d-%02d %02d:%02d:%02d w%d", T == t0 ? "" : ",", q.tm_year + 1900, q.tm_mon + 1, q.tm_mday, q.tm_hour, q.tm_min, q.tm_sec, q.tm_wday);
    }
    printf("\n");
}

/* Pins the getdate clock at `want` seconds after midnight (on the date of the test's instant): the
   getdate cases see a clock the test sets, not the time of day of the run. Prints the rule without its
   offset and the local time pinned. */
static void clockat(const char *std, const char *rest, long want) {
    struct tm c;
    pin_std = std; pin_rest = rest; pin_mode = 1; pin_tod = want;
    pin(&c);
    printf("== clock %s%s at %02d:%02d:%02d\n", std, rest, c.tm_hour, c.tm_min, c.tm_sec);
}

static void getdates(const char *scratch) {
    static const char *const tzs[][2] = {{"UTC", ""}, {"EST", "EDT,M3.2.0,M11.1.0"}};
    static const char *const cases[][2] = {
        {"%H:%M %Z", "10:30 UTC"}, {"%H:%M %Z", "10:30 GMT"}, {"%H:%M %Z", "10:30 EST"}, {"%H:%M %Z", "10:30 EDT"},
        {"%H:%M %Z", "10:30 XYZ"}, {"%H:%M %Z", "10:30 +0100"}, {"%Z", "UTC"}, {"%Z", "EST"},
        {"%Y-%m-%d %H:%M %Z", "2024-07-15 10:30 UTC"}, {"%Y-%m-%d %H:%M %Z", "2024-07-15 10:30 EST"},
        {"%Y-%m-%d %H:%M %Z", "2024-01-15 10:30 EDT"},
        {"%H:%M", "00:00"}, {"%H:%M", "23:59"}, {"%H", "12"}, {"%M", "30"}, {"%S", "15"}, {"%H:%M:%S", "23:59:59"},
        {"%a", "Mon"}, {"%A", "Sunday"}, {"%a %H:%M", "Wed 10:00"}, {"%Y", "2030"}, {"%Y", "1990"}, {"%Y-%m", "2030-06"},
        {"%d", "15"}, {"%b %d", "Jun 15"}, {"%y", "30"}, {"%Y %H:%M", "2030 10:00"}, {"%Y-%m-%d", "2030-06-15"},
        {"%Y-%m-%d %a", "2030-06-15 Mon"}};
    for (unsigned t = 0; t < sizeof tzs / sizeof *tzs; t++) {
        clockat(tzs[t][0], tzs[t][1], 12 * 3600 + 30 * 60 + 30);
        for (unsigned i = 0; i < sizeof cases / sizeof *cases; i++) gd(scratch, cases[i][0], cases[i][1]);
    }
    /* A time alone against the current hour (s23_7_c_3): later, the same hour with the minutes ahead
       or past, earlier; at 12:30:30, then at 00:40:30 for minutes and seconds alone (hour 0). */
    static const char *const at1230[][2] = {{"%H:%M", "13:00"}, {"%H:%M", "12:45"}, {"%H:%M", "12:10"}, {"%H", "12"},
        {"%H:%M:%S", "12:29:59"}, {"%H:%M", "11:59"}, {"%H", "11"}, {"%M", "10"}};
    static const char *const at0040[][2] = {{"%M", "30"}, {"%M", "50"}, {"%M:%S", "40:10"}, {"%S", "15"}, {"%H", "0"}, {"%H:%M", "00:10"}};
    clockat("UTC", "", 12 * 3600 + 30 * 60 + 30);
    for (unsigned i = 0; i < sizeof at1230 / sizeof *at1230; i++) gd(scratch, at1230[i][0], at1230[i][1]);
    clockat("UTC", "", 40 * 60 + 30);
    for (unsigned i = 0; i < sizeof at0040 / sizeof *at0040; i++) gd(scratch, at0040[i][0], at0040[i][1]);
}

/* The controlled clock reads `at` (epoch seconds, within a day of the real clock) before every call:
   the rule is chosen from the test's instant, so UTC midnight is crossed (or not) as the test says. */
static void midnight(const char *scratch, long long at) {
    static const char *const tzs[][2] = {{"UTC", ""}, {"EST", "EDT,M3.2.0,M11.1.0"}};
    static const char *const cases[][2] = {{"%H:%M", "00:00"}, {"%H:%M", "23:59"}, {"%H", "12"}, {"%M", "30"}, {"%S", "15"},
        {"%a", "Mon"}, {"%A", "Sunday"}, {"%a %H:%M", "Wed 10:00"}, {"%Y", "2030"}, {"%Y-%m", "2030-06"}, {"%d", "15"}, {"%b %d", "Jun 15"},
        {"%y", "30"}, {"%Y-%m-%d", "2030-06-15"}, {"%H:%M %Z", "10:30 EST"}, {"%Y-%m-%d %H:%M %Z", "2024-07-15 10:30 UTC"}};
    for (unsigned t = 0; t < sizeof tzs / sizeof *tzs; t++) {
        struct tm c;
        pin_std = tzs[t][0]; pin_rest = tzs[t][1]; pin_mode = 2; pin_at = at;
        pin(&c);
        printf("== clock %s%s at %04d-%02d-%02d hour %02d\n", tzs[t][0], tzs[t][1], c.tm_year + 1900, c.tm_mon + 1, c.tm_mday, c.tm_hour);
        for (unsigned i = 0; i < sizeof cases / sizeof *cases; i++) gd(scratch, cases[i][0], cases[i][1]);
    }
}

/* The forced-edge test (argv "!<instant>,<edge>"): each case with the pinned clock one second before a
   second, minute, hour and day boundary (the boundary itself for edge 1), so the boundary falls inside
   the call's bracket where `edge` puts it. The headers do not depend on `edge`: the test compares a
   program forced one way with a program forced another. */
static void edges(const char *scratch) {
    static const struct { const char *name; long last; } b[] = {
        {"second", 12 * 3600 + 30 * 60 + 30}, {"minute", 12 * 3600 + 30 * 60 + 59}, {"hour", 12 * 3600 + 59 * 60 + 59}, {"midnight", 86399}};
    static const char *const cases[][2] = {{"%a", "Mon"}, {"%Y-%m-%d", "2030-06-15"}, {"%H:%M", "00:00"}};
    pin_std = "UTC"; pin_rest = ""; pin_mode = 1;
    for (unsigned i = 0; i < sizeof b / sizeof *b; i++) {
        printf("== edge %s\n", b[i].name);
        pin_tod = edge == 1 ? b[i].last + 1 : b[i].last;
        for (unsigned k = 0; k < sizeof cases / sizeof *cases; k++) gd(scratch, cases[k][0], cases[k][1]);
    }
}

static void signed_y(void) {
    printf("== strptime %%y\n");
    static const char *const ins[] = {"5", "05", "-5", "+5", "-05", "+05", " 5", "-0", "+99", "-99", "99", "100", "-", "+", "--5", "-5x"};
    static const char *const fmts[] = {"%y", "%C%y", "%y%m"};
    static const char *const ins2[] = {"20-5", "20+5", "-1-5", "-5-3", "+5+3"};
    for (unsigned i = 0; i < sizeof ins / sizeof *ins; i++) {
        struct tm t;
        memset(&t, 0, sizeof t);
        char *e = strptime(ins[i], fmts[0], &t);
        if (!e) printf("strptime(\"%s\", \"%%y\") -> NULL\n", ins[i]);
        else printf("strptime(\"%s\", \"%%y\") -> +%d year=%d\n", ins[i], (int)(e - ins[i]), t.tm_year + 1900);
    }
    for (unsigned f = 1; f < 3; f++)
        for (unsigned i = 0; i < sizeof ins2 / sizeof *ins2; i++) {
            struct tm t;
            memset(&t, 0, sizeof t);
            char *e = strptime(ins2[i], fmts[f], &t);
            if (!e) printf("strptime(\"%s\", \"%s\") -> NULL\n", ins2[i], fmts[f]);
            else printf("strptime(\"%s\", \"%s\") -> +%d year=%d mon=%d\n", ins2[i], fmts[f], (int)(e - ins2[i]), t.tm_year + 1900, t.tm_mon + 1);
        }
}

static void unset(void) {
    settz(NULL);
    static const int ys[][3] = {{1970, 1, 1}, {2024, 1, 15}, {2024, 7, 15}};
    for (int i = 0; i < 3; i++) loc(ys[i][0], ys[i][1], ys[i][2]);
    mk(2024, 7, 15, 12, 0, -1);
    mk(2024, 7, 15, 12, 0, 1);
}

int main(int argc, char **argv) {
    if (argc < 2) return 2;
    setvbuf(stdout, NULL, _IOLBF, 0);
    if (argc > 2 && strcmp(argv[2], "unset") == 0) { unset(); return 0; }
    if (argc > 2 && argv[2][0] == '@') inst = atoll(argv[2] + 1);
    if (argc > 2 && argv[2][0] == '=') { midnight(argv[1], atoll(argv[2] + 1)); return 0; }
    if (argc > 2 && argv[2][0] == '!') {
        const char *e = strchr(argv[2], ',');
        if (!e) return 2;
        inst = atoll(argv[2] + 1);
        edge = atoi(e + 1);
        edges(argv[1]);
        return 0;
    }
    gaps();
    pre1970(argv[1]);
    getdates(argv[1]);
    signed_y();
    return 0;
}
"##;

/// Lines that differ by decision: (patterns of the reference line, patterns Liella's line must
/// match, whether a run must show the difference, why). `*` stands for any text. An entry marked
/// `always` that no longer differs fails the test (the difference went away: update the record).
const KNOWN: &[(&[&str], &[&str], bool, &str)] = &[
    (
        &[
            "localtime 1600-07-15 12:00Z -> * isdst=0 *",
            "localtime 1800-07-15 12:00Z -> * isdst=0 *",
            "localtime 1899-07-15 12:00Z -> * isdst=0 *",
            "localtime 1900-07-15 12:00Z -> * isdst=0 *",
            "localtime 1901-07-15 12:00Z -> * isdst=0 *",
            "localtime 1930-07-15 12:00Z -> * isdst=0 *",
            "localtime 1960-07-15 12:00Z -> * isdst=0 *",
            "localtime 1969-07-15 12:00Z -> * isdst=0 *",
            "mktime 1960-07-15 12:00 isdst=-1 -> * isdst=0 *",
            "mktime 1900-07-15 12:00 isdst=-1 -> * isdst=0 *",
        ],
        &["* isdst=1 *"],
        true,
        "D-23.6 (s23_6_c_1): a POSIX TZ rule applies in every year; glibc keeps 1970's state for all earlier times",
    ),
    (
        &["getdate(\"2030-06\", \"%Y-%m\") -> *"],
        &["getdate(\"2030-06\", \"%Y-%m\") -> days=* year=2030 mon=6 mday=1 *"],
        false,
        "D-23.7 (s23_7_c_3): POSIX getdate assumes the first of the month when no day is given; glibc picks another day",
    ),
    (
        &[
            "strptime(\"+5\", \"%y\") -> NULL",
            "strptime(\"+05\", \"%y\") -> NULL",
            "strptime(\"-0\", \"%y\") -> NULL",
            "strptime(\"+99\", \"%y\") -> NULL",
            "strptime(\"20+5\", \"%C%y\") -> NULL",
            "strptime(\"+5+3\", \"%C%y\") -> NULL",
        ],
        &["* -> +*"],
        true,
        "D-23.7 (s23_7_c_2): POSIX lets %y (and %C) take a leading + or -; glibc rejects any sign",
    ),
    (
        &["mktime 2024-11-03 01:30 isdst=-1 -> * isdst=0 *"],
        &["* isdst=1 off=-14400 zone=EDT"],
        true,
        "D-23.5 (s23_5_c_2): tm_isdst -1 in a repeated hour is the first occurrence, always; glibc's choice follows the previous mktime call",
    ),
];

/// Whether `text` matches `pat`, where `*` stands for any text.
fn glob(pat: &str, text: &str) -> bool {
    let parts: Vec<&str> = pat.split('*').collect();
    if parts.len() == 1 {
        return pat == text;
    }
    let (first, last) = (parts[0], parts[parts.len() - 1]);
    if text.len() < first.len() + last.len() || !text.starts_with(first) || !text[first.len()..].ends_with(last) {
        return false;
    }
    let mut rest = &text[first.len()..text.len() - last.len()];
    for p in &parts[1..parts.len() - 1] {
        match rest.find(p) {
            Some(i) => rest = &rest[i + p.len()..],
            None => return false,
        }
    }
    true
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The probe built against the host C library and against the products, or None (no compiler).
fn programs() -> Option<(PathBuf, PathBuf, PathBuf)> {
    // The tests of this file run in parallel threads of one process and share the probe's source
    // and programs: one thread at a time writes and builds them (a fresh tree raced on the rename).
    static BUILD: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _one = BUILD.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let r = root();
    assert!(r.join("target/liella/libc.a").exists(), "the products are missing: run `cargo xtask so`");
    if skip_note::missing("cc") {
        skip_note::skipped("skipped: no C compiler");
        return None;
    }
    let dir = r.join("target/test-oracles");
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("time_gaps_probe.c");
    if std::fs::read_to_string(&src).ok().as_deref() != Some(PROBE) {
        let tmp = dir.join(format!("time_gaps_probe.c.{}", std::process::id()));
        std::fs::write(&tmp, PROBE).unwrap();
        std::fs::rename(&tmp, &src).unwrap();
    }
    let (host, ours) = (dir.join("time_gaps_probe_host"), dir.join("time_gaps_probe_liella"));
    let mut cmd = Command::new("cc");
    cmd.args(["-std=gnu17", "-O1"]).arg(&src);
    assert!(oracle::build(cmd, &host), "time_gaps_probe.c does not build against the host C library");
    let mut cmd = Command::new("cc");
    cmd.args(["-std=gnu17", "-O1", "-static", "-nostdlib", "-fno-builtin"]).arg(r.join("target/liella/liella-crt1.o")).arg(&src).arg(r.join("target/liella/libc.a")).arg("-lgcc");
    assert!(oracle::build(cmd, &ours), "time_gaps_probe.c does not link against the products");
    Some((host, ours, dir))
}

/// A private scratch directory with an empty `emptyzones` (the `TZDIR` of the rule strings).
fn scratch(dir: &Path, name: &str) -> PathBuf {
    let s = dir.join(format!("time_gaps_{name}.{}.d", std::process::id()));
    let _ = std::fs::remove_dir_all(&s);
    std::fs::create_dir_all(s.join("emptyzones")).unwrap();
    s
}

/// The real clock in epoch seconds, read once per test.
fn instant() -> i64 {
    i64::try_from(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()).unwrap()
}

/// Runs both programs with `arg` (`@<instant>` or `=<instant>`) and returns (reference, Liella) output.
fn run_both(host: &Path, ours: &Path, dir: &Path, arg: &str) -> (String, String) {
    let run = |exe: &Path| -> String {
        let s = scratch(dir, exe.file_name().unwrap().to_str().unwrap());
        let o = Command::new("timeout").arg(timeout::secs(120)).arg(exe).arg(&s).arg(arg).env_clear().output().unwrap();
        let _ = std::fs::remove_dir_all(&s);
        assert!(o.status.success(), "{} failed: {:?}", exe.display(), o.status);
        String::from_utf8_lossy(&o.stdout).into_owned()
    };
    (run(host), run(ours))
}

#[test]
fn time_gaps_equal_the_reference() {
    // Clauses: s23_5_c_2, s23_6_c_1, s23_7_c_2, s23_7_c_3 (record spec/behaviour/time-gaps.md).
    // One instant, read once, gives both programs the same rule strings (#2012).
    let Some((host, ours, dir)) = programs() else { return };
    let (want, got) = run_both(&host, &ours, &dir, &format!("@{}", instant()));
    check(&want, &got, true);
}

#[test]
fn time_gaps_agree_across_utc_midnight() {
    // Clauses: s23_7_c_3 (record spec/behaviour/time-gaps.md): the getdate dates follow a clock the test
    // sets (#2012): both programs run with it reading 23:59:50 UTC, and 00:00:10 UTC, the nearest such
    // instants to the real clock (within a day of it, so a rule string can carry the offset). The probe
    // pins the clock again before every call, so the 10 s margin bounds one call, not the run; a
    // program reading the real clock gives other dates and fails.
    let Some((host, ours, dir)) = programs() else { return };
    let now = instant();
    let before = (now + 12 * 3600) / 86400 * 86400 - 10; // 23:59:50 of some day, within 12 h of now
    let after = before + 20; // 00:00:10 of the next day
    let (want_b, got_b) = run_both(&host, &ours, &dir, &format!("={before}"));
    let (want_a, got_a) = run_both(&host, &ours, &dir, &format!("={after}"));
    assert_ne!(settled(&want_b), settled(&want_a), "the reference ignores the test's clock: the two sides of midnight give one answer");
    check(&want_b, &got_b, false);
    check(&want_a, &got_a, false);
}

#[test]
fn time_gaps_hold_across_forced_clock_edges() {
    // Clauses: s23_7_c_3 (record spec/behaviour/time-gaps.md): getdate's current hour, minute and second
    // are judged against each program's own clock reads around its call. Each program runs with a
    // second, minute, hour and day boundary placed in turn within its call's second (edge 1), between
    // the probe's first clock read and the call (2), and between the call and the probe's last read
    // (3); every pairing of two placements across the programs must agree.
    let Some((host, ours, dir)) = programs() else { return };
    let inst = instant();
    let run = |exe: &Path, e: u32| -> String {
        let s = scratch(&dir, &format!("edge{e}_{}", exe.file_name().unwrap().to_str().unwrap()));
        let o = Command::new("timeout").arg(timeout::secs(120)).arg(exe).arg(&s).arg(format!("!{inst},{e}")).env_clear().output().unwrap();
        let _ = std::fs::remove_dir_all(&s);
        assert!(o.status.success(), "{} failed: {:?}", exe.display(), o.status);
        let out = String::from_utf8_lossy(&o.stdout).into_owned();
        assert_eq!(out.matches("== edge ").count(), 4, "edge {e}: the probe did not run every boundary:\n{out}");
        assert!(!out.contains("edge not arranged"), "edge {e}: a call could not be placed against its boundary:\n{out}");
        out
    };
    let want: Vec<String> = (1..=3).map(|e| run(&host, e)).collect();
    let got: Vec<String> = (1..=3).map(|e| run(&ours, e)).collect();
    // Edges 1 and 2 call in the boundary's second, edge 3 in the second before. Two calls on either
    // side of a day boundary ask about two dates ("Mon" is days away from a Thursday and from a
    // Friday), so those pairings compare the second, minute and hour boundaries; the pairings with
    // both calls on one side compare all four, a bracket spanning midnight on both sides included.
    let after = |e: usize| e != 2;
    for (a, b) in [(0, 1), (1, 0), (1, 1), (2, 2), (0, 2), (2, 0), (1, 2), (2, 1)] {
        eprintln!("forced edges: reference edge {} against Liella edge {}", a + 1, b + 1);
        let keep = |o: &str| if after(a) == after(b) { o.to_owned() } else { o.split("== edge midnight").next().unwrap().to_owned() };
        check(&keep(&want[a]), &keep(&got[b]), false);
    }
}

/// A local time as the probe prints it: `Y-M-D h:m:s w<wday>`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Civil {
    y: i64,
    mo: i64,
    d: i64,
    h: i64,
    mi: i64,
    s: i64,
    wday: i64,
}

fn civil(t: &str) -> Option<Civil> {
    let n: Vec<i64> = t.split(['-', ' ', ':', 'w']).filter(|p| !p.is_empty()).map(str::parse).collect::<Result<_, _>>().ok()?;
    let [y, mo, d, h, mi, s, wday] = n[..] else { return None };
    Some(Civil { y, mo, d, h, mi, s, wday })
}

/// Days from 1970-01-01 to a proleptic Gregorian date (March-based years, 400-year eras of 146097 days).
fn day_number(c: &Civil) -> i64 {
    let y = if c.mo <= 2 { c.y - 1 } else { c.y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * ((c.mo + 9) % 12) + 2) / 5 + c.d - 1;
    era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468
}

/// A getdate line of the probe: its head (`getdate("in", "tmpl") ->`), the result's fields, the rest
/// of the result (`isdst=.. zone=..`) and the local time of every second of the call's bracket.
struct Gd<'a> {
    head: &'a str,
    at: Civil,
    tail: &'a str,
    now: Vec<Civil>,
}

fn gd(line: &str) -> Option<Gd<'_>> {
    let (head, rest) = line.split_once(" -> at=")?;
    let (at, rest) = rest.split_at(rest.find(" isdst=")?);
    let (tail, now) = rest.split_once(" now=")?;
    let now = if now.is_empty() { Vec::new() } else { now.split(',').map(civil).collect::<Option<_>>()? };
    Some(Gd { head, at: civil(at)?, tail, now })
}

/// The result `r` read against each second of `now`: days from that date, each of hour, minute and
/// second as `cur` when it equals that second's, and whether the weekday follows from the days.
fn render(r: &Gd<'_>, now: &[Civil]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for c in now {
        let days = day_number(&r.at) - day_number(c);
        let f = |n: &str, v: i64, cur: i64| if v == cur { format!(" {n}=cur") } else { format!(" {n}={v}") };
        let wday = if r.at.wday == (c.wday + days).rem_euclid(7) { "ok" } else { "BAD" };
        let line = format!(
            "{} -> days={days:+} year={} mon={} mday={}{}{}{} wday={wday}{}",
            r.head,
            r.at.y,
            r.at.mo,
            r.at.d,
            f("hour", r.at.h, c.h),
            f("min", r.at.mi, c.mi),
            f("sec", r.at.s, c.s),
            r.tail
        );
        if !out.contains(&line) {
            out.push(line);
        }
    }
    out
}

/// The two programs' lines as compared: a getdate line becomes its result read against a second of
/// its own program's bracket, the same reading for both when one exists (a call is current when it
/// matches its own clock, wherever a second boundary fell between the two programs' calls).
fn settle(w: &str, g: &str) -> (String, String) {
    let (Some(rw), Some(rg)) = (gd(w), gd(g)) else { return (w.to_owned(), g.to_owned()) };
    let (aw, ag) = (render(&rw, &rw.now), render(&rg, &rg.now));
    assert!(!aw.is_empty() && !ag.is_empty(), "a getdate call with an empty clock bracket:\n  glibc  {w}\n  liella {g}");
    match aw.iter().find(|a| ag.contains(a)) {
        Some(a) => (a.clone(), a.clone()),
        None => (aw[0].clone(), ag[0].clone()),
    }
}

/// An output with each getdate line read against the first second of its bracket.
fn settled(out: &str) -> String {
    out.lines().map(|l| settle(l, l).0 + "\n").collect()
}

/// `full`: the run covers every section, so each recorded difference must still show.
fn check(want: &str, got: &str, full: bool) {
    assert_eq!(want.lines().count(), got.lines().count(), "the outputs differ in length:\n{got}");
    let (mut bad, mut seen) = (Vec::new(), vec![false; KNOWN.len()]);
    for (w, g) in want.lines().zip(got.lines()) {
        let (w, g) = settle(w, g);
        let (w, g) = (w.as_str(), g.as_str());
        match KNOWN.iter().position(|(ref_pats, _, _, _)| ref_pats.iter().any(|p| glob(p, w))) {
            Some(i) => {
                if w != g {
                    seen[i] = true;
                }
                if !KNOWN[i].1.iter().any(|p| glob(p, g)) {
                    bad.push(format!("recorded difference ({}) has the wrong Liella line:\n  glibc  {w}\n  liella {g}", KNOWN[i].3));
                }
            }
            None if w != g => bad.push(format!("unrecorded difference:\n  glibc  {w}\n  liella {g}")),
            None => {}
        }
    }
    for (i, k) in KNOWN.iter().enumerate() {
        if full && k.2 && !seen[i] {
            bad.push(format!("recorded difference no longer seen: {}", k.3));
        }
    }
    assert!(bad.is_empty(), "{} problems:\n{}", bad.len(), bad.join("\n"));
}

/// Each state of `/etc/localtime`, set up in a private user and mount namespace over a tmpfs
/// `/etc`; `eacces` drops every capability (securebits noroot) so the mode-0 file cannot be read.
const UNSET: &str = r#"p=$1; s=$2
for v in missing empty garbage dir eacces dangling; do
  echo "== variant $v"
  unshare -r -m sh -c '
    v=$1; p=$2; s=$3
    mount -t tmpfs none /etc || exit 3
    case $v in
      empty) : > /etc/localtime ;;
      garbage) printf "TZif2 not a zone file\n" > /etc/localtime ;;
      dir) mkdir /etc/localtime ;;
      eacces) printf "TZif2" > /etc/localtime; chmod 000 /etc/localtime ;;
      dangling) ln -s /nonexistent/zone /etc/localtime ;;
    esac
    drop="setpriv --securebits +noroot,+noroot_locked,+no_setuid_fixup,+no_setuid_fixup_locked --bounding-set -all --inh-caps -all --"
    if [ $v = eacces ]; then
      $drop sh -c "cat /etc/localtime" >/dev/null 2>&1 && echo "setup: readable" || echo "setup: unreadable"
      exec $drop "$p" "$s" unset
    fi
    exec "$p" "$s" unset
  ' sh "$v" "$p" "$s" 2>&1
  echo "rc=$?"
done
"#;

#[test]
fn tz_unset_with_localtime_unreadable_is_utc() {
    // Clauses: s23_6_c_0, s23_6_c_1 (record spec/behaviour/time-gaps.md): TZ unset and /etc/localtime
    // missing, empty, not a zone file, a directory, unreadable (EACCES) or a dangling link: UTC.
    let Some((host, ours, dir)) = programs() else { return };
    if skip_note::missing("unshare") || skip_note::missing("setpriv") || !Command::new("unshare").args(["-r", "-m", "true"]).status().is_ok_and(|s| s.success()) {
        skip_note::skipped("skipped: no unprivileged user and mount namespaces");
        return;
    }
    let run = |exe: &Path| -> String {
        let s = scratch(&dir, &format!("unset_{}", exe.file_name().unwrap().to_str().unwrap()));
        let o = Command::new("timeout").arg(timeout::secs(120)).args(["sh", "-c", UNSET, "sh"]).arg(exe).arg(&s).env_clear().env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin").output().unwrap();
        let _ = std::fs::remove_dir_all(&s);
        assert!(o.status.success(), "unset runner failed: {:?}", o.status);
        String::from_utf8_lossy(&o.stdout).into_owned()
    };
    let (want, got) = (run(&host), run(&ours));
    assert!(!want.contains("setup: readable") && want.contains("setup: unreadable"), "the EACCES case was not set up:\n{want}");
    assert_eq!(want.matches("rc=0").count(), 6, "a variant failed under the reference:\n{want}");
    assert_eq!(want.matches("tzname=[UTC,UTC] timezone=0 daylight=0").count(), 6, "the reference is not UTC in every variant:\n{want}");
    assert_eq!(want, got, "unset: Liella differs from the reference");
}
