//! `lab-names` (static): lab host names and lab addresses stay out of main (owner, 2026-10-24).
//!
//! The owner's lab machines may be named on task branches (under `handoff/`, which this check skips) but never
//! in what reaches main. Every tracked file outside `handoff/` is scanned for the host names in [`NAMES`] as
//! whole words (ASCII case-insensitive; a letter, digit or underscore next to the name makes it part of a
//! longer word) and for `10.0.100.x` / `10.0.0.x` addresses. Prose names the processor class instead
//! ("a Zen 1 server part"), as RELEASE-TESTED-CPUS did.
//!
//! Exemptions, each with its reason:
//! - [`ALLOW`]: whole files, matched by exact path or by a directory prefix ending in `/`. Only tool host maps
//!   and tables keyed by host that need the real name belong here; the reason is the second field.
//! - a line containing `LAB-ONLY` is skipped (a host map line inside an otherwise checked script).
//! - file NAMES are never checked, so result files named after a host under `validation/results/` pass while
//!   their contents are still scanned.

use std::path::Path;
use std::process::Command;

/// The lab host names (the owner's list, 2026-10-24).
const NAMES: [&str; 13] = ["keke", "shiki", "kanon", "mei", "sumire", "nico", "kotori", "nozomi", "natsumi", "chisato", "alma", "nijika", "hayase"];

/// Whole files exempt from the scan: `(path or directory prefix, reason)`.
const ALLOW: [(&str, &str); 15] = [
    ("xtask/src/labnames.rs", "this check's own name list and its tests"),
    ("validation/wl_gate.sh", "tool host map: maps a role to its host address"),
    ("validation/wl_thresholds.tsv", "table keyed by the host that measured; the tools look rows up by host name"),
    ("validation/wl_alloctrace.sh", "tool host map (LAB-ONLY)"),
    ("validation/wl_fallback.sh", "tool host map (LAB-ONLY)"),
    ("validation/wl_shapes.sh", "tool host map (LAB-ONLY)"),
    ("validation/wl_entries.sh", "tool host map (LAB-ONLY)"),
    ("validation/wl_install.sh", "tool host map (LAB-ONLY)"),
    ("validation/wl_stakes.sh", "tool host map (LAB-ONLY)"),
    ("validation/wl_percall_run.sh", "tool host map (LAB-ONLY)"),
    ("validation/wl_percall.sh", "tool host map (LAB-ONLY)"),
    ("validation/wl_launch.sh", "tool host map (LAB-ONLY)"),
    ("validation/run_remote.sh", "tool host map: reaches a host by address"),
    ("tools/probe/remote.sh", "tool host map: reaches a host by address"),
    ("tools/neutral_hosts.sh", "tool that maps lab names to neutral ones; it has to know the names"),
];

fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// What a line holds that must not reach main: the first host name or lab address found, if any.
fn hit(line: &str) -> Option<String> {
    let b = line.as_bytes();
    let lower = line.to_ascii_lowercase();
    for name in NAMES {
        let mut from = 0;
        while let Some(i) = lower[from..].find(name) {
            let (s, e) = (from + i, from + i + name.len());
            if (s == 0 || !is_word(b[s - 1])) && (e >= b.len() || !is_word(b[e])) {
                return Some(format!("host name `{name}`"));
            }
            from = s + 1;
        }
    }
    for prefix in ["10.0.100.", "10.0.0."] {
        let mut from = 0;
        while let Some(i) = line[from..].find(prefix) {
            let s = from + i;
            let digit = b.get(s + prefix.len()).is_some_and(u8::is_ascii_digit);
            if digit && (s == 0 || !(b[s - 1].is_ascii_digit() || b[s - 1] == b'.')) {
                return Some(format!("lab address `{prefix}x`"));
            }
            from = s + 1;
        }
    }
    None
}

fn allowed(path: &str) -> bool {
    ALLOW.iter().any(|(p, _)| if p.ends_with('/') { path.starts_with(p) } else { path == *p })
}

/// Findings (`path:line: what`) in one file's text; empty for an allowlisted path.
fn scan(path: &str, text: &str) -> Vec<String> {
    if path.starts_with("handoff/") || allowed(path) {
        return Vec::new();
    }
    text.lines().enumerate().filter(|(_, l)| !l.contains("LAB-ONLY")).filter_map(|(n, l)| hit(l).map(|w| format!("{path}:{}: {w}", n + 1))).collect()
}

fn walk(root: &Path, dir: &Path, handoff: bool, out: &mut Vec<String>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let rel = p.strip_prefix(root).map(|r| r.to_string_lossy().into_owned()).unwrap_or_default();
        // Sync bookkeeping (`.synced*`, `.changed*`, e.g. `.changed.xtask`) and build output (`target/`, `fuzz/target/`, and
        // `conformance/build/`, which the gate's conformance step writes) are not part of the tree.
        // Python bytecode caches are build debris wherever they appear (they embed the path they were compiled from).
        if p.file_name().is_some_and(|n| n == "__pycache__") || rel.ends_with(".pyc") {
            continue;
        }
        if rel == "target" || rel == "fuzz/target" || rel == "conformance/build" || rel == ".git" || (!handoff && rel.starts_with("handoff")) || rel.starts_with(".synced") || rel.starts_with(".changed") {
            continue;
        }
        if p.is_dir() {
            walk(root, &p, handoff, out);
        } else if p.is_file() {
            out.push(rel);
        }
    }
}

/// The files to scan: `git ls-files`; in a synced role tree without git metadata, every file outside `target/`.
pub(crate) fn tracked(root: &Path) -> Result<Vec<String>, String> {
    tracked_with(root, false)
}

/// [`tracked`], with `handoff/` walked too when `handoff` is set (only matters without git metadata).
pub(crate) fn tracked_with(root: &Path, handoff: bool) -> Result<Vec<String>, String> {
    match Command::new("git").current_dir(root).args(["ls-files", "-z"]).output() {
        Ok(o) if o.status.success() => Ok(o.stdout.split(|&b| b == 0).filter(|n| !n.is_empty()).map(|n| String::from_utf8_lossy(n).into_owned()).collect()),
        _ => {
            let mut v = Vec::new();
            walk(root, root, handoff, &mut v);
            Ok(v)
        }
    }
}

pub fn check(root: &Path) -> Result<(), String> {
    let mut found = Vec::new();
    for path in tracked(root)? {
        // Files that are not UTF-8 text (binaries, fuzz seeds) are skipped; a deleted-but-listed file too.
        if let Ok(text) = std::fs::read_to_string(root.join(&path)) {
            found.extend(scan(&path, &text));
        }
    }
    if found.is_empty() {
        return Ok(());
    }
    let n = found.len();
    let mut per_file: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for f in &found {
        *per_file.entry(f.split(':').next().unwrap_or("")).or_default() += 1;
    }
    let files: Vec<String> = per_file.iter().map(|(f, c)| format!("{f} ({c})")).collect();
    let first: Vec<&str> = found.iter().take(40).map(String::as_str).collect();
    Err(format!("{n} lab host name / address hit(s) in {} file(s) outside handoff/ (name the processor class instead, or add an ALLOW entry with a reason in xtask/src/labnames.rs):\n  files: {}\n  {}", per_file.len(), files.join(", "), first.join("\n  ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walk_skips_build_output() {
        let root = std::env::temp_dir().join(format!("liella-walk-{}", std::process::id()));
        for d in ["conformance/build", "target", "src", "tools/hottext/__pycache__"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        for f in ["conformance/build/s27_a.key", "target/x", "src/a.rs", ".synced", "tools/hottext/__pycache__/keys.cpython-312.pyc", "tools/hottext/__pycache__/other", "tools/stray.pyc"] {
            std::fs::write(root.join(f), "x").unwrap();
        }
        let mut v = Vec::new();
        walk(&root, &root, false, &mut v);
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(v, vec!["src/a.rs".to_string()]);
    }

    /// The trace-at-build helpers must not write bytecode into the tree: every `python3` run by a tools/hottext
    /// script passes `-B` (no spec clause: build hygiene).
    #[test]
    fn hottext_scripts_run_python_without_bytecode() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tools/hottext");
        let mut seen = 0;
        for e in std::fs::read_dir(&dir).unwrap().flatten() {
            let p = e.path();
            if p.extension().is_none_or(|x| x != "sh") {
                continue;
            }
            for (n, l) in std::fs::read_to_string(&p).unwrap().lines().enumerate() {
                let t = l.trim_start();
                if t.starts_with('#') || !t.contains("python3 ") {
                    continue;
                }
                seen += 1;
                assert!(t.contains("python3 -B "), "{}:{}: python3 without -B writes __pycache__ into the tree", p.display(), n + 1);
            }
        }
        assert!(seen >= 8, "found only {seen} python3 invocations: the scan is not looking at the scripts");
    }

    #[test]
    fn planted_host_name_is_caught() {
        for n in NAMES {
            assert_eq!(scan("docs/x.md", &format!("measured on {n} today")).len(), 1, "{n}");
            assert_eq!(scan("docs/x.md", &format!("{}:~/w", n.to_uppercase())).len(), 1, "{n} upper");
        }
    }

    #[test]
    fn longer_words_are_not_host_names() {
        assert!(scan("docs/x.md", "nicotine, meiosis, shikimi, alma_mater, keke2, xmei, 3keke, _nico, Rakanon").is_empty());
    }

    #[test]
    fn planted_address_is_caught() {
        // Addresses are built at run time so this file passes the private-paths check.
        let (lab, lan) = (["10", "0", "100", ""].join("."), ["10", "0", "0", ""].join("."));
        assert_eq!(scan("a.sh", &format!("ssh claude@{lab}53")).len(), 1);
        assert_eq!(scan("a.sh", &format!("ssh claude@{lan}7")).len(), 1);
        assert!(scan("a.sh", &format!("ssh claude@1{lab}53 and 10.0.1.5 and {lab}")).is_empty());
    }

    #[test]
    fn allowlisted_path_and_marked_line_and_handoff_are_not_caught() {
        let a = ["10", "0", "100", "53"].join(".");
        assert!(scan("xtask/src/labnames.rs", &format!("keke {a}")).is_empty());
        assert!(scan("handoff/x.md", &format!("keke {a}")).is_empty());
        assert!(scan("v/h.sh", &format!("keke) addr={a} # LAB-ONLY: host map")).is_empty());
        assert_eq!(scan("v/h.sh", &format!("keke) addr={a}")).len(), 1);
    }
}
