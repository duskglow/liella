//! The schedule check: which phase, track or stage owns each pending test, and whether that
//! owner can still be holding it (`spec/schedule.tsv`; implementation-plan.md §4.3).
//!
//! Every pending stub (`clause!`, `symbol!`, `family!`, `primitive!`, `layout!`) is matched
//! against the table's `assign` rows, the first match deciding. Pending work is *flagged* when
//! its owner is a phase that is done, a track that has not started or whose join phase is done,
//! or nobody (`UNASSIGNED`, or no matching row). A `DEFERRAL` row names its reference and
//! reason, and is listed but not flagged. The table must also cover every status group and
//! every spec section, and every selector must name something in the manifests.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

/// One pending test.
pub(crate) struct Item {
    /// The status group: the test file's name up to its first `_`.
    group: String,
    kind: Kind,
    /// The test's identifier (`s24_5_c_1`, `sym_uname`, ...).
    id: String,
    /// The symbol, family, primitive or layout name; empty for a clause.
    name: String,
    /// The spec section the stub names (`24.5` for a clause, `24` for a symbol).
    section: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Clause,
    Symbol,
    Family,
    Primitive,
    Layout,
}

impl Item {
    fn label(&self) -> String {
        match self.kind {
            Kind::Clause => self.id.clone(),
            Kind::Symbol => format!("sym:{}", self.name),
            Kind::Family => format!("fam:{}", self.name),
            Kind::Primitive => format!("prim:{}", self.name),
            Kind::Layout => format!("layout:{}", self.name),
        }
    }
}

/// The pending stub on a test-file line, if there is one.
fn parse_stub(group: &str, line: &str) -> Option<Item> {
    let (kind, rest) = [("clause!(", Kind::Clause), ("symbol!(", Kind::Symbol), ("family!(", Kind::Family), ("primitive!(", Kind::Primitive), ("layout!(", Kind::Layout)]
        .into_iter()
        .find_map(|(p, k)| line.strip_prefix(p).map(|r| (k, r)))?;
    let (id, rest) = rest.split_once(',')?;
    // The plain string literals that follow the identifier, up to the first raw one.
    let mut lits = Vec::new();
    let mut s = rest.trim_start();
    while let Some(r) = s.strip_prefix('"') {
        let (lit, after) = r.split_once('"')?;
        lits.push(lit.to_string());
        s = after.trim_start().trim_start_matches(',').trim_start();
    }
    let lit = |i: usize| lits.get(i).cloned().unwrap_or_default();
    let (name, section) = match kind {
        Kind::Clause => (String::new(), lit(0)),
        Kind::Symbol => (lit(0), lit(1)),
        Kind::Family => (lit(0), "10".to_string()),
        Kind::Primitive => (lit(0), "A".to_string()),
        Kind::Layout => (lit(0), "C".to_string()),
    };
    Some(Item { group: group.to_string(), kind, id: id.trim().to_string(), name, section })
}

/// Every pending stub under `crates/*/tests`, and every status group (pending or not).
fn pending(root: &Path) -> Result<(Vec<Item>, BTreeSet<String>), String> {
    let mut items = Vec::new();
    let mut groups = BTreeSet::new();
    let crates = fs::read_dir(root.join("crates")).map_err(|e| e.to_string())?;
    let mut files: Vec<_> = crates.flatten().filter_map(|c| fs::read_dir(c.path().join("tests")).ok()).flat_map(|rd| rd.flatten().map(|f| f.path())).collect();
    files.sort();
    for p in files {
        if p.extension().is_none_or(|x| x != "rs") {
            continue;
        }
        let name = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let group = name.split('_').next().unwrap_or("").to_string();
        let text = fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        items.extend(text.lines().filter_map(|l| parse_stub(&group, l.trim_start())));
        groups.insert(group);
    }
    Ok((items, groups))
}

/// `*` matches any run of characters; everything else matches itself.
fn glob(pat: &str, s: &str) -> bool {
    match pat.split_once('*') {
        None => pat == s,
        Some((head, tail)) => {
            let Some(rest) = s.strip_prefix(head) else { return false };
            (0..=rest.len()).filter(|&i| rest.is_char_boundary(i)).any(|i| rest.get(i..).is_some_and(|r| glob(tail, r)))
        }
    }
}

/// A status group of a spec section's generated tests (`s00` to `s29`).
fn is_section_group(g: &str) -> bool {
    g.strip_prefix('s').is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// A section selector covers its own section and every subsection below it.
fn covers_section(sel: &str, section: &str) -> bool {
    section == sel || section.strip_prefix(sel).is_some_and(|r| r.starts_with('.'))
}

enum Selector {
    Sections(Vec<String>),
    Clauses(Vec<String>),
    Symbols(Vec<String>),
    Families(Vec<String>),
    Primitives(Vec<String>),
    Layouts(Vec<String>),
    CompatOnly,
    Groups(Vec<String>),
}

fn list(s: &str) -> Vec<String> {
    s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect()
}

impl Selector {
    fn parse(key: &str) -> Option<Selector> {
        if key == "compat-only" {
            return Some(Selector::CompatOnly);
        }
        if key.starts_with('§') {
            return Some(Selector::Sections(list(key).iter().map(|x| x.trim_start_matches('§').to_string()).collect()));
        }
        let (kind, rest) = key.split_once(':')?;
        let l = list(rest);
        Some(match kind {
            "clause" => Selector::Clauses(l),
            "sym" => Selector::Symbols(l),
            "fam" => Selector::Families(l),
            "prim" => Selector::Primitives(l),
            "layout" => Selector::Layouts(l),
            "group" => Selector::Groups(l),
            _ => return None,
        })
    }

    fn matches(&self, it: &Item, compat_only: &BTreeSet<String>) -> bool {
        let any = |pats: &[String], s: &str| pats.iter().any(|p| glob(p, s));
        match self {
            Selector::Sections(secs) => is_section_group(&it.group) && secs.iter().any(|s| covers_section(s, &it.section)),
            Selector::Clauses(p) => it.kind == Kind::Clause && any(p, &it.id),
            Selector::Symbols(p) => it.kind == Kind::Symbol && any(p, &it.name),
            Selector::Families(p) => it.kind == Kind::Family && any(p, &it.name),
            Selector::Primitives(p) => it.kind == Kind::Primitive && any(p, &it.name),
            Selector::Layouts(p) => it.kind == Kind::Layout && any(p, &it.name),
            Selector::CompatOnly => it.kind == Kind::Symbol && compat_only.contains(&it.name),
            Selector::Groups(g) => g.contains(&it.group),
        }
    }
}

struct Owner {
    order: String,
    state: String,
    from: String,
    join: String,
    started: String,
    reference: String,
    note: String,
}

struct Assign {
    key: String,
    selector: Selector,
    owner: String,
    reference: String,
    note: String,
}

pub(crate) struct Schedule {
    stage: String,
    owners: BTreeMap<String, Owner>,
    rows: Vec<Assign>,
    /// Problems with the table itself.
    problems: Vec<String>,
}

const DEFERRAL: &str = "DEFERRAL";
const UNASSIGNED: &str = "UNASSIGNED";

fn read_schedule(root: &Path) -> Result<Schedule, String> {
    let text = fs::read_to_string(root.join("spec/schedule.tsv")).map_err(|e| format!("spec/schedule.tsv: {e}"))?;
    let mut s = Schedule { stage: String::new(), owners: BTreeMap::new(), rows: Vec::new(), problems: Vec::new() };
    for (n, line) in text.lines().enumerate() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split('\t').collect();
        let col = |i: usize| f.get(i).map(|x| x.trim().to_string()).unwrap_or_default();
        let at = n.saturating_add(1);
        match col(0).as_str() {
            "kind" => {}
            "fact" if col(1) == "stage" => s.stage = col(2),
            "owner" => {
                let mut o = Owner { order: String::new(), state: String::new(), from: String::new(), join: String::new(), started: String::new(), reference: col(3), note: col(4) };
                for kv in col(2).split(';') {
                    let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
                    let v = v.to_string();
                    match k {
                        "order" => o.order = v,
                        "state" => o.state = v,
                        "from" => o.from = v,
                        "join" => o.join = v,
                        "started" => o.started = v,
                        _ => s.problems.push(format!("line {at}: unknown owner setting `{kv}`")),
                    }
                }
                if !matches!(o.state.as_str(), "done" | "current" | "future" | "track") {
                    s.problems.push(format!("line {at}: owner {} has state `{}`", col(1), o.state));
                }
                s.owners.insert(col(1), o);
            }
            "assign" => match Selector::parse(&col(1)) {
                Some(selector) => s.rows.push(Assign { key: col(1), selector, owner: col(2), reference: col(3), note: col(4) }),
                None => s.problems.push(format!("line {at}: cannot read selector `{}`", col(1))),
            },
            other => s.problems.push(format!("line {at}: unknown kind `{other}`")),
        }
    }
    for r in &s.rows {
        let known = r.owner == DEFERRAL || r.owner == UNASSIGNED || s.owners.contains_key(&r.owner);
        if !known {
            s.problems.push(format!("`{}` names unknown owner `{}`", r.key, r.owner));
        }
        if r.owner == DEFERRAL && (r.reference.is_empty() || r.note.is_empty()) {
            s.problems.push(format!("`{}`: a DEFERRAL needs a reference and a reason", r.key));
        }
    }
    for (id, o) in &s.owners {
        for dep in [&o.from, &o.join] {
            if !dep.is_empty() && !s.owners.contains_key(dep) {
                s.problems.push(format!("owner {id} refers to unknown owner {dep}"));
            }
        }
    }
    if !s.owners.contains_key(&s.stage) {
        s.problems.push(format!("the current stage `{}` is not an owner", s.stage));
    }
    Ok(s)
}

/// Names with no default version in Appendix C.3 (`spec/abi/versions.tsv`).
fn compat_only(root: &Path) -> Result<BTreeSet<String>, String> {
    let text = fs::read_to_string(root.join("spec/abi/versions.tsv")).map_err(|e| format!("spec/abi/versions.tsv: {e}"))?;
    let mut all = BTreeSet::new();
    let mut default = BTreeSet::new();
    for l in text.lines().skip(1) {
        let f: Vec<&str> = l.split('\t').collect();
        if let (Some(name), Some(binding)) = (f.get(1), f.get(3)) {
            all.insert((*name).to_string());
            if *binding == "default" {
                default.insert((*name).to_string());
            }
        }
    }
    Ok(all.difference(&default).cloned().collect())
}

/// The first column of a manifest, and its second (the section) where present.
fn manifest(root: &Path, file: &str) -> Result<Vec<(String, String)>, String> {
    let text = fs::read_to_string(root.join(file)).map_err(|e| format!("{file}: {e}"))?;
    Ok(text
        .lines()
        .skip(1)
        .filter_map(|l| {
            let mut f = l.split('\t');
            Some((f.next()?.to_string(), f.next().unwrap_or("").to_string()))
        })
        .collect())
}

/// Coverage of the table: every selector names something, every status group and every spec
/// section has a row.
fn check_coverage(root: &Path, s: &mut Schedule, groups: &BTreeSet<String>) -> Result<(), String> {
    let clauses = manifest(root, "spec/clauses.tsv")?;
    let symbols = manifest(root, "spec/symbols.tsv")?;
    let prims = manifest(root, "spec/primitives.tsv")?;
    let mut problems = Vec::new();
    let none = |pats: &[String], names: &mut dyn Iterator<Item = &str>| -> Vec<String> {
        let names: Vec<&str> = names.collect();
        pats.iter().filter(|p| !names.iter().any(|n| glob(p, n))).cloned().collect()
    };
    for r in &s.rows {
        let missing = match &r.selector {
            Selector::Clauses(p) => none(p, &mut clauses.iter().map(|c| c.0.as_str())),
            Selector::Symbols(p) | Selector::Families(p) => none(p, &mut symbols.iter().map(|c| c.0.as_str())),
            Selector::Primitives(p) => none(p, &mut prims.iter().map(|c| c.0.as_str())),
            Selector::Groups(g) => g.iter().filter(|g| !groups.contains(*g)).cloned().collect(),
            Selector::Sections(secs) => secs.iter().filter(|x| !clauses.iter().chain(symbols.iter()).any(|c| covers_section(x, &c.1))).cloned().collect(),
            Selector::Layouts(_) | Selector::CompatOnly => Vec::new(),
        };
        for m in missing {
            problems.push(format!("`{}`: `{m}` names nothing in the manifests", r.key));
        }
    }
    let section_rows = |sec: &str| s.rows.iter().any(|r| matches!(&r.selector, Selector::Sections(x) if x.iter().any(|x| covers_section(x, sec))));
    for g in groups {
        let by_group = s.rows.iter().any(|r| matches!(&r.selector, Selector::Groups(x) if x.contains(g)));
        let by_section = is_section_group(g) && g.strip_prefix('s').and_then(|n| n.parse::<u32>().ok()).is_some_and(|n| s.rows.iter().any(|r| matches!(&r.selector, Selector::Sections(x) if x.contains(&n.to_string()))));
        if !by_group && !by_section {
            problems.push(format!("status group `{g}` has no row (group:{g}, or a whole-section row)"));
        }
    }
    let sections: BTreeSet<&str> = clauses.iter().chain(symbols.iter()).map(|c| c.1.as_str()).collect();
    for sec in sections {
        if !section_rows(sec) {
            problems.push(format!("spec section {sec} has no row"));
        }
    }
    s.problems.extend(problems);
    Ok(())
}

/// Why pending work held by `owner` is flagged, if it is.
fn verdict(s: &Schedule, owner: &str) -> Option<String> {
    if owner == DEFERRAL {
        return None;
    }
    if owner == UNASSIGNED {
        return Some("no owner in the plan".into());
    }
    let o = s.owners.get(owner)?;
    match o.state.as_str() {
        "done" => Some(format!("{owner} is done")),
        "track" if o.started.is_empty() => Some(format!("track {owner} has not started")),
        "track" => s.owners.get(&o.join).filter(|j| j.state == "done").map(|_| format!("track {owner} joins at {}, which is done", o.join)),
        _ => None,
    }
}

pub(crate) struct Report {
    text: String,
    pub(crate) flagged: usize,
    pub(crate) problems: usize,
}

/// The schedule report; `full` lists every pending item under its owner.
pub(crate) fn report(root: &Path, full: bool) -> Result<Report, String> {
    let (items, groups) = pending(root)?;
    let mut s = read_schedule(root)?;
    check_coverage(root, &mut s, &groups)?;
    let compat = compat_only(root)?;
    // owner -> row index (None: no row matched) -> items
    let mut by_owner: BTreeMap<String, BTreeMap<Option<usize>, Vec<&Item>>> = BTreeMap::new();
    for it in &items {
        let row = s.rows.iter().position(|r| r.selector.matches(it, &compat));
        let owner = row.and_then(|i| s.rows.get(i)).map_or(UNASSIGNED, |r| r.owner.as_str());
        by_owner.entry(owner.to_string()).or_default().entry(row).or_default().push(it);
    }
    let order = |o: &str| s.owners.get(o).map_or(f64::MAX, |x| x.order.parse::<f64>().unwrap_or(f64::MAX));
    let mut owners: Vec<&String> = by_owner.keys().collect();
    owners.sort_by(|a, b| order(a).total_cmp(&order(b)).then(a.cmp(b)));

    let mut out = String::new();
    let mut flagged = 0usize;
    let _ = writeln!(out, "\nschedule (spec/schedule.tsv): stage {}, {} pending", s.stage, items.len());
    let _ = writeln!(out, "{:<12} {:<28} {:>8}  verdict", "owner", "state", "pending");
    for o in &owners {
        let rows = by_owner.get(*o).map(|m| m.values().map(Vec::len).sum::<usize>()).unwrap_or(0);
        let state = s.owners.get(*o).map_or_else(String::new, |x| {
            if x.state == "track" { format!("track {}..{}{}", x.from, x.join, if x.started.is_empty() { String::new() } else { format!(", from {}", x.started) }) } else { x.state.clone() }
        });
        let v = verdict(&s, o);
        if v.is_some() {
            flagged = flagged.saturating_add(rows);
        }
        let shown = match (&v, o.as_str()) {
            (Some(why), _) => format!("FLAGGED: {why}"),
            (None, DEFERRAL) => "deferred (listed)".into(),
            (None, _) => "ok".into(),
        };
        let _ = writeln!(out, "{o:<12} {state:<28} {rows:>8}  {shown}");
    }
    if full {
        for o in &owners {
            let desc = s.owners.get(*o).map_or("", |x| x.note.as_str());
            let oref = s.owners.get(*o).map_or("", |x| x.reference.as_str());
            let v = verdict(&s, o).map_or_else(|| "not flagged".to_string(), |w| format!("FLAGGED: {w}"));
            let n: usize = by_owner.get(*o).map(|m| m.values().map(Vec::len).sum()).unwrap_or(0);
            let _ = writeln!(out, "\n== {o} ({n} pending; {v}){}{}", if desc.is_empty() { String::new() } else { format!(": {desc}") }, if oref.is_empty() { String::new() } else { format!(" [{oref}]") });
            for (row, its) in by_owner.get(*o).into_iter().flatten() {
                match row.and_then(|i| s.rows.get(i)) {
                    Some(r) => {
                        let _ = writeln!(out, "  {} -> {} ({}) [{}]{}", r.key, r.owner, its.len(), r.reference, if r.note.is_empty() { String::new() } else { format!(" {}", r.note) });
                    }
                    None => {
                        let _ = writeln!(out, "  (no matching row) ({})", its.len());
                    }
                }
                let mut line = String::from("   ");
                for it in its {
                    let l = it.label();
                    if line.len().saturating_add(l.len()) > 100 {
                        let _ = writeln!(out, "{line}");
                        line = String::from("   ");
                    }
                    line.push(' ');
                    line.push_str(&l);
                }
                let _ = writeln!(out, "{line}");
            }
        }
    }
    for p in &s.problems {
        let _ = writeln!(out, "schedule table: {p}");
    }
    let _ = writeln!(
        out,
        "schedule: {flagged} pending tests flagged, {} table problems{}",
        s.problems.len(),
        if full { "" } else { " (a report; `cargo xtask status --schedule` lists them and fails)" }
    );
    Ok(Report { text: out, flagged, problems: s.problems.len() })
}

impl Report {
    pub(crate) fn print(&self) {
        print!("{}", self.text);
    }
}
