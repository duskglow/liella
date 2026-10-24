//! Formatting and parsing time end to end (spec §23.7; D-23.7): `tests/oracle/timefmt_diff.c` built
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
    let src = r.join("crates/core/tests/oracle/timefmt_diff.c");
    let mut cmd = Command::new("cc");
    if liella {
        cmd.args(["-std=gnu17", "-O1", "-static", "-nostdlib", "-fno-builtin"]).arg(r.join("target/liella/liella-crt1.o")).arg(&src).arg(r.join("target/liella/libc.a")).arg("-lgcc");
    } else {
        cmd.args(["-std=gnu17", "-O1"]).arg(&src);
    }
    oracle::build(cmd, out)
}

/// Lines that differ from the reference by decision (D-23.7): patterns of the reference line
/// (`*` stands for any text), a pattern of the section it is in (the text after `== `), and why.
/// One entry per cause; an entry that no longer matches any differing line fails the test.
const KNOWN: &[(&[&str], &str, &str)] = &[
    (
        &["* %10z [*", "* %_z [*", "* %-z [*"],
        "strftime *",
        "%z is the sign and hhmm as one field: flags and width pad it as a whole (glibc pads the digits after the sign, and twice for a width)",
    ),
    (&["4 %C [0] 1", "4 %EC [0] 1"], "strftime *", "%C has at least two digits (ISO C 00-99; POSIX: the number of digits or two)"),
    (&["4 %#B [M\\xC3\\xA4RZ] 5"], "strftime de_DE.UTF-8", "case changes are per character (LC_CTYPE's maps), as wcsftime's; glibc's strftime changes ASCII bytes only"),
    (
        &["strptime(\"*\", \"%Ey\") -> NULL", "strptime(\"*\", \"%OH:%OM\") -> NULL", "strptime(\"*\", \"%Od %Om\") -> NULL"],
        "strptime *",
        "POSIX: an E or O conversion the locale has no alternative for behaves as the unmodified one",
    ),
    (
        &["strptime(\"*\", \"%Y %m\") -> *", "strptime(\"*\", \"%b\") -> *", "strptime(\"*\", \"%h\") -> *", "strptime(\"*\", \"%A %B\") -> *", "strptime(\"*\", \"%Od %Om\") -> +*"],
        "strptime *",
        "tm_wday and tm_yday are computed only from fields the input gave (glibc also reads the caller's untouched fields)",
    ),
    (&["strptime(\"-5\", \"%Y\") -> NULL", "strptime(\"+2024\", \"%Y\") -> NULL"], "strptime *", "POSIX: %Y accepts a leading + or -"),
    (&["strptime(\"-1\", \"%s\") -> NULL"], "strptime *", "%s reads the negative values strftime's %s prints"),
    (&["strptime(\"*\", \"%r\") -> NULL"], "strptime * (localized)", "without T_FMT_AMPM, %r is %I:%M:%S %p for strptime as for strftime, so its output reads back"),
    (
        &["strptime(\"10:11:12\", \"%X\") -> +8 *"],
        "strptime en_US.UTF-8",
        "%c %x %X %r read the locale's format only; glibc retries the C locale's when it fails (en_US's %X needs AM/PM)",
    ),
    (&["DATEMSK missing: NULL getdate_err=3"], "getdate", "POSIX: a template file that cannot be opened is 2 (glibc reports 3)"),
];

/// Whether `text` matches `pat`, where `*` stands for any text.
fn glob(pat: &str, text: &str) -> bool {
    let parts: Vec<&str> = pat.split('*').collect();
    let (first, last) = (parts[0], parts[parts.len() - 1]);
    if parts.len() == 1 {
        return pat == text;
    }
    if !text.starts_with(first) || !text[first.len()..].ends_with(last) || text.len() < first.len() + last.len() {
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

#[test]
fn timefmt_equals_the_reference() {
    // Clauses: s23_7_c_1, s23_7_c_1_1, s23_7_c_1_2, s23_7_c_1_3, s23_7_c_1_4 (D-23.7; §23.7 strptime and
    // getdate are not parsed into spec/clauses.tsv).
    let r = root();
    assert!(r.join("target/liella/libc.a").exists(), "the products are missing: run `cargo xtask so`");
    let locales = r.join("target/test-locales");
    std::fs::create_dir_all(&locales).unwrap();
    if !["en_US.UTF-8", "de_DE.UTF-8", "ja_JP.UTF-8", "th_TH.UTF-8", "fa_IR.UTF-8"].iter().all(|n| ensure_locale(&locales, n)) {
        skip_note::skipped("skipped: no localedef");
        return;
    }
    let dir = r.join("target/test-oracles");
    std::fs::create_dir_all(&dir).unwrap();
    let (host, ours) = (dir.join("timefmt_diff_host"), dir.join("timefmt_diff_liella"));
    // Without a C compiler the test cannot run; with one, the program must build.
    if skip_note::missing("cc") {
        skip_note::skipped("skipped: no C compiler");
        return;
    }
    assert!(build(&host, false), "timefmt_diff.c does not build against the host C library");
    assert!(build(&ours, true), "timefmt_diff.c does not link against the products");
    let run = |exe: &Path| -> String {
        // A hang (a timer that never fires) fails the test instead of stopping it.
        let scratch = dir.join(format!("{}.d", exe.file_name().unwrap().to_string_lossy()));
        let _ = std::fs::remove_dir_all(&scratch);
        std::fs::create_dir_all(&scratch).unwrap();
        let o = Command::new("timeout").arg(timeout::secs(120)).arg(exe).arg(&scratch).env_clear().env("LOCPATH", &locales).output().unwrap();
        assert!(o.status.success(), "{} failed: {:?}", exe.display(), o.status);
        String::from_utf8_lossy(&o.stdout).into_owned()
    };
    let (want, got) = (run(&host), run(&ours));
    let (mut diffs, mut locale, mut seen) = (0, String::from("C"), vec![false; KNOWN.len()]);
    for (k, (w, g)) in want.lines().zip(got.lines()).enumerate() {
        if let Some(rest) = w.strip_prefix("== ") {
            locale = rest.to_string();
        }
        let known = KNOWN.iter().position(|(pats, section, _)| glob(section, &locale) && pats.iter().any(|p| glob(p, w)));
        if let Some(i) = known {
            if w != g {
                seen[i] = true;
                eprintln!("recorded difference ({}), {locale}:\n  glibc  {w}\n  liella {g}", KNOWN[i].2);
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
