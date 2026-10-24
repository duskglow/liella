//! Spec parsing and stub generation. The spec is the only input: clause stubs come from the
//! Contract/Inputs/Guarantees/Failure fields, symbol stubs from Appendix B, kernel primitive
//! stubs from Appendix A.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

/// File slugs per top-level spec section (shared with crates/posix/src/sNN_*.rs).
pub const SLUGS: [&str; 30] = [
    "conventions", "startup", "diagnostics", "memory", "strings", "ctype", "multibyte", "locale",
    "numeric", "random", "math", "lowio", "stdio", "filesystem", "search", "sockets",
    "nameservices", "users", "processes", "terminals", "signals", "contexts", "threads", "time",
    "system", "ipc", "syslog", "dynlink", "profiling", "legacy",
];

/// Appendix B entries the generator relocates. Each is a filing error in the spec's generated
/// index, corrected to the section whose text specifies the symbol (see spec/ERRATA.md).
const ERRATA: &[(&str, u32, &str)] = &[
    ("strtok", 4, "specified in §4.11; Appendix B filed it under §8 by name pattern"),
    ("strtok_r", 4, "§4.11"),
    ("__strtok_r", 4, "§4.11"),
    ("__strtok_r_1c", 4, "§4.11"),
    ("wcstok", 4, "§4.11"),
    ("wcstombs", 6, "§6.3"),
    ("__wcstombs_chk", 6, "§6.3"),
    ("stat", 13, "§13.6; Appendix B filed it under §14"),
    ("stat64", 13, "§13.6"),
    ("utimes", 13, "§13.7; Appendix B filed it under §23"),
    ("futimes", 13, "§13.7"),
    ("futimesat", 13, "§13.7"),
    ("lutimes", 13, "§13.7"),
    ("__sysconf", 24, "§24.4; Appendix B filed it under §22"),
    ("sched_yield", 24, "§24.3"),
    ("__sched_yield", 24, "§24.3"),
    ("__getpagesize", 24, "§11.10/§24.4; Appendix B filed it under §25"),
];

#[derive(Clone)]
pub struct Clause {
    pub id: String,
    pub section: String,
    pub top: u32,
    pub field: &'static str,
    pub text: String,
}

/// The marker of a commentary item (spec §0.3): design commentary, not testable behaviour.
pub const COMMENTARY: &str = "\u{2691} COMMENTARY:";

impl Clause {
    /// Whether the item is marked as commentary.
    pub fn is_commentary(&self) -> bool {
        self.text.contains(COMMENTARY)
    }
}

/// The problems of a commentary item's mark: what follows the marker must name the clause that
/// covers the behaviour the item implies (`covered by` and an existing clause ID) or say
/// `nothing testable`.
fn commentary_problems(c: &Clause, ids: &std::collections::BTreeSet<&str>) -> Vec<String> {
    let note = c.text.split(COMMENTARY).nth(1).unwrap_or("").trim();
    if note.starts_with("nothing testable") {
        return Vec::new();
    }
    let Some(after) = note.split("covered by").nth(1) else {
        return vec![format!("{}: the COMMENTARY mark says neither `covered by <clause>` nor `nothing testable`", c.id)];
    };
    let cited: Vec<&str> = after.split('`').skip(1).step_by(2).collect();
    if cited.is_empty() {
        return vec![format!("{}: the COMMENTARY mark's `covered by` names no clause ID", c.id)];
    }
    cited.iter().filter(|id| !ids.contains(**id)).map(|id| format!("{}: the COMMENTARY mark cites `{id}`, which is no clause", c.id)).collect()
}

fn field_code(marker: &str) -> Option<(&'static str, &'static str)> {
    let m = marker.trim_end_matches('.');
    let pairs: [(&str, (&str, &str)); 15] = [
        ("Contract", ("c", "Contract")),
        ("Contract (current)", ("c", "Contract")),
        ("Inputs", ("i", "Inputs")),
        ("Guarantees", ("g", "Guarantees")),
        ("Guarantees (glibc)", ("g", "Guarantees")),
        ("Guarantees (Linux)", ("g", "Guarantees")),
        ("Guarantees (fork in a threaded program)", ("g", "Guarantees")),
        ("Failure", ("f", "Failure")),
        ("Failure / edge cases", ("f", "Failure")),
        ("Edge cases", ("f", "Failure")),
        ("Threads", ("t", "Threads")),
        ("Cancellation", ("x", "Cancellation")),
        ("Cancellation point", ("x", "Cancellation")),
        ("Standard streams", ("c", "Contract")),
        ("Default stack size", ("g", "Guarantees")),
    ];
    pairs.iter().find(|(k, _)| *k == m).map(|(_, v)| *v).or(match m {
        "Constants exported as data" | "Type variants" | "Finite-only entry points" => Some(("c", "Contract")),
        "Error reporting" => Some(("f", "Failure")),
        "Accuracy" | "Special values" => Some(("g", "Guarantees")),
        "Section-wide notes" => Some(("n", "Notes")),
        _ => None,
    })
}

fn clean(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(crate) fn ident_part(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).collect()
}

/// Parse all testable clauses of the spec body (sections 0-29).
pub fn clauses(spec: &str) -> Vec<Clause> {
    let mut out: Vec<Clause> = Vec::new();
    let mut top: u32 = 0;
    let mut section = String::new();
    let mut in_body = false;
    // Current field state.
    let mut field: Option<(&'static str, &'static str)> = None;
    let mut counters: BTreeMap<String, u32> = BTreeMap::new();
    let mut cur: Option<Clause> = None;
    let mut top_idx: u32 = 0;
    let mut sub_idx: u32 = 0;
    let mut para_open = false;
    let mut field_base = String::new();
    // A flag annotation (a line that starts with the flag character) continues the clause it
    // follows; one that starts a paragraph of its own annotates the field and is no clause. Either
    // way the field goes on (until 2026-10-04 a flag line ended the field, and the items after it
    // got no clause rows).
    let mut flag_para = false;
    let mut blank = false;

    let flush = |cur: &mut Option<Clause>, out: &mut Vec<Clause>| {
        if let Some(c) = cur.take() {
            let t = clean(&c.text);
            if t.len() > 3 {
                out.push(Clause { text: t, ..c });
            }
        }
    };

    for raw in spec.lines() {
        let line = raw.trim_end();
        if line.starts_with("## Appendix") || line.starts_with("## 30.") {
            flush(&mut cur, &mut out);
            break;
        }
        if let Some(rest) = line.strip_prefix("## ") {
            flush(&mut cur, &mut out);
            field = None;
            if let Some((num, _)) = rest.split_once(". ") {
                if let Ok(n) = num.parse::<u32>() {
                    top = n;
                    section = n.to_string();
                    in_body = true;
                }
            }
            continue;
        }
        if !in_body {
            continue;
        }
        if let Some(rest) = line.strip_prefix("### ") {
            flush(&mut cur, &mut out);
            field = None;
            flag_para = false;
            section = rest.split_whitespace().next().unwrap_or("").to_string();
            continue;
        }
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        let after_blank = core::mem::replace(&mut blank, trimmed.is_empty());
        // Field markers: **Name.** at line start, or **C0.n Title.** conventions.
        if indent == 0 && trimmed.starts_with("**") {
            if let Some(end) = trimmed[2..].find("**") {
                let marker = &trimmed[2..2 + end];
                let rest = trimmed[2 + end + 2..].trim();
                flush(&mut cur, &mut out);
                let conv = marker.starts_with("C0.");
                let fc = if conv { Some(("b", "Convention")) } else { field_code(marker) };
                field = fc;
                flag_para = false;
                if let Some((code, name)) = fc {
                    let key = if conv {
                        ident_part(marker.split_whitespace().next().unwrap_or("c0"))
                    } else {
                        code.to_string()
                    };
                    let base = format!("s{}_{}", ident_part(&section), key);
                    let n = counters.entry(base.clone()).or_insert(0);
                    *n += 1;
                    let base = if *n > 1 { format!("{base}{n}") } else { base };
                    top_idx = 0;
                    sub_idx = 0;
                    para_open = false;
                    if !rest.is_empty() {
                        cur = Some(Clause { id: format!("{base}_0"), section: section.clone(), top, field: name, text: rest.to_string() });
                        para_open = true;
                    }
                    field_base = base;
                }
                continue;
            }
        }
        // Tables are clauses wherever they appear in a section (for example the libm contract
        // tables of §10.2-10.4). The row just before a `|---` separator is a header: drop it.
        if trimmed.starts_with('|') {
            flush(&mut cur, &mut out);
            if trimmed.starts_with("|-") {
                if out.last().is_some_and(|c| c.id.contains("_r")) {
                    if let Some(header) = out.pop() {
                        if let Some((b, _)) = header.id.rsplit_once("_r") {
                            if let Some(n) = counters.get_mut(&format!("{b}#rows")) {
                                *n -= 1;
                            }
                        }
                    }
                }
                continue;
            }
            let (base, name) = match field {
                Some((_, name)) => (field_base.clone(), name),
                None => (format!("s{}_t", ident_part(&section)), "Contract"),
            };
            let n = counters.entry(format!("{base}#rows")).or_insert(0);
            *n += 1;
            let row = clean(&trimmed.replace('|', " | "));
            out.push(Clause { id: format!("{base}_r{n}"), section: section.clone(), top, field: name, text: row });
            continue;
        }
        let Some((_, name)) = field else { continue };
        let base = field_base.clone();
        if trimmed.starts_with("---") {
            flush(&mut cur, &mut out);
            field = None;
            continue;
        }
        if trimmed.is_empty() {
            if para_open {
                flush(&mut cur, &mut out);
                para_open = false;
            }
            flag_para = false;
            continue;
        }
        if trimmed.starts_with('⚑') {
            match cur.as_mut() {
                Some(c) if !after_blank => {
                    c.text.push(' ');
                    c.text.push_str(trimmed);
                }
                _ => {
                    flush(&mut cur, &mut out);
                    para_open = false;
                    flag_para = true;
                }
            }
            continue;
        }
        if flag_para && !trimmed.starts_with("* ") {
            continue;
        }
        flag_para = false;
        if let Some(b) = trimmed.strip_prefix("* ") {
            flush(&mut cur, &mut out);
            para_open = false;
            if indent == 0 {
                top_idx += 1;
                sub_idx = 0;
                cur = Some(Clause { id: format!("{base}_{top_idx}"), section: section.clone(), top, field: name, text: b.to_string() });
            } else {
                sub_idx += 1;
                cur = Some(Clause { id: format!("{base}_{top_idx}_{sub_idx}"), section: section.clone(), top, field: name, text: b.to_string() });
            }
            continue;
        }
        // Continuation or a new paragraph inside the field.
        match cur.as_mut() {
            Some(c) => {
                c.text.push(' ');
                c.text.push_str(trimmed);
            }
            None => {
                top_idx += 1;
                cur = Some(Clause { id: format!("{base}_p{top_idx}"), section: section.clone(), top, field: name, text: trimmed.to_string() });
                para_open = true;
            }
        }
    }
    flush(&mut cur, &mut out);
    // Marker tables of §0 (annotation legend, abstract types) are definitions, not clauses.
    out.retain(|c| !(c.section == "0.3" || c.section == "0.4" && c.id.contains("_t_")));
    out
}

pub struct Symbol {
    pub name: String,
    pub section: u32,
    pub version: String,
    pub undeclared: bool,
    pub family_count: Option<u32>,
    pub note: String,
}

/// Parse Appendix B, applying ERRATA.
pub fn symbols(spec: &str) -> Vec<Symbol> {
    let mut out = Vec::new();
    let mut section: Option<u32> = None;
    let mut in_b = false;
    for line in spec.lines() {
        if line.starts_with("## Appendix B") {
            in_b = true;
            continue;
        }
        if !in_b {
            continue;
        }
        if line.starts_with("## Appendix C") {
            break;
        }
        if let Some(rest) = line.strip_prefix("### B.") {
            section = rest.split_whitespace().next().and_then(|n| n.parse().ok());
            continue;
        }
        let Some(sec) = section else { continue };
        let families = line.starts_with("**Families");
        let body = match line.find(":** ") {
            Some(i) if line.starts_with("**") => &line[i + 4..],
            _ => line,
        };
        if !body.starts_with('`') {
            continue;
        }
        for item in body.split(", ") {
            let item = item.trim();
            let Some(rest) = item.strip_prefix('`') else { continue };
            let Some(end) = rest.find('`') else { continue };
            let name = rest[..end].to_string();
            let marks = &rest[end + 1..];
            if families {
                let count = marks.trim_start_matches('×').parse().ok();
                out.push(Symbol { name, section: sec, version: String::new(), undeclared: false, family_count: count, note: String::new() });
                continue;
            }
            let undeclared = marks.contains('†');
            let version = marks.split_once('@').map(|(_, v)| v.to_string()).unwrap_or_else(|| "2.2.5".into());
            let mut s = Symbol { name, section: sec, version, undeclared, family_count: None, note: String::new() };
            if let Some((_, to, why)) = ERRATA.iter().find(|(n, _, _)| *n == s.name) {
                s.note = format!("erratum: Appendix B §{} -> §{}: {}", s.section, to, why);
                s.section = *to;
            }
            out.push(s);
        }
    }
    out
}

/// Parse Appendix A tables into (primitive, area) pairs.
pub fn primitives(spec: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    let mut in_a = false;
    for line in spec.lines() {
        if line.starts_with("## Appendix A") {
            in_a = true;
            continue;
        }
        if line.starts_with("### A.3") || line.starts_with("## Appendix B") {
            break;
        }
        if !in_a || !line.starts_with("| **") {
            continue;
        }
        let cells: Vec<&str> = line.split('|').map(str::trim).collect();
        let Some(area) = cells.get(1) else { continue };
        let area = area.trim_matches('*').split("**").next().unwrap_or("").trim().to_string();
        let a2 = cells.len() == 4; // A.2 rows: | interface | needed by |
        let src = if a2 { cells.get(1).copied().unwrap_or("") } else { cells.get(2).copied().unwrap_or("") };
        if a2 {
            let key = ident_part(&area);
            if seen.insert(key.clone()) {
                out.push((area.clone(), "A.2".to_string()));
            }
            continue;
        }
        let mut rest = src;
        while let Some(i) = rest.find('`') {
            let after = &rest[i + 1..];
            let Some(j) = after.find('`') else { break };
            let name = after[..j].split('(').next().unwrap_or("").to_string();
            rest = &after[j + 1..];
            for n in name.split('/') {
                let n = n.trim();
                if !n.is_empty() && seen.insert(n.to_string()) {
                    out.push((n.to_string(), area.clone()));
                }
            }
        }
    }
    out
}

fn raw(s: &str) -> String {
    format!("r##\"{}\"##", s.replace("\"##", "\" ##"))
}

const HEADER: &str = "// @generated by `cargo xtask stubs` from glibc-functional-spec.md.\n\
// Specify a test by replacing its stub with a real #[test] fn of the SAME NAME (see crates/spectest).\n\
// Once a file contains specified tests, `cargo xtask stubs` stops rewriting it and only checks coverage.\n\n";

/// Write `content` unless the file already holds specified tests; in that case verify that all
/// `ids` still occur in it and report the ones that do not.
fn emit(path: &Path, content: &str, ids: &[String], force: bool, problems: &mut Vec<String>) -> Result<(), String> {
    if let Ok(existing) = fs::read_to_string(path) {
        let specified = existing.lines().any(|l| l.trim() == "#[test]");
        if specified && !force {
            for id in ids {
                if !existing.contains(id.as_str()) {
                    problems.push(format!("{}: spec clause/symbol `{id}` has neither a stub nor a test", path.display()));
                }
            }
            return Ok(());
        }
    }
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    fs::write(path, content).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn stubs(root: &Path, force: bool) -> Result<(), String> {
    let spec = fs::read_to_string(root.join("glibc-functional-spec.md")).map_err(|e| format!("spec: {e}"))?;
    let clauses = clauses(&spec);
    let symbols = symbols(&spec);
    let prims = primitives(&spec);
    let mut problems = Vec::new();

    // --- Clause stubs: §0 -> posix (C conventions), §10 -> math, others -> core.
    let mut by_top: BTreeMap<u32, Vec<&Clause>> = BTreeMap::new();
    for c in &clauses {
        by_top.entry(c.top).or_default().push(c);
    }
    let mut tsv = String::from("id\tsection\tfield\ttext\n");
    for (top, list) in &by_top {
        let slug = SLUGS.get(*top as usize).copied().unwrap_or("misc");
        let krate = match top { 0 => "boundary", 10 => "math", _ => "core" };
        let file = root.join(format!("crates/{krate}/tests/s{top:02}_{slug}_clauses.rs"));
        let imports = match (list.iter().any(|c| c.is_commentary()), list.iter().any(|c| !c.is_commentary())) {
            (true, true) => "{clause, commentary}",
            (true, false) => "commentary",
            _ => "clause",
        };
        let mut body = format!("{HEADER}//! Clause tests for spec §{top} ({} clauses).\n\nuse spectest::{imports};\n\n", list.len());
        let mut ids = Vec::new();
        for c in list {
            let mac = if c.is_commentary() { "commentary" } else { "clause" };
            let _ = writeln!(body, "{mac}!({}, \"{}\", \"{}\", {});", c.id, c.section, c.field, raw(&c.text));
            let _ = writeln!(tsv, "{}\t{}\t{}\t{}", c.id, c.section, c.field, c.text.replace('\t', " "));
            ids.push(c.id.clone());
        }
        emit(&file, &body, &ids, force, &mut problems)?;
    }
    fs::write(root.join("spec/clauses.tsv"), tsv).map_err(|e| e.to_string())?;

    // --- Commentary items (spec §0.3): the mark's form, and each one present as `commentary!`
    // (not a pending `clause!` stub, not a test).
    let all_ids: std::collections::BTreeSet<&str> = clauses.iter().map(|c| c.id.as_str()).collect();
    let marked: Vec<&Clause> = clauses.iter().filter(|c| c.is_commentary()).collect();
    for c in &marked {
        problems.extend(commentary_problems(c, &all_ids));
        let slug = SLUGS.get(c.top as usize).copied().unwrap_or("misc");
        let krate = match c.top { 0 => "boundary", 10 => "math", _ => "core" };
        let file = root.join(format!("crates/{krate}/tests/s{:02}_{slug}_clauses.rs", c.top));
        let text = fs::read_to_string(&file).unwrap_or_default();
        if !text.contains(&format!("commentary!({},", c.id)) {
            problems.push(format!("{}: `{}` is marked COMMENTARY in the spec but is not a `commentary!` item there", file.display(), c.id));
        }
    }

    // --- Symbol stubs (translation layer), one file per section.
    let mut by_sec: BTreeMap<u32, Vec<&Symbol>> = BTreeMap::new();
    for s in &symbols {
        by_sec.entry(s.section).or_default().push(s);
    }
    let mut stsv = String::from("symbol\tsection\tfirst_version\tdeclared\tfamily_count\tnote\n");
    for (sec, list) in &by_sec {
        let slug = SLUGS.get(*sec as usize).copied().unwrap_or("misc");
        let file = root.join(format!("crates/boundary/tests/s{sec:02}_{slug}_symbols.rs"));
        let has_family = list.iter().any(|s| s.family_count.is_some());
        let has_symbol = list.iter().any(|s| s.family_count.is_none());
        let imports = match (has_family, has_symbol) {
            (true, true) => "use spectest::{family, symbol};",
            (true, false) => "use spectest::family;",
            _ => "use spectest::symbol;",
        };
        let mut body = format!("{HEADER}//! Translation-layer tests for the exported symbols of spec §{sec} ({} entries).\n//! Test names keep the exported C name verbatim (`sym_<name>`) for traceability.\n#![allow(non_snake_case)]\n\n{imports}\n\n", list.len());
        let mut ids = Vec::new();
        for s in list {
            let id = match s.family_count {
                Some(_) => format!("fam_{}", ident_part(&s.name)),
                None => format!("sym_{}", s.name),
            };
            match s.family_count {
                Some(n) => { let _ = writeln!(body, "family!({id}, \"{}\", {n});", s.name); }
                None => {
                    let note = if s.undeclared { "(no installed header declares it)" } else { "" };
                    let note = if s.note.is_empty() { note.to_string() } else { format!("{note} {}", s.note) };
                    let _ = writeln!(body, "symbol!({id}, \"{}\", \"{}\", \"{}\", {});", s.name, s.section, s.version, raw(note.trim()));
                }
            }
            let _ = writeln!(stsv, "{}\t{}\t{}\t{}\t{}\t{}", s.name, s.section, s.version, if s.undeclared { "no" } else { "yes" },
                s.family_count.map(|n| n.to_string()).unwrap_or_default(), s.note);
            ids.push(id);
        }
        emit(&file, &body, &ids, force, &mut problems)?;
    }
    fs::write(root.join("spec/symbols.tsv"), stsv).map_err(|e| e.to_string())?;

    // --- Kernel primitive stubs (Appendix A).
    let mut body = format!("{HEADER}//! Kernel-boundary tests, one per primitive of spec Appendix A ({} primitives).\n\nuse spectest::primitive;\n\n", prims.len());
    let mut ptsv = String::from("primitive\tarea\n");
    let mut ids = Vec::new();
    for (p, area) in &prims {
        let mut id = format!("k_{}", ident_part(p).trim_matches('_'));
        while id.contains("__") {
            id = id.replace("__", "_");
        }
        let _ = writeln!(body, "primitive!({id}, {}, \"{}\");", raw(p), area);
        let _ = writeln!(ptsv, "{p}\t{area}");
        ids.push(id);
    }
    emit(&root.join("crates/boundary/tests/appendix_a_primitives.rs"), &body, &ids, force, &mut problems)?;
    fs::write(root.join("spec/primitives.tsv"), ptsv).map_err(|e| e.to_string())?;

    // --- ABI data stubs (spec Appendix C): one per type layout and per static initializer.
    let (layout_ids, layout_n) = abi_stubs(root, force, &mut problems)?;
    let _ = layout_ids;

    let families: u32 = symbols.iter().filter_map(|s| s.family_count).sum();
    let plain = symbols.iter().filter(|s| s.family_count.is_none()).count();
    println!("clauses: {} across {} sections", clauses.len(), by_top.len());
    println!("commentary items (marked in the spec, not tested, not pending): {}: {}", marked.len(), marked.iter().map(|c| c.id.as_str()).collect::<Vec<_>>().join(", "));
    println!("symbols: {} named + {} libm family members = {} exported names ({} families)",
        plain, families, plain as u32 + families, symbols.iter().filter(|s| s.family_count.is_some()).count());
    println!("kernel primitives: {}", prims.len());
    println!("ABI data items (Appendix C): {layout_n}");
    for (sec, list) in &by_sec {
        let n: u32 = list.iter().map(|s| s.family_count.unwrap_or(1)).sum();
        let c = by_top.get(sec).map(Vec::len).unwrap_or(0);
        println!("  §{sec:<2} {:<13} symbols {n:>5}   clauses {c:>4}", SLUGS.get(*sec as usize).copied().unwrap_or("?"));
    }
    if let Some(c) = by_top.get(&0) {
        println!("  §0  conventions   clauses {:>4}", c.len());
    }
    if !problems.is_empty() {
        for p in &problems {
            eprintln!("{p}");
        }
        return Err(format!("{} coverage problems", problems.len()));
    }
    // The tests and documents that cite clauses, against the clauses just written.
    crate::trace::run(root)
}

/// Layout and initializer stubs from spec/abi/layouts.tsv and initializers.tsv (Appendix C).
fn abi_stubs(root: &Path, force: bool, problems: &mut Vec<String>) -> Result<(Vec<String>, usize), String> {
    let lay = fs::read_to_string(root.join("spec/abi/layouts.tsv")).map_err(|e| format!("layouts.tsv: {e}"))?;
    let ini = fs::read_to_string(root.join("spec/abi/initializers.tsv")).map_err(|e| format!("initializers.tsv: {e}"))?;
    let mut body = String::new();
    let mut ids = Vec::new();
    for line in lay.lines().skip(1) {
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() < 4 || !f[1].is_empty() {
            continue;
        }
        let id = format!("abi_{}", ident_part(f[0]).trim_matches('_'));
        let id = collapse(&id);
        let _ = writeln!(body, "layout!({id}, \"{}\", \"size {} align {}; members per spec/abi/layouts.tsv\");", f[0], f[2], f[3]);
        ids.push(id);
    }
    for line in ini.lines().skip(1) {
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() < 4 {
            continue;
        }
        let id = collapse(&format!("init_{}", ident_part(if f[1].starts_with('{') { f[0] } else { f[1] }).trim_matches('_')));
        let _ = writeln!(body, "layout!({id}, \"{} = {}\", \"{} bytes, {}; bytes per spec/abi/initializers.tsv\");", f[0], f[1], f[2], f[3]);
        ids.push(id);
    }
    let n = ids.len();
    let content = format!("{HEADER}//! ABI data tests, one per type layout and static initializer of spec Appendix C ({n} items).
//! Each asserts size, alignment and member offsets (or initializer bytes) of the `liella_abi::layout`
//! definition against the normative tables in spec/abi/.

use spectest::layout;

{body}");
    emit(&root.join("crates/abi/tests/appendix_c_layouts.rs"), &content, &ids, force, problems)?;
    Ok((ids, n))
}

pub(crate) fn collapse(s: &str) -> String {
    let mut out = s.to_string();
    while out.contains("__") {
        out = out.replace("__", "_");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::clauses;

    #[test]
    fn a_flag_line_does_not_end_the_field() {
        let spec = "## 1. A\n\n### 1.1 F\n\n**Contract.**\n\n* **`a`** first.\n  \u{2691} HIST.\n* **`b`** second.\n\n\u{2691} ARB: a note on the field,\nwrapped.\n\n* **`c`** third.\n\n**Failure.** none.\n";
        let c = clauses(spec);
        let ids: Vec<(&str, &str)> = c.iter().map(|c| (c.id.as_str(), c.text.as_str())).collect();
        assert_eq!(
            ids,
            [
                ("s1_1_c_1", "**`a`** first. \u{2691} HIST."),
                ("s1_1_c_2", "**`b`** second."),
                ("s1_1_c_3", "**`c`** third."),
                ("s1_1_f_0", "none."),
            ]
        );
    }
}
