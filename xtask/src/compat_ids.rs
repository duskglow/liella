//! `COMPAT-N` ids (COMPAT-FLAG-IDS-UNIQUE): one record per flag, `records/compat/COMPAT-<n>.toml` (RECORDS-1).
//!
//! Ids come from the ops store (`ops reserve COMPAT`); one file per id makes a duplicate impossible. The check (part
//! of `cargo xtask static`, alone as `cargo xtask compat-ids`) fails when an id mentioned in `docs/`, `crates/`,
//! `plan/` or `records/` has no record, or when the ids are not contiguous from 1 (a gap must be listed in
//! [`LISTED_GAPS`]). The generated view `docs/compat-flags.md` and the compat records themselves are not scanned;
//! handoff notes and reports are history and are not scanned either.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const REGISTRY: &str = "records/compat";
const VIEW: &str = "docs/compat-flags.md";
const SCANNED: [&str; 4] = ["docs", "crates", "plan", "records"];
const EXTENSIONS: [&str; 4] = ["rs", "md", "toml", "tsv"];
const PREFIX: &str = "COMPAT-";
/// Ids deliberately absent from the registry (none).
const LISTED_GAPS: [u32; 0] = [];

/// Every `COMPAT-<digits>` in `text` as `(number, 1-based line)`.
fn ids_in(text: &str) -> Vec<(u32, usize)> {
    let mut out = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let mut rest = line;
        while let Some(at) = rest.find(PREFIX) {
            let after = &rest[at + PREFIX.len()..];
            let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
            if !digits.is_empty()
                && let Ok(v) = digits.parse()
            {
                out.push((v, n + 1));
            }
            rest = after;
        }
    }
    out
}

/// Problems of the record ids plus the references `(file, id, line)` found elsewhere.
fn check_ids(rows: &BTreeSet<u32>, refs: &[(String, u32, usize)]) -> Vec<String> {
    let mut errors = Vec::new();
    if let Some(&max) = rows.iter().next_back() {
        for id in 1..max {
            if !rows.contains(&id) && !LISTED_GAPS.contains(&id) {
                errors.push(format!("{REGISTRY}: {PREFIX}{id} is missing (ids run to {PREFIX}{max}; list a deliberate gap in LISTED_GAPS)"));
            }
        }
    }
    for (file, id, line) in refs {
        if !rows.contains(id) {
            errors.push(format!("{file}:{line}: {PREFIX}{id} has no record in {REGISTRY}/"));
        }
    }
    errors
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            if p.file_name().is_some_and(|n| n == "target") {
                continue;
            }
            walk(&p, out);
        } else if p.extension().and_then(|e| e.to_str()).is_some_and(|e| EXTENSIONS.contains(&e)) {
            out.push(p);
        }
    }
}

pub fn check(root: &Path) -> Result<(), String> {
    let mut rows = BTreeSet::new();
    for id in crate::records::ids(root, crate::records::Kind::Compat) {
        if let Some(n) = id.strip_prefix(PREFIX).and_then(|n| n.parse::<u32>().ok()) {
            rows.insert(n);
        }
    }
    let mut files = Vec::new();
    for d in SCANNED {
        walk(&root.join(d), &mut files);
    }
    let mut refs = Vec::new();
    for f in files {
        let rel = f.strip_prefix(root).unwrap_or(&f).to_string_lossy().replace('\\', "/");
        if rel == VIEW || rel.starts_with(&format!("{REGISTRY}/")) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&f) else { continue };
        for (id, line) in ids_in(&text) {
            refs.push((rel.clone(), id, line));
        }
    }
    let errors = check_ids(&rows, &refs);
    if errors.is_empty() {
        return Ok(());
    }
    for e in errors.iter().take(40) {
        eprintln!("compat-ids: {e}");
    }
    Err(format!("{REGISTRY}: {} problem(s)", errors.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(ids: &[u32]) -> BTreeSet<u32> {
        ids.iter().copied().collect()
    }

    #[test]
    fn contiguous_ids_and_known_refs_pass() {
        assert!(check_ids(&rows(&[1, 2]), &[("a.rs".into(), 2, 3)]).is_empty());
    }

    #[test]
    fn gap_fails() {
        let e = check_ids(&rows(&[1, 3]), &[]);
        assert_eq!(e.len(), 1);
        assert!(e[0].contains("COMPAT-2 is missing"));
    }

    #[test]
    fn reference_without_record_fails() {
        let e = check_ids(&rows(&[1]), &[("a.rs".into(), 9, 4)]);
        assert_eq!(e.len(), 1);
        assert!(e[0].contains("has no record"));
    }

    #[test]
    fn ids_found_in_text() {
        let t = format!("see {PREFIX}12, and {PREFIX}x and {PREFIX}7.");
        assert_eq!(ids_in(&t), vec![(12, 1), (7, 1)]);
    }
}
