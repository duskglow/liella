//! strfmon end to end (spec §7.5; D-7.11): `tests/oracle/monetary_diff.c` built
//! against the host C library and statically against Liella's products, run with the same
//! `LOCPATH`; the outputs must be identical line by line, except the recorded differences in
//! [`KNOWN`]. Needs the products (`cargo xtask so`), a C compiler and `localedef`; without the
//! latter two the test says so and passes.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, missing_docs)]
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
    let src = r.join("crates/core/tests/oracle/monetary_diff.c");
    let mut cmd = Command::new("cc");
    if liella {
        cmd.args(["-std=gnu17", "-O1", "-static", "-nostdlib", "-fno-builtin"]).arg(r.join("target/liella/liella-crt1.o")).arg(&src).arg(r.join("target/liella/libc.a")).arg("-lgcc");
    } else {
        cmd.args(["-std=gnu17", "-O1"]).arg(&src);
    }
    oracle::build(cmd, out)
}

/// Lines that differ from the reference by decision: the start of the line, the locale it is
/// under (`*` any), and why. A recorded line that no longer differs fails the test.
const ZERO: &str = "D-7.11: -0.0 (and -NaN) is negative by its sign bit; glibc prints `$-0.00`";
const KNOWN: [(&str, &str, &str); 22] = [
    ("[%n] 0 max=200", "*", ZERO),
    ("[%i] 0 max=200", "*", ZERO),
    ("[%=*#10n] 0 max=200", "*", ZERO),
    ("[%^n] 0 max=200", "*", ZERO),
    ("[%+n] 0 max=200", "*", ZERO),
    ("[%(n] 0 max=200", "*", ZERO),
    ("[%!n] 0 max=200", "*", ZERO),
    ("[%-14n] 0 max=200", "*", ZERO),
    ("[%14n] 0 max=200", "*", ZERO),
    ("[%#5n] 0 max=200", "*", ZERO),
    ("[%#5.0n] 0 max=200", "*", ZERO),
    ("[%.3i] 0 max=200", "*", ZERO),
    ("[%=0#8n] 0 max=200", "*", ZERO),
    ("[%(#5i] 0 max=200", "*", ZERO),
    ("[%^#9.1n] 0 max=200", "*", ZERO),
    ("[%!(#7n] 0 max=200", "*", ZERO),
    ("[%+#6i] 0 max=200", "*", ZERO),
    ("[%-#6.2n|] 0 max=200", "*", ZERO),
    ("[a %n b %% c] 0 max=200", "*", ZERO),
    ("[%=x-16.1i|] 0 max=200", "*", ZERO),
    ("[%!n] ", "de_CH.UTF-8", "D-7.11: `!` drops the symbol with its separating space; a space only between non-empty parts"),
    ("[%!(#7n] ", "nl_NL.UTF-8", "D-7.11: `#` aligns on the printed text, not on the suppressed symbol"),
];
#[test]
fn strfmon_equals_the_reference() {
    // Clauses: s7_5_c_0, s7_5_f_0 (D-7.4, D-7.11).
    let r = root();
    assert!(r.join("target/liella/libc.a").exists(), "the products are missing: run `cargo xtask so`");
    let locales = r.join("target/test-locales");
    std::fs::create_dir_all(&locales).unwrap();
    if !["en_US.UTF-8", "de_DE.UTF-8", "fr_FR.UTF-8", "ja_JP.UTF-8", "it_IT.UTF-8", "de_CH.UTF-8", "nl_NL.UTF-8", "en_IN.UTF-8"].iter().all(|n| ensure_locale(&locales, n)) {
        skip_note::skipped("skipped: no localedef");
        return;
    }
    // An own test locale (en_US but an empty mon_thousands_sep with mon_grouping 3): strfmon must not group.
    let nosep = r.join("crates/core/tests/oracle/xx_NOSEP.locale");
    let nosep = nosep.to_str().unwrap();
    assert!(oracle::locale(&locales.join("xx_NOSEP.UTF-8"), nosep, "UTF-8", "LC_MONETARY"), "localedef could not build {nosep}");
    let dir = r.join("target/test-oracles");
    std::fs::create_dir_all(&dir).unwrap();
    let (host, ours) = (dir.join("monetary_diff_host"), dir.join("monetary_diff_liella"));
    // Without a C compiler the test cannot run; with one, the program must build.
    if skip_note::missing("cc") {
        skip_note::skipped("skipped: no C compiler");
        return;
    }
    assert!(build(&host, false), "monetary_diff.c does not build against the host C library");
    assert!(build(&ours, true), "monetary_diff.c does not link against the products");
    let run = |exe: &Path| -> String {
        let o = Command::new(exe).env_clear().env("LOCPATH", &locales).output().unwrap();
        assert!(o.status.success(), "{} failed: {:?}", exe.display(), o.status);
        String::from_utf8_lossy(&o.stdout).into_owned()
    };
    let (want, got) = (run(&host), run(&ours));
    let (mut diffs, mut locale, mut seen) = (0, String::from("C"), [false; KNOWN.len()]);
    for (k, (w, g)) in want.lines().zip(got.lines()).enumerate() {
        if let Some(rest) = w.strip_prefix("== ") {
            locale = rest.to_string();
        }
        let known = KNOWN.iter().position(|(start, l, _)| (*l == "*" || *l == locale) && w.starts_with(start));
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
