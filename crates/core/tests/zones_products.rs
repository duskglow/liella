//! Broken-down time and time zones end to end (spec §23.5-23.6; D-23.4 to D-23.6):
//! `tests/oracle/zones_diff.c` built
//! against the host C library and statically against Liella's products, run with the same
//! `LOCPATH`; the outputs must be identical line by line, except the recorded differences in
//! [`KNOWN`]. Needs the products (`cargo xtask so`), a C compiler and `localedef`; without the
//! latter two the test says so and passes.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects, missing_docs)]
#[path = "support/skip_note.rs"]
mod skip_note;

#[path = "support/timeout.rs"]
mod timeout;
#[path = "support/oracle.rs"]
mod oracle;
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn ensure_locale(dir: &Path, name: &str) -> bool {
    if dir.join(name).join("LC_COLLATE").exists() {
        return true;
    }
    let Some((lang, charset)) = name.split_once('.') else { return false };
    oracle::locale(&dir.join(name), lang, charset, "LC_COLLATE")
}

fn build(out: &Path, liella: bool) -> bool {
    let r = root();
    let src = r.join("crates/core/tests/oracle/zones_diff.c");
    let mut cmd = Command::new("cc");
    if liella {
        cmd.args(["-std=gnu17", "-O1", "-static", "-nostdlib", "-fno-builtin"]).arg(r.join("target/liella/liella-crt1.o")).arg(&src).arg(r.join("target/liella/libc.a")).arg("-lgcc");
    } else {
        cmd.args(["-std=gnu17", "-O1"]).arg(&src);
    }
    oracle::build(cmd, out)
}

// Why each recorded difference is kept (Liella's choice where glibc's behaviour is an artifact or
// departs from the spec; D-23.5, D-23.6).
const FIRST: &str = "mktime, tm_isdst=-1, repeated hour: glibc's choice depends on the previous mktime call of the process (the first call, east of UTC, takes the later occurrence; later calls follow the previous result's offset); Liella always takes the first occurrence";
const BEFORE_1970: &str = "a POSIX rule is an annual rule without a first year: Liella applies it in every year; glibc keeps the state of 1970 for every earlier time (so New Zealand's July 1950 is daylight time)";
const DATE: &str = "a rule date outside the POSIX ranges (month 1-12, week 1-5, Jn 1-365, n 0-365) is not valid: Liella uses the default date (M3.2.0); glibc computes odd transitions from it";
const OFFSET: &str = "a standard offset beyond 24 hours is not valid POSIX (hh 0-24): Liella treats the TZ value as invalid (UTC named AAA); glibc clamps it to 24 hours";
const ASCTIME: &str = "asctime's text is the fixed 26-byte layout (spec 23.5): wider fields are EOVERFLOW for asctime as for asctime_r; glibc prints them into a larger static buffer";
const ERRNO: &str = "glibc sets errno to EOVERFLOW on this success (errno is written only on failure, X3) and, for Lord Howe, reads standard time with a historical offset (+10:00); Liella uses the nearest standard offset (+10:30)";

/// Lines that differ from the reference by decision: text the line starts with, the section it
/// is under (`*` any), whether the entry also covers the `  normalized:` line that follows (the
/// `struct tm` of the same call), and why. A recorded difference that is no longer seen (in the
/// line or the line it covers) fails the test.
const KNOWN: [(&str, &str, bool, &str); 45] = [
    ("asctime mday 123 hour 100:", "asctime", false, ASCTIME),
    ("transition 0 wall+-899s isdst=-1:", "TZ=Australia/Lord_Howe", true, FIRST),
    ("mktime year INT_MAX:", "TZ=Australia/Lord_Howe", false, ERRNO),
    ("transition 0 wall+-1799s isdst=-1:", "TZ=Africa/Casablanca", true, FIRST),
    ("1850-07-15T12Z:", "TZ=EST5EDT,M3.2.0,M11.1.0", false, BEFORE_1970),
    ("1900-07-15T12Z:", "TZ=EST5EDT,M3.2.0,M11.1.0", false, BEFORE_1970),
    ("1950-07-15T12Z:", "TZ=EST5EDT,M3.2.0,M11.1.0", false, BEFORE_1970),
    ("1850-07-15T12Z:", "TZ=CET-1CEST,M3.5.0,M10.5.0/3", false, BEFORE_1970),
    ("1900-07-15T12Z:", "TZ=CET-1CEST,M3.5.0,M10.5.0/3", false, BEFORE_1970),
    ("1950-07-15T12Z:", "TZ=CET-1CEST,M3.5.0,M10.5.0/3", false, BEFORE_1970),
    ("1850-07-15T12Z:", "TZ=NZST-12NZDT,M9.5.0,M4.1.0/3", false, BEFORE_1970),
    ("1900-07-15T12Z:", "TZ=NZST-12NZDT,M9.5.0,M4.1.0/3", false, BEFORE_1970),
    ("1950-07-15T12Z:", "TZ=NZST-12NZDT,M9.5.0,M4.1.0/3", false, BEFORE_1970),
    ("transition 0 wall+-1799s isdst=-1:", "TZ=NZST-12NZDT,M9.5.0,M4.1.0/3", true, FIRST),
    ("mktime year INT_MAX:", "TZ=NZST-12NZDT,M9.5.0,M4.1.0/3", false, ERRNO),
    ("1850-07-15T12Z:", "TZ=<-03>3<-02>,M3.5.0/-2,M10.5.0/-1", false, BEFORE_1970),
    ("1900-07-15T12Z:", "TZ=<-03>3<-02>,M3.5.0/-2,M10.5.0/-1", false, BEFORE_1970),
    ("1950-07-15T12Z:", "TZ=<-03>3<-02>,M3.5.0/-2,M10.5.0/-1", false, BEFORE_1970),
    ("1850-07-15T12Z:", "TZ=AAA3BBB,J60/2,J300/2", false, BEFORE_1970),
    ("1900-07-15T12Z:", "TZ=AAA3BBB,J60/2,J300/2", false, BEFORE_1970),
    ("1950-07-15T12Z:", "TZ=AAA3BBB,J60/2,J300/2", false, BEFORE_1970),
    ("1850-07-15T12Z:", "TZ=AAA3BBB,59/2,299/2", false, BEFORE_1970),
    ("1900-07-15T12Z:", "TZ=AAA3BBB,59/2,299/2", false, BEFORE_1970),
    ("1950-07-15T12Z:", "TZ=AAA3BBB,59/2,299/2", false, BEFORE_1970),
    ("1850-07-15T12Z:", "TZ=AAA3BBB,M3.2.0/-1,M11.1.0/26", false, BEFORE_1970),
    ("1900-07-15T12Z:", "TZ=AAA3BBB,M3.2.0/-1,M11.1.0/26", false, BEFORE_1970),
    ("1950-07-15T12Z:", "TZ=AAA3BBB,M3.2.0/-1,M11.1.0/26", false, BEFORE_1970),
    ("1850-07-15T12Z:", "TZ=AAA3BBB,M3.2.0/167,M11.1.0/-167", false, BEFORE_1970),
    ("1900-07-15T12Z:", "TZ=AAA3BBB,M3.2.0/167,M11.1.0/-167", false, BEFORE_1970),
    ("1950-07-15T12Z:", "TZ=AAA3BBB,M3.2.0/167,M11.1.0/-167", false, BEFORE_1970),
    ("1850-07-15T12Z:", "TZ=AAA3BBB2,M3.2.0,M11.1.0", false, BEFORE_1970),
    ("1900-07-15T12Z:", "TZ=AAA3BBB2,M3.2.0,M11.1.0", false, BEFORE_1970),
    ("1950-07-15T12Z:", "TZ=AAA3BBB2,M3.2.0,M11.1.0", false, BEFORE_1970),
    ("1850-07-15T12Z:", "TZ=AAA3BBB-1:30,M3.2.0,M11.1.0", false, BEFORE_1970),
    ("1900-07-15T12Z:", "TZ=AAA3BBB-1:30,M3.2.0,M11.1.0", false, BEFORE_1970),
    ("1950-07-15T12Z:", "TZ=AAA3BBB-1:30,M3.2.0,M11.1.0", false, BEFORE_1970),
    ("", "TZ=AAA-25", false, OFFSET),
    ("", "TZ=AAA25", false, OFFSET),
    ("", "TZ=AAA3BBB,M13.1.0,M11.1.0", false, DATE),
    ("", "TZ=AAA3BBB,M3.6.0,M11.1.0", false, DATE),
    ("", "TZ=AAA3BBB,J0/2,J300/2", false, DATE),
    ("", "TZ=AAA3BBB,366/2,J300/2", false, DATE),
    ("1850-07-15T12Z:", "TZ=AAA3BBB,M3.2.0", false, BEFORE_1970),
    ("1900-07-15T12Z:", "TZ=AAA3BBB,M3.2.0", false, BEFORE_1970),
    ("1950-07-15T12Z:", "TZ=AAA3BBB,M3.2.0", false, BEFORE_1970),
];

#[test]
fn zones_equal_the_reference() {
    // Clauses: s23_5_c_0, s23_5_c_1, s23_5_c_2, s23_5_c_3, s23_5_c_4, s23_5_c_5, s23_5_c_6, s23_5_g_0, s23_5_f_1,
    // s23_5_f_2, s23_5_f_3, s23_6_c_0, s23_6_c_1, s23_6_c_2, s23_6_c_3, s23_6_g_0 (D-23.4 to D-23.6).
    let r = root();
    assert!(r.join("target/liella/libc.a").exists(), "the products are missing: run `cargo xtask so`");
    let locales = r.join("target/test-locales");
    std::fs::create_dir_all(&locales).unwrap();
    if !["en_US.UTF-8", "de_DE.UTF-8"].iter().all(|n| ensure_locale(&locales, n)) {
        skip_note::skipped("skipped: no localedef");
        return;
    }
    let dir = r.join("target/test-oracles");
    std::fs::create_dir_all(&dir).unwrap();
    let (host, ours) = (dir.join("zones_diff_host"), dir.join("zones_diff_liella"));
    // Without a C compiler the test cannot run; with one, the program must build.
    if skip_note::missing("cc") {
        skip_note::skipped("skipped: no C compiler");
        return;
    }
    assert!(build(&host, false), "zones_diff.c does not build against the host C library");
    assert!(build(&ours, true), "zones_diff.c does not link against the products");
    let run = |exe: &Path| -> String {
        // A hang (a timer that never fires) fails the test instead of stopping it.
        let o = Command::new("timeout").arg(timeout::secs(120)).arg(exe).env_clear().env("LOCPATH", &locales).output().unwrap();
        assert!(o.status.success(), "{} failed: {:?}", exe.display(), o.status);
        String::from_utf8_lossy(&o.stdout).into_owned()
    };
    let (want, got) = (run(&host), run(&ours));
    let (mut diffs, mut locale, mut seen) = (0, String::from("C"), [false; KNOWN.len()]);
    let (wl, gl): (Vec<&str>, Vec<&str>) = (want.lines().collect(), got.lines().collect());
    let mut covered = None;
    for (k, (w, g)) in wl.iter().zip(gl.iter()).enumerate() {
        if let Some(rest) = w.strip_prefix("== ") {
            locale = rest.to_string();
        }
        if covered == Some(k) {
            continue;
        }
        let known = KNOWN.iter().position(|(needle, l, _, _)| (*l == "*" || *l == locale) && w.starts_with(needle));
        if let Some(i) = known {
            let mut differs = w != g;
            if KNOWN[i].2 && wl.get(k + 1).is_some_and(|n| n.starts_with("  normalized:")) {
                covered = Some(k + 1);
                differs |= wl.get(k + 1) != gl.get(k + 1);
            }
            if differs {
                seen[i] = true;
                eprintln!("recorded difference ({}), {locale}:\n  glibc  {w}\n  liella {g}", KNOWN[i].3);
            }
            continue;
        }
        if w != g {
            diffs += 1;
            if diffs <= 40 {
                eprintln!("line {} ({locale}):\n  glibc  {}\n  liella {}", k + 1, &w[..w.len().min(400)], &g[..g.len().min(400)]);
            }
        }
    }
    assert_eq!(want.lines().count(), got.lines().count(), "line counts differ");
    assert_eq!(diffs, 0, "lines that differ from the reference");
    for (i, s) in seen.iter().enumerate() {
        assert!(s, "recorded difference no longer seen: {:?}", KNOWN[i]);
    }
}
