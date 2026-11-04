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
const ALLOW: [(&str, &str); 1] = [("xtask/src/labnames.rs", "this check's own name list and its tests")];

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

fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let rel = p.strip_prefix(root).map(|r| r.to_string_lossy().into_owned()).unwrap_or_default();
        if rel == "target" || rel == ".git" || rel.starts_with("handoff") {
            continue;
        }
        if p.is_dir() {
            walk(root, &p, out);
        } else if p.is_file() {
            out.push(rel);
        }
    }
}

/// The files to scan: `git ls-files`; in a synced role tree without git metadata, every file outside `target/`.
fn tracked(root: &Path) -> Result<Vec<String>, String> {
    match Command::new("git").current_dir(root).args(["ls-files", "-z"]).output() {
        Ok(o) if o.status.success() => Ok(o.stdout.split(|&b| b == 0).filter(|n| !n.is_empty()).map(|n| String::from_utf8_lossy(n).into_owned()).collect()),
        _ => {
            let mut v = Vec::new();
            walk(root, root, &mut v);
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
    let files: Vec<String> = per_file.iter().take(60).map(|(f, c)| format!("{f} ({c})")).collect();
    let first: Vec<&str> = found.iter().take(40).map(String::as_str).collect();
    Err(format!("{n} lab host name / address hit(s) in {} file(s) outside handoff/ (name the processor class instead, or add an ALLOW entry with a reason in xtask/src/labnames.rs):\n  files: {}\n  {}", per_file.len(), files.join(", "), first.join("\n  ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn planted_host_name_is_caught() {
        for n in NAMES {
            assert_eq!(scan("docs/x.md", &format!("measured on {n} today")).len(), 1, "{n}");
            assert_eq!(scan("docs/x.md", &format!("{}:~/w", n.to_uppercase())).len(), 1, "{n} upper");
        }
    }

    #[test]
    fn longer_words_are_not_host_names() {
        assert!(scan("docs/x.md", "nicotine, meiosis, shikimi, alma_mater, keke2").is_empty());
    }

    #[test]
    fn planted_address_is_caught() {
        assert_eq!(scan("a.sh", "ssh claude@10.0.100.53").len(), 1);
        assert_eq!(scan("a.sh", "ssh claude@10.0.0.7").len(), 1);
        assert!(scan("a.sh", "ssh claude@110.0.100.53 and 10.0.1.5 and 10.0.100.").is_empty());
    }

    #[test]
    fn allowlisted_path_and_marked_line_and_handoff_are_not_caught() {
        assert!(scan("xtask/src/labnames.rs", "keke 10.0.100.53").is_empty());
        assert!(scan("handoff/x.md", "keke 10.0.100.53").is_empty());
        assert!(scan("v/h.sh", "keke) addr=10.0.100.53 # LAB-ONLY: host map").is_empty());
        assert_eq!(scan("v/h.sh", "keke) addr=10.0.100.53").len(), 1);
    }
}
