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
    bad.extend(stale_references(root, max)?);
    if bad.is_empty() {
        Ok(())
    } else {
        Err(bad.join("\n"))
    }
}

/// Section numbers cited by register references in `text`: a "register" (also "saturation register",
/// "register's") followed by `§N`, `section N` or `N.M`. Returns (1-based line, cited section, the cited text).
fn register_refs(text: &str) -> Vec<(usize, u32, String)> {
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let lower = line.to_ascii_lowercase();
        let mut from = 0;
        while let Some(at) = lower[from..].find("register") {
            let start = from + at;
            from = start + "register".len();
            if line[..start].chars().next_back().is_some_and(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
                continue;
            }
            let mut rest = &line[from..];
            rest = rest.strip_prefix("'s").or_else(|| rest.strip_prefix("\u{2019}s")).unwrap_or(rest);
            let rest = rest.trim_start_matches(' ');
            let (rest, marked) = if let Some(r) = rest.strip_prefix('\u{a7}') {
                (r.trim_start_matches(' '), true)
            } else if let Some(r) = rest.strip_prefix("section ") {
                (r.trim_start_matches(' '), true)
            } else {
                (rest, false)
            };
            let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
            if digits.is_empty() || digits.len() > 3 {
                continue;
            }
            let after = &rest[digits.len()..];
            let dotted = after.strip_prefix('.').is_some_and(|a| a.chars().next().is_some_and(|c| c.is_ascii_digit()));
            if !marked && !dotted {
                continue;
            }
            if let Ok(n) = digits.parse() {
                out.push((i + 1, n, format!("register {}{}", if marked { "\u{a7}" } else { "" }, digits)));
            }
        }
    }
    out
}

/// Every tracked text file outside the register itself that cites a register section number which does not
/// exist (`max` is the highest section). Reported with file and line.
fn stale_references(root: &Path, max: u32) -> Result<Vec<String>, String> {
    // Walk the directory (a synced role tree has no .git), skipping build output and hidden directories.
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let path = e.path();
            if path.is_dir() {
                if !name.starts_with('.') && name != "target" {
                    walk(root, &path, out);
                }
            } else if let Ok(rel) = path.strip_prefix(root) {
                out.push(rel.to_string_lossy().into_owned());
            }
        }
    }
    let mut files = Vec::new();
    walk(root, root, &mut files);
    files.sort();
    let mut bad = Vec::new();
    for rel in files.iter().map(String::as_str) {
        if rel.starts_with("docs/saturation/") || rel.starts_with("spec/") || rel == "glibc-functional-spec.md" || rel.ends_with(".tsv") || rel.starts_with("xtask/src/register.rs") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(root.join(rel)) else { continue };
        for (line, n, what) in register_refs(&text) {
            if n < 1 || n > max {
                bad.push(format!("{rel}:{line}: {what} cites register section {n}, but the register has sections 1..={max}"));
            }
        }
    }
    Ok(bad)
}

#[cfg(test)]
mod tests {
    use super::register_refs;

    #[test]
    fn current_and_stale_references() {
        // Plant: a comment citing a section number the register no longer has.
        let refs = register_refs("// see register 13.5 #8\n// the register's \u{a7}7 and saturation register section 16\n// unregister 99.1, register_x 5.5, register a\n");
        assert_eq!(refs.iter().map(|r| (r.0, r.1)).collect::<Vec<_>>(), vec![(1, 13), (2, 7), (2, 16)]);
        let stale: Vec<_> = refs.iter().filter(|r| r.1 > 15).collect();
        assert_eq!(stale.len(), 1);
        assert_eq!(stale[0].0, 2);
    }
}
