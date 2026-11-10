//! Validation of the records (the static check `records`, always fatal). Every problem names the file and line.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use super::schema::{self, Kind};
use super::toml::{self, Value};
use super::{DIR, Record, View, ViewKind};

/// Keys that are not view columns.
const META: [&str; 12] = ["id", "series", "section", "status", "status_text", "supersedes", "amends", "story", "date", "tests", "decisions", "body"];

fn views_sane(root: &Path, views: &[View], bad: &mut Vec<String>) {
    let mut paths = BTreeSet::new();
    let mut names = BTreeSet::new();
    let mut sections = BTreeSet::new();
    let files: BTreeSet<&str> = views.iter().flat_map(|v| std::iter::once(v.head.as_str()).chain(v.tail.as_deref())).collect();
    for v in views {
        let at = format!("{}: view {}", super::VIEWS, v.name);
        if !names.insert(v.name.as_str()) {
            bad.push(format!("{at}: name used twice"));
        }
        if !paths.insert(v.path.as_str()) || files.contains(v.path.as_str()) || v.path.starts_with(&format!("{DIR}/")) {
            bad.push(format!("{at}: path {} is another view, a head/tail file or under records/", v.path));
        }
        for f in std::iter::once(&v.head).chain(&v.tail) {
            if !root.join(f).is_file() {
                bad.push(format!("{at}: {f} does not exist"));
            }
        }
        if let ViewKind::Table(k) = v.kind {
            if k != Kind::Compat && v.section.is_none() {
                bad.push(format!("{at}: no section"));
            }
            if !sections.insert((k, v.section.clone())) {
                bad.push(format!("{at}: a second view of the same kind and section"));
            }
            if v.columns.first().map(String::as_str) != Some(if k == Kind::Saturation { "row" } else { "id" }) || v.columns.iter().any(|c| !k.keys().contains(&c.as_str())) {
                bad.push(format!("{at}: columns must start with {} and be keys of {}", if k == Kind::Saturation { "row" } else { "id" }, k.name()));
            }
        }
    }
}

/// Keys known, in order, of the right type; the required ones present; canonical text.
fn keys(root: &Path, r: &Record, bad: &mut Vec<String>) {
    let order = r.kind.keys();
    let mut pos = 0;
    for e in &r.entries {
        match order.iter().position(|k| *k == e.key) {
            None => bad.push(format!("{}:{}: key `{}` is not a {} key ({})", r.path, e.line, e.key, r.kind.name(), order.join(" "))),
            Some(p) if p < pos => bad.push(format!("{}:{}: key `{}` out of order or twice (canonical order: {}; `cargo xtask records fmt`)", r.path, e.line, e.key, order.join(" "))),
            Some(p) => pos = p + 1,
        }
        if !schema::value_type_ok(&e.key, &e.value) {
            bad.push(format!("{}:{}: `{}` has the wrong type", r.path, e.line, e.key));
        }
    }
    for k in r.kind.required() {
        if r.get(k).is_none() {
            bad.push(format!("{}: required key `{k}` missing", r.path));
        }
    }
    let pairs: Vec<(String, Value)> = r.entries.iter().map(|e| (e.key.clone(), e.value.clone())).collect();
    if let Ok(text) = std::fs::read_to_string(root.join(&r.path))
        && toml::write(&pairs) != text
    {
        bad.push(format!("{}: not in canonical form (`cargo xtask records fmt`)", r.path));
    }
}

/// Validate the records against each other and the views. `records` are sorted by path.
pub fn validate(root: &Path, views: &[View], records: &[Record]) -> Vec<String> {
    let mut bad = Vec::new();
    views_sane(root, views, &mut bad);
    let mut by_id: BTreeMap<String, &Record> = BTreeMap::new();
    let mut lower: BTreeMap<String, &str> = BTreeMap::new();
    for r in records {
        keys(root, r, &mut bad);
        let id = r.id();
        let stem = r.path.rsplit('/').next().and_then(|f| f.strip_suffix(".toml")).unwrap_or("");
        if r.pending {
            if id != r.kind.placeholder() {
                bad.push(format!("{}:1: a pending record's id is {}", r.path, r.kind.placeholder()));
            }
            if r.kind == Kind::Decision && r.str("series").and_then(|s| schema::parse_id(Kind::Decision, &format!("{s}.1")).or_else(|| schema::parse_id(Kind::Decision, &format!("{s}1")))).is_none() {
                bad.push(format!("{}: a pending decision names its `series` (D-10, D-X, ...)", r.path));
            }
            continue;
        }
        if r.get("series").is_some() {
            bad.push(format!("{}:{}: `series` is only for pending records (the id says it)", r.path, r.line("series")));
        }
        let Some(parts) = schema::parse_id(r.kind, id) else {
            bad.push(format!("{}:1: {id:?} is not a {} id{}", r.path, r.kind.name(), if id.ends_with("-NEW") { " (placeholders live under records/pending/)" } else { "" }));
            continue;
        };
        if stem != id {
            bad.push(format!("{}:1: file name {stem}.toml differs from its id {id}", r.path));
        }
        let want = match r.kind {
            Kind::Decision => format!("{DIR}/decisions/{}/{stem}.toml", parts.series),
            k => format!("{DIR}/{}/{stem}.toml", k.dir()),
        };
        if r.path != want {
            bad.push(format!("{}: a {} record with id {id} lives at {want}", r.path, r.kind.name()));
        }
        match lower.insert(id.to_ascii_lowercase(), &r.path) {
            Some(other) => bad.push(format!("{}:1: duplicate id {id} (also {other}; ids are unique, ignoring case)", r.path)),
            None => {
                by_id.insert(id.to_string(), r);
            }
        }
    }
    let mut rows: BTreeMap<&str, &str> = BTreeMap::new();
    for r in records {
        fields(root, r, views, &by_id, &mut bad);
        if r.kind == Kind::Saturation
            && !r.pending
            && let Some(row) = r.str("row")
            && let Some(other) = rows.insert(row, &r.path)
        {
            bad.push(format!("{}:{}: row {row} is also the row of {other}", r.path, r.line("row")));
        }
    }
    links(records, &by_id, &mut bad);
    bad
}

fn fields(root: &Path, r: &Record, views: &[View], by_id: &BTreeMap<String, &Record>, bad: &mut Vec<String>) {
    let at = |k: &str| format!("{}:{}", r.path, r.line(k));
    if let Some(s) = r.str("status")
        && !r.kind.statuses().contains(&s)
    {
        bad.push(format!("{}: status {s:?} is not one of {}", at("status"), r.kind.statuses().join(", ")));
    }
    if let Some(d) = r.str("date")
        && !schema::date_ok(d)
    {
        bad.push(format!("{}: date {d:?} is not YYYY-MM-DD", at("date")));
    }
    if let Some(s) = r.str("story")
        && !schema::story_ok(s)
    {
        bad.push(format!("{}: story {s:?} is not a story id ([A-Z0-9-]+)", at("story")));
    }
    if r.get("story").is_some() != r.get("date").is_some() {
        bad.push(format!("{}: `story` and `date` go together", r.path));
    }
    if let Some(Value::Body(b)) = r.get("body")
        && r.kind != Kind::Decision
        && b.contains("'''")
    {
        bad.push(format!("{}: body contains '''", at("body")));
    }
    // The view: section, columns, cells.
    let section = r.str("section").map(str::to_string).or_else(|| r.str("series").filter(|_| r.kind == Kind::Decision).map(schema::default_section));
    let view = views.iter().find(|v| v.kind == ViewKind::Table(r.kind) && (r.kind == Kind::Compat || v.section == section));
    let Some(view) = view else {
        bad.push(format!("{}: section {:?} names no {} view in {}", at("section"), section.unwrap_or_default(), r.kind.name(), super::VIEWS));
        return;
    };
    if !r.pending
        && let Some(p) = schema::parse_id(r.kind, r.id())
        && !view.series.contains(&p.series)
    {
        bad.push(format!("{}: series {} is not in view {}'s series {:?}", at("section"), p.series, view.name, view.series));
    }
    for e in &r.entries {
        if !META.contains(&e.key.as_str()) && !view.columns.contains(&e.key) {
            bad.push(format!("{}:{}: `{}` is not a column of view {} ({})", r.path, e.line, e.key, view.name, view.columns.join(" ")));
        }
    }
    for c in &view.columns {
        let text = match c.as_str() {
            "status" => r.str("status_text").or(r.str("status")),
            c => r.str(c),
        };
        let Some(text) = text else {
            bad.push(format!("{}: column `{c}` of view {} missing", r.path, view.name));
            continue;
        };
        let may_be_empty = r.kind == Kind::Saturation && c != "row";
        if text.is_empty() && !may_be_empty {
            bad.push(format!("{}: `{c}` is empty", at(c)));
        }
        if let Some(p) = schema::cell_problem(text) {
            bad.push(format!("{}: `{c}` (a table cell) has {p}", at(if c == "status" && r.get("status_text").is_some() { "status_text" } else { c })));
        }
    }
    for t in r.list("tests") {
        let (file, name) = t.split_once("::").map_or((t.as_str(), None), |(f, n)| (f, Some(n)));
        match std::fs::read_to_string(root.join(file)) {
            Err(_) => bad.push(format!("{}: test file {file} does not exist", at("tests"))),
            Ok(src) => {
                if let Some(n) = name
                    && !defines(&src, n)
                {
                    bad.push(format!("{}: {file} has no `fn {n}` or `clause!({n}`", at("tests")));
                }
            }
        }
    }
    if r.kind == Kind::Compat {
        if r.get("date").is_some() && r.list("decisions").is_empty() {
            bad.push(format!("{}: a new compat record names the decision record(s) it rests on in `decisions`", r.path));
        }
        for d in r.list("decisions") {
            if by_id.get(d.as_str()).is_none_or(|t| t.kind != Kind::Decision) {
                bad.push(format!("{}: decision {d:?} has no record", at("decisions")));
            }
        }
    }
}

fn defines(src: &str, name: &str) -> bool {
    let ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
    ["fn ", "clause!("].iter().any(|pre| {
        let pat = format!("{pre}{name}");
        src.match_indices(&pat).any(|(i, _)| !src[i + pat.len()..].starts_with(ident) && (i == 0 || !src[..i].ends_with(ident)))
    })
}

/// supersedes/amends: targets exist, same kind, not itself, never a withdrawn target, at most one active superseder,
/// no cycle.
fn links(records: &[Record], by_id: &BTreeMap<String, &Record>, bad: &mut Vec<String>) {
    let mut superseders: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    let mut edges: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for r in records.iter().filter(|r| !r.pending) {
        for key in ["supersedes", "amends"] {
            for t in r.list(key) {
                let at = format!("{}:{}", r.path, r.line(key));
                match by_id.get(t.as_str()) {
                    None => bad.push(format!("{at}: {key} {t}, which has no record")),
                    Some(target) if target.kind != r.kind => bad.push(format!("{at}: {key} {t}, a {} record", target.kind.name())),
                    Some(_) if t == r.id() => bad.push(format!("{at}: {key} itself")),
                    Some(target) if target.str("status") == Some("withdrawn") => bad.push(format!("{at}: {key} {t}, which is withdrawn")),
                    Some(_) => {
                        edges.entry(r.id()).or_default().push(t);
                        if key == "supersedes" && r.str("status") != Some("withdrawn") {
                            superseders.entry(t).or_default().push(r.id());
                        }
                    }
                }
            }
        }
    }
    for (t, by) in &superseders {
        if by.len() > 1 {
            bad.push(format!("{t} has more than one active superseder: {}", by.join(", ")));
        }
    }
    // Cycles: depth-first over the links, in id order.
    let mut state: BTreeMap<&str, u8> = BTreeMap::new();
    fn visit<'a>(n: &'a str, edges: &BTreeMap<&'a str, Vec<&'a str>>, state: &mut BTreeMap<&'a str, u8>, path: &mut Vec<&'a str>, bad: &mut Vec<String>) {
        match state.get(n) {
            Some(2) => return,
            Some(1) => {
                let from = path.iter().position(|p| *p == n).unwrap_or(0);
                bad.push(format!("supersedes/amends cycle: {} -> {n}", path[from..].join(" -> ")));
                return;
            }
            _ => {}
        }
        state.insert(n, 1);
        path.push(n);
        for m in edges.get(n).into_iter().flatten() {
            visit(m, edges, state, path, bad);
        }
        path.pop();
        state.insert(n, 2);
    }
    let starts: Vec<&str> = edges.keys().copied().collect();
    for n in starts {
        visit(n, &edges, &mut state, &mut Vec::new(), bad);
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{dec, root_with};
    use super::super::{check, load_records, load_views};
    use super::*;

    fn problems(changes: &[(&'static str, Option<String>)]) -> Vec<String> {
        let root = root_with(changes);
        let views = load_views(&root).expect("views");
        let (records, mut bad) = load_records(&root);
        bad.extend(validate(&root, &views, &records));
        bad
    }

    fn one(changes: &[(&'static str, Option<String>)], want: &str) {
        let p = problems(changes);
        assert!(p.iter().any(|x| x.contains(want)), "want {want:?} in {p:?}");
    }

    #[test]
    fn the_base_tree_is_valid() {
        assert_eq!(problems(&[]), Vec::<String>::new());
        check(&root_with(&[])).expect("valid");
    }

    #[test]
    fn identity_problems() {
        one(&[("records/decisions/D-4/D-4.2.toml", Some(dec("D-4.3", "")))], "differs from its id");
        one(&[("records/decisions/D-3/D-4.2.toml", Some(dec("D-4.2", "")))], "duplicate id D-4.2");
        one(&[("records/decisions/D-4/d-4.2.toml", Some(dec("d-4.2", "")))], "not a decision id");
        one(&[("records/decisions/D-3/D-4.11.toml", Some(dec("D-4.11", "")))], "lives at records/decisions/D-4/D-4.11.toml");
        one(&[("records/decisions/D-4/D-NEW.toml", Some(dec("D-NEW", "")))], "placeholders live under records/pending/");
        one(&[("records/notes.txt", Some("x".into()))], "not a record path");
    }

    #[test]
    fn content_problems() {
        one(&[("records/decisions/D-4/D-4.2.toml", Some(dec("D-4.2", "").replace("body of", "a | b")))], "bare `|`");
        one(&[("records/decisions/D-4/D-4.2.toml", Some(dec("D-4.2", "").replace("status = \"active\"\n", "")))], "required key `status`");
        one(&[("records/decisions/D-4/D-4.2.toml", Some(dec("D-4.2", "").replace("\"active\"", "\"superseded\"")))], "status \"superseded\"");
        one(&[("records/decisions/D-4/D-4.2.toml", Some(dec("D-4.2", "story = \"S\"\ndate = \"2026-1-1\"\n")))], "not YYYY-MM-DD");
        one(&[("records/decisions/D-4/D-4.2.toml", Some(dec("D-4.2", "").replace("s04", "s99")))], "names no decision view");
        one(&[("records/decisions/D-4/D-4.2.toml", Some(dec("D-4.2", "spec = \"x\"\n").replace("status = \"active\"\nspec = \"x\"\n", "spec = \"x\"\nstatus = \"active\"\n")))], "not a column of view s04");
        one(&[("records/decisions/D-4/D-4.2.toml", Some(dec("D-4.2", "tests = [\"xtask/none.rs::t\"]\n")))], "does not exist");
        one(&[("records/compat/COMPAT-2.toml", Some("id = \"COMPAT-2\"\nstatus = \"registered\"\nstory = \"S-1\"\ndate = \"2026-11-08\"\nbody = '''\n'''\n".into()))], "names the decision record");
        one(&[("records/compat/COMPAT-2.toml", Some("id = \"COMPAT-2\"\nstatus = \"registered\"\ndecisions = [\"D-9.9\"]\nbody = '''\n'''\n".into()))], "decision \"D-9.9\" has no record");
    }

    #[test]
    fn link_problems() {
        one(&[("records/decisions/D-4/D-4.2.toml", Some(dec("D-4.2", "supersedes = [\"D-4.99\"]\n")))], "which has no record");
        one(&[("records/decisions/D-4/D-4.2.toml", Some(dec("D-4.2", "supersedes = [\"D-4.2\"]\n")))], "supersedes itself");
        one(&[("records/decisions/D-4/D-4.2.toml", Some(dec("D-4.2", "supersedes = [\"D-4.10\"]\n"))), ("records/decisions/D-4/D-4.10.toml", Some(dec("D-4.10", "supersedes = [\"D-4.2\"]\n")))], "cycle");
        one(&[("records/decisions/D-4/D-4.2.toml", Some(dec("D-4.2", "supersedes = [\"D-3.9\"]\n"))), ("records/decisions/D-4/D-4.10.toml", Some(dec("D-4.10", "supersedes = [\"D-3.9\"]\n")))], "more than one active superseder");
        one(&[("records/decisions/D-4/D-4.2.toml", Some(dec("D-4.2", "amends = [\"D-3.9\"]\n"))), ("records/decisions/D-3/D-3.9.toml", Some(dec("D-3.9", "").replace("active", "withdrawn")))], "which is withdrawn");
        one(&[("records/decisions/D-4/D-4.2.toml", Some(dec("D-4.2", "supersedes = [\"COMPAT-1\"]\n")))], "a compat record");
    }

    #[test]
    fn test_names_are_found() {
        assert!(defines("#[test]\nfn abc() {}", "abc") && !defines("fn abcd() {}", "abc") && defines("clause!(s1_1, \"1\"", "s1_1"));
    }
}
