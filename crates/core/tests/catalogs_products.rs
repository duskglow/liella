//! Message catalogs end to end (spec §7.7, §7.8; D-7.5, D-7.6; Appendix D.2):
//! `tests/oracle/catalogs_diff.c` built against the host C library and statically against
//! Liella's products, each run by `tests/oracle/catalogs_run.sh` (fixture catalogs built with
//! `msgfmt` and `gencat`, every other catalog written by the probe below a scratch directory under
//! `target/`), with `LOCPATH` naming the test locales; Liella with the name-service cache on and
//! off (`nscache_mode.sh`, D-16.10). The outputs must be identical line by line, except the
//! recorded differences: [`KNOWN`] (a line that differs), [`ADDED`] (a line only Liella prints) and
//! [`DROPPED`] (a line only the reference prints). Every entry names the case it belongs to: the
//! text of the last `== ` or `-- ` header line before it starts with the entry's first string, so a
//! line repeated in many cases is recorded only where it differs.
//!
//! The record (`spec/behaviour/catalogs.glibc.txt`) was made on the test machine; the test reports
//! whether the reference still equals it.
//!
//! Needs the products (`cargo xtask so`), a C compiler, `localedef`, `msgfmt` (package `gettext`),
//! `gencat` and passwordless `sudo` (the name-service cache modes); without them the test says so
//! and passes.
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
    let src = r.join("crates/core/tests/oracle/catalogs_diff.c");
    let mut cmd = Command::new("cc");
    if liella {
        cmd.args(["-std=gnu17", "-O1", "-static", "-nostdlib", "-fno-builtin"]).arg(r.join("target/liella/liella-crt1.o")).arg(&src).arg(r.join("target/liella/libc.a")).arg("-lgcc");
    } else {
        cmd.args(["-std=gnu17", "-O1"]).arg(&src);
    }
    oracle::build_quiet(cmd, out)
}

const REPLACED: &str = "D-7.5 (caching; owner ruling 2026-10-05): a loaded catalog is checked against its file at most once per second and at once after a setting changes, so a replaced catalog is loaded again; the reference never re-reads a file whose binding string is unchanged";
const LANGUAGE: &str = "D-7.5 (caching): LANGUAGE and the locale are read at every lookup, nothing is cached per msgid; the reference keeps a msgid's earlier translation until _nl_msg_cat_cntr moves";
const PATHS: &str = "D-7.5 (security default, reported): a locale name with `/` is never a catalog directory, so LANGUAGE cannot leave the bound directory; the reference uses it as a path";
const FORMS: &str = "D-7.5: plural forms are found in the stored translation and each is converted on its own; the reference splits the converted text at its first NUL byte, which in UTF-16 is inside the first character";
const TRANSLIT: &str = "D-6.9: the built-in C locale's transliteration is derived from the Unicode database, which has no Latin rendering of Cyrillic; the reference's own table gives ZHa";
const NO_TRAP: &str = "§7.7 never fails visibly (three-way: the spec and gettext(3) agree): a division or remainder by zero in Plural-Forms selects the default rule n != 1; the reference is killed by SIGFPE";
const HASH: &str = "D-7.5 (decision 2026-10-04): the sorted originals are searched by binary search and the hash table is not used, so every entry of a catalog with a damaged hash table is found; the reference trusts the table alone (zeroed: nothing found; wrong indices: it never returns)";
const DAMAGED_MO: &str = "D-7.5: a .mo whose strings start beyond the file is not a catalog (absent); the reference reads past the end of the file";
const FIFO: &str = "D-7.5/D-7.6: a catalog that is not a regular file (here a FIFO) is opened non-blocking and treated as absent (catopen: EINVAL, as for a directory); the reference blocks forever";
const CATGETS_ERRNO: &str = "§7.8 and C0.1 (three-way: the spec and POSIX catgets agree): a message the catalog lacks sets ENOMSG, negative numbers and set INT_MAX included; the reference leaves errno alone for those";
const CATGETS_BADF: &str = "§7.8 and C0.1 (three-way: the spec and POSIX catgets, EBADF, agree): an invalid descriptor sets EBADF; the reference leaves errno alone";
const PERCENT: &str = "D-7.6: an NLSPATH template ending in a lone % is skipped like one with an unknown conversion; the reference abandons the whole search there";
const NOT_CAT: &str = "§7.8 and C0.1 (three-way: the spec and POSIX catopen agree that a failure sets errno): a file that is not a catalog fails with EINVAL; the reference leaves errno 0";
const LANG_PATH: &str = "D-7.6 (security default, reported): a locale value with `/` (or `.`, `..`) is never substituted into an NLSPATH or default template; the reference substitutes it unchecked";
const CAT_CUT: &str = "D-7.6: a .cat whose strings start beyond the file is not a catalog; the reference accepts it and reads past the end";
const CAT_ZERO: &str = "D-7.6: a .cat with plane size 0 is not a catalog (EINVAL); the reference is killed by SIGFPE";

/// Lines that differ from the reference by decision: the case (the start of the last header
/// line), the text the reference's line starts with, and why. A recorded line that no longer
/// differs fails the test.
const KNOWN: [(&str, &str, &str); 36] = [
    ("-- catalog file replaced on disk after it was loaded", "  lookup 1.1 s after the file was replaced: [old]", REPLACED),
    ("-- catalog file replaced on disk after it was loaded", "  after bindtextdomain with the same directory: [old]", REPLACED),
    ("-- UTF-8 catalog, de_DE.UTF-8 locale, codeset [UTF-16LE]", "  file n=2 [\\xe9]", FORMS),
    ("-- UTF-8 catalog, LC_MESSAGES de_DE.UTF-8, LC_CTYPE C\n", "  cyr [ZHa]", TRANSLIT),
    ("-- LANG=fr LANGUAGE=../sel/it", "  LC_MESSAGES [fr_FR.UTF-8] where [it]", PATHS),
    ("-- LANG=fr LANGUAGE=/it", "  LC_MESSAGES [fr_FR.UTF-8] where [it]", PATHS),
    ("-- LANGUAGE changed at run time", "  LANGUAGE=nl, nothing else: [it]", LANGUAGE),
    ("-- LANGUAGE changed at run time", "  LANGUAGE unset: [nl]", LANGUAGE),
    ("-- LANGUAGE changed at run time", "  LANGUAGE=it again: [nl]", LANGUAGE),
    ("-- environment changed after setlocale", "  LC_MESSAGES/LANG now fr_FR.UTF-8: [nl]", LANGUAGE),
    ("-- division by zero", "  child killed by signal SIGFPE", NO_TRAP),
    ("-- remainder by zero", "  child killed by signal SIGFPE", NO_TRAP),
    ("-- division by n", "  child killed by signal SIGFPE", NO_TRAP),
    ("-- msgfmt catalog big.mo with its hash table altered", "  every hash entry 0 (empty): 0 right", HASH),
    ("-- msgfmt catalog big.mo with its hash table altered", "  every hash entry 2 (a valid index, the wrong string):  child killed", HASH),
    ("-- half the file", "  hello []", DAMAGED_MO),
    ("-- a FIFO in place of the file (3 s limit)", "  child killed by signal SIGALRM", FIFO),
    ("-- the gencat fixture main.cat", "  1,-1: (default) errno 0", CATGETS_ERRNO),
    ("-- the gencat fixture main.cat", "  -1,1: (default) errno 0", CATGETS_ERRNO),
    ("-- the gencat fixture main.cat", "  2147483647,1: (default) errno 0", CATGETS_ERRNO),
    ("-- the gencat fixture main.cat", "  -2147483648,-2147483648: (default) errno 0", CATGETS_ERRNO),
    ("-- the gencat fixture main.cat", "  -2,1: (default) errno 0", CATGETS_ERRNO),
    ("-- the gencat fixture main.cat", "  catgets((nl_catd)-1, 1, 1): (default) errno 0", CATGETS_BADF),
    ("-- trailing % in the first template, a good second one", "  catopen: -1 errno ENOENT", PERCENT),
    ("-- first candidate is not a catalog", "  catopen: -1 errno 0", NOT_CAT),
    ("-- only candidate is not a catalog", "  catopen: -1 errno 0", NOT_CAT),
    ("-- LANG with a slash", "  catopen: ok", LANG_PATH),
    ("-- LANG=../x (parent directory)", "  catopen: ok", LANG_PATH),
    ("-- 12 bytes (header only)", "  catopen: -1 errno 0", NOT_CAT),
    ("-- tables cut (40 bytes)", "  catopen: -1 errno 0", NOT_CAT),
    ("-- strings cut (90 bytes)", "  catopen: ok; 1,1 [one-on]", CAT_CUT),
    ("-- wrong magic\n", "  catopen: -1 errno 0", NOT_CAT),
    ("-- a big-endian writer's file", "  catopen: -1 errno 0", NOT_CAT),
    ("-- plane size 0", "  child killed by signal SIGFPE", CAT_ZERO),
    ("-- string offset of 1,1 beyond the file", "  catopen: -1 errno 0", NOT_CAT),
    ("-- a FIFO (3 s limit)", "  child killed by signal SIGALRM", FIFO),
];

/// Lines only Liella prints: the case, the text the line starts with, and why (the first such
/// line of the case is taken; a repeated entry takes the next one).
const ADDED: [(&str, &str, &str); 6] = [
    ("-- division by zero", "  22:[f1]", NO_TRAP),
    ("-- division by zero", "  singular-only entry:", NO_TRAP),
    ("-- remainder by zero", "  22:[f1]", NO_TRAP),
    ("-- remainder by zero", "  singular-only entry:", NO_TRAP),
    ("-- division by n", "  22:[f0]", NO_TRAP),
    ("-- division by n", "  singular-only entry:", NO_TRAP),
];

/// Lines only the reference prints: as [`ADDED`].
const DROPPED: [(&str, &str, &str); 0] = [];

/// Each line of `text` with the header line (`== ` or `-- `) it falls under.
fn with_headers(text: &str) -> Vec<(String, String)> {
    let mut header = String::new();
    text.lines()
        .map(|l| {
            if l.starts_with("== ") || l.starts_with("-- ") {
                header = format!("{l}\n");
            }
            (header.clone(), l.to_string())
        })
        .collect()
}

/// `lines` without the lines recorded in `list` (each entry is taken once, by the first line
/// of its case that starts with its text and has not been taken).
fn without(lines: Vec<(String, String)>, list: &[(&str, &str, &str)], who: &str) -> Vec<(String, String)> {
    let mut seen = vec![false; list.len()];
    let mut out = Vec::new();
    for (h, l) in lines {
        match list.iter().enumerate().position(|(i, (case, needle, _))| !seen[i] && h.starts_with(case) && l.starts_with(needle)) {
            Some(i) => {
                seen[i] = true;
                eprintln!("recorded line only {who} prints ({}):\n  {l}", list[i].2);
            }
            None => out.push((h, l)),
        }
    }
    for (i, s) in seen.iter().enumerate() {
        assert!(s, "recorded line no longer printed by {who}: {:?}", list[i]);
    }
    out
}

/// A line diff of `want` (glibc) and `got` (Liella) by longest common subsequence: each run of
/// differing lines under the header of its case, `-` for glibc and `+` for Liella, at most 200 lines.
fn line_diff(want: &[(String, String)], got: &[(String, String)]) -> String {
    let (n, m) = (want.len(), got.len());
    // lcs[i][j]: the common subsequence length of want[i..] and got[j..].
    let mut lcs = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if want[i].1 == got[j].1 { lcs[i + 1][j + 1] + 1 } else { lcs[i + 1][j].max(lcs[i][j + 1]) };
        }
    }
    let (mut i, mut j, mut out, mut shown, mut last) = (0, 0, Vec::new(), 0usize, None);
    let mut emit = |out: &mut Vec<String>, h: &str, line: String, at: usize| {
        if last != Some(h.to_string()) {
            out.push(format!("@ line {} in {}", at + 1, h.trim_end()));
            last = Some(h.to_string());
        }
        out.push(line);
    };
    while (i < n || j < m) && shown < 200 {
        if i < n && j < m && want[i].1 == got[j].1 {
            i += 1;
            j += 1;
        } else if j < m && (i == n || lcs[i][j + 1] >= lcs[i + 1][j]) {
            emit(&mut out, &got[j].0, format!("+ {}", got[j].1), i);
            j += 1;
            shown += 1;
        } else {
            emit(&mut out, &want[i].0, format!("- {}", want[i].1), i);
            i += 1;
            shown += 1;
        }
    }
    if shown == 200 {
        out.push("(diff cut at 200 lines)".to_string());
    }
    out.join("\n")
}

/// The damaged catalogs the record leaves out (offsets and sizes outside the file, where the
/// reference crashes or reads foreign memory; catalogs.md, "Unstable cases") are safe in Liella
/// (D-7.5, D-7.6: a damaged catalog is absent): every case ends normally, a `.mo` translates
/// nothing it cannot read, and `catopen` fails with `EINVAL`.
#[test]
fn damaged_catalogs_outside_the_record_are_safe() {
    // Clauses: s7_7_g_0, s7_7_c_1, s7_7_c_2, s7_8_f_0 (D-7.5, D-7.6).
    let r = root();
    assert!(r.join("target/liella/libc.a").exists(), "the products are missing: run `cargo xtask so`");
    let absent: Vec<&str> = ["cc", "msgfmt", "gencat"].into_iter().filter(|t| skip_note::missing(t)).collect();
    if !absent.is_empty() {
        skip_note::skipped(&format!("no {} (the fixture catalogs and the probe need them)", absent.join(", ")));
        return;
    }
    let locales = r.join("target/test-locales");
    std::fs::create_dir_all(&locales).unwrap();
    if !ensure_locale(&locales, "de_DE.UTF-8") {
        skip_note::skipped("skipped: no localedef");
        return;
    }
    let dir = r.join("target/test-oracles");
    std::fs::create_dir_all(&dir).unwrap();
    let ours = dir.join("catalogs_unstable_liella");
    assert!(build(&ours, true), "catalogs_diff.c does not link against the products");
    let o = Command::new("timeout")
        .arg(timeout::secs(600))
        .arg("env")
        .arg(format!("LOCPATH={}", locales.display()))
        .arg("sh")
        .arg(r.join("crates/core/tests/oracle/catalogs_run.sh"))
        .arg(&ours)
        .arg(dir.join("catalogs-unstable"))
        .arg("unstable")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(o.status.success(), "{:?}\n{}", o.status, String::from_utf8_lossy(&o.stderr));
    let out = String::from_utf8_lossy(&o.stdout);
    eprintln!("{out}");
    assert!(out.lines().last() == Some("done"), "the run ends");
    assert!(!out.contains("child killed"), "no case is killed");
    let mut cases = 0;
    for (h, l) in with_headers(&out) {
        if !h.starts_with("-- ~") || l.starts_with("-- ") || l == "done" {
            continue;
        }
        cases += 1;
        if h.contains(".cat") || l.starts_with("  catopen") {
            assert_eq!(l, "  catopen: -1 errno EINVAL", "{h}");
        } else if !h.contains("hash size") {
            assert_eq!(l, "  hello [hello] zz [zz] hell [hell] file n=2 (msgid_plural) errno EINTR", "{h}");
        } else {
            assert_eq!(l, "  hello [hallo] zz [ZZ] hell [hell] file n=2 [Dateien] errno EINTR", "{h}: the hash table is not used");
        }
    }
    assert_eq!(cases, 14, "every unstable .mo and .cat case printed its result");
    assert_eq!(out.matches(": 500 right, 0 untranslated, 0 other").count(), 3, "hash entries beyond N: the table is not used");
}

#[test]
fn catalogs_equal_the_reference() {
    // Clauses: s7_7_c_0, s7_7_c_1, s7_7_c_2, s7_7_c_3, s7_7_c_4, s7_7_c_5, s7_7_g_0, s7_8_c_0, s7_8_c_p1,
    // s7_8_f_0 (D-7.5, D-7.6, D-7.12).
    let r = root();
    assert!(r.join("target/liella/libc.a").exists(), "the products are missing: run `cargo xtask so`");
    if skip_note::missing("cc") {
        skip_note::skipped("skipped: no C compiler");
        return;
    }
    for tool in ["msgfmt", "gencat"] {
        if skip_note::missing(tool) {
            skip_note::skipped(&format!("skipped: no {tool} (the fixture catalogs need it)"));
            return;
        }
    }
    if !Command::new("sudo").args(["-n", "true"]).status().is_ok_and(|s| s.success()) {
        skip_note::skipped("skipped: no passwordless sudo (the name-service cache modes need a private mount namespace)");
        return;
    }
    let locales = r.join("target/test-locales");
    std::fs::create_dir_all(&locales).unwrap();
    let needed = ["de_DE.UTF-8", "fr_FR.UTF-8", "fr_FR.ISO-8859-1", "cs_CZ.UTF-8", "hu_HU.UTF-8", "it_IT.UTF-8", "ru_RU.KOI8-R", "ja_JP.EUC-JP"];
    if !needed.iter().all(|n| ensure_locale(&locales, n)) {
        skip_note::skipped("skipped: no localedef");
        return;
    }
    let dir = r.join("target/test-oracles");
    std::fs::create_dir_all(&dir).unwrap();
    let (host, ours) = (dir.join("catalogs_diff_host"), dir.join("catalogs_diff_liella"));
    assert!(build(&host, false), "catalogs_diff.c does not build against the host C library");
    assert!(build(&ours, true), "catalogs_diff.c does not link against the products");
    let script = r.join("crates/core/tests/oracle/catalogs_run.sh");
    let wrap = r.join("crates/core/tests/oracle/nscache_mode.sh");
    let run = |exe: &Path, scratch: &str, mode: Option<&str>| -> String {
        let mut cmd = Command::new("timeout");
        cmd.arg(timeout::secs(1200));
        if let Some(m) = mode {
            cmd.arg("sh").arg(&wrap).arg(m);
        }
        // nscache_mode.sh clears the environment: LOCPATH is passed on through `env`.
        cmd.arg("env").arg(format!("LOCPATH={}", locales.display()));
        let start = std::time::Instant::now();
        let o = cmd.arg("sh").arg(&script).arg(exe).arg(dir.join(scratch)).env_clear().env("PATH", "/usr/bin:/bin").output().unwrap();
        let err = String::from_utf8_lossy(&o.stderr);
        // Diagnostics for a failure: how long the run took, the load, and what the probe said on stderr
        // (a child that met its watchdog is named there).
        let load = std::fs::read_to_string("/proc/loadavg").unwrap_or_default();
        eprintln!("{scratch}: {:.1} s, loadavg {}", start.elapsed().as_secs_f64(), load.trim_end());
        if !err.is_empty() {
            eprintln!("{scratch} stderr:\n{err}");
        }
        if !o.status.success() {
            // The end of stdout says which step the run was in (the probe's stderr says only which child).
            let out = String::from_utf8_lossy(&o.stdout);
            let lines: Vec<&str> = out.lines().collect();
            let tail = lines.get(lines.len().saturating_sub(40)..).unwrap_or_default().join("\n");
            panic!("{} failed: {:?}\n{err}\nlast lines of stdout:\n{tail}", exe.display(), o.status);
        }
        String::from_utf8_lossy(&o.stdout).into_owned()
    };
    let reference = run(&host, "catalogs-host", None);
    let record = std::fs::read_to_string(r.join("spec/behaviour/catalogs.glibc.txt")).unwrap();
    if reference == record {
        eprintln!("the reference equals the record");
    } else {
        eprintln!("note: the reference differs from the record");
    }
    for mode in ["on", "off"] {
        eprintln!("name-service cache {mode}");
        let got = run(&ours, &format!("catalogs-liella-{mode}"), Some(mode));
        let want = without(with_headers(&reference), &DROPPED, "the reference");
        let got = without(with_headers(&got), &ADDED, "Liella");
        let mut diffs = 0;
        let mut seen = [false; KNOWN.len()];
        for (k, ((h, w), (_, g))) in want.iter().zip(got.iter()).enumerate() {
            if let Some(i) = KNOWN.iter().position(|(case, needle, _)| h.starts_with(case) && w.starts_with(needle)) {
                if w != g {
                    seen[i] = true;
                    eprintln!("recorded difference ({}):\n  glibc  {w}\n  liella {g}", KNOWN[i].2);
                }
                continue;
            }
            if w != g {
                diffs += 1;
                if diffs <= 400 {
                    eprintln!("line {} ({}):\n  glibc  {w}\n  liella {g}", k + 1, h.trim_end());
                }
            }
        }
        let text = |v: &[(String, String)]| v.iter().map(|(_, l)| l.as_str()).collect::<Vec<_>>().join("\n");
        if want.len() != got.len() {
            // The diff comes first: a log that keeps only the start of a long message still shows it.
            let (wf, gf) = (dir.join("catalogs-failed-glibc.txt"), dir.join(format!("catalogs-failed-liella-{mode}.txt")));
            let _ = std::fs::write(&wf, text(&want));
            let _ = std::fs::write(&gf, text(&got));
            panic!(
                "line counts differ (glibc {}, liella {}; name-service cache {mode}); line diff (- glibc, + liella):\n{}\nfull outputs (recorded lines removed): {} and {}\n--- glibc\n{}\n--- liella\n{}",
                want.len(),
                got.len(),
                line_diff(&want, &got),
                wf.display(),
                gf.display(),
                text(&want),
                text(&got)
            );
        }
        assert_eq!(diffs, 0, "lines that differ from the reference");
        for (i, s) in seen.iter().enumerate() {
            assert!(s, "recorded difference no longer seen: {:?}", KNOWN[i]);
        }
    }
}
