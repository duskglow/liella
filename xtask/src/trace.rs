//! `cargo xtask trace`: clause traceability (owner item 6, 2026-10-04). A spec correction must
//! yield its affected set by a search for the clause's ID, so:
//!
//! * every test (`#[test]` in `crates/*/tests`) carries the ID of the clause it came from: its
//!   name is a spec test ID (a clause ID, `sym_`/`fam_`, `k_`, `abi_`/`init_`), or its body names
//!   a clause ID, or it says why it has none: `no spec clause` (it checks something no clause
//!   states: an internal engine, the kernel ABI types, the native interface) or `no clause ID`
//!   (the spec states it, but `spec/clauses.tsv` has no row for the text);
//! * every row of the tables of `docs/differences-from-glibc.md` and every behaviour document
//!   (`spec/behaviour/*.md` but the README) names a clause ID, or says `no clause ID`;
//! * every clause ID named in those files exists in `spec/clauses.tsv` (a renumbered or removed
//!   clause shows as a stale reference).
//!
//! `cargo xtask stubs` runs it after regenerating `spec/clauses.tsv`, and `cargo xtask test`
//! runs it with the other policy checks.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

const DIFFERENCES: &str = "docs/differences-from-glibc.md";
const BEHAVIOUR: &str = "spec/behaviour";

/// Whether `tok` has the shape of a clause ID: `s<digits>_<digits>[a-z]_<letter>...`.
fn clause_shaped(tok: &str) -> bool {
    let b = tok.as_bytes();
    let digits = |mut i: usize| {
        let s = i;
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        (i > s).then_some(i)
    };
    if b.first() != Some(&b's') {
        return false;
    }
    let Some(i) = digits(1) else { return false };
    if b.get(i) != Some(&b'_') {
        return false;
    }
    let Some(mut i) = digits(i + 1) else { return false };
    if b.get(i).is_some_and(u8::is_ascii_lowercase) && b.get(i + 1) == Some(&b'_') {
        i += 1;
    }
    b.get(i) == Some(&b'_') && b.get(i + 1).is_some_and(u8::is_ascii_lowercase)
}

/// The identifier-like words of `text`.
fn words(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).filter(|w| !w.is_empty())
}

/// What `text` names: (valid clause IDs, clause-shaped words that are not IDs).
fn named<'a>(text: &'a str, ids: &BTreeSet<String>) -> (usize, Vec<&'a str>) {
    let (mut valid, mut stale) = (0, Vec::new());
    for w in words(text).filter(|w| clause_shaped(w)) {
        // A test named after its clause with a word appended (`s3_2_c_1_perturb`) names it.
        let mut base = w;
        while !ids.contains(base) {
            match base.rsplit_once('_') {
                Some((b, tail)) if tail.len() >= 3 && tail.bytes().all(|c| c.is_ascii_lowercase()) => base = b,
                _ => break,
            }
        }
        if ids.contains(base) {
            valid += 1;
        } else {
            stale.push(w);
        }
    }
    (valid, stale)
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    let mut entries: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            rust_files(&p, out);
        } else if p.extension().is_some_and(|e| e == "rs") {
            out.push(p);
        }
    }
}

/// How a test is traced.
#[derive(Default)]
struct Counts {
    named: usize,
    by_clause: usize,
    no_spec_clause: usize,
    no_clause_id: usize,
    untraced: usize,
}

/// The text that belongs to each test of `src`, in order. A test owns the comment/attribute lines directly above its
/// `#[test]` (a doc comment, a `//` comment, another attribute; a blank line ends the block) and everything from its
/// `#[test]` line up to the leading block of the next test. A tag is never credited to a neighbour.
fn test_texts(src: &str) -> Vec<String> {
    let lines: Vec<&str> = src.lines().collect();
    let attached = |l: &str| {
        let t = l.trim();
        t.starts_with("//") || t.starts_with("#[")
    };
    let starts: Vec<usize> = (0..lines.len()).filter(|&i| lines[i].trim() == "#[test]").collect();
    // The first line of each test's own text: its #[test] line, moved up over the block directly above it.
    let firsts: Vec<usize> = starts
        .iter()
        .enumerate()
        .map(|(n, &t)| {
            let floor = if n == 0 { 0 } else { starts[n - 1] + 1 };
            let mut j = t;
            while j > floor && attached(lines[j - 1]) && lines[j - 1].trim() != "#[test]" {
                j -= 1;
            }
            j
        })
        .collect();
    (0..starts.len())
        .map(|n| {
            let end = firsts.get(n + 1).copied().unwrap_or(lines.len());
            lines[firsts[n]..end].join("\n")
        })
        .collect()
}

/// Check the traceability; the counts are printed.
pub fn run(root: &Path) -> Result<(), String> {
    let tsv = fs::read_to_string(root.join("spec/clauses.tsv")).map_err(|e| format!("spec/clauses.tsv: {e}"))?;
    let ids: BTreeSet<String> = tsv.lines().skip(1).filter_map(|l| l.split('\t').next()).map(String::from).collect();
    let mut problems = Vec::new();
    let rel = |p: &Path| p.strip_prefix(root).unwrap_or(p).display().to_string();

    // Tests.
    let mut files = Vec::new();
    let crates = root.join("crates");
    let mut krates: Vec<PathBuf> = fs::read_dir(&crates).map_err(|e| e.to_string())?.flatten().map(|e| e.path()).collect();
    krates.sort();
    for k in krates {
        rust_files(&k.join("tests"), &mut files);
    }
    let mut c = Counts::default();
    for f in &files {
        let src = fs::read_to_string(f).map_err(|e| format!("{}: {e}", rel(f)))?;
        let (_, stale) = named(&src, &ids);
        for s in stale.iter().collect::<BTreeSet<_>>() {
            problems.push(format!("{}: `{s}` is not a clause ID of spec/clauses.tsv", rel(f)));
        }
        // Each test: its own leading comment block plus its body.
        let bodies = test_texts(&src);
        for body in &bodies {
            let name = body.split("#[test]").nth(1).unwrap_or("").split("fn ").nth(1).and_then(|r| words(r).next()).unwrap_or("?");
            let spec_named = ids.contains(name) || ["sym_", "fam_", "k_", "abi_", "init_"].iter().any(|p| name.starts_with(p));
            if spec_named {
                c.named += 1;
            } else if named(body, &ids).0 > 0 {
                c.by_clause += 1;
            } else if body.contains("no spec clause") {
                c.no_spec_clause += 1;
            } else if body.contains("no clause ID") {
                c.no_clause_id += 1;
            } else {
                c.untraced += 1;
                problems.push(format!("{}: test `{name}` names no clause ID (nor `no spec clause` / `no clause ID`)", rel(f)));
            }
        }
    }

    // The differences from glibc: every table row.
    let diff = fs::read_to_string(root.join(DIFFERENCES)).map_err(|e| format!("{DIFFERENCES}: {e}"))?;
    let lines: Vec<&str> = diff.lines().collect();
    let (mut rows, mut rows_id, mut rows_none) = (0, 0, 0);
    for (i, l) in lines.iter().enumerate() {
        let header = lines.get(i + 1).is_some_and(|n| n.starts_with("|---"));
        if !l.starts_with("| ") || header {
            continue;
        }
        rows += 1;
        let (valid, stale) = named(l, &ids);
        for s in stale {
            problems.push(format!("{DIFFERENCES}:{}: `{s}` is not a clause ID of spec/clauses.tsv", i + 1));
        }
        if valid > 0 {
            rows_id += 1;
        } else if l.contains("no clause ID") {
            rows_none += 1;
        } else {
            problems.push(format!("{DIFFERENCES}:{}: the row names no clause ID", i + 1));
        }
    }

    // Behaviour documents.
    let mut docs: Vec<PathBuf> = fs::read_dir(root.join(BEHAVIOUR)).map_err(|e| e.to_string())?.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "md") && !p.ends_with("README.md")).collect();
    docs.sort();
    let mut docs_id = 0;
    for d in &docs {
        let text = fs::read_to_string(d).map_err(|e| format!("{}: {e}", rel(d)))?;
        let (valid, stale) = named(&text, &ids);
        for s in stale.iter().collect::<BTreeSet<_>>() {
            problems.push(format!("{}: `{s}` is not a clause ID of spec/clauses.tsv", rel(d)));
        }
        if valid > 0 {
            docs_id += 1;
        } else if !text.contains("no clause ID") {
            problems.push(format!("{}: names no clause ID", rel(d)));
        }
    }

    problems.extend(decisions(root));

    let tests = c.named + c.by_clause + c.no_spec_clause + c.no_clause_id + c.untraced;
    println!(
        "trace: {tests} tests: {} named by a spec ID, {} name clause IDs, {} `no spec clause`, {} `no clause ID`, {} untraced",
        c.named, c.by_clause, c.no_spec_clause, c.no_clause_id, c.untraced
    );
    println!("trace: {DIFFERENCES}: {rows} rows, {rows_id} with clause IDs, {rows_none} `no clause ID`");
    println!("trace: {BEHAVIOUR}: {} documents, {docs_id} with clause IDs", docs.len());
    if problems.is_empty() {
        return Ok(());
    }
    for p in &problems {
        eprintln!("{p}");
    }
    Err(format!("{} traceability problems", problems.len()))
}

/// The decision IDs (`D-3.14`, `D-C0.2`, `D-24.11b`) cited in `text`: `D-`, capitals or digits,
/// one or more `.digits`, an optional lowercase suffix; not inside a longer word.
fn decision_ids(text: &str) -> Vec<&str> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(off) = text.get(i..).and_then(|t| t.find("D-")) {
        let start = i + off;
        i = start + 2;
        if start > 0 && b.get(start - 1).is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_' || *c == b'-') {
            continue;
        }
        let mut j = i;
        while b.get(j).is_some_and(|c| c.is_ascii_uppercase() || c.is_ascii_digit()) {
            j += 1;
        }
        if j == i {
            continue;
        }
        let mut dots = 0;
        while b.get(j) == Some(&b'.') && b.get(j + 1).is_some_and(u8::is_ascii_digit) {
            j += 1;
            while b.get(j).is_some_and(u8::is_ascii_digit) {
                j += 1;
            }
            dots += 1;
        }
        if dots == 0 {
            continue;
        }
        if b.get(j).is_some_and(u8::is_ascii_lowercase) && !b.get(j + 1).is_some_and(|c| c.is_ascii_alphanumeric()) {
            j += 1;
        }
        if b.get(j).is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_') {
            continue;
        }
        if let Some(id) = text.get(start..j) {
            out.push(id);
        }
        i = j;
    }
    out
}

fn text_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    let mut entries: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            if !p.ends_with("target") {
                text_files(&p, out);
            }
        } else if p.extension().is_some_and(|e| ["rs", "c", "h", "md", "tsv", "txt", "sh", "toml"].iter().any(|x| e == *x)) {
            out.push(p);
        }
    }
}

/// Decision IDs (owner, 2026-10-04): every `D-…` cited in the tree resolves to exactly one row
/// of the plan's decision register (a row starting `| D-… |`). Report-only: it prints what it
/// finds, except DUP-D (TOOL-CLEAN, 2026-10-08): a decision number that heads two rows of `plan/07-decisions` is
/// returned as a problem, which fails the check (D-X.17 was duplicated on main for a day).
fn decisions(root: &Path) -> Vec<String> {
    // The plan is an index (implementation-plan.md) plus part files under plan/ (2026-10-05).
    let mut plan_files = vec![root.join("implementation-plan.md")];
    text_files(&root.join("plan"), &mut plan_files);
    plan_files.sort();
    let plan: String = plan_files.iter().filter_map(|f| fs::read_to_string(f).ok()).collect::<Vec<_>>().join("\n");
    let mut rows: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for l in plan.lines() {
        if let Some(id) = l.strip_prefix("| ").and_then(|r| r.split(" |").next()).filter(|id| decision_ids(id) == [*id]) {
            *rows.entry(id).or_default() += 1;
        }
    }
    let mut files = vec![root.join("implementation-plan.md"), root.join("README.md")];
    for d in ["plan", "crates", "csys", "conformance", "docs", "spec", "xtask", "benchmarks", "bench", "validation", "scripts"] {
        text_files(&root.join(d), &mut files);
    }
    let mut unknown: std::collections::BTreeMap<String, BTreeSet<String>> = std::collections::BTreeMap::new();
    for f in &files {
        // This file's own examples are not citations.
        if f.ends_with("xtask/src/trace.rs") {
            continue;
        }
        let Ok(text) = fs::read_to_string(f) else { continue };
        let rel = f.strip_prefix(root).unwrap_or(f).display().to_string();
        for id in decision_ids(&text) {
            if !rows.contains_key(id) {
                unknown.entry(id.to_string()).or_default().insert(rel.clone());
            }
        }
    }
    let dup: Vec<&str> = rows.iter().filter(|(_, n)| **n > 1).map(|(id, _)| *id).collect();
    println!("trace: decision IDs: {} plan rows, {} defined more than once, {} cited without a row (report only)", rows.len(), dup.len(), unknown.len());
    for id in dup {
        println!("trace:   {id}: more than one plan row");
    }
    for (id, files) in &unknown {
        println!("trace:   {id}: no plan row (cited in {})", files.iter().cloned().collect::<Vec<_>>().join(", "));
    }
    let mut parts = Vec::new();
    text_files(&root.join("plan").join("07-decisions"), &mut parts);
    let named: Vec<(String, String)> = parts.iter().map(|f| (f.strip_prefix(root).unwrap_or(f).display().to_string().replace('\\', "/"), fs::read_to_string(f).unwrap_or_default())).collect();
    duplicate_rows(&named).into_iter().map(|(id, at)| format!("plan/07-decisions: {id} heads {} rows ({})", at.len(), at.join(", "))).collect()
}

/// The decision numbers that head more than one row (`| D-x.y |`) across the given (file, text) pairs, with where.
fn duplicate_rows(files: &[(String, String)]) -> Vec<(String, Vec<String>)> {
    let mut at: std::collections::BTreeMap<&str, Vec<String>> = std::collections::BTreeMap::new();
    for (f, text) in files {
        for (n, l) in text.lines().enumerate() {
            if let Some(id) = l.strip_prefix("| ").and_then(|r| r.split(" |").next()).filter(|id| decision_ids(id) == [*id]) {
                at.entry(id).or_default().push(format!("{f}:{}", n + 1));
            }
        }
    }
    at.into_iter().filter(|(_, v)| v.len() > 1).map(|(id, v)| (id.to_string(), v)).collect()
}

#[cfg(test)]
mod tests {
    use super::{clause_shaped, decision_ids, duplicate_rows, test_texts};

    #[test]
    fn a_tag_is_credited_to_the_test_it_belongs_to() {
        // Trace: no spec clause (internal check of the trace tool itself); TOOL-TRACE-TAG-SCOPE.
        let src = "#[test]
fn above_none() {
    // Trace: TAG_INSIDE
}

/// TAG_DOC for the next one
#[test]
fn doc_above() {
}

// TAG_LINE between two tests
#[test]
fn line_above() {
}
#[test]
fn inside() {
    // Clauses: TAG_BODY
}
#[test]
fn none() {}
";
        let t = test_texts(src);
        assert_eq!(t.len(), 5);
        let has = |n: usize, tag: &str| t[n].contains(tag);
        assert!(has(0, "TAG_INSIDE") && !has(0, "TAG_DOC") && !has(0, "TAG_LINE"), "no tag from below leaks up");
        assert!(has(1, "TAG_DOC") && !has(1, "TAG_INSIDE"), "a doc comment above belongs to its test");
        assert!(has(2, "TAG_LINE") && !has(2, "TAG_DOC"), "a line comment above belongs to its test");
        assert!(has(3, "TAG_BODY") && !has(3, "TAG_LINE"), "a tag inside the body belongs to it");
        assert!(!t[4].contains("TAG_"), "a test with none gets none");
    }

    #[test]
    fn a_decision_number_heading_two_rows_is_found() {
        let a = ("plan/07-decisions/x.md".to_string(), "| D-X.16 | one |\n| D-X.17 | first |\nprose citing D-X.17 and | D-X.18 | mid-line\n".to_string());
        let b = ("plan/07-decisions/s03.md".to_string(), "| D-3.1 | row |\n| D-X.17 | second |\n".to_string());
        let d = duplicate_rows(&[a.clone(), b.clone()]);
        assert_eq!(d, [("D-X.17".to_string(), vec!["plan/07-decisions/x.md:2".to_string(), "plan/07-decisions/s03.md:2".to_string()])]);
        assert!(duplicate_rows(&[a]).is_empty() && duplicate_rows(&[b]).is_empty(), "a citation or one row is not a duplicate");
        let same_file = ("p.md".to_string(), "| D-1.1 | a |\n| D-1.1 | b |\n".to_string());
        assert_eq!(duplicate_rows(&[same_file]).len(), 1, "two rows in one file");
    }

    #[test]
    fn decision_shapes() {
        assert_eq!(decision_ids("D-3.14, D-C0.2; (D-24.11b) D-X.10 and D-T.1."), ["D-3.14", "D-C0.2", "D-24.11b", "D-X.10", "D-T.1"]);
        assert!(decision_ids("D-Bus, OD-2, D-7, xD-3.1, D-3.1x2, D-3.14abc").is_empty());
    }

    #[test]
    fn clause_shapes() {
        for id in ["s13_9_c_1", "s17_5a_c_0", "s0_6_c0_1_1", "s10_2_c_r3", "s21_1_i_0", "s13_t_r1"] {
            assert_eq!(clause_shaped(id), id != "s13_t_r1", "{id}");
        }
        for w in ["s04_strings", "s12_stdio", "sym_getpass", "s1", "s1_2", "s1_2_", "s1_2a3_c"] {
            assert!(!clause_shaped(w), "{w}");
        }
    }
}
