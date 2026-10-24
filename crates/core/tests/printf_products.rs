//! printf end to end (spec §12.6; D-12.4, D-12.5, D-12.12): `tests/oracle/printf_diff.c` built
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
    let src = r.join("crates/core/tests/oracle/printf_diff.c");
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
const KNOWN: [(&str, &str, &str); 2] = [
    ("argmax [%4097$.0d", "*", "a position over NL_ARGMAX is EINVAL (POSIX range; D-12.18)"),
    ("argmax [%1$*4097$d", "*", "a width position over NL_ARGMAX is EINVAL (POSIX range; D-12.18)"),
];
#[test]
fn printf_behaviour_equals_the_reference() {
    // Clauses: s12_6_c_0, s12_6_c_r1, s12_6_c_r2, s12_6_c_r3, s12_6_c_r4, s12_6_c_r5, s12_6_c_r6, s12_6_c_p1,
    // s12_6_c_2, s12_6_c_3, s12_6_c_4, s12_6_c_5, s12_6_c_6, s12_6_g_1, s12_6_g_2, s12_6_g_3, s12_6_g_3_1,
    // s12_6_g_3_2, s12_6_g_3_3, s12_6_g_4, s12_6_g_5, s12_6_f_0, s12_6_f_1, s12_6_f_2, s12_6_f_3, s12_6_f_4
    // (D-12.4, D-12.5, D-12.12).
    let r = root();
    assert!(r.join("target/liella/libc.a").exists(), "the products are missing: run `cargo xtask so`");
    let locales = r.join("target/test-locales");
    std::fs::create_dir_all(&locales).unwrap();
    if !["en_US.UTF-8", "de_DE.UTF-8", "ps_AF.UTF-8"].iter().all(|n| ensure_locale(&locales, n)) {
        skip_note::skipped("skipped: no localedef");
        return;
    }
    let dir = r.join("target/test-oracles");
    std::fs::create_dir_all(&dir).unwrap();
    let (host, ours) = (dir.join("printf_diff_host"), dir.join("printf_diff_liella"));
    // Without a C compiler the test cannot run; with one, the program must build.
    if skip_note::missing("cc") {
        skip_note::skipped("skipped: no C compiler");
        return;
    }
    assert!(build(&host, false), "printf_diff.c does not build against the host C library");
    assert!(build(&ours, true), "printf_diff.c does not link against the products");
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

/// A found-by-fuzzing timeout (2026-10-05): `%100000000$s` made `parse` record that many argument
/// classes (gigabytes, seconds) before anything checked them. Positions are limited to
/// `[1, NL_ARGMAX]` as POSIX says, for `%n$` and for `*m$`; a higher one is `EINVAL` (D-12.18).
/// Needs no products.
#[test]
fn printf_position_over_nl_argmax_is_einval() {
    // Clauses: s12_6_c_6 (D-12.18).
    use liella::error::Errno;
    use liella::printf::{self, Plain, NL_ARGMAX};
    let top = format!("%{NL_ARGMAX}$d");
    assert!(printf::parse(top.as_bytes(), &mut Plain).is_ok());
    for f in [
        format!("%{}$d", NL_ARGMAX + 1),
        "%142949672950$s".to_string(),
        "%100000000$s".to_string(),
        format!("%1$*{}$d", NL_ARGMAX + 1),
        format!("%1$.*{}$d", NL_ARGMAX + 1),
        "%1$.*99999999999999999999$d".to_string(),
    ] {
        assert_eq!(printf::parse(f.as_bytes(), &mut Plain).err(), Some(Errno::EINVAL), "{f}");
    }
    // Wide formats go through the same parse.
    let wide: Vec<u32> = "%5000$d".chars().map(u32::from).collect();
    assert_eq!(printf::parse(&wide, &mut Plain).err(), Some(Errno::EINVAL));
}
