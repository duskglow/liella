//! `cargo xtask codemap [--check]`: regenerates the code map (`docs/codemap/README.md`, `docs/codemap/symbols.tsv`)
//! from the tree. `--check` (run by `cargo xtask static`) writes nothing and fails when either file differs from what
//! the tree gives ("out of date: run cargo xtask codemap"). `codemap-asm` is kept as an alias.
//!
//! `symbols.tsv` (header `symbol, version, defined at, first implementation call`; rows sorted bytewise, one per
//! definition) has a row for:
//! * every `pub extern "C" fn NAME` in `crates/boundary/src/` whose item starts with `export! { [kind] "V";` (version
//!   `V`; `local` exports are hidden names and get no row), `internal_entry! {` or a `no_mangle` attribute (version
//!   empty). "Defined at" is the `fn` line; the first call is the first qualified call (`a::b(` or `a::b::<`) of the body.
//! * every `export! { asm "V"; ... }`: "defined at" is the global label (`.globl NAME` and `NAME:`) in
//!   `csys/src/machine/*.S`, the first call the first `call`/`jmp`/`jcc` target after it. No label is an error.
//!   Labels are read after substituting each product's object-like `#define`s (`preprocess`), so a label written as
//!   a macro (`SQRTL:` with `#define SQRTL sqrtl` under `LIELLA_LIBM`) gives the name the product exports.
//! * every `LIELLA_VFWD(NAME)` definition in `csys/src/adapter/*.c` (version empty; first call: the callee after `FWD(`).
//! * every family row of `crates/boundary/src/posix/s10_math.tsv` (version `(math-gen)`; first call from the body).
//!
//! `README.md`: the header, the section table (one row per spec section `## N. Title`), the differential tests, the
//! crates, the system-file paths named by string literals in `crates/*/src/` and the public functions of the shared
//! helper files are generated. Everything from the first `## ` heading that is not a generated one to the end of the
//! file is hand-written and kept verbatim; a repository path in it that does not exist is an error.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

const TSV: &str = "docs/codemap/symbols.tsv";
const README: &str = "docs/codemap/README.md";
const BOUNDARY: &str = "crates/boundary/src";
const POSIX: &str = "crates/boundary/src/posix";
const MACHINE: &str = "csys/src/machine";
const ADAPTER: &str = "csys/src/adapter";
const MATH_TSV: &str = "crates/boundary/src/posix/s10_math.tsv";
const SPEC: &str = "glibc-functional-spec.md";
/// The shared helper files listed in the README (their `pub fn` items).
const HELPERS: [&str; 3] = ["crates/boundary/src/errno.rs", "crates/core/src/system.rs", "crates/kernel/src/errno.rs"];
const SYS_ROOTS: [&str; 7] = ["/dev/", "/etc/", "/proc/", "/sys/", "/usr/", "/var/", "/run/"];
const PATH_ROOTS: [&str; 9] = ["crates/", "csys/", "xtask/", "plan/", "docs/", "spec/", "tests/", "benchmarks/", "validation/"];
const SYS_HEADING: &str = "## System files the library reads or writes";
const HELPER_HEADING: &str = "## Shared helpers (errno save/restore, reading files, ...)";

fn is_ident(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// The first qualified call (`a::b(` or `a::b::<`) in Rust text, skipping strings, char literals and `//` comments.
fn first_call(text: &str) -> Option<String> {
    let b = text.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == b'"' {
            i += 1;
            while i < b.len() && b[i] != b'"' {
                i += if b[i] == b'\\' { 2 } else { 1 };
            }
            i += 1;
        } else if c == b'\'' && i + 2 < b.len() && (b[i + 2] == b'\'' || b[i + 1] == b'\\') {
            i += 2;
            while i < b.len() && b[i] != b'\'' {
                i += 1;
            }
            i += 1;
        } else if c == b'/' && b.get(i + 1) == Some(&b'/') {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
        } else if (c.is_ascii_alphabetic() || c == b'_') && (i == 0 || !(is_ident(b[i - 1]) || b[i - 1] == b':')) {
            let start = i;
            let mut segs = 0;
            loop {
                while i < b.len() && is_ident(b[i]) {
                    i += 1;
                }
                segs += 1;
                if b[i..].starts_with(b"::") && b.get(i + 2).is_some_and(|&n| n.is_ascii_alphabetic() || n == b'_') {
                    i += 2;
                } else {
                    break;
                }
            }
            let rest = text[i..].trim_start_matches(' ');
            if segs > 1 && (rest.starts_with('(') || text[i..].starts_with("::<")) {
                return Some(text[start..i].to_string());
            }
        } else {
            i += 1;
        }
    }
    None
}

/// `(symbol, version, line, first call)` of each Rust export in one boundary source; asm exports have version
/// prefixed with `asm `.
fn rust_exports(text: &str) -> Vec<(String, String, usize, String)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        let t = l.trim_start();
        let Some(rest) = t.strip_prefix("pub extern \"C\" fn ").or_else(|| t.strip_prefix("pub unsafe extern \"C\" fn ")) else { continue };
        let name: String = rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
        if name.is_empty() {
            continue;
        }
        let mut header = "";
        let mut no_mangle = false;
        for p in lines[..i].iter().rev() {
            let p = p.trim();
            if p.starts_with("#[") || p.starts_with("//") || p.ends_with(")]") {
                no_mangle |= p.contains("no_mangle");
                continue;
            }
            header = p;
            break;
        }
        let version = if let Some(h) = header.strip_prefix("export! {") {
            let h = h.trim();
            let (kind, lit) = h.split_once('"').unwrap_or(("", ""));
            let Some((v, _)) = lit.split_once('"') else { continue };
            match kind.trim() {
                "local" => continue,
                "asm" => format!("asm {v}"),
                _ => v.to_string(),
            }
        } else if header.ends_with("internal_entry! {") || no_mangle {
            String::new()
        } else {
            continue;
        };
        let indent = l.len() - t.len();
        let end = lines[i + 1..]
            .iter()
            .position(|x| x.trim_start().starts_with('}') && x.len() - x.trim_start().len() <= indent)
            .map_or(lines.len(), |k| i + 1 + k);
        let body_start = rest.find('{').map_or("", |k| &rest[k..]);
        let body = std::iter::once(body_start).chain(lines[i + 1..end].iter().copied()).collect::<Vec<_>>().join("\n");
        out.push((name, version, i + 1, first_call(&body).unwrap_or_default()));
    }
    out
}

/// The `-D` sets `crates/boundary/build.rs` assembles `csys/src/machine/*.S` with, one per product: libc.so.6,
/// libm.so.6, ld.so.
const PRODUCT_DEFINES: [&[&str]; 3] = [&["LIELLA_EXPORT", "LIELLA_RUNTIME"], &["LIELLA_LIBM"], &["LIELLA_LD"]];

/// One `.S` source with the object-like `#define NAME VALUE`s active under `defined` substituted into every other
/// line, each directive line blanked (line numbers are kept). Only the `#define`s follow the conditionals
/// (`#ifdef`, `#ifndef`, `#if`/`#elif` of `defined(X)` terms joined by `||`, `#else`, `#endif`; another `#if`
/// expression counts as false); code lines are all kept, as before, so a label under `#ifdef LIELLA_LD` still counts.
fn preprocess(text: &str, defined: &[&str]) -> String {
    fn holds(expr: &str, defined: &[&str], macros: &BTreeMap<String, String>) -> bool {
        expr.split("||").any(|t| {
            let t = t.trim();
            let (neg, t) = t.strip_prefix('!').map_or((false, t), |r| (true, r.trim()));
            let Some(name) = t.strip_prefix("defined") else { return false };
            let name = name.trim().trim_start_matches('(').trim_end_matches(')').trim();
            neg != (defined.contains(&name) || macros.contains_key(name))
        })
    }
    let mut macros: BTreeMap<String, String> = BTreeMap::new();
    // Per open conditional: (this branch active, some branch already taken, enclosing active).
    let mut stack: Vec<(bool, bool, bool)> = Vec::new();
    let mut out = String::with_capacity(text.len());
    for l in text.lines() {
        let active = stack.last().is_none_or(|s| s.0);
        if let Some(d) = l.trim_start().strip_prefix('#') {
            let d = d.trim_start();
            let (kw, rest) = d.split_once(char::is_whitespace).map_or((d, ""), |(k, r)| (k, r.trim()));
            match kw {
                "ifdef" | "ifndef" | "if" => {
                    let c = match kw {
                        "ifdef" => holds(&format!("defined({rest})"), defined, &macros),
                        "ifndef" => !holds(&format!("defined({rest})"), defined, &macros),
                        _ => holds(rest, defined, &macros),
                    };
                    stack.push((active && c, c, active));
                }
                "elif" => {
                    if let Some(s) = stack.last_mut() {
                        let c = !s.1 && holds(rest, defined, &macros);
                        *s = (s.2 && c, s.1 || c, s.2);
                    }
                }
                "else" => {
                    if let Some(s) = stack.last_mut() {
                        *s = (s.2 && !s.1, true, s.2);
                    }
                }
                "endif" => {
                    stack.pop();
                }
                "define" if active => {
                    let (name, value) = rest.split_once(char::is_whitespace).map_or((rest, ""), |(n, v)| (n, v.trim()));
                    if name.bytes().all(is_ident) {
                        macros.insert(name.to_string(), value.to_string());
                    }
                }
                "undef" if active => {
                    macros.remove(rest);
                }
                _ => {}
            }
            out.push('\n');
            continue;
        }
        let b = l.as_bytes();
        let mut i = 0;
        while i < b.len() {
            if is_ident(b[i]) {
                let start = i;
                while i < b.len() && is_ident(b[i]) {
                    i += 1;
                }
                let word = &l[start..i];
                out.push_str(macros.get(word).map_or(word, String::as_str));
            } else {
                let c = l[i..].chars().next().unwrap_or(' ');
                out.push(c);
                i += c.len_utf8();
            }
        }
        out.push('\n');
    }
    out
}

/// Labels of one `.S` source: name -> (line, first `call`/`jmp`/`jcc` target up to its `.size`, global).
fn asm_labels(text: &str) -> BTreeMap<String, (usize, String, bool)> {
    let lines: Vec<&str> = text.lines().collect();
    let globl: BTreeSet<&str> = lines
        .iter()
        .filter_map(|l| l.trim().strip_prefix(".globl").or_else(|| l.trim().strip_prefix(".global")))
        .flat_map(|r| r.split(',').map(str::trim))
        .collect();
    let mut out = BTreeMap::new();
    for (i, l) in lines.iter().enumerate() {
        let Some(name) = l.trim().strip_suffix(':').or_else(|| l.trim().split_once(':').map(|(n, _)| n)) else { continue };
        if name.is_empty() || name.starts_with(|c: char| c.is_ascii_digit()) || !name.bytes().all(is_ident) || out.contains_key(name) {
            continue;
        }
        let mut target = String::new();
        for x in &lines[i + 1..] {
            let x = x.split("/*").next().unwrap_or("").trim();
            if x.starts_with(".size") && x.contains(name) {
                break;
            }
            let mut w = x.split_whitespace();
            let op = w.next().unwrap_or("");
            let is_branch = op == "call" || (op.starts_with('j') && op.len() <= 5 && op.bytes().all(|c| c.is_ascii_lowercase()));
            if let (true, Some(t)) = (is_branch, w.next()) {
                if t.as_bytes()[0].is_ascii_alphabetic() || t.starts_with('_') {
                    target = t.split('@').next().unwrap_or("").trim_end_matches(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).to_string();
                    break;
                }
            }
        }
        out.insert(name.to_string(), (i + 1, target, globl.contains(name)));
    }
    out
}

/// `(target, name, version)` of each `.symver TARGET, NAME@VERSION` (or `@@`) line of one `.S` source.
fn symvers(text: &str) -> Vec<(String, String, String)> {
    text.lines()
        .filter_map(|l| {
            let (target, alias) = l.trim().strip_prefix(".symver")?.split_once(',')?;
            let (name, version) = alias.trim().split_once('@')?;
            Some((target.trim().to_string(), name.to_string(), version.trim_start_matches('@').to_string()))
        })
        .collect()
}

/// `(symbol, line, first call)` of each `LIELLA_VFWD(NAME)` definition in one C adapter source.
fn c_forwarders(text: &str) -> Vec<(String, usize, String)> {
    let mut out = Vec::new();
    for (i, l) in text.lines().enumerate() {
        if l.trim_start().starts_with('#') {
            continue;
        }
        let Some((_, r)) = l.split_once("LIELLA_VFWD(") else { continue };
        let Some((name, after)) = r.split_once(')') else { continue };
        let call = after.split_once("FWD(").map(|(_, f)| {
            let b = f.as_bytes();
            let mut s = None;
            for (k, &c) in b.iter().enumerate() {
                if is_ident(c) {
                    s = s.or(Some(k));
                } else if c == b'(' && k > 0 && s.is_some() {
                    return f[s.unwrap_or(0)..k].to_string();
                } else {
                    s = None;
                }
            }
            String::new()
        });
        out.push((name.to_string(), i + 1, call.unwrap_or_default()));
    }
    out
}

fn walk(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            if p.file_name().is_some_and(|n| n != "target") {
                walk(&p, ext, out);
            }
        } else if p.extension().is_some_and(|x| x == ext) {
            out.push(p);
        }
    }
}

fn files(root: &Path, dir: &str, ext: &str) -> Vec<PathBuf> {
    let mut v = Vec::new();
    walk(&root.join(dir), ext, &mut v);
    v.sort();
    v
}

fn rel(root: &Path, p: &Path) -> String {
    p.strip_prefix(root).unwrap_or(p).display().to_string()
}

fn read(root: &Path, p: &Path) -> Result<String, String> {
    fs::read_to_string(p).map_err(|e| format!("{}: {e}", rel(root, p)))
}

fn symbols(root: &Path, errors: &mut Vec<String>) -> Result<String, String> {
    let mut labels: BTreeMap<String, (String, usize, String, bool)> = BTreeMap::new();
    let mut aliases = Vec::new();
    for p in files(root, MACHINE, "S") {
        let text = read(root, &p)?;
        // A label may be an object-like macro (`#define SQRTL sqrtl`): read the names each product assembles.
        for config in PRODUCT_DEFINES {
            for (k, (line, t, g)) in asm_labels(&preprocess(&text, config)) {
                labels.entry(k).or_insert((rel(root, &p), line, t, g));
            }
        }
        aliases.extend(symvers(&text).into_iter().map(|a| (rel(root, &p), a)));
    }
    let mut rows = BTreeSet::new();
    // Rust definitions by name: ("file:line", first call).
    let mut defs: BTreeMap<String, (String, String)> = BTreeMap::new();
    let mut named = BTreeSet::new();
    for p in files(root, BOUNDARY, "rs") {
        let file = rel(root, &p);
        for (name, version, line, call) in rust_exports(&read(root, &p)?) {
            named.insert(name.clone());
            if let Some(v) = version.strip_prefix("asm ") {
                match labels.get(&name).filter(|l| l.3) {
                    Some((f, l, t, _)) => rows.insert(format!("{name}\t{v}\t{f}:{l}\t{t}")),
                    None => {
                        errors.push(format!("{file}:{line}: asm export {name} has no global label in {MACHINE}/*.S"));
                        false
                    }
                };
            } else {
                defs.entry(name.clone()).or_insert((format!("{file}:{line}"), call.clone()));
                rows.insert(format!("{name}\t{version}\t{file}:{line}\t{call}"));
            }
        }
    }
    // Compat-only symbols: `.symver STUB, NAME@V`; the row names the Rust function the stub jumps to (or is), else
    // the stub's label. An undefined target is a reference binding when another `.symver` defines the same
    // NAME@V; otherwise nothing defines the symbol, which is an error.
    let mut defined = BTreeSet::new();
    let mut undefined = Vec::new();
    for (file, (target, name, version)) in aliases {
        named.insert(name.clone());
        let label = labels.get(&target);
        let at = defs.get(&target).or_else(|| label.and_then(|l| defs.get(&l.2))).cloned().or_else(|| label.map(|(f, l, t, _)| (format!("{f}:{l}"), t.clone())));
        match at {
            Some((at, call)) => {
                rows.insert(format!("{name}\t{version}\t{at}\t{call}"));
                defined.insert(format!("{name}@{version}"));
            }
            None => undefined.push((file, target, format!("{name}@{version}"))),
        }
    }
    for (file, target, alias) in undefined {
        if !defined.contains(&alias) {
            errors.push(format!("{file}: .symver {target}, {alias}: nothing defines {alias}"));
        }
    }
    // Exported names that only assembly defines (a global label without a `liella_` prefix and no row above).
    for (name, (f, l, t, g)) in &labels {
        if *g && !name.starts_with("liella_") && !named.contains(name) {
            rows.insert(format!("{name}\t\t{f}:{l}\t{t}"));
        }
    }
    for p in files(root, ADAPTER, "c") {
        let file = rel(root, &p);
        for (name, line, call) in c_forwarders(&read(root, &p)?) {
            rows.insert(format!("{name}\t\t{file}:{line}\t{call}"));
        }
    }
    if let Ok(text) = fs::read_to_string(root.join(MATH_TSV)) {
        for (i, l) in text.lines().enumerate() {
            if l.trim().is_empty() || l.starts_with('#') {
                continue;
            }
            let name = l.split('\t').next().unwrap_or("");
            let call = first_call(l.rsplit('\t').next().unwrap_or("")).unwrap_or_default();
            rows.insert(format!("{name}\t(math-gen)\t{MATH_TSV}:{}\t{call}", i + 1));
        }
    }
    let mut out = String::from("symbol\tversion\tdefined at\tfirst implementation call\n");
    for r in rows {
        out.push_str(&r);
        out.push('\n');
    }
    Ok(out)
}

/// Section numbers a behaviour record's lead (`**Spec:**` or `**Clauses:**` paragraph) names.
fn record_sections(text: &str) -> BTreeSet<u32> {
    let lead: Vec<&str> = text.lines().skip_while(|l| !(l.starts_with("**Spec:**") || l.starts_with("**Clauses:**"))).take_while(|l| !l.trim().is_empty()).collect();
    let lead = lead.join(" ");
    let mut out = BTreeSet::new();
    for (k, _) in lead.match_indices(['§', '`']) {
        let r = &lead[k..];
        let digits = |s: &str| s.chars().take_while(char::is_ascii_digit).collect::<String>();
        let n = if let Some(s) = r.strip_prefix('§') { digits(s) } else if let Some(s) = r.strip_prefix("`s") { let d = digits(s); if s[d.len()..].starts_with('_') { d } else { String::new() } } else { String::new() };
        if let Ok(n) = n.parse() {
            out.insert(n);
        }
    }
    out
}

fn names(root: &Path, dir: &str, ext: &str, prefix: &str, full: bool) -> Vec<String> {
    files(root, dir, ext)
        .into_iter()
        .filter(|p| p.parent() == Some(&root.join(dir)) && p.file_name().is_some_and(|n| n.to_string_lossy().starts_with(prefix)))
        .map(|p| if full { rel(root, &p) } else { p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default() })
        .collect()
}

/// `boundary::x` / `core::x` modules a boundary source names (`crate::x`, `crate::{x, y}`, `liella_core::x`).
fn modules(text: &str, out: &mut BTreeSet<String>) {
    let mut add = |krate: &str, rest: &str| {
        let m: String = rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
        let next = rest[m.len()..].chars().next();
        if m.starts_with(|c: char| c.is_ascii_lowercase()) && next != Some('!') && !["export", "posix"].contains(&m.as_str()) {
            out.insert(format!("{krate}::{m}"));
        }
    };
    for (k, _) in text.match_indices("crate::") {
        if text[..k].ends_with(|c: char| c.is_ascii_alphanumeric() || c == '_' || c == ':') {
            continue;
        }
        let rest = &text[k + 7..];
        if let Some(list) = rest.strip_prefix('{') {
            for item in list.split('}').next().unwrap_or("").split(',') {
                add("boundary", item.trim());
            }
        } else {
            add("boundary", rest);
        }
    }
    for (k, _) in text.match_indices("liella_") {
        if k > 0 && is_ident(text.as_bytes()[k - 1]) {
            continue;
        }
        let rest = &text[k + 7..];
        let krate: String = rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
        if let Some(r) = rest[krate.len()..].strip_prefix("::") {
            add(&krate, r);
        }
    }
}

fn readme(root: &Path, old: &str, errors: &mut Vec<String>) -> Result<String, String> {
    let spec = fs::read_to_string(root.join(SPEC)).unwrap_or_default();
    let records: Vec<(String, BTreeSet<u32>)> = files(root, "spec/behaviour", "md")
        .iter()
        .filter(|p| p.file_name().is_some_and(|n| n != "README.md"))
        .map(|p| Ok((p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), record_sections(&read(root, p)?))))
        .collect::<Result<_, String>>()?;
    let mut tests = Vec::new();
    for c in fs::read_dir(root.join("crates")).into_iter().flatten().flatten() {
        tests.extend(files(root, &rel(root, &c.path().join("tests")), "rs").into_iter().filter(|p| p.parent() == Some(&c.path().join("tests"))));
    }
    tests.sort();
    let mut o = String::from("# Code map (generated: `cargo xtask codemap`; do not edit by hand above the hand-written sections)\n\n\
        Where each spec section lives. Exported symbols: `grep -P '^strlen\\t' docs/codemap/symbols.tsv`\n\
        (symbol, version, defining file:line, first implementation call). Plan index: implementation-plan.md.\n\
        Differential (`*_products.rs`) tests are listed once at the end; `cargo xtask affected --list` shows\n\
        exactly which tests a change reaches.\n\n\
        | § | Title | Boundary (C entry points) | Library modules used | Clause tests | Conformance | Decisions | Behaviour records |\n\
        |---|---|---|---|---|---|---|---|\n");
    for l in spec.lines() {
        let Some(r) = l.strip_prefix("## ") else { continue };
        let Some((n, title)) = r.split_once(". ") else { continue };
        let Ok(n) = n.parse::<u32>() else { continue };
        if n == 0 {
            continue;
        }
        let pre = format!("s{n:02}_");
        let boundary = names(root, POSIX, "rs", &pre, true);
        let mut mods = BTreeSet::new();
        for b in &boundary {
            modules(&read(root, &root.join(b))?, &mut mods);
        }
        let mut clause: Vec<(u8, String)> = tests
            .iter()
            .filter(|p| p.file_name().is_some_and(|f| f.to_string_lossy().starts_with(&pre)))
            .map(|p| {
                let f = p.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
                let rank = if f.ends_with("_symbols.rs") { 0 } else if f.ends_with("_clauses.rs") { 1 } else { 2 };
                (rank, if p.starts_with(root.join("crates/boundary/tests")) { f } else { rel(root, p) })
            })
            .collect();
        clause.sort();
        let conf = names(root, "conformance/c", "c", &pre, false);
        let dec = names(root, "plan/07-decisions", "md", &format!("s{n:02}-"), true);
        let rec: Vec<&str> = records.iter().filter(|(_, s)| s.contains(&n)).map(|(f, _)| f.as_str()).collect();
        let clause: Vec<String> = clause.into_iter().map(|(_, f)| f).collect();
        let mods: Vec<String> = mods.into_iter().collect();
        o += &format!("| {n} | {title} | {} | {} | {} | {} | {} | {} |\n", boundary.join(", "), mods.join(", "), clause.join(", "), conf.join(", "), dec.join(", "), rec.join(", "));
    }
    let diff: Vec<String> = tests.iter().filter(|p| p.to_string_lossy().ends_with("_products.rs")).map(|p| rel(root, p)).collect();
    o += &format!("\nDifferential tests (`crates/*/tests/*_products.rs`): {}\n", diff.join(", "));
    let crates: Vec<String> = fs::read_dir(root.join("crates")).into_iter().flatten().flatten().filter(|e| e.path().join("Cargo.toml").exists()).map(|e| e.file_name().to_string_lossy().into_owned()).collect::<BTreeSet<_>>().into_iter().collect();
    o += &format!("\nOther crates: {}\n\n{SYS_HEADING}\n\n| Path | Named at (file:line; the first is usually the constant) |\n|---|---|\n", crates.join(", "));
    let mut sys: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for c in &crates {
        for p in files(root, &format!("crates/{c}/src"), "rs") {
            for (i, l) in read(root, &p)?.lines().enumerate() {
                for (k, _) in l.match_indices("\"/") {
                    let path: String = l[k + 1..].chars().take_while(|c| c.is_ascii_alphanumeric() || "._/-".contains(*c)).collect();
                    let end = l[k + 1 + path.len()..].chars().next();
                    if SYS_ROOTS.iter().any(|r| path.starts_with(r)) && matches!(end, Some('"' | '\\')) {
                        sys.entry(path).or_default().push(format!("{}:{}", rel(root, &p), i + 1));
                    }
                }
            }
        }
    }
    for (path, at) in &sys {
        o += &format!("| `{path}` | {} |\n", at.join(", "));
    }
    o += &format!("\n{HELPER_HEADING}\n\n");
    for h in HELPERS {
        let Ok(text) = fs::read_to_string(root.join(h)) else {
            errors.push(format!("{h}: shared helper file does not exist"));
            continue;
        };
        let fns: Vec<String> = text
            .lines()
            .enumerate()
            .filter_map(|(i, l)| l.trim_start().strip_prefix("pub fn ").map(|r| format!("`{}` :{}", r.split(['(', '<']).next().unwrap_or(""), i + 1)))
            .collect();
        o += &format!("* {h}: {}\n", fns.join(", "));
    }
    let tail = hand_written(old);
    for (i, line) in tail.lines().enumerate() {
        for tok in line.split(|c: char| !(c.is_ascii_alphanumeric() || "_./-".contains(c))) {
            let tok = tok.trim_end_matches('.');
            if PATH_ROOTS.iter().any(|r| tok.starts_with(r)) && !root.join(tok).exists() {
                errors.push(format!("{README} (hand-written section, line {} of it): {tok} does not exist", i + 1));
            }
        }
    }
    if !tail.is_empty() {
        o += "\n";
        o += tail;
    }
    Ok(o)
}

/// The hand-written part of the README: from the first `## ` heading that the generator does not write.
fn hand_written(old: &str) -> &str {
    let mut at = 0;
    for l in old.split_inclusive('\n') {
        if l.starts_with("## ") && l.trim_end() != SYS_HEADING && l.trim_end() != HELPER_HEADING {
            return &old[at..];
        }
        at += l.len();
    }
    ""
}

/// Both files as the tree gives them, or every definition that could not be resolved.
pub fn generate(root: &Path) -> Result<(String, String), String> {
    let mut errors = Vec::new();
    let tsv = symbols(root, &mut errors)?;
    let old = fs::read_to_string(root.join(README)).unwrap_or_default();
    let md = readme(root, &old, &mut errors)?;
    if errors.is_empty() {
        Ok((tsv, md))
    } else {
        Err(format!("{} problem(s):\n  {}", errors.len(), errors.join("\n  ")))
    }
}

pub fn run(root: &Path, check: bool) -> Result<(), String> {
    let (tsv, md) = generate(root)?;
    let mut stale = Vec::new();
    for (path, new) in [(TSV, &tsv), (README, &md)] {
        if fs::read_to_string(root.join(path)).ok().as_ref() != Some(new) {
            if check {
                stale.push(path);
            } else {
                fs::write(root.join(path), new).map_err(|e| format!("{path}: {e}"))?;
                println!("codemap: wrote {path}");
            }
        }
    }
    if stale.is_empty() { Ok(()) } else { Err(format!("{} out of date: run cargo xtask codemap", stale.join(", "))) }
}

#[cfg(test)]
mod tests {
    // no spec clause: code map tooling (XTASK-CODEMAP).
    use super::*;

    const RS: &str = "export! { asm \"GLIBC_2.2.5\";\npub extern \"C\" fn asm_fn(x: usize) -> usize {\n    x\n}}\n\n\
                      export! { \"GLIBC_2.2.5\";\n/// Doc.\npub extern \"C\" fn rust_fn() {\n    ret::int(core::bytes::len::<u8>(p))\n}}\n\n\
                      export! { local \"hidden\";\npub extern \"C\" fn hidden_fn() {}}\n";
    const S: &str = "\t.globl asm_fn\nasm_fn:\n\tmovq %rdi, %rax\n\tjmp liella_machine_x\n\t.size asm_fn, .-asm_fn\nnot_global:\n\tret\n\t.globl only_asm\nonly_asm:\n\tcall liella_y\n\t.symver rust_fn, old_name@GLIBC_2.0\n";
    const SPEC_MD: &str = "## 0. How to read\n## 1. Startup\n";
    const OLD_README: &str = "# Code map\n\nold generated text\n\n## Hand notes\n\nSee crates/boundary/src/posix/s01_x.rs.\n";

    fn fixture(tag: &str, files: &[(&str, &str)]) -> PathBuf {
        let d = std::env::temp_dir().join(format!("xtask-codemap-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        for (p, text) in files {
            fs::create_dir_all(d.join(p).parent().unwrap_or(&d)).unwrap();
            fs::write(d.join(p), text).unwrap();
        }
        d
    }

    fn base(tag: &str, rs: &str) -> PathBuf {
        let helper = "pub fn set() {}\n";
        fixture(tag, &[("crates/boundary/src/posix/s01_x.rs", rs), ("csys/src/machine/x.S", S), (SPEC, SPEC_MD), (README, OLD_README), (HELPERS[0], helper), (HELPERS[1], helper), (HELPERS[2], helper)])
    }

    // A macro label (`SQRTL:` with `#define SQRTL sqrtl` under LIELLA_LIBM) gives the exported name, never the
    // macro (#4293); the libc branch's `liella_` names stay rowless. no spec clause: code map tooling.
    const MACRO_S: &str = "#if defined(LIELLA_LIBM)\n#define SQRTL sqrtl\n#define SLOW liella_slow_sqrtl\n#else\n#define SQRTL liella_fwd_sqrtl\n#define SLOW liella_fwd_slow_sqrtl\n#endif\n\t.globl SQRTL\nSQRTL:\n\tjmp SLOW\n\t.size SQRTL, .-SQRTL\n";

    #[test]
    fn macro_labels_give_the_exported_name() {
        let d = base("macro", RS);
        fs::write(d.join("csys/src/machine/m.S"), MACRO_S).unwrap();
        let (tsv, _) = generate(&d).unwrap();
        assert!(tsv.contains("\nsqrtl\t\tcsys/src/machine/m.S:9\tliella_slow_sqrtl\n"), "{tsv}");
        assert!(!tsv.contains("SQRTL") && !tsv.contains("SLOW") && !tsv.contains("liella_fwd_sqrtl"), "{tsv}");
        let libc = preprocess(MACRO_S, &["LIELLA_EXPORT"]);
        assert_eq!(libc.lines().nth(8), Some("liella_fwd_sqrtl:"), "else branch outside libm; line numbers kept");
        assert_eq!(preprocess("#ifndef A\n#define X y\n#elif defined(B)\n#define X z\n#endif\nX:\n", &["A", "B"]).lines().nth(5), Some("z:"));
        assert_eq!(preprocess("#ifdef A\n#define X y\n#endif\nX:\n", &[]).lines().nth(3), Some("X:"), "inactive define not applied");
    }

    #[test]
    fn rust_and_asm_exports_get_their_definition_lines() {
        let d = base("rows", RS);
        let (tsv, md) = generate(&d).unwrap();
        let rows: Vec<&str> = tsv.lines().collect();
        assert_eq!(rows, ["symbol\tversion\tdefined at\tfirst implementation call", "asm_fn\tGLIBC_2.2.5\tcsys/src/machine/x.S:2\tliella_machine_x", "old_name\tGLIBC_2.0\tcrates/boundary/src/posix/s01_x.rs:8\tret::int", "only_asm\t\tcsys/src/machine/x.S:9\tliella_y", "rust_fn\tGLIBC_2.2.5\tcrates/boundary/src/posix/s01_x.rs:8\tret::int"]);
        assert!(md.contains("| 1 | Startup | crates/boundary/src/posix/s01_x.rs |"), "{md}");
        assert!(md.ends_with("## Hand notes\n\nSee crates/boundary/src/posix/s01_x.rs.\n"), "hand-written section kept: {md}");
        assert!(!md.contains("old generated text"));
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn asm_export_without_global_label_is_an_error() {
        let d = base("nolabel", "export! { asm \"GLIBC_2.34\";\npub extern \"C\" fn not_global() {}}\n");
        let e = generate(&d).unwrap_err();
        assert!(e.contains("asm export not_global has no global label"), "{e}");
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn removed_file_leaves_no_row_and_check_sees_it() {
        let d = base("removed", RS);
        fs::write(d.join("crates/boundary/src/posix/s01_y.rs"), "export! { \"GLIBC_2.3\";\npub extern \"C\" fn gone() {}}\n").unwrap();
        run(&d, false).unwrap();
        assert!(fs::read_to_string(d.join(TSV)).unwrap().contains("\ngone\t"));
        run(&d, true).unwrap();
        fs::remove_file(d.join("crates/boundary/src/posix/s01_y.rs")).unwrap();
        let e = run(&d, true).unwrap_err();
        assert!(e.contains("out of date: run cargo xtask codemap"), "{e}");
        run(&d, false).unwrap();
        assert!(!fs::read_to_string(d.join(TSV)).unwrap().contains("gone"));
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn dead_path_in_hand_written_section_is_an_error() {
        let d = fixture("dead", &[(SPEC, SPEC_MD), (README, "# Code map\n\n## Notes\n\ncrates/nowhere.rs:3\n")]);
        let e = generate(&d).unwrap_err();
        assert!(e.contains("crates/nowhere.rs:3 does not exist") || e.contains("crates/nowhere.rs does not exist"), "{e}");
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn first_call_rules() {
        assert_eq!(first_call("{ unsafe { strz::len(s) } }").as_deref(), Some("strz::len"));
        assert_eq!(first_call("mret::ret(math::fused::add::<{S}>({x}))").as_deref(), Some("mret::ret"));
        assert_eq!(first_call("// a::b(\n\"c::d(\" Some(x) e::f::<u8>()").as_deref(), Some("e::f"));
        assert_eq!(first_call("x.len() + T::C"), None);
        let c = c_forwarders("#define LIELLA_VFWD(name) name\nint LIELLA_VFWD(printf)(const char *fmt, ...) FWD(fmt, liella_vf(NULL, fmt, &ap))\n");
        assert_eq!(c, [("printf".to_string(), 2, "liella_vf".to_string())]);
    }
}
