//! Rendering the views: `@generated` line, head file, header row, separator, one row per record (sorted by id), tail
//! file. Cells are the stored text, verbatim. Derived markers: a row superseded by an active record starts
//! `**Superseded by <id>.** `, a withdrawn row `**Withdrawn.** `, and an amended row ends ` (Amended by <ids>.)`; they
//! go in the last column of decision and saturation views (compat rows say it in their Status cell).

use std::collections::BTreeMap;
use std::path::Path;

use super::schema::{self, Kind};
use super::{GENERATED, Record, View, ViewKind};

fn read(root: &Path, rel: &str) -> Result<String, String> {
    std::fs::read_to_string(root.join(rel)).map_err(|e| format!("{rel}: {e}"))
}

/// Who supersedes and amends each id (active records only), sorted.
struct Links {
    superseded_by: BTreeMap<String, Vec<String>>,
    amended_by: BTreeMap<String, Vec<String>>,
}

fn links(records: &[Record]) -> Links {
    let mut l = Links { superseded_by: BTreeMap::new(), amended_by: BTreeMap::new() };
    for r in records.iter().filter(|r| !r.pending && r.str("status") != Some("withdrawn")) {
        for t in r.list("supersedes") {
            l.superseded_by.entry(t.clone()).or_default().push(r.id().to_string());
        }
        for t in r.list("amends") {
            l.amended_by.entry(t.clone()).or_default().push(r.id().to_string());
        }
    }
    for v in l.superseded_by.values_mut().chain(l.amended_by.values_mut()) {
        v.sort();
    }
    l
}

fn cells(r: &Record, v: &View, l: &Links) -> Vec<String> {
    let mut out: Vec<String> = v
        .columns
        .iter()
        .map(|c| match c.as_str() {
            "status" => r.str("status_text").or(r.str("status")).unwrap_or("").to_string(),
            c => r.str(c).unwrap_or("").to_string(),
        })
        .collect();
    if r.kind != Kind::Compat
        && let Some(last) = out.last_mut()
    {
        let id = r.id();
        if let Some(by) = l.superseded_by.get(id) {
            *last = format!("**Superseded by {}.** {last}", by.join(", "));
        }
        if r.str("status") == Some("withdrawn") {
            *last = format!("**Withdrawn.** {last}");
        }
        if let Some(by) = l.amended_by.get(id) {
            last.push_str(&format!(" (Amended by {}.)", by.join(", ")));
        }
    }
    out
}

/// A table view's rows: its kind's records in its section, sorted by id.
pub fn rows(v: &View, records: &[Record]) -> Vec<String> {
    let ViewKind::Table(kind) = v.kind else { return Vec::new() };
    let l = links(records);
    let mut mine: Vec<(&Record, (usize, u64, u8))> = records
        .iter()
        .filter(|r| r.kind == kind && !r.pending && (kind == Kind::Compat || r.str("section") == v.section.as_deref()))
        .map(|r| (r, schema::parse_id(kind, r.id()).map_or((usize::MAX, 0, 0), |p| schema::sort_key(&p, &v.series))))
        .collect();
    mine.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.id().cmp(b.0.id())));
    mine.into_iter().map(|(r, _)| format!("| {} |", cells(r, v, &l).join(" | "))).collect()
}

/// The section files of an index view's directory with a number >= 1, in number order: (number, file name).
fn sections(root: &Path, dir: &str) -> Result<Vec<(u32, String)>, String> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(root.join(dir)).map_err(|e| format!("{dir}: {e}"))?.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if let Some(n) = name.strip_suffix(".md").and_then(|s| s.split('-').next()).and_then(|p| p.parse::<u32>().ok())
            && n >= 1
        {
            out.push((n, name));
        }
    }
    out.sort();
    Ok(out)
}

/// Render every view: (path, text), in views.toml order. Table views first, so the index reads the section files'
/// headings from their new text.
pub fn render_all(root: &Path, views: &[View], records: &[Record]) -> Result<Vec<(String, String)>, String> {
    let mut out: Vec<(String, String)> = Vec::new();
    for v in views.iter().filter(|v| v.kind != ViewKind::Index) {
        let mut t = format!("{GENERATED}\n{}{}\n{}\n", read(root, &v.head)?, v.header, v.separator);
        for row in rows(v, records) {
            t.push_str(&row);
            t.push('\n');
        }
        if let Some(tail) = &v.tail {
            t.push_str(&read(root, tail)?);
        }
        out.push((v.path.clone(), t));
    }
    for v in views.iter().filter(|v| v.kind == ViewKind::Index) {
        let dir = v.dir.as_deref().unwrap_or_default();
        let parent = Path::new(&v.path).parent().map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_default();
        let link = dir.strip_prefix(&format!("{parent}/")).unwrap_or(dir);
        let mut t = format!("{GENERATED}\n{}", read(root, &v.head)?);
        for (n, file) in sections(root, dir)? {
            let p = format!("{dir}/{file}");
            let text = match out.iter().find(|(q, _)| *q == p) {
                Some((_, t)) => t.clone(),
                None => read(root, &p)?,
            };
            let title = text.lines().find_map(|l| l.strip_prefix("## ")).ok_or_else(|| format!("{p}: no `## {n}. <title>` heading for the index"))?;
            t.push_str(&format!("- [{title}]({link}/{file}): section {n}\n"));
        }
        if let Some(tail) = &v.tail {
            t.push_str(&read(root, tail)?);
        }
        out.push((v.path.clone(), t));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::super::tests::{dec, root_with};
    use super::super::{gen_cmd, load_records, load_views};
    use super::*;

    #[test]
    fn derived_markers() {
        let root = root_with(&[
            ("records/decisions/D-4/D-4.11.toml", Some(dec("D-4.11", "supersedes = [\"D-4.2\"]\namends = [\"D-3.9\"]\nstory = \"S-1\"\ndate = \"2026-11-08\"\n"))),
            ("records/decisions/D-4/D-4.10.toml", Some(dec("D-4.10", "").replace("active", "withdrawn"))),
        ]);
        let views = load_views(&root).expect("views");
        let rows = rows(&views[0], &load_records(&root).0);
        assert_eq!(rows, ["| D-3.9 | body of D-3.9 (Amended by D-4.11.) |", "| D-4.2 | **Superseded by D-4.11.** body of D-4.2 |", "| D-4.10 | **Withdrawn.** body of D-4.10 |", "| D-4.11 | body of D-4.11 |"]);
    }

    #[test]
    fn index_lines_come_from_headings() {
        let root = root_with(&[
            ("records/views.toml", Some(format!("{}\n[[view]]\nname = \"idx\"\npath = \"docs/reg.md\"\nkind = \"index\"\ndir = \"docs/sat\"\nhead = \"records/views/idx.head.md\"\n", super::super::tests::VIEWS_TOML))),
            ("records/views/idx.head.md", Some("# Register\n".into())),
            ("docs/sat/00-intro.md", Some("# intro\n".into())),
            ("docs/sat/10-ten.md", Some("## 10. Ten\n".into())),
            ("docs/sat/2-two.md", Some("## 2. Two (x)\n".into())),
        ]);
        gen_cmd(&root, false).expect("gen");
        let t = std::fs::read_to_string(root.join("docs/reg.md")).expect("index");
        assert_eq!(t, format!("{GENERATED}\n# Register\n- [2. Two (x)](sat/2-two.md): section 2\n- [10. Ten](sat/10-ten.md): section 10\n"));
    }
}
