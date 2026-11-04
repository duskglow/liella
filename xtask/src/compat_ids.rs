//! `COMPAT-N` ids of `docs/compat-flags.md` (COMPAT-FLAG-IDS-UNIQUE).
//!
//! Ids are assigned by hand on each branch, so parallel branches picked the same number. The check
//! (part of `cargo xtask static`, alone as `cargo xtask compat-ids`) fails when two rows of the
//! registry share an id, when an id mentioned in `docs/`, `crates/` or `plan/` has no row, or when
//! the ids are not contiguous from 1 (a gap must be listed in [`LISTED_GAPS`]).
//! Handoff notes and reports are history and are not scanned.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const REGISTRY: &str = "docs/compat-flags.md";
const SCANNED: [&str; 3] = ["docs", "crates", "plan"];
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

/// The id of each registry row (a table line whose first cell is `COMPAT-<n>`), with its line.
fn registry_rows(text: &str) -> Vec<(u32, usize)> {
    let mut out = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let Some(cell) = line.strip_prefix('|').and_then(|l| l.split('|').next()) else { continue };
        let Some(num) = cell.trim().strip_prefix(PREFIX) else { continue };
        if let Ok(v) = num.parse::<u32>() {
            out.push((v, n + 1));
        }
    }
    out
}

/// Problems of the registry text plus the references `(file, id, line)` found elsewhere.
fn check_text(registry: &str, refs: &[(String, u32, usize)]) -> Vec<String> {
    let mut errors = Vec::new();
    let mut rows: BTreeMap<u32, usize> = BTreeMap::new();
    for (id, line) in registry_rows(registry) {
        if let Some(first) = rows.insert(id, line) {
            errors.push(format!("{REGISTRY}:{line}: {PREFIX}{id} is already a row at line {first}"));
        }
    }
    if let Some(&max) = rows.keys().next_back() {
        for id in 1..max {
            if !rows.contains_key(&id) && !LISTED_GAPS.contains(&id) {
                errors.push(format!("{REGISTRY}: {PREFIX}{id} is missing (ids run to {PREFIX}{max}; list a deliberate gap in LISTED_GAPS)"));
            }
        }
    }
    for (file, id, line) in refs {
        if !rows.contains_key(id) {
            errors.push(format!("{file}:{line}: {PREFIX}{id} has no row in {REGISTRY}"));
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
    let registry = std::fs::read_to_string(root.join(REGISTRY)).map_err(|e| format!("{REGISTRY}: {e}"))?;
    let mut files = Vec::new();
    for d in SCANNED {
        walk(&root.join(d), &mut files);
    }
    let mut refs = Vec::new();
    for f in files {
        let rel = f.strip_prefix(root).unwrap_or(&f).to_string_lossy().into_owned();
        if rel == REGISTRY {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&f) else { continue };
        for (id, line) in ids_in(&text) {
            refs.push((rel.clone(), id, line));
        }
    }
    let errors = check_text(&registry, &refs);
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

    fn row(n: u32) -> String {
        format!("| {PREFIX}{n} | x | y |\n")
    }

    #[test]
    fn unique_rows_and_known_refs_pass() {
        let reg = format!("{}{}", row(1), row(2));
        assert!(check_text(&reg, &[("a.rs".into(), 2, 3)]).is_empty());
    }

    #[test]
    fn gap_fails() {
        let reg = format!("{}{}", row(1), row(3));
        let e = check_text(&reg, &[]);
        assert_eq!(e.len(), 1);
        assert!(e[0].contains("COMPAT-2 is missing"));
    }

    #[test]
    fn duplicate_row_fails() {
        let reg = format!("{}{}{}", row(1), row(2), row(1));
        let e = check_text(&reg, &[]);
        assert_eq!(e.len(), 1);
        assert!(e[0].contains("already a row"));
    }

    #[test]
    fn reference_without_row_fails() {
        let e = check_text(&row(1), &[("a.rs".into(), 9, 4)]);
        assert_eq!(e.len(), 1);
        assert!(e[0].contains("has no row"));
    }

    #[test]
    fn ids_found_in_text() {
        let t = format!("see {PREFIX}12, and {PREFIX}x and {PREFIX}7.");
        assert_eq!(ids_in(&t), vec![(12, 1), (7, 1)]);
    }
}
