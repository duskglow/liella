//! scanf end to end (spec §12.7; D-12.6, D-12.13): `tests/oracle/scanf_diff.c` built
//! against the host C library and statically against Liella's products, run with the same
//! `LOCPATH`; the outputs must be identical line by line, except the recorded differences in
//! [`KNOWN`]. Needs the products (`cargo xtask so`), a C compiler and `localedef`; without the
//! latter two the test says so and passes.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects, missing_docs)]
#[path = "support/skip_note.rs"]
mod skip_note;

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
    let src = r.join("crates/core/tests/oracle/scanf_diff.c");
    let mut cmd = Command::new("cc");
    if liella {
        cmd.args(["-std=gnu17", "-O1", "-static", "-nostdlib", "-fno-builtin"]).arg(r.join("target/liella/liella-crt1.o")).arg(&src).arg(r.join("target/liella/libc.a")).arg("-lgcc");
    } else {
        cmd.args(["-std=gnu17", "-O1"]).arg(&src);
    }
    oracle::build(cmd, out)
}

/// Lines that differ from the reference by decision: text the line contains (its input and
/// format), the locale it is under (`*` any), and why. A recorded line that no longer differs
/// fails the test.
const PARTIAL: &str = "spec §12.7: a partial match consumes the prefix and fails; glibc converts the number in it";
const KNOWN: [(&str, &str, &str); 23] = [
    (" [1e] [%f] ", "*", PARTIAL),
    (" [1e+] [%f] ", "*", PARTIAL),
    (" [1e] [%lf] ", "*", PARTIAL),
    (" [1e+] [%lf] ", "*", PARTIAL),
    (" [1e] [%Lf] ", "*", PARTIAL),
    (" [1e+] [%Lf] ", "*", PARTIAL),
    (" [1e] [%5f] ", "*", PARTIAL),
    (" [1e+] [%5f] ", "*", PARTIAL),
    (" [1e] [%2lf] ", "*", PARTIAL),
    (" [1e+] [%2lf] ", "*", PARTIAL),
    (" [1e] [%3Lf] ", "*", PARTIAL),
    (" [1e+] [%3Lf] ", "*", PARTIAL),
    (" [1.5e-x] [%f] ", "*", PARTIAL),
    (" [1.5e-x] [%lf] ", "*", PARTIAL),
    (" [1.5e-x] [%Lf] ", "*", PARTIAL),
    (" [1.5e-x] [%5f] ", "*", PARTIAL),
    (" [4.9e-324] [%5f] ", "*", PARTIAL),
    (" [1e10] [%2lf] ", "*", PARTIAL),
    (" [1e-400] [%2lf] ", "*", PARTIAL),
    (" [1e-400] [%3Lf] ", "*", PARTIAL),
    (" [0x1p3] [%2lf] ", "*", PARTIAL),
    (" [0x] [%2lf] ", "*", PARTIAL),
    (" [0xp] [%2lf] ", "*", PARTIAL),
];

/// D-12.6: an integer conversion out of its type's range stores the clamped value, where glibc
/// stores the value converted to the type. Such a line differs only in the first argument's
/// bytes, and Liella's are the type's minimum or maximum.
fn clamped(w: &str, g: &str) -> bool {
    let field = |l: &str| -> Option<(String, String)> {
        let at = l.find(" a=")?;
        let end = l[at + 3..].find(' ').map_or(l.len(), |e| at + 3 + e);
        Some((format!("{}{}", &l[..at], &l[end..]), l[at + 3..end].to_string()))
    };
    let (Some((wr, _)), Some((gr, v))) = (field(w), field(g)) else { return false };
    let integer = ["d]", "i]", "u]", "o]", "x]", "X]", "b]"].iter().any(|c| w.contains(&format!("{c} ->")));
    let bytes = v.len() / 2;
    let (ff, zeros) = ("ff".repeat(bytes.saturating_sub(1)), "00".repeat(bytes.saturating_sub(1)));
    let saturated = v == "ff".repeat(bytes) || v == format!("{ff}7f") || v == format!("{zeros}80");
    wr == gr && integer && saturated
}

/// D-12.13: wide input stored as a narrow string (`%s`, `%[`) gets one terminator; glibc writes a
/// second zero byte past it (which can overrun a buffer sized for the string). Such a line is a
/// wide one (`ws`, `wf`) that differs only in string arguments whose reference bytes are Liella's
/// followed by one more `00`.
fn one_terminator(w: &str, g: &str) -> bool {
    if !(w.starts_with("ws ") || w.starts_with("wf ")) {
        return false;
    }
    let (wt, gt): (Vec<&str>, Vec<&str>) = (w.split(' ').collect(), g.split(' ').collect());
    if wt.len() != gt.len() {
        return false;
    }
    let mut extra = 0;
    for (a, b) in wt.iter().zip(&gt) {
        if a == b {
            continue;
        }
        let arg = ["a=", "b=", "c=", "d="].iter().any(|p| a.starts_with(p));
        if arg && b.ends_with("00") && a.len() == b.len() + 2 && a.starts_with(b) && a.ends_with("0000") {
            extra += 1;
        } else {
            return false;
        }
    }
    extra > 0
}

#[test]
fn scanf_behaviour_equals_the_reference() {
    // Clauses: s12_7_c_0, s12_7_c_1, s12_7_c_2, s12_7_c_3, s12_7_g_0, s12_7_f_0 (D-12.6, D-12.13).
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
    let (host, ours) = (dir.join("scanf_diff_host"), dir.join("scanf_diff_liella"));
    // Without a C compiler the test cannot run; with one, the program must build.
    if skip_note::missing("cc") {
        skip_note::skipped("skipped: no C compiler");
        return;
    }
    assert!(build(&host, false), "scanf_diff.c does not build against the host C library");
    assert!(build(&ours, true), "scanf_diff.c does not link against the products");
    let run = |exe: &Path| -> String {
        let o = Command::new(exe).env_clear().env("LOCPATH", &locales).output().unwrap();
        assert!(o.status.success(), "{} failed: {:?}", exe.display(), o.status);
        String::from_utf8_lossy(&o.stdout).into_owned()
    };
    let (want, got) = (run(&host), run(&ours));
    let (mut diffs, mut locale, mut seen, mut clamps, mut nuls) = (0, String::from("C"), [false; KNOWN.len()], 0, 0);
    for (k, (w, g)) in want.lines().zip(got.lines()).enumerate() {
        if let Some(rest) = w.strip_prefix("== ") {
            locale = rest.to_string();
        }
        let known = KNOWN.iter().position(|(needle, l, _)| (*l == "*" || *l == locale) && w.contains(needle));
        if let Some(i) = known {
            if w != g {
                seen[i] = true;
                eprintln!("recorded difference ({}), {locale}:\n  glibc  {w}\n  liella {g}", KNOWN[i].2);
            }
            continue;
        }
        if w != g && clamped(w, g) {
            clamps += 1;
            continue;
        }
        if w != g && one_terminator(w, g) {
            nuls += 1;
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
    assert!(clamps > 0, "the clamped conversions (D-12.6) no longer differ");
    eprintln!("{clamps} lines store a clamped value (D-12.6)");
    assert!(nuls > 0, "the single terminator for wide input (D-12.13) no longer differs");
    eprintln!("{nuls} lines store one terminator where glibc stores two (D-12.13)");
    for (i, s) in seen.iter().enumerate() {
        assert!(s, "recorded difference no longer seen: {:?}", KNOWN[i]);
    }
}
