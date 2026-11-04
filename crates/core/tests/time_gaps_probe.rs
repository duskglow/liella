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

static void field(const char *n, int v, int cur) {
    if (v == cur) printf(" %s=cur", n); else printf(" %s=%d", n, v);
}

static void gd(const char *scratch, const char *tmpl, const char *in) {
    char path[4096];
    snprintf(path, sizeof path, "%s/datemsk", scratch);
    FILE *f = fopen(path, "w");
    if (!f) { printf("getdate: cannot write %s\n", path); exit(1); }
    fprintf(f, "%s\n", tmpl);
    fclose(f);
    setenv("DATEMSK", path, 1);
    time_t now;
    struct tm c, *r;
    int tries = 0;
    do { /* again if the second changed during the call: the current fields must be the call's */
        now = time(NULL);
        localtime_r(&now, &c);
        r = getdate(in);
    } while (time(NULL) != now && ++tries < 5);
    printf("getdate(\"%s\", \"%s\") ->", in, tmpl);
    if (!r) { printf(" NULL getdate_err=%d\n", getdate_err); return; }
    struct tm a = *r, b = c;
    a.tm_hour = b.tm_hour = 12; a.tm_min = b.tm_min = a.tm_sec = b.tm_sec = 0; a.tm_isdst = b.tm_isdst = -1;
    long days = (long)((timegm(&a) - timegm(&b)) / 86400);
    printf(" days=%+ld", days);
    printf(" year=%d mon=%d mday=%d", r->tm_year + 1900, r->tm_mon + 1, r->tm_mday);
    field("hour", r->tm_hour, c.tm_hour); field("min", r->tm_min, c.tm_min); field("sec", r->tm_sec, c.tm_sec);
    printf(" wday=%s isdst=%d zone=%s\n", r->tm_wday == (c.tm_wday + days % 7 + 7) % 7 ? "ok" : "BAD", r->tm_isdst, r->tm_zone ? r->tm_zone : "(null)");
}

/* The test's instant (argv "@<epoch seconds>"), captured once by the test and given to both programs;
   -1 when absent. The TZ offsets of the getdate cases derive from it alone, never from the program's
   own clock, so both programs get the same rule string and a run straddling UTC midnight cannot give
   them two dates (#2012). */
static long long inst = -1;

/* Sets TZ to `std`<offset>`rest`, the offset chosen so the local time of day is now `want` seconds
   after midnight (on the date of the test's instant): the getdate cases see a clock the test sets, not
   the time of day of the run. Prints the rule without its offset and the local time reached. */
static void clockat(const char *std, const char *rest, long want) {
    time_t now = time(NULL);
    /* the POSIX offset: west of UTC is positive */
    long w = (long)((inst >= 0 ? inst : (long long)now) % 86400) - want;
    char tz[96];
    struct tm c;
    for (int i = 0; i < 3; i++) {
        long a = w < 0 ? -w : w;
        snprintf(tz, sizeof tz, "%s%s%ld:%02ld:%02ld%s", std, w < 0 ? "-" : "", a / 3600, a / 60 % 60, a % 60, rest);
        setenv("TZ", tz, 1);
        tzset();
        localtime_r(&now, &c);
        long got = c.tm_hour * 3600L + c.tm_min * 60L + c.tm_sec;
        if (got == want) break;
        w += got - want; /* DST in force: one more hour west */
    }
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
        clockat(tzs[t][0], tzs[t][1], 12 * 3600 + 30 * 60);
        for (unsigned i = 0; i < sizeof cases / sizeof *cases; i++) gd(scratch, cases[i][0], cases[i][1]);
    }
    /* A time alone against the current hour (s23_7_c_3): later, the same hour with the minutes ahead
       or past, earlier; at 12:30:00, then at 00:40:30 for minutes and seconds alone (hour 0). */
    static const char *const at1230[][2] = {{"%H:%M", "13:00"}, {"%H:%M", "12:45"}, {"%H:%M", "12:10"}, {"%H", "12"},
        {"%H:%M:%S", "12:29:59"}, {"%H:%M", "11:59"}, {"%H", "11"}, {"%M", "10"}};
    static const char *const at0040[][2] = {{"%M", "30"}, {"%M", "50"}, {"%M:%S", "40:10"}, {"%S", "15"}, {"%H", "0"}, {"%H:%M", "00:10"}};
    clockat("UTC", "", 12 * 3600 + 30 * 60);
    for (unsigned i = 0; i < sizeof at1230 / sizeof *at1230; i++) gd(scratch, at1230[i][0], at1230[i][1]);
    clockat("UTC", "", 40 * 60 + 30);
    for (unsigned i = 0; i < sizeof at0040 / sizeof *at0040; i++) gd(scratch, at0040[i][0], at0040[i][1]);
}

/* The controlled clock reads `at` (epoch seconds, within a day of the real clock) when this starts:
   the rule is chosen from the test's instant, so UTC midnight is crossed (or not) as the test says. */
static void midnight(const char *scratch, long long at) {
    static const char *const tzs[][2] = {{"UTC", ""}, {"EST", "EDT,M3.2.0,M11.1.0"}};
    static const char *const cases[][2] = {{"%H:%M", "00:00"}, {"%H:%M", "23:59"}, {"%H", "12"}, {"%M", "30"}, {"%S", "15"},
        {"%a", "Mon"}, {"%A", "Sunday"}, {"%a %H:%M", "Wed 10:00"}, {"%Y", "2030"}, {"%Y-%m", "2030-06"}, {"%d", "15"}, {"%b %d", "Jun 15"},
        {"%y", "30"}, {"%Y-%m-%d", "2030-06-15"}, {"%H:%M %Z", "10:30 EST"}, {"%Y-%m-%d %H:%M %Z", "2024-07-15 10:30 UTC"}};
    for (unsigned t = 0; t < sizeof tzs / sizeof *tzs; t++) {
        time_t now = time(NULL);
        long w = (long)((long long)now - at); /* the POSIX offset: west of UTC is positive */
        long a = w < 0 ? -w : w;
        char tz[96];
        struct tm c;
        snprintf(tz, sizeof tz, "%s%s%ld:%02ld:%02ld%s", tzs[t][0], w < 0 ? "-" : "", a / 3600, a / 60 % 60, a % 60, tzs[t][1]);
        setenv("TZ", tz, 1);
        tzset();
        localtime_r(&now, &c);
        printf("== clock %s%s at %04d-%02d-%02d hour %02d\n", tzs[t][0], tzs[t][1], c.tm_year + 1900, c.tm_mon + 1, c.tm_mday, c.tm_hour);
        for (unsigned i = 0; i < sizeof cases / sizeof *cases; i++) gd(scratch, cases[i][0], cases[i][1]);
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
    // instants to the real clock (within a day of it, so a rule string can carry the offset). The 10 s
    // margin is the time the run may take; a program reading the real clock gives other dates and fails.
    let Some((host, ours, dir)) = programs() else { return };
    let now = instant();
    let before = (now + 12 * 3600) / 86400 * 86400 - 10; // 23:59:50 of some day, within 12 h of now
    let after = before + 20; // 00:00:10 of the next day
    let (want_b, got_b) = run_both(&host, &ours, &dir, &format!("={before}"));
    let (want_a, got_a) = run_both(&host, &ours, &dir, &format!("={after}"));
    assert_ne!(want_b, want_a, "the reference ignores the test's clock: the two sides of midnight give one answer");
    check(&want_b, &got_b, false);
    check(&want_a, &got_a, false);
}

/// `full`: the run covers every section, so each recorded difference must still show.
fn check(want: &str, got: &str, full: bool) {
    assert_eq!(want.lines().count(), got.lines().count(), "the outputs differ in length:\n{got}");
    let (mut bad, mut seen) = (Vec::new(), vec![false; KNOWN.len()]);
    for (w, g) in want.lines().zip(got.lines()) {
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
