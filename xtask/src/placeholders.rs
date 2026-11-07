//! The placeholder libraries (phase 14 DL-1 `DL-PLACEHOLDERS`, design `plan/ph14/design.md` §16,
//! D-27.9): `libpthread.so.0`, `libdl.so.2`, `librt.so.1`, `libutil.so.1`, `libanl.so.1` and an
//! empty `libc_malloc_debug.so.0`, built by `cargo xtask so` after `libc.so.6`.
//!
//! **Generated from `spec/abi/versions.tsv`, nothing hand-listed.** For each library:
//! * its version nodes are the versions of its rows, in numeric order, each node's parent the one
//!   before it (no base node is written here: the first node is whichever version the table's
//!   rows start with, so another architecture's table gives its own base);
//! * it exports each of its rows whose name `libc.so.6` has no row for, at the row's version
//!   (`name@VER` for `compat`, `name@@VER` for `default`). A name `libc.so.6` has a row for is
//!   reached through the library's `DT_NEEDED libc.so.6`, never defined twice (no duplicated
//!   logic): for the placeholders that is everything but `__lib<x>_version_placeholder`, for
//!   `libc_malloc_debug.so.0` everything but `malloc_get_state`/`malloc_set_state`;
//! * no `GLIBC_PRIVATE` node and no `GLIBC_PRIVATE` export (R14-12: Liella's private interface is
//!   `LIELLA_PRIVATE`, owned by `libc.so.6`/`ld.so`).
//!
//! The bodies: a `__<lib>_version_placeholder` returns (it only carries the version nodes; no
//! header declares it). Any other export takes its body from `crates/libc-so/placeholders/<lib>.c`
//! (`liella_ph_<name>`), or, for a library with a failure table `crates/libc-so/placeholders/<lib>.tsv`
//! (`libnsl.so.1`, D-29.6), from the C [`stub_bodies`] generates out of it, one stub per row; an
//! export without a body is an error, so a new table row never becomes a silent no-op.
//!
//! **The check** ([`product_errors`]) reads the built object back with `readelf` and compares it
//! with the table independently of the generator: soname, `DT_NEEDED libc.so.6`, the version
//! definitions, every defined dynamic symbol (unversioned, another library's row or a `libc.so.6`
//! name are errors, each message naming what versions.tsv says), the full export list, and no
//! `GLIBC_PRIVATE` anywhere. [`private_errors`] applies the last rule to every shared product.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::process::Command;

/// The placeholder libraries (design §16). Which libraries are placeholders is the design's
/// choice; what each exports and which version nodes it has come from versions.tsv.
pub const LIBRARIES: [&str; 7] = ["libpthread.so.0", "libdl.so.2", "librt.so.1", "libutil.so.1", "libanl.so.1", "libc_malloc_debug.so.0", "libnsl.so.1"];

/// The libraries whose empty static archive lets `-static -l<x>` resolve (their symbols are in
/// `libc.a`); `libpthread.a` is one of them.
pub const ARCHIVES: [&str; 5] = ["libpthread.a", "libdl.a", "librt.a", "libutil.a", "libanl.a"];

/// The library every placeholder forwards to.
const LIBC: &str = "libc.so.6";

/// The version node no Liella library defines (R14-12).
pub const PRIVATE_NODE: &str = "GLIBC_PRIVATE";

/// One export: name, version, default (`@@`) or not.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Export {
    pub name: String,
    pub version: String,
    pub default: bool,
}

/// What versions.tsv gives one placeholder library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub soname: String,
    /// Version nodes in definition order (each the parent of the next).
    pub nodes: Vec<String>,
    pub exports: Vec<Export>,
}

/// Version node order: numeric by the dotted number after the first `_` (`X_2.2.5 < X_2.3 <
/// X_2.10`), anything without a number last.
pub fn node_key(v: &str) -> (u32, Vec<u32>) {
    match v.split_once('_').map(|(_, n)| n.split('.').map(str::parse::<u32>).collect::<Result<Vec<u32>, _>>()) {
        Some(Ok(n)) => (0, n),
        _ => (1, Vec::new()),
    }
}

/// The rows of `table` (versions.tsv text): `(library, name, version, binding)`.
fn rows(table: &str) -> impl Iterator<Item = (&str, &str, &str, &str)> {
    table.lines().skip(1).filter_map(|line| match line.split('\t').collect::<Vec<_>>().as_slice() {
        [lib, sym, ver, binding, ..] => Some((*lib, *sym, *ver, *binding)),
        _ => None,
    })
}

/// The plan of library `soname` from versions.tsv text.
pub fn plan(table: &str, soname: &str) -> Result<Plan, String> {
    let libc_names: BTreeSet<&str> = rows(table).filter(|r| r.0 == LIBC).map(|r| r.1).collect();
    let mut placeholder_nodes: BTreeSet<String> = BTreeSet::new();
    let mut exports = BTreeSet::new();
    for (lib, sym, ver, binding) in rows(table) {
        if lib != soname {
            continue;
        }
        if ver == PRIVATE_NODE {
            continue;
        }
        if binding != "default" && binding != "compat" {
            return Err(format!("versions.tsv: {soname} {sym}@{ver}: binding `{binding}` is neither default nor compat"));
        }
        placeholder_nodes.insert(ver.to_string());
        if !libc_names.contains(sym) {
            exports.insert(Export { name: sym.to_string(), version: ver.to_string(), default: binding == "default" });
        }
    }
    if placeholder_nodes.is_empty() {
        return Err(format!("versions.tsv has no rows for {soname}"));
    }
    let mut nodes: Vec<String> = placeholder_nodes.into_iter().collect();
    nodes.sort_by_key(|v| node_key(v));
    Ok(Plan { soname: soname.to_string(), nodes, exports: exports.into_iter().collect() })
}

/// The version script: every node, each extending the one before; no names (the exports carry
/// their version through `.symver`) and no `local:` (the object defines nothing else).
pub fn version_script(plan: &Plan) -> String {
    let mut s = format!("/* @generated by `cargo xtask so` from spec/abi/versions.tsv: {} (D-27.9). */\n", plan.soname);
    let mut prev: Option<&String> = None;
    for v in &plan.nodes {
        match prev {
            Some(p) => s.push_str(&format!("{v} {{ }} {p};\n")),
            None => s.push_str(&format!("{v} {{ }};\n")),
        }
        prev = Some(v);
    }
    s
}

/// A `__<lib>_version_placeholder` name: its body only returns.
fn is_version_placeholder(name: &str) -> bool {
    name.starts_with("__lib") && name.ends_with("_version_placeholder")
}

/// The C source: the hand-written bodies (`body_file`, included), a returning body for each
/// version placeholder, and one `.symver` per export.
pub fn c_source(plan: &Plan, body_file: Option<(&Path, &str)>) -> Result<String, String> {
    let mut s = format!("/* @generated by `cargo xtask so` from spec/abi/versions.tsv: {} (D-27.9). */\n", plan.soname);
    if let Some((path, _)) = body_file {
        s.push_str(&format!("#include \"{}\"\n", path.display()));
    }
    let mut versions: BTreeMap<&str, Vec<&Export>> = BTreeMap::new();
    for e in &plan.exports {
        versions.entry(e.name.as_str()).or_default().push(e);
    }
    for (name, exports) in &versions {
        let has_body = body_file.is_some_and(|(_, text)| text.contains(&format!("liella_ph_{name}(")));
        if !has_body {
            if !is_version_placeholder(name) {
                let file = body_file.map_or_else(|| "no body file".to_string(), |(p, _)| p.display().to_string());
                return Err(format!("{}: export {name} has no body `liella_ph_{name}` ({file}); versions.tsv lists it", plan.soname));
            }
            s.push_str(&format!("void liella_ph_{name}(void);\nvoid liella_ph_{name}(void) {{}}\n"));
        }
        for (i, e) in exports.iter().enumerate() {
            let at = if e.default { "@@" } else { "@" };
            // `remove` on the last one: the object keeps only the versioned names.
            let remove = if i + 1 == exports.len() { ", remove" } else { "" };
            s.push_str(&format!("__asm__(\".symver liella_ph_{name}, {name}{at}{}{remove}\");\n", e.version));
        }
    }
    Ok(s)
}

/// The size of the NIS+ result object a `result` stub returns: the status (first member, the
/// only one a stub sets), then the object list, the cookie and the four tick counts, all zero.
/// The reference's result object is the same 56 bytes (probe, D-29.6).
pub const NIS_RESULT_SIZE: usize = 56;

/// One row of a failure table (`crates/libc-so/placeholders/<lib>.tsv`, D-29.6): what the stub of
/// `name` does. The kinds are documented in the table's header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stub {
    /// Return the number, `errno` untouched.
    Int(i32),
    /// Return `NULL` with `errno` `EOPNOTSUPP`.
    Null,
    /// Return a new result object with this status (freed by the `free` stub).
    Result(i32),
    /// No effect.
    Void,
    /// Free the pointer given.
    Free,
    /// Return the constant text.
    Text(String),
    /// Copy the text into the caller's buffer `(status, label, buf, len)` and return the buffer.
    TextBuf(String),
    /// Print `<label>: <text>` to stderr `(status, label)`.
    Perror(String),
}

/// The rows of a failure table: `(name, stub)` in table order. Lines starting with `#` and the
/// `symbol` header line are skipped; anything malformed is an error naming the line.
pub fn stub_table(text: &str) -> Result<Vec<(String, Stub)>, String> {
    let mut out: Vec<(String, Stub)> = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if line.starts_with('#') || line.starts_with("symbol\t") || line.is_empty() {
            continue;
        }
        let bad = |why: &str| format!("failure table line {}: {why}: {line:?}", i + 1);
        let [name, kind, value] = line.split('\t').collect::<Vec<_>>()[..] else { return Err(bad("not `symbol<TAB>kind<TAB>value`")) };
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
            return Err(bad("the symbol is not a C name"));
        }
        let num = || value.parse::<i32>().map_err(|_| bad("the value is not a number"));
        let text = || {
            if value.is_empty() || value.bytes().any(|b| !(b' '..=b'~').contains(&b) || b == b'"' || b == b'\\' || b == b'%') {
                Err(bad("the text is empty or has a character other than printable ASCII without `\"`, `\\` and `%`"))
            } else {
                Ok(value.to_string())
            }
        };
        let stub = match kind {
            "int" => Stub::Int(num()?),
            "result" => Stub::Result(num()?),
            "null" | "void" | "free" if value != "-" => return Err(bad("the value of this kind is `-`")),
            "null" => Stub::Null,
            "void" => Stub::Void,
            "free" => Stub::Free,
            "text" => Stub::Text(text()?),
            "textbuf" => Stub::TextBuf(text()?),
            "perror" => Stub::Perror(text()?),
            _ => return Err(bad("unknown kind")),
        };
        if out.iter().any(|(n, _)| n == name) {
            return Err(bad("the symbol has a row already"));
        }
        out.push((name.to_string(), stub));
    }
    Ok(out)
}

/// The C bodies of `plan`'s exports from its failure table `table` (`stub_table` text): one
/// `liella_ph_<name>` per row. Every export must have exactly one row and every row an export,
/// so the table, versions.tsv and the product never drift apart.
pub fn stub_bodies(plan: &Plan, table: &str) -> Result<String, String> {
    let rows = stub_table(table)?;
    let exports: BTreeSet<&str> = plan.exports.iter().map(|e| e.name.as_str()).filter(|n| !is_version_placeholder(n)).collect();
    let named: BTreeSet<&str> = rows.iter().map(|(n, _)| n.as_str()).collect();
    let mut errors = Vec::new();
    for n in exports.difference(&named) {
        errors.push(format!("{}: export {n} has no row in its failure table", plan.soname));
    }
    for n in named.difference(&exports) {
        errors.push(format!("{}: failure table row {n} is not an export of versions.tsv", plan.soname));
    }
    if !errors.is_empty() {
        return Err(errors.join("\n"));
    }
    let mut s = format!("/* @generated by `cargo xtask so` from the failure table of {} (D-29.6). */\n", plan.soname);
    s.push_str("#include <errno.h>\n#include <stddef.h>\n#include <stdio.h>\n#include <stdlib.h>\n#include <string.h>\n\n");
    if rows.iter().any(|(_, k)| matches!(k, Stub::Result(_))) {
        s.push_str(&format!("/* A result object: status first, everything after it zero; freed by the `free` stub. */\nstatic void *liella_ph_result(int status) {{\n    int *r = calloc(1, {NIS_RESULT_SIZE});\n    if (r != NULL)\n        *r = status;\n    return r;\n}}\n\n"));
    }
    for (name, stub) in &rows {
        let f = format!("liella_ph_{name}");
        let (proto, body) = match stub {
            Stub::Int(v) => (format!("int {f}(void)"), format!("    return {v};\n")),
            Stub::Null => (format!("void *{f}(void)"), "    errno = EOPNOTSUPP;\n    return NULL;\n".to_string()),
            Stub::Result(v) => (format!("void *{f}(void)"), format!("    return liella_ph_result({v});\n")),
            Stub::Void => (format!("void {f}(void)"), String::new()),
            Stub::Free => (format!("void {f}(void *p)"), "    free(p);\n".to_string()),
            Stub::Text(t) => (format!("const char *{f}(void)"), format!("    return \"{t}\";\n")),
            Stub::TextBuf(t) => (format!("char *{f}(int status, const char *label, char *buf, size_t len)"), format!(
                "    (void)status;\n    (void)label;\n    if (buf == NULL || len == 0)\n        return NULL;\n    size_t n = sizeof \"{t}\" - 1 < len - 1 ? sizeof \"{t}\" - 1 : len - 1;\n    memcpy(buf, \"{t}\", n);\n    buf[n] = '\\0';\n    return buf;\n")),
            Stub::Perror(t) => (format!("void {f}(int status, const char *label)"), format!(
                "    (void)status;\n    if (label != NULL && *label != '\\0')\n        fprintf(stderr, \"%s: {t}\\n\", label);\n    else\n        fputs(\"{t}\\n\", stderr);\n")),
        };
        s.push_str(&format!("{proto};\n{proto} {{\n{body}}}\n"));
    }
    Ok(s)
}

/// The failure table of library `soname`, if it has one (D-29.6).
fn stub_table_path(root: &Path, soname: &str) -> std::path::PathBuf {
    body_path(root, soname).with_extension("tsv")
}

/// The hand-written bodies of library `soname`, if it has any.
fn body_path(root: &Path, soname: &str) -> std::path::PathBuf {
    let stem = soname.split(".so").next().unwrap_or(soname);
    root.join("crates/libc-so/placeholders").join(format!("{stem}.c"))
}

fn readelf(args: &[&str], so: &Path) -> Result<String, String> {
    let out = Command::new("readelf").args(args).arg(so).output().map_err(|e| format!("readelf: {e}"))?;
    if !out.status.success() {
        return Err(format!("readelf {} {} failed", args.join(" "), so.display()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The version definitions in `readelf -V` text: `(name, base?)`.
fn verdefs(v: &str) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    let mut inside = false;
    for line in v.lines() {
        if line.contains("Version definition section") {
            inside = true;
        } else if line.contains("section '") {
            inside = false;
        } else if inside && line.contains("Rev:") {
            if let Some((_, name)) = line.split_once("Name: ") {
                out.push((name.trim().to_string(), line.contains("BASE")));
            }
        }
    }
    out
}

/// No `GLIBC_PRIVATE` version definition or versioned export in a shared object (R14-12), from its
/// `readelf -V` and `readelf --dyn-syms -W` text.
pub fn private_errors(so: &str, readelf_v: &str, dynsyms: &str) -> Vec<String> {
    let mut errors = Vec::new();
    if verdefs(readelf_v).iter().any(|(n, _)| n == PRIVATE_NODE) {
        errors.push(format!("{so} defines the {PRIVATE_NODE} version node (R14-12: no {PRIVATE_NODE} exports; the private interface is LIELLA_PRIVATE)"));
    }
    let nodes: BTreeSet<String> = verdefs(readelf_v).into_iter().map(|(n, _)| n).collect();
    for (name, version, _) in defined_dynsyms(dynsyms, &nodes) {
        if version.as_deref() == Some(PRIVATE_NODE) {
            errors.push(format!("{so} exports {name}@{PRIVATE_NODE} (R14-12: no {PRIVATE_NODE} exports)"));
        }
    }
    errors
}

/// The defined dynamic symbols in `readelf --dyn-syms -W` text, the symbols of the version
/// definitions `nodes` left out: `(name, version, default?)`.
fn defined_dynsyms(dynsyms: &str, nodes: &BTreeSet<String>) -> Vec<(String, Option<String>, bool)> {
    let mut out = Vec::new();
    for line in dynsyms.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        // Num: Value Size Type Bind Vis Ndx Name[@[@]Version]
        let [num, _, _, _, _, _, ndx, name, ..] = f.as_slice() else { continue };
        if num.strip_suffix(':').is_none_or(|n| n.parse::<u32>().is_err()) || *ndx == "UND" {
            continue;
        }
        let (n, v, d) = match name.split_once('@') {
            Some((n, v)) => (n, Some(v.trim_start_matches('@').to_string()), v.starts_with('@')),
            None => (*name, None, false),
        };
        // The symbol ld writes for each version definition: absolute, unversioned, named as the
        // version.
        if *ndx == "ABS" && v.is_none() && nodes.contains(n) {
            continue;
        }
        out.push((n.to_string(), v, d));
    }
    out
}

/// Everything wrong with built placeholder `soname` against versions.tsv text `table`, from its
/// `readelf -d`, `readelf -V` and `readelf --dyn-syms -W` text. Empty when it is right.
pub fn product_errors(table: &str, soname: &str, dynamic: &str, readelf_v: &str, dynsyms: &str) -> Vec<String> {
    let mut errors = private_errors(soname, readelf_v, dynsyms);
    if !dynamic.contains(&format!("Library soname: [{soname}]")) {
        errors.push(format!("{soname}: DT_SONAME is not {soname}"));
    }
    if !dynamic.contains(&format!("Shared library: [{LIBC}]")) {
        errors.push(format!("{soname}: no DT_NEEDED {LIBC} (the placeholders forward to it)"));
    }
    // Every row of the table, by name and version: which libraries have it, at which binding.
    let mut owners: BTreeMap<(&str, &str), Vec<(&str, bool)>> = BTreeMap::new();
    let mut libc_names = BTreeSet::new();
    let mut want_nodes = BTreeSet::new();
    let mut want = BTreeSet::new();
    for (lib, sym, ver, binding) in rows(table) {
        owners.entry((sym, ver)).or_default().push((lib, binding == "default"));
        if lib == LIBC {
            libc_names.insert(sym);
        }
    }
    for (lib, sym, ver, binding) in rows(table) {
        if lib == soname && ver != PRIVATE_NODE {
            want_nodes.insert(ver.to_string());
            if !libc_names.contains(sym) {
                want.insert(Export { name: sym.to_string(), version: ver.to_string(), default: binding == "default" });
            }
        }
    }
    let defs = verdefs(readelf_v);
    match defs.iter().filter(|(_, base)| *base).map(|(n, _)| n.as_str()).collect::<Vec<_>>().as_slice() {
        [b] if *b == soname => {}
        other => errors.push(format!("{soname}: base version definition {other:?}, expected [{soname:?}]")),
    }
    let got_nodes: BTreeSet<String> = defs.iter().filter(|(_, base)| !*base).map(|(n, _)| n.clone()).collect();
    for n in got_nodes.difference(&want_nodes) {
        errors.push(format!("{soname}: defines version {n}, which versions.tsv gives none of its rows"));
    }
    for n in want_nodes.difference(&got_nodes) {
        errors.push(format!("{soname}: version {n} of versions.tsv is not defined"));
    }
    let mut got = BTreeSet::new();
    let all_nodes: BTreeSet<String> = defs.iter().map(|(n, _)| n.clone()).collect();
    for (name, version, default) in defined_dynsyms(dynsyms, &all_nodes) {
        let Some(version) = version else {
            errors.push(format!("{soname}: exports {name} unversioned; versions.tsv gives every export a version"));
            continue;
        };
        let at = if default { "@@" } else { "@" };
        let e = Export { name: name.clone(), version: version.clone(), default };
        match owners.get(&(name.as_str(), version.as_str())) {
            None => errors.push(format!("{soname}: exports {name}{at}{version}, which versions.tsv does not list for any library")),
            Some(libs) if !libs.iter().any(|(l, _)| *l == soname) => {
                let names: Vec<&str> = libs.iter().map(|(l, _)| *l).collect();
                errors.push(format!("{soname}: exports {name}{at}{version}, which versions.tsv assigns to {}, not {soname}", names.join(", ")));
            }
            Some(libs) if !libs.iter().any(|(l, d)| *l == soname && *d == default) => {
                errors.push(format!("{soname}: exports {name}{at}{version}, binding differs from versions.tsv"));
            }
            Some(_) if libc_names.contains(name.as_str()) => {
                errors.push(format!("{soname}: exports {name}{at}{version}, which {LIBC} provides (reached through DT_NEEDED, never defined twice)"));
            }
            Some(_) => {}
        }
        got.insert(e);
    }
    for e in want.difference(&got) {
        errors.push(format!("{soname}: does not export {}{}{} (versions.tsv)", e.name, if e.default { "@@" } else { "@" }, e.version));
    }
    errors
}

/// Build every placeholder into `release` (linked against `release/libc.so.6`), check each, and
/// write the empty archives. `work` holds the generated sources. One summary line.
pub fn build(root: &Path, release: &Path, work: &Path, cc: &str, ar: &str) -> Result<String, String> {
    let table = fs::read_to_string(root.join("spec/abi/versions.tsv")).map_err(|e| format!("versions.tsv: {e}"))?;
    fs::create_dir_all(work).map_err(|e| e.to_string())?;
    let mut summary = Vec::new();
    for soname in LIBRARIES {
        let plan = plan(&table, soname)?;
        // A failure table (D-29.6) gives the bodies; otherwise the hand-written file, if any.
        let (bodies, body_text) = match fs::read_to_string(stub_table_path(root, soname)) {
            Ok(table) => {
                let text = stub_bodies(&plan, &table)?;
                let p = work.join(format!("{soname}.bodies.c"));
                fs::write(&p, &text).map_err(|e| e.to_string())?;
                (p, Some(text))
            }
            Err(_) => {
                let p = body_path(root, soname);
                let text = fs::read_to_string(&p).ok();
                (p, text)
            }
        };
        let src = c_source(&plan, body_text.as_deref().map(|t| (bodies.as_path(), t)))?;
        let c = work.join(format!("{soname}.c"));
        let map = work.join(format!("{soname}.map"));
        fs::write(&c, src).map_err(|e| e.to_string())?;
        fs::write(&map, version_script(&plan)).map_err(|e| e.to_string())?;
        let dst = release.join(soname);
        let tmp = crate::so::tmp_name(&dst);
        crate::so::run(Command::new(cc)
            .args(["-shared", "-nostdlib", "-fPIC", "-O2", "-Wl,-z,now", "-Wl,-z,relro", "-Wl,--hash-style=gnu"])
            .arg(format!("-Wl,-soname,{soname}"))
            .arg(format!("-Wl,--version-script={}", map.display()))
            .arg(&c)
            .arg("-Wl,--no-as-needed")
            .arg(release.join(LIBC))
            .arg("-o")
            .arg(&tmp))?;
        let errors = product_errors(&table, soname, &readelf(&["-d"], &tmp)?, &readelf(&["-V"], &tmp)?, &readelf(&["--dyn-syms", "-W"], &tmp)?);
        if !errors.is_empty() {
            let _ = fs::remove_file(&tmp);
            return Err(format!("placeholder {soname} differs from spec/abi/versions.tsv (D-27.9):\n{}", errors.join("\n")));
        }
        crate::so::publish(&tmp, &dst)?;
        summary.push(format!("{soname} ({} nodes, {} exports)", plan.nodes.len(), plan.exports.len()));
    }
    for a in ARCHIVES {
        crate::so::empty_archive(ar, &release.join(a))?;
    }
    Ok(format!("placeholders {}, each checked against spec/abi/versions.tsv; empty {}", summary.join(", "), ARCHIVES.join(", ")))
}

/// No shared product in `release` defines or exports a `GLIBC_PRIVATE` version (R14-12).
pub fn check_no_private(release: &Path) -> Result<(), String> {
    let mut errors = Vec::new();
    for so in ["libc.so.6", "libm.so.6"].into_iter().chain(LIBRARIES) {
        let p = release.join(so);
        errors.extend(private_errors(so, &readelf(&["-V"], &p)?, &readelf(&["--dyn-syms", "-W"], &p)?));
    }
    if errors.is_empty() { Ok(()) } else { Err(errors.join("\n")) }
}

#[cfg(test)]
mod tests {
    use super::*;

    // no spec clause: the generator and the check over a small table (D-27.9, R14-12).
    const TABLE: &str = "library\tsymbol\tversion\tbinding\ttype\tscope\tsize\n\
        libfoo.so.1\t__libfoo_version_placeholder\tV_1.10\tcompat\tFUNC\tGLOBAL\t\n\
        libfoo.so.1\t__libfoo_version_placeholder\tV_1.2\tcompat\tFUNC\tGLOBAL\t\n\
        libfoo.so.1\tbar\tV_1.2\tcompat\tFUNC\tGLOBAL\t\n\
        libfoo.so.1\tbaz\tGLIBC_PRIVATE\tdefault\tFUNC\tGLOBAL\t\n\
        libc.so.6\tbar\tV_1.2\tdefault\tFUNC\tGLOBAL\t\n\
        libother.so.1\tqux\tV_1.2\tcompat\tFUNC\tGLOBAL\t\n";

    fn dynsyms(lines: &[&str]) -> String {
        let mut s = String::from("Symbol table '.dynsym' contains 5 entries:\n   Num:    Value          Size Type    Bind   Vis      Ndx Name\n");
        s.push_str("     0: 0000000000000000     0 NOTYPE  LOCAL  DEFAULT  UND \n");
        s.push_str("     1: 0000000000000000     0 FUNC    GLOBAL DEFAULT  UND __errno_location@V_1.2 (3)\n");
        s.push_str("     2: 0000000000000000     0 OBJECT  GLOBAL DEFAULT  ABS V_1.2\n");
        for (i, l) in lines.iter().enumerate() {
            s.push_str(&format!("     {}: 0000000000001100     1 FUNC    GLOBAL DEFAULT   7 {l}\n", i + 3));
        }
        s
    }

    fn verdef(nodes: &[&str]) -> String {
        let mut s = String::from("Version definition section '.gnu.version_d' contains 3 entries:\n  Addr: 0x0  Offset: 0x0  Link: 3 (.dynstr)\n");
        s.push_str("  000000: Rev: 1  Flags: BASE  Index: 1  Cnt: 1  Name: libfoo.so.1\n");
        for (i, n) in nodes.iter().enumerate() {
            s.push_str(&format!("  0x001c: Rev: 1  Flags: none  Index: {}  Cnt: 1  Name: {n}\n", i + 2));
        }
        s.push_str("\nVersion needs section '.gnu.version_r' contains 1 entry:\n  000000: Version: 1  File: libc.so.6  Cnt: 1\n  0x0010:   Name: V_1.2  Flags: none  Version: 3\n");
        s
    }

    const DYNAMIC: &str = " 0x0000000000000001 (NEEDED)             Shared library: [libc.so.6]\n 0x000000000000000e (SONAME)             Library soname: [libfoo.so.1]\n";

    fn good() -> (String, String) {
        (verdef(&["V_1.2", "V_1.10"]), dynsyms(&["__libfoo_version_placeholder@V_1.2", "__libfoo_version_placeholder@V_1.10"]))
    }

    #[test]
    fn plan_reads_the_table() {
        let p = plan(TABLE, "libfoo.so.1").unwrap();
        assert_eq!(p.nodes, ["V_1.2", "V_1.10"], "numeric order, no private node");
        let names: Vec<String> = p.exports.iter().map(|e| format!("{}@{}", e.name, e.version)).collect();
        assert_eq!(names, ["__libfoo_version_placeholder@V_1.10", "__libfoo_version_placeholder@V_1.2"], "bar is libc.so.6's");
        assert_eq!(version_script(&p).lines().skip(1).collect::<Vec<_>>(), ["V_1.2 { };", "V_1.10 { } V_1.2;"]);
        let c = c_source(&p, None).unwrap();
        assert!(c.contains("liella_ph___libfoo_version_placeholder, __libfoo_version_placeholder@V_1.2, remove"), "{c}");
    }

    #[test]
    fn a_body_is_required_except_for_version_placeholders() {
        let table = format!("{TABLE}libfoo.so.1\tquux\tV_1.2\tcompat\tFUNC\tGLOBAL\t\n");
        let p = plan(&table, "libfoo.so.1").unwrap();
        assert!(c_source(&p, None).unwrap_err().contains("quux has no body"));
        let body = "void *liella_ph_quux(void) { return 0; }\n";
        assert!(c_source(&p, Some((Path::new("b.c"), body))).unwrap().contains("#include \"b.c\""));
    }

    #[test]
    fn a_failure_table_gives_one_body_per_export() {
        // no spec clause: the failure table of a placeholder (D-29.6).
        let table = format!("{TABLE}libfoo.so.1\tquux\tV_1.2\tcompat\tFUNC\tGLOBAL\t\nlibfoo.so.1\tcorge\tV_1.2\tcompat\tFUNC\tGLOBAL\t\n");
        let p = plan(&table, "libfoo.so.1").unwrap();
        let rows = "# comment\nsymbol\tkind\tvalue\nquux\tresult\t41\ncorge\tnull\t-\n";
        let c = stub_bodies(&p, rows).unwrap();
        assert!(c.contains("void *liella_ph_quux(void) {\n    return liella_ph_result(41);\n}"), "{c}");
        assert!(c.contains("void *liella_ph_corge(void) {\n    errno = EOPNOTSUPP;\n    return NULL;\n}"), "{c}");
        assert!(c.contains(&format!("calloc(1, {NIS_RESULT_SIZE})")), "{c}");
        assert!(c_source(&p, Some((Path::new("b.c"), &c))).is_ok());
        assert!(stub_bodies(&p, "quux\tint\t6\n").unwrap_err().contains("export corge has no row"));
        assert!(stub_bodies(&p, "quux\tint\t6\ncorge\tvoid\t-\nbar\tvoid\t-\n").unwrap_err().contains("row bar is not an export"));
        assert!(stub_table("quux\tint\tsix\n").unwrap_err().contains("not a number"));
        assert!(stub_table("quux\tnull\t0\n").unwrap_err().contains("value of this kind is `-`"));
        assert!(stub_table("quux\ttext\t100%\n").unwrap_err().contains("printable ASCII"));
        assert!(stub_table("quux\tint\t1\nquux\tint\t2\n").unwrap_err().contains("has a row already"));
    }

    #[test]
    fn the_check_accepts_the_right_product() {
        let (v, d) = good();
        assert_eq!(product_errors(TABLE, "libfoo.so.1", DYNAMIC, &v, &d), Vec::<String>::new());
    }

    #[test]
    fn the_check_names_a_wrong_library_export() {
        let (v, _) = good();
        let d = dynsyms(&["__libfoo_version_placeholder@V_1.2", "__libfoo_version_placeholder@V_1.10", "qux@V_1.2"]);
        let e = product_errors(TABLE, "libfoo.so.1", DYNAMIC, &v, &d);
        assert_eq!(e, ["libfoo.so.1: exports qux@V_1.2, which versions.tsv assigns to libother.so.1, not libfoo.so.1"]);
    }

    #[test]
    fn the_check_refuses_private_nodes_and_exports() {
        let v = verdef(&["V_1.2", "V_1.10", "GLIBC_PRIVATE"]);
        let d = dynsyms(&["__libfoo_version_placeholder@V_1.2", "__libfoo_version_placeholder@V_1.10", "baz@@GLIBC_PRIVATE"]);
        let e = product_errors(TABLE, "libfoo.so.1", DYNAMIC, &v, &d);
        assert!(e.iter().any(|m| m.contains("defines the GLIBC_PRIVATE version node")), "{e:?}");
        assert!(e.iter().any(|m| m.contains("exports baz@GLIBC_PRIVATE")), "{e:?}");
    }

    #[test]
    fn the_check_refuses_libc_names_unversioned_names_and_gaps() {
        let (v, _) = good();
        let d = dynsyms(&["__libfoo_version_placeholder@V_1.2", "bar@V_1.2", "liella_ph_bar"]);
        let e = product_errors(TABLE, "libfoo.so.1", DYNAMIC, &v, &d);
        assert!(e.iter().any(|m| m.contains("bar@V_1.2, which libc.so.6 provides")), "{e:?}");
        assert!(e.iter().any(|m| m.contains("exports liella_ph_bar unversioned")), "{e:?}");
        assert!(e.iter().any(|m| m.contains("does not export __libfoo_version_placeholder@V_1.10")), "{e:?}");
        let no_needed = " 0x000000000000000e (SONAME)             Library soname: [libfoo.so.1]\n";
        assert!(product_errors(TABLE, "libfoo.so.1", no_needed, &v, &good().1).iter().any(|m| m.contains("no DT_NEEDED libc.so.6")));
        let missing_node = verdef(&["V_1.2"]);
        assert!(product_errors(TABLE, "libfoo.so.1", DYNAMIC, &missing_node, &good().1).iter().any(|m| m.contains("version V_1.10 of versions.tsv is not defined")));
    }
}
