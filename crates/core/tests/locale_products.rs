//! Locales end to end (phase 7): `tests/oracle/locale_diff.c` built against the host C library
//! and statically against Liella's products, run with the same environment and `LOCPATH`; the
//! outputs must be identical, line by line (D-7.2: the same compiled data gives the same
//! answers), except the recorded differences in [`KNOWN`]. Needs the products (`cargo xtask so`), a C compiler and `localedef`; without
//! the latter two the test says so and passes.
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
    let src = r.join("crates/core/tests/oracle/locale_diff.c");
    let mut cmd = Command::new("cc");
    if liella {
        cmd.args(["-std=gnu17", "-O1", "-static", "-nostdlib", "-fno-builtin"]).arg(r.join("target/liella/liella-crt1.o")).arg(&src).arg(r.join("target/liella/libc.a")).arg("-lgcc");
    } else {
        cmd.args(["-std=gnu17", "-O1"]).arg(&src);
    }
    oracle::build(cmd, out)
}

/// Lines that differ from the reference by decision (D-6.8): the locale (`*` any), the line's
/// start, and why. A recorded line that no longer differs fails the test, so the list stays true.
const KNOWN: [(&str, &str, &str); 3] = [
    // §6.2: -2 only for a valid prefix; the reference also returns it for a prefix no valid
    // character extends (a UTF-8 surrogate, an EUC-JP lead byte with no listed sequence).
    ("*", "  mbrtowc eda080 -> ", "surrogate prefix"),
    ("ja_JP.EUC-JP", "  mbrtowc a0 -> ", "lead byte without a listed sequence"),
    // The GB18030 charmap lists supplementary characters only up to its Unicode version; the
    // reference module maps the whole linear four-byte range.
    ("zh_CN.GB18030", "  wcrtomb 1f600 -> ", "not in the charmap"),
];

#[test]
fn locale_behaviour_equals_the_reference() {
    // Clauses: s7_1_c_0, s7_1_c_r1, s7_1_c_r2, s7_1_c_r3, s7_1_c_r4, s7_1_c_r5, s7_1_c_r6, s7_1_c_r7, s7_1_c_p1,
    // s7_2_c_0, s7_2_c_1, s7_2_c_2, s7_2_c_3, s7_2_f_0, s7_2_t_0, s7_2_g_0, s7_3_c_1, s7_3_c_2, s7_3_c_3,
    // s7_3_c_4, s7_3_f_0, s7_4_c_1, s7_4_c_2, s7_4_g_0, s4_3_g_0, s4_4_c_0, s7_6_c_0 (D-7.1, D-7.2, D-7.7, D-7.9).
    let r = root();
    assert!(r.join("target/liella/libc.a").exists(), "the products are missing: run `cargo xtask so`");
    let locales = r.join("target/test-locales");
    std::fs::create_dir_all(&locales).unwrap();
    let names = ["en_US.UTF-8", "de_DE.UTF-8", "tr_TR.UTF-8", "fr_FR.ISO-8859-1", "ja_JP.EUC-JP", "ru_RU.KOI8-R", "zh_CN.GB18030", "ps_AF.UTF-8"];
    if !names.iter().all(|n| ensure_locale(&locales, n)) {
        skip_note::skipped("skipped: no localedef");
        return;
    }
    let dir = r.join("target/test-oracles");
    std::fs::create_dir_all(&dir).unwrap();
    let (host, ours) = (dir.join("locale_diff_host"), dir.join("locale_diff_liella"));
    // Without a C compiler the test cannot run; with one, the program must build.
    if skip_note::missing("cc") {
        skip_note::skipped("skipped: no C compiler");
        return;
    }
    assert!(build(&host, false), "locale_diff.c does not build against the host C library");
    assert!(build(&ours, true), "locale_diff.c does not link against the products");
    let args = ["C", "POSIX", "C.UTF-8", "en_US.UTF-8", "de_DE.UTF-8", "tr_TR.UTF-8", "fr_FR.ISO-8859-1", "ja_JP.EUC-JP", "ru_RU.KOI8-R", "zh_CN.GB18030", "ps_AF.UTF-8"];
    let run = |exe: &Path| -> String {
        let o = Command::new(exe).args(args).env_clear().env("LOCPATH", &locales).env("LANG", "en_US.UTF-8").output().unwrap();
        assert!(o.status.success(), "{} failed: {:?}", exe.display(), o.status);
        String::from_utf8_lossy(&o.stdout).into_owned()
    };
    let (want, got) = (run(&host), run(&ours));
    let (mut diffs, mut locale, mut seen) = (0, String::new(), [false; KNOWN.len()]);
    for (k, (w, g)) in want.lines().zip(got.lines()).enumerate() {
        if let Some(rest) = w.strip_prefix("== ") {
            locale = rest.split(" -> ").next().unwrap_or("").to_string();
        }
        let known = KNOWN.iter().position(|(l, start, _)| (*l == "*" || *l == locale) && w.starts_with(start));
        if let Some(i) = known {
            if w != g {
                seen[i] = true;
                eprintln!("recorded difference ({}), {locale}:\n  glibc  {w}\n  liella {g}", KNOWN[i].2);
            }
            continue;
        }
        if w != g {
            diffs += 1;
            if diffs <= 15 {
                eprintln!("line {}:\n  glibc  {}\n  liella {}", k + 1, &w[..w.len().min(300)], &g[..g.len().min(300)]);
            }
        }
    }
    assert_eq!(want.lines().count(), got.lines().count(), "line counts differ");
    assert_eq!(diffs, 0, "lines that differ from the reference");
    for (i, s) in seen.iter().enumerate() {
        assert!(s, "recorded difference no longer seen: {:?}", KNOWN[i]);
    }
}

/// The environments `newlocale_mask.c` runs in: the first without arguments (every mask row),
/// the rest with one (only the `""` rows): `LC_ALL` set, `LC_ALL` empty with `LC_CTYPE` set,
/// an unknown `LC_CTYPE`, and no variable at all.
const MASK_RUNS: [&[(&str, &str)]; 5] = [
    &[("LANG", "en_US.UTF-8"), ("LC_NUMERIC", "C.UTF-8")],
    &[("LC_ALL", "C.UTF-8"), ("LANG", "en_US.UTF-8")],
    &[("LC_ALL", ""), ("LC_CTYPE", "C.UTF-8"), ("LANG", "en_US.UTF-8")],
    &[("LC_CTYPE", "xx_YY.NOPE"), ("LANG", "en_US.UTF-8")],
    &[],
];

#[test]
fn newlocale_masks_equal_the_reference() {
    // Clauses: s7_3_c_1, s7_3_f_0 (NEWLOCALE-LC-ALL-BIT; spec/behaviour/newlocale-mask.md):
    // `1 << LC_ALL` alone is every category, with any other bit `EINVAL`; `""` takes each
    // category from the environment; a NULL name is `EINVAL`.
    let r = root();
    assert!(r.join("target/liella/libc.a").exists(), "the products are missing: run `cargo xtask so`");
    let locales = r.join("target/test-locales");
    std::fs::create_dir_all(&locales).unwrap();
    if !ensure_locale(&locales, "en_US.UTF-8") {
        skip_note::skipped("skipped: no localedef");
        return;
    }
    if skip_note::missing("cc") {
        skip_note::skipped("skipped: no C compiler");
        return;
    }
    let dir = r.join("target/test-oracles");
    std::fs::create_dir_all(&dir).unwrap();
    let src = r.join("crates/core/tests/oracle/newlocale_mask.c");
    let (host, ours) = (dir.join("newlocale_mask_host"), dir.join("newlocale_mask_liella"));
    let mut cmd = Command::new("cc");
    cmd.args(["-std=gnu17", "-O1"]).arg(&src);
    assert!(oracle::build(cmd, &host), "newlocale_mask.c does not build against the host C library");
    let mut cmd = Command::new("cc");
    cmd.args(["-std=gnu17", "-O1", "-static", "-nostdlib", "-fno-builtin"]).arg(r.join("target/liella/liella-crt1.o")).arg(&src).arg(r.join("target/liella/libc.a")).arg("-lgcc");
    assert!(oracle::build(cmd, &ours), "newlocale_mask.c does not link against the products");
    let run = |exe: &Path| -> String {
        let mut out = String::new();
        for (k, vars) in MASK_RUNS.iter().enumerate() {
            let mut c = Command::new(exe);
            c.env_clear().env("LOCPATH", &locales).envs(vars.iter().copied());
            if k > 0 {
                c.arg("env");
            }
            let o = c.output().unwrap();
            assert!(o.status.success(), "{} failed: {:?}", exe.display(), o.status);
            out.push_str(&String::from_utf8_lossy(&o.stdout));
        }
        out
    };
    let (want, got) = (run(&host), run(&ours));
    assert!(want.contains("bit6 mask=0x40 name=\"C\" base=NULL: ok"), "the reference accepts 1 << LC_ALL:\n{want}");
    // The one recorded difference: where the reference leaves errno 0 on a failed load into a
    // base (or from the environment), Liella sets ENOENT (s7_3_f_0). Both return NULL.
    let mut diffs = 0;
    for (k, (w, g)) in want.lines().zip(got.lines()).enumerate() {
        if w != g && !(w.ends_with("NULL errno=0 base_kept=yes") || w.ends_with("NULL errno=0 base_kept=-")) {
            diffs += 1;
            eprintln!("line {}:\n  glibc  {w}\n  liella {g}", k + 1);
        } else if w != g {
            assert_eq!(w.replace("errno=0", "errno=2"), g, "line {}: only errno may differ", k + 1);
        }
    }
    assert_eq!(want.lines().count(), got.lines().count(), "line counts differ");
    assert_eq!(diffs, 0, "lines that differ from the reference");
}
