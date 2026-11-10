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

/// The clause ID a clause-shaped word `w` names, if any. A test named after its clause with a word appended
/// (`s3_2_c_1_perturb`) names it.
fn resolve<'a>(w: &'a str, ids: &BTreeSet<String>) -> Option<&'a str> {
    let mut base = w;
    while !ids.contains(base) {
        match base.rsplit_once('_') {
            Some((b, tail)) if tail.len() >= 3 && tail.bytes().all(|c| c.is_ascii_lowercase()) => base = b,
            _ => break,
        }
    }
    ids.contains(base).then_some(base)
}

/// What `text` names: (valid clause IDs, clause-shaped words that are not IDs).
fn named<'a>(text: &'a str, ids: &BTreeSet<String>) -> (usize, Vec<&'a str>) {
    let (mut valid, mut stale) = (0, Vec::new());
    for w in words(text).filter(|w| clause_shaped(w)) {
        if resolve(w, ids).is_some() {
            valid += 1;
        } else {
            stale.push(w);
        }
    }
    (valid, stale)
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    files_with(dir, &["rs"], out);
}

/// The files under `dir` whose extension is one of `exts`, in sorted path order.
fn files_with(dir: &Path, exts: &[&str], out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    let mut entries: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            files_with(&p, exts, out);
        } else if p.extension().and_then(|e| e.to_str()).is_some_and(|e| exts.contains(&e)) {
            out.push(p);
        }
    }
}

/// `src` without its `clause!(...)` / `commentary!(...)` items (a pending stub or a commentary item is an ignored
/// test or a constant, not a test, so it must not credit its ID to the test above it), plus the IDs of the
/// commentary items it held. An item runs from its macro name at a line start to the `"##);` closing its raw string.
fn strip_items(src: &str) -> (String, Vec<String>) {
    let (mut out, mut commentary, mut rest) = (String::with_capacity(src.len()), Vec::new(), src);
    loop {
        let at = ["clause!(", "commentary!("].iter().filter_map(|m| rest.match_indices(m).find(|(i, _)| *i == 0 || rest.as_bytes()[i - 1] == b'\n').map(|(i, _)| (i, *m))).min();
        let Some((i, m)) = at else { break };
        out.push_str(&rest[..i]);
        let item = &rest[i..];
        let end = item.find("\"##);").map_or(item.len(), |e| e + 5);
        if m == "commentary!(" {
            commentary.extend(words(&item[m.len()..]).next().map(String::from));
        }
        rest = &item[end..];
    }
    out.push_str(rest);
    (out, commentary)
}

/// Sort key of a spec section number (`11` < `11.1` < `11.2` < `11.10`, `12.5` < `12.5a`).
fn section_key(s: &str) -> Vec<(u32, String)> {
    s.split('.')
        .map(|part| {
            let d = part.bytes().take_while(u8::is_ascii_digit).count();
            (part[..d].parse().unwrap_or(u32::MAX), part[d..].to_string())
        })
        .collect()
}

/// `cargo xtask trace --untested [--out <file>]`: report only, never fails on what it finds. Lists every clause of
/// `spec/clauses.tsv` that no test names, grouped by spec section with a count per section. A clause counts as
/// tested when a Rust `#[test]` under `crates/*/tests` or `crates/*/src` names its ID in its own text (as
/// [`test_texts`] splits a file: its name, leading comment block or body), or a conformance program under
/// `conformance/` names it anywhere. Prints the per-section summary; `--out` also writes the full list as Markdown.
pub fn untested(root: &Path, out: Option<&Path>) -> Result<(), String> {
    let tsv = fs::read_to_string(root.join("spec/clauses.tsv")).map_err(|e| format!("spec/clauses.tsv: {e}"))?;
    let rows: Vec<Vec<&str>> = tsv.lines().skip(1).map(|l| l.splitn(4, '\t').collect::<Vec<_>>()).filter(|r| r.len() == 4).collect();
    let ids: BTreeSet<String> = rows.iter().map(|r| r[0].to_string()).collect();
    let mut credited: BTreeSet<String> = BTreeSet::new();
    let credit = |credited: &mut BTreeSet<String>, text: &str| {
        for w in words(text).filter(|w| clause_shaped(w)) {
            if let Some(id) = resolve(w, &ids) {
                credited.insert(id.to_string());
            }
        }
    };
    let mut files = Vec::new();
    let mut krates: Vec<PathBuf> = fs::read_dir(root.join("crates")).map_err(|e| e.to_string())?.flatten().map(|e| e.path()).collect();
    krates.sort();
    for k in &krates {
        rust_files(&k.join("tests"), &mut files);
        rust_files(&k.join("src"), &mut files);
    }
    let mut commentary: BTreeSet<String> = BTreeSet::new();
    for f in &files {
        let (src, items) = strip_items(&fs::read_to_string(f).map_err(|e| format!("{}: {e}", f.display()))?);
        commentary.extend(items);
        test_texts(&src).iter().for_each(|t| credit(&mut credited, t));
    }
    let by_rust = credited.len();
    let mut progs = Vec::new();
    files_with(&root.join("conformance"), &["c", "h", "sh", "rs"], &mut progs);
    for f in &progs {
        credit(&mut credited, &fs::read_to_string(f).map_err(|e| format!("{}: {e}", f.display()))?);
    }

    // Section -> (clauses, untested rows), in section order.
    let mut sections: Vec<(&str, usize, Vec<&Vec<&str>>)> = Vec::new();
    for r in &rows {
        let i = match sections.iter().position(|s| s.0 == r[1]) {
            Some(i) => i,
            None => {
                sections.push((r[1], 0, Vec::new()));
                sections.len() - 1
            }
        };
        sections[i].1 += 1;
        if !credited.contains(r[0]) && !commentary.contains(r[0]) {
            sections[i].2.push(r);
        }
    }
    sections.sort_by_key(|s| section_key(s.0));
    let untested: usize = sections.iter().map(|s| s.2.len()).sum();
    let summary = format!(
        "trace --untested: {} clauses, {} tested ({by_rust} by Rust tests, {} more by conformance programs), {} commentary (not testable), {untested} untested in {} of {} sections",
        rows.len(),
        credited.len(),
        credited.len() - by_rust,
        commentary.iter().filter(|c| !credited.contains(*c)).count(),
        sections.iter().filter(|s| !s.2.is_empty()).count(),
        sections.len()
    );
    println!("{summary}");
    for s in sections.iter().filter(|s| !s.2.is_empty()) {
        println!("  {:<6} {:>3} of {:>3} untested", s.0, s.2.len(), s.1);
    }
    let Some(out) = out else { return Ok(()) };
    let mut md = String::from("# Spec clauses with no traced test\n\nProduced by `cargo xtask trace --untested --out <this file>` (report only: it never fails the build). ");
    md.push_str("A clause counts as tested when a Rust `#[test]` (`crates/*/tests`, `crates/*/src`) names its ID in its own text ");
    md.push_str("(name, leading comment block or body), or a program under `conformance/` names it. A test named only after a ");
    md.push_str("symbol (`sym_`, `fam_`) credits no clause, so a clause listed here may still be exercised by a test that lacks its tag.\n\n");
    md.push_str(&format!("{summary}.\n\n## Per section\n\n| Section | Clauses | Untested |\n|---|---|---|\n"));
    for s in &sections {
        md.push_str(&format!("| {} | {} | {} |\n", s.0, s.1, s.2.len()));
    }
    md.push_str("\n## Untested clauses\n");
    for s in sections.iter().filter(|s| !s.2.is_empty()) {
        md.push_str(&format!("\n### {} ({} of {})\n\n", s.0, s.2.len(), s.1));
        for r in &s.2 {
            let text: String = r[3].chars().take(200).collect();
            let more = if r[3].chars().count() > 200 { " ..." } else { "" };
            md.push_str(&format!("- `{}` ({}): {text}{more}\n", r[0], r[2]));
        }
    }
    fs::write(out, md).map_err(|e| format!("{}: {e}", out.display()))
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

    decisions(root);

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

/// Decision IDs (owner, 2026-10-04): every `D-…` cited in the tree resolves to a decision record
/// (`records/decisions/<series>/<id>.toml`, RECORDS-1, D-X.55). Report-only. One file per id makes a decision defined
/// twice impossible (the DUP-D check of TOOL-CLEAN, 2026-10-08, is gone with the tables it scanned); record bodies are
/// scanned for citations like any other text.
fn decisions(root: &Path) {
    let ids = crate::records::ids(root, crate::records::Kind::Decision);
    let rows: BTreeSet<&str> = ids.iter().map(String::as_str).collect();
    let mut files = vec![root.join("implementation-plan.md"), root.join("README.md")];
    for d in ["plan", "records", "crates", "csys", "conformance", "docs", "spec", "xtask", "benchmarks", "bench", "validation", "scripts"] {
        text_files(&root.join(d), &mut files);
    }
    let mut unknown: std::collections::BTreeMap<String, BTreeSet<String>> = std::collections::BTreeMap::new();
    for f in &files {
        // This file's own examples are not citations, nor are the records module's (id shapes, test records).
        if f.ends_with("xtask/src/trace.rs") || f.starts_with(root.join("xtask/src/records")) {
            continue;
        }
        let Ok(text) = fs::read_to_string(f) else { continue };
        let rel = f.strip_prefix(root).unwrap_or(f).display().to_string();
        for id in decision_ids(&text) {
            if !rows.contains(id) {
                unknown.entry(id.to_string()).or_default().insert(rel.clone());
            }
        }
    }
    println!("trace: decision IDs: {} decision records, {} cited without a record (report only)", rows.len(), unknown.len());
    for (id, files) in &unknown {
        println!("trace:   {id}: no decision record (cited in {})", files.iter().cloned().collect::<Vec<_>>().join(", "));
    }
}

#[cfg(test)]
mod tests {
    use super::{clause_shaped, decision_ids, resolve, section_key, strip_items, test_texts};
    use std::collections::BTreeSet;

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

    // no spec clause: the report's section order (numeric, a letter suffix after its number).
    #[test]
    fn sections_sort_by_number() {
        let mut s = vec!["11.10", "2.1", "11", "12.5a", "11.2", "12.5", "0.6"];
        s.sort_by_key(|x| section_key(x));
        assert_eq!(s, ["0.6", "2.1", "11", "11.2", "11.10", "12.5", "12.5a"]);
    }

    // no spec clause: a clause-named test with a word appended credits its clause; an unknown ID credits nothing.
    #[test]
    fn resolve_strips_appended_words() {
        let ids: BTreeSet<String> = ["s3_2_c_1".to_string()].into();
        assert_eq!(resolve("s3_2_c_1_perturb", &ids), Some("s3_2_c_1"));
        assert_eq!(resolve("s3_2_c_1", &ids), Some("s3_2_c_1"));
        assert_eq!(resolve("s3_2_c_2", &ids), None);
    }

    // no spec clause: a pending `clause!` stub after a test is not credited to it; a commentary item is reported.
    #[test]
    fn stubs_are_not_tests() {
        let src = "#[test]\nfn s1_1_c_0() {}\nclause!(s1_1_c_1, \"1.1\", \"Contract\", r##\"two\nlines\"##);\ncommentary!(s1_1_n_0, \"1.1\", \"Notes\", r##\"x\"##);\nfn tail() { s1_1_c_2(); }\n";
        let (out, commentary) = strip_items(src);
        assert!(!out.contains("s1_1_c_1") && !out.contains("s1_1_n_0") && out.contains("s1_1_c_2"), "{out}");
        assert_eq!(commentary, ["s1_1_n_0"]);
    }
}
