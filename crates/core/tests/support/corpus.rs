//! Shared by the corpus replay tests (validation item C): build a replayer from
//! `tests/oracle/<name>_corpus.c` against the host C library and statically against Liella's
//! products, run both over `validation/corpus/<name>.corpus`, and compare line by line. A line
//! may disagree only if `validation/corpus/<name>.known` records it (`<line>\t<reason>`, `#`
//! comments); a recorded line that agrees again fails the test, so the list stays exact.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects, dead_code)]

#[path = "oracle.rs"]
mod oracle;
#[path = "skip_note.rs"]
mod skip_note;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

pub(crate) fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn ensure_locale(dir: &Path, name: &str) -> bool {
    if dir.join(name).join("LC_COLLATE").exists() {
        return true;
    }
    let Some((lang, charset)) = name.split_once('.') else { return false };
    oracle::locale(&dir.join(name), lang, charset, "LC_COLLATE")
}

fn build(name: &str, out: &Path, liella: bool) -> bool {
    let r = root();
    let src = r.join(format!("crates/core/tests/oracle/{name}_corpus.c"));
    let mut cmd = Command::new("cc");
    if liella {
        cmd.args(["-std=gnu17", "-O1", "-static", "-nostdlib", "-fno-builtin"]).arg(r.join("target/liella/liella-crt1.o")).arg(&src).arg(r.join("target/liella/libc.a")).arg("-lgcc");
    } else {
        cmd.args(["-std=gnu17", "-O1"]).arg(&src);
    }
    oracle::build(cmd, out)
}

/// The recorded disagreements: corpus line number -> reason.
fn known(name: &str) -> BTreeMap<usize, String> {
    let path = root().join(format!("validation/corpus/{name}.known"));
    let text = std::fs::read_to_string(path).unwrap_or_default();
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .filter_map(|l| {
            let (n, why) = l.split_once('\t')?;
            Some((n.trim().parse().ok()?, why.to_string()))
        })
        .collect()
}

/// Replay corpus `name` under both libraries; `field` picks the part of an output line that must
/// agree (the replayers print the line number first).
pub(crate) fn replay(name: &str, locales: &[&str]) {
    let r = root();
    let corpus = r.join(format!("validation/corpus/{name}.corpus"));
    assert!(corpus.exists(), "{} is missing", corpus.display());
    assert!(r.join("target/liella/libc.a").exists(), "the products are missing: run `cargo xtask so`");
    if skip_note::missing("cc") {
        skip_note::skipped("skipped: no C compiler");
        return;
    }
    let locdir = r.join("target/test-locales");
    std::fs::create_dir_all(&locdir).unwrap();
    for l in locales {
        if !ensure_locale(&locdir, l) {
            skip_note::skipped(&format!("skipped: no localedef; the lines of {name} that need locale {l} are skipped by both engines"));
        }
    }
    let dir = r.join("target/test-oracles");
    std::fs::create_dir_all(&dir).unwrap();
    let (host, ours) = (dir.join(format!("{name}_corpus_host")), dir.join(format!("{name}_corpus_liella")));
    assert!(build(name, &host, false), "{name}_corpus.c does not build against the host C library");
    assert!(build(name, &ours, true), "{name}_corpus.c does not link against the products");
    let run = |exe: &Path| -> Vec<String> {
        let o = Command::new(exe).arg(&corpus).env_clear().env("LOCPATH", &locdir).output().unwrap();
        assert!(o.status.success(), "{} failed: {:?}", exe.display(), o.status);
        String::from_utf8_lossy(&o.stdout).lines().map(str::to_string).collect()
    };
    let (want, got) = (run(&host), run(&ours));
    assert_eq!(want.len(), got.len(), "line counts differ");
    let known = known(name);
    let mut seen = BTreeMap::new();
    let mut diffs = 0;
    for (w, g) in want.iter().zip(&got) {
        let n: usize = w.split('\t').next().and_then(|x| x.parse().ok()).unwrap_or(0);
        if w == g {
            continue;
        }
        if let Some(why) = known.get(&n) {
            seen.insert(n, why.clone());
            continue;
        }
        diffs += 1;
        if diffs <= 40 {
            eprintln!("corpus line {n}:\n  glibc  {}\n  liella {}", &w[..w.len().min(400)], &g[..g.len().min(400)]);
        }
    }
    eprintln!("{name} corpus: {} lines, {} recorded disagreements seen", want.len(), seen.len());
    assert_eq!(diffs, 0, "{name} corpus lines that disagree with the reference and are not recorded in {name}.known");
    let stale: Vec<_> = known.keys().filter(|n| !seen.contains_key(n)).collect();
    assert!(stale.is_empty(), "recorded disagreements that now agree (remove them from {name}.known): {stale:?}");
}

