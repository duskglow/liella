//! iconv end to end (spec §6.4; D-6.8, D-6.9): `tests/oracle/iconv_diff.c` built against the
//! host C library and statically against Liella's products, run with the same environment and
//! `LOCPATH`; the outputs must be identical line by line, except the recorded differences in
//! [`KNOWN`]. Needs the products (`cargo xtask so`), a C compiler, `localedef` and the
//! installed charmaps; without the latter three the test says so and passes.
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
    let src = r.join("crates/core/tests/oracle/iconv_diff.c");
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
const KNOWN: [(&str, &str, &str); 4] = [
    // Stateful encodings, which only the reference library's modules implement (D-6.8).
    ("ISO-2022-JP <- ", "*", "stateful"),
    ("UTF-7 <- ", "*", "stateful"),
    // An alias the reference library's module list has and no installed charmap gives.
    ("WINDOWS-1252 <- ", "*", "alias not in the charmaps"),
    // CP932 is WINDOWS-31J's charmap, which maps 0x815C from U+2015 only; the reference
    // module also takes U+2014 (D-6.8: the charmap is followed).
    ("CP932 <- UTF-8 [61e38182", "*", "U+2014 not in the charmap"),
];

#[test]
fn iconv_behaviour_equals_the_reference() {
    // Clauses: s6_4_c_1, s6_4_c_1_1, s6_4_c_1_2, s6_4_c_1_3, s6_4_c_2, s6_4_c_3, s6_4_c_4, s6_4_c_5, s6_4_f_1,
    // s6_4_f_2, s6_4_f_2_1, s6_4_f_2_2, s6_4_f_2_3, s6_4_f_3, s6_4_g_0 (D-6.8, D-6.9).
    let r = root();
    assert!(r.join("target/liella/libc.a").exists(), "the products are missing: run `cargo xtask so`");
    if !Path::new("/usr/share/i18n/charmaps").exists() {
        skip_note::skipped("skipped: no charmaps");
        return;
    }
    let locales = r.join("target/test-locales");
    std::fs::create_dir_all(&locales).unwrap();
    if !["en_US.UTF-8", "de_DE.UTF-8"].iter().all(|n| ensure_locale(&locales, n)) {
        skip_note::skipped("skipped: no localedef");
        return;
    }
    let dir = r.join("target/test-oracles");
    std::fs::create_dir_all(&dir).unwrap();
    let (host, ours) = (dir.join("iconv_diff_host"), dir.join("iconv_diff_liella"));
    // Without a C compiler the test cannot run; with one, the program must build.
    if skip_note::missing("cc") {
        skip_note::skipped("skipped: no C compiler");
        return;
    }
    assert!(build(&host, false), "iconv_diff.c does not build against the host C library");
    assert!(build(&ours, true), "iconv_diff.c does not link against the products");
    let run = |exe: &Path| -> String {
        let o = Command::new(exe).env_clear().env("LOCPATH", &locales).output().unwrap();
        assert!(o.status.success(), "{} failed: {:?}", exe.display(), o.status);
        String::from_utf8_lossy(&o.stdout).into_owned()
    };
    let (want, got) = (run(&host), run(&ours));
    let (mut diffs, mut locale, mut seen) = (0, String::from("C"), [false; KNOWN.len()]);
    for (k, (w, g)) in want.lines().zip(got.lines()).enumerate() {
        if let Some(rest) = w.strip_prefix("locale ") {
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
