//! `cargo xtask register --check`: the saturation register is one file per section under
//! `docs/saturation/`, listed in `docs/saturation-register.md`. Checks: every section file is listed in the
//! index and every listed file exists; section numbers (the `NN-` file prefix and the `## N.` heading) are
//! unique and contiguous from 1 (00 is the introduction); every row id (first cell of a table whose header
//! starts with `Row`) is unique across all files. Renumbering is done by tool, never by hand.
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

fn section_files(dir: &Path) -> Result<BTreeMap<String, u32>, String> {
    let mut out = BTreeMap::new();
    for e in std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let name = e.map_err(|e| e.to_string())?.file_name().to_string_lossy().into_owned();
        if !name.ends_with(".md") {
            continue;
        }
        let num = name.split('-').next().and_then(|p| p.parse::<u32>().ok()).ok_or_else(|| format!("docs/saturation/{name}: file name must start with a section number and '-'"))?;
        out.insert(name, num);
    }
    Ok(out)
}

/// Check the register layout; returns the problems found as an error.
pub fn check(root: &Path) -> Result<(), String> {
    let mut bad: Vec<String> = Vec::new();
    let dir = root.join("docs/saturation");
    let files = section_files(&dir)?;
    let index = std::fs::read_to_string(root.join("docs/saturation-register.md")).map_err(|e| format!("docs/saturation-register.md: {e}"))?;
    let listed: BTreeSet<String> = index.split("](saturation/").skip(1).filter_map(|r| r.split(')').next()).map(str::to_string).collect();
    for f in files.keys() {
        if !listed.contains(f) {
            bad.push(format!("{f}: not listed in docs/saturation-register.md"));
        }
    }
    for l in &listed {
        if !files.contains_key(l) {
            bad.push(format!("index lists {l}, which does not exist"));
        }
    }
    let mut seen: BTreeMap<u32, &str> = BTreeMap::new();
    for (f, n) in &files {
        if let Some(prev) = seen.insert(*n, f) {
            bad.push(format!("section number {n} used by both {prev} and {f}"));
        }
    }
    let max = files.values().copied().max().unwrap_or(0);
    for n in 0..=max {
        if !seen.contains_key(&n) {
            bad.push(format!("section numbers are not contiguous: {n} is missing"));
        }
    }
    let mut rows: BTreeMap<String, String> = BTreeMap::new();
    for (f, n) in &files {
        let text = std::fs::read_to_string(dir.join(f)).map_err(|e| format!("{f}: {e}"))?;
        if *n > 0 {
            let head = text.lines().next().unwrap_or("");
            if !head.starts_with(&format!("## {n}. ")) {
                bad.push(format!("{f}: first line must be the heading '## {n}. <title>'"));
            }
        }
        let mut in_rows = false;
        for line in text.lines() {
            if !line.starts_with('|') {
                in_rows = false;
                continue;
            }
            let first = line.trim_matches('|').split('|').next().unwrap_or("").trim().to_string();
            if first == "Row" {
                in_rows = true;
            } else if in_rows && !first.chars().all(|c| matches!(c, '-' | ':' | ' ')) {
                if let Some(prev) = rows.insert(first.clone(), f.clone()) {
                    bad.push(format!("row id {first} appears in both {prev} and {f}"));
                }
            }
        }
    }
    if bad.is_empty() {
        Ok(())
    } else {
        Err(bad.join("\n"))
    }
}
