//! `cargo xtask native-gen [--check]`: the native C ABI's header (D-X.10).
//!
//! `spec/abi/native.tsv` is the interface's single description. It is written by hand, one row
//! per item, with the columns `kind name node type detail doc`:
//!
//! | kind     | name                  | type                         | detail                        |
//! |----------|-----------------------|------------------------------|-------------------------------|
//! | `const`  | `LIELLA_O_CREAT`      | `uint32_t`, `int32_t`, `size_t` | the value, as C writes it  |
//! | `struct` | `liella_stat`         | the size in bytes            | the alignment                 |
//! | `field`  | `liella_stat.dev`     | the C type (`uint8_t[16]`)   | the offset                    |
//! | `fn`     | `liella_fd_read`      | the C return type (`noreturn void`) | the C parameters       |
//!
//! `node` is the version node that introduced the item (`LIELLA_NATIVE_1`, ...); a published
//! node never changes. From the table:
//! * this command writes `include/liella/native.h`: every constant, structure (with its size,
//!   alignment and offsets asserted) and function, grouped by node;
//! * `--check` fails if the header is stale, if a structure has implicit padding or a size that
//!   disagrees with its fields, or if the exports in `crates/boundary/src/native` differ from
//!   the `fn` rows in name, parameter names or C types (the Rust types are mapped to C);
//!   likewise the Rust facade's declarations (`crates/native/src/ffi.rs`, which must declare
//!   every `fn` row), and the facade's constants with literal values (`O_CREAT` in any module,
//!   or `ABBR_CAP` in module `tz`, stands for `LIELLA_O_CREAT` or `LIELLA_TZ_ABBR_CAP`);
//! * `cargo xtask so` places the `fn` rows in their nodes of `libc.so.6` (`symbols`).
//!
//! The Rust structures and constants are compared with the table by
//! `crates/boundary/tests/native_v1.rs`; the C compiler checks the header's assertions when
//! the conformance program `conformance/c/native_v1.c` is built.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

const TABLE: &str = "spec/abi/native.tsv";
const HEADER: &str = "include/liella/native.h";

/// One row of the table.
pub struct Row {
    pub kind: String,
    pub name: String,
    pub node: String,
    pub ty: String,
    pub detail: String,
    pub doc: String,
}

pub fn table(root: &Path) -> Result<Vec<Row>, String> {
    let text = fs::read_to_string(root.join(TABLE)).map_err(|e| format!("{TABLE}: {e}"))?;
    let mut rows = Vec::new();
    for (n, line) in text.lines().enumerate().skip(1) {
        let f: Vec<&str> = line.split('\t').collect();
        let [kind, name, node, ty, detail, doc] = f.as_slice() else {
            return Err(format!("{TABLE}:{}: expected 6 columns", n + 1));
        };
        if !["const", "struct", "field", "fn"].contains(kind) {
            return Err(format!("{TABLE}:{}: unknown kind `{kind}`", n + 1));
        }
        if node.strip_prefix("LIELLA_NATIVE_").and_then(|v| v.parse::<u32>().ok()).is_none() {
            return Err(format!("{TABLE}:{}: node `{node}` is not LIELLA_NATIVE_<n>", n + 1));
        }
        rows.push(Row { kind: kind.to_string(), name: name.to_string(), node: node.to_string(), ty: ty.to_string(), detail: detail.to_string(), doc: doc.to_string() });
    }
    Ok(rows)
}

/// The exported functions and their version nodes, for the version script.
pub fn symbols(root: &Path) -> Result<Vec<(String, String)>, String> {
    Ok(table(root)?.into_iter().filter(|r| r.kind == "fn").map(|r| (r.name, r.node)).collect())
}

/// The version nodes in order (`LIELLA_NATIVE_1` first).
pub fn node_order(nodes: &BTreeSet<String>) -> Vec<String> {
    let mut v: Vec<String> = nodes.iter().cloned().collect();
    v.sort_by_key(|n| n.strip_prefix("LIELLA_NATIVE_").and_then(|x| x.parse::<u32>().ok()).unwrap_or(u32::MAX));
    v
}

/// `text` as a C comment of lines of at most about 100 columns (`/* ...`, ` * ...`, `*/`).
fn comment(text: &str) -> String {
    let mut lines: Vec<String> = vec![String::from("/*")];
    for w in text.split_whitespace() {
        let line = lines.last().map_or(0, String::len);
        if line > 2 && line + 1 + w.len() > 96 {
            lines.push(String::from(" *"));
        }
        if let Some(l) = lines.last_mut() {
            l.push(' ');
            l.push_str(w);
        }
    }
    let mut out = lines.join("\n");
    out.push_str(" */\n");
    out
}

/// The size of a field type, given the structures' sizes.
fn type_size(ty: &str, structs: &BTreeMap<String, (u64, u64)>) -> Option<(u64, u64)> {
    let scalar = |t: &str| -> Option<u64> {
        match t {
            "int8_t" | "uint8_t" => Some(1),
            "int16_t" | "uint16_t" => Some(2),
            "int32_t" | "uint32_t" => Some(4),
            "int64_t" | "uint64_t" | "size_t" => Some(8),
            _ => None,
        }
    };
    if let Some((base, rest)) = ty.split_once('[') {
        let n: u64 = rest.strip_suffix(']')?.parse().ok()?;
        let s = scalar(base)?;
        return Some((s * n, s));
    }
    if let Some(s) = ty.strip_prefix("struct ") {
        return structs.get(s).copied();
    }
    scalar(ty).map(|s| (s, s))
}

/// Check the structures: fields in order, without implicit padding, filling the size.
fn check_layouts(rows: &[Row]) -> Vec<String> {
    let mut errors = Vec::new();
    let mut structs: BTreeMap<String, (u64, u64)> = BTreeMap::new();
    let mut fields: BTreeMap<String, Vec<&Row>> = BTreeMap::new();
    for r in rows {
        match r.kind.as_str() {
            "struct" => match (r.ty.parse::<u64>(), r.detail.parse::<u64>()) {
                (Ok(size), Ok(align)) if align.is_power_of_two() && size % align == 0 => {
                    structs.insert(r.name.clone(), (size, align));
                }
                _ => errors.push(format!("{TABLE}: struct {}: size {} and alignment {} are not a valid pair", r.name, r.ty, r.detail)),
            },
            "field" => match r.name.split_once('.') {
                Some((s, _)) => fields.entry(s.to_string()).or_default().push(r),
                None => errors.push(format!("{TABLE}: field {} is not <struct>.<field>", r.name)),
            },
            _ => {}
        }
    }
    for (s, (size, align)) in &structs {
        let mut at = 0u64;
        let mut max_align = 1u64;
        for f in fields.get(s).map(Vec::as_slice).unwrap_or(&[]) {
            let Some((fsize, falign)) = type_size(&f.ty, &structs) else {
                errors.push(format!("{TABLE}: field {}: unknown type `{}`", f.name, f.ty));
                continue;
            };
            let off: u64 = f.detail.parse().unwrap_or(u64::MAX);
            if off != at {
                errors.push(format!("{TABLE}: field {} at offset {}, expected {at} (fields in order, no padding)", f.name, f.detail));
            }
            if off % falign != 0 {
                errors.push(format!("{TABLE}: field {} is misaligned", f.name));
            }
            max_align = max_align.max(falign);
            at = off.saturating_add(fsize);
        }
        if at != *size || max_align != *align {
            errors.push(format!("{TABLE}: struct {s}: fields give size {at} and alignment {max_align}, the row says {size} and {align}"));
        }
    }
    for s in fields.keys() {
        if !structs.contains_key(s) {
            errors.push(format!("{TABLE}: fields of unknown struct {s}"));
        }
    }
    errors
}

/// The C spelling of a constant's value.
fn const_value(ty: &str, v: &str) -> String {
    match ty {
        "uint32_t" => format!("UINT32_C({v})"),
        "int32_t" => format!("INT32_C({v})"),
        "uint64_t" => format!("UINT64_C({v})"),
        "int64_t" => format!("INT64_C({v})"),
        _ => format!("(({ty}){v})"),
    }
}

/// `type name` with C's pointer spacing (`uint8_t *buf`).
fn declarator(ty: &str, name: &str) -> String {
    if ty.ends_with('*') { format!("{ty}{name}") } else { format!("{ty} {name}") }
}

const PREAMBLE: &str = "\
/* @generated by `cargo xtask native-gen` from spec/abi/native.tsv. Do not edit. */

/*
 * liella/native.h: Liella's native C ABI (implementation-plan.md, decision D-X.10).
 *
 * The stateful services of the C library (heap, environment, file descriptors and the file
 * system, clocks, the current time zone and locale, process arguments and exit) for programs
 * that are not written against POSIX's conventions. They reach the same state as the POSIX
 * functions of the same library: one heap, one environment, one set of exit handlers.
 *
 * Conventions (every version node follows them):
 *  - Symbols are `liella_<area>_<name>`, each in one version node (LIELLA_NATIVE_1, ...) of
 *    libc.so.6 and defined in libc.a. A published node never changes; additions go in a new
 *    node.
 *  - Integer types are fixed-width; lengths and indexes are size_t. Descriptors are int32_t;
 *    flags, modes and `whence` are uint32_t with the kernel's values (the constants below).
 *  - Byte slices are a pointer and a length, never NUL-terminated. NULL with length 0 is the
 *    empty slice; NULL with a non-zero length is -EFAULT. A path or stored text must not
 *    contain a NUL (-EINVAL); a path must be shorter than PATH_MAX (-ENAMETOOLONG).
 *  - Results are int64_t: the value (never negative) on success, the Linux error number
 *    negated on failure (-ENOENT). errno is never read or written.
 *  - Text output goes into a caller buffer: the result is the text's full length, and the text
 *    is copied only if it fits whole. A result above the buffer length means: call again with a
 *    buffer of that many bytes. Nothing partial is delivered.
 *  - Structures are written through explicit out-parameters (NULL: -EFAULT). They have natural
 *    alignment and no implicit padding; their layouts are asserted below.
 *  - The heap functions mirror an allocator interface instead: an address, or NULL when no
 *    block can be had. Blocks are interchangeable with malloc/free. Misusing a block (an invalid
 *    or double free) is a diagnostic and abort.
 *  - None of these functions is a cancellation point.
 */

#ifndef LIELLA_NATIVE_H
#define LIELLA_NATIVE_H 1

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
#define LIELLA_STATIC_ASSERT(c, m) static_assert(c, m)
#define LIELLA_ALIGNOF(t) alignof(t)
#else
#define LIELLA_STATIC_ASSERT(c, m) _Static_assert(c, m)
#define LIELLA_ALIGNOF(t) _Alignof(t)
#endif

#if defined(__GNUC__) || defined(__clang__)
#define LIELLA_NORETURN __attribute__((__noreturn__))
#else
#define LIELLA_NORETURN
#endif

#ifdef __cplusplus
extern \"C\" {
#endif
";

const POSTAMBLE: &str = "
#ifdef __cplusplus
}
#endif

#endif /* LIELLA_NATIVE_H */
";

/// The header for `rows`.
pub fn header(rows: &[Row]) -> String {
    let nodes: BTreeSet<String> = rows.iter().map(|r| r.node.clone()).collect();
    let mut h = String::from(PREAMBLE);
    for node in node_order(&nodes) {
        let this = &node;
        let of = |kind: &'static str| rows.iter().filter(move |r| r.kind == kind && r.node == *this);
        h.push_str(&format!("\n/* ======================================================================= {node} */\n"));
        h.push_str("\n/* Constants. */\n\n");
        for c in of("const") {
            h.push_str(&comment(&c.doc));
            h.push_str(&format!("#define {} {}\n", c.name, const_value(&c.ty, &c.detail)));
        }
        for s in of("struct") {
            h.push('\n');
            h.push_str(&comment(&format!("{} Size {} bytes, alignment {}.", s.doc, s.ty, s.detail)));
            h.push_str(&format!("struct {} {{\n", s.name));
            let prefix = format!("{}.", s.name);
            let fields: Vec<&Row> = rows.iter().filter(|r| r.kind == "field" && r.name.starts_with(&prefix)).collect();
            for f in &fields {
                let fname = &f.name[prefix.len()..];
                let decl = match f.ty.split_once('[') {
                    Some((base, rest)) => format!("{base} {fname}[{rest}"),
                    None => declarator(&f.ty, fname),
                };
                h.push_str(&format!("    {decl}; /* {} */\n", f.doc));
            }
            h.push_str("};\n");
            h.push_str(&format!("LIELLA_STATIC_ASSERT(sizeof(struct {0}) == {1}, \"struct {0}: size\");\n", s.name, s.ty));
            h.push_str(&format!("LIELLA_STATIC_ASSERT(LIELLA_ALIGNOF(struct {0}) == {1}, \"struct {0}: alignment\");\n", s.name, s.detail));
            for f in &fields {
                let fname = &f.name[prefix.len()..];
                h.push_str(&format!("LIELLA_STATIC_ASSERT(offsetof(struct {0}, {fname}) == {1}, \"struct {0}: offset of {fname}\");\n", s.name, f.detail));
            }
        }
        h.push_str("\n/* Functions. */\n");
        for f in of("fn") {
            h.push('\n');
            h.push_str(&comment(&f.doc));
            let ret = match f.ty.strip_prefix("noreturn ") {
                Some(t) => format!("LIELLA_NORETURN {t}"),
                None => f.ty.clone(),
            };
            h.push_str(&format!("{}({});\n", declarator(&ret, &f.name), f.detail));
        }
    }
    h.push_str(POSTAMBLE);
    h
}

/// `LiellaTimespec` -> `liella_timespec`.
fn snake(camel: &str) -> String {
    let mut s = String::new();
    for (i, c) in camel.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                s.push('_');
            }
            s.push(c.to_ascii_lowercase());
        } else {
            s.push(c);
        }
    }
    s
}

/// The C type of a Rust parameter or return type of an export.
fn c_type(rust: &str) -> Option<String> {
    let t = rust.trim();
    if let Some(inner) = t.strip_prefix("Option<&mut ").and_then(|x| x.strip_suffix('>')) {
        return Some(format!("{} *", c_type(inner)?));
    }
    if let Some(inner) = t.strip_prefix("Option<&").and_then(|x| x.strip_suffix('>')) {
        return Some(format!("const {} *", c_type(inner)?));
    }
    if t.starts_with("Liella") {
        return Some(format!("struct {}", snake(t)));
    }
    Some(match t {
        "usize" => "size_t",
        "i64" => "int64_t",
        "u64" => "uint64_t",
        "i32" => "int32_t",
        "u32" => "uint32_t",
        "*const u8" => "const uint8_t *",
        "*mut u8" => "uint8_t *",
        "*const c_void" => "const void *",
        "*mut c_void" => "void *",
        "!" => "noreturn void",
        _ => return None,
    }.to_string())
}

/// A Rust function signature after `fn ` (`name(params) -> ret {` or `... ;`) as (name, C
/// return type, C parameters), with `ctype` mapping each Rust type.
fn signature(sig: &str, ctype: &dyn Fn(&str) -> Option<String>) -> Result<(String, String, String), String> {
    let (name, rest) = sig.split_once('(').ok_or_else(|| format!("cannot parse `{sig}`"))?;
    let (params, tail) = rest.rsplit_once(')').ok_or_else(|| format!("cannot parse `{sig}`"))?;
    let ret = tail.trim().trim_end_matches(['{', ';']).trim();
    let ret = match ret.strip_prefix("->") {
        Some(t) => ctype(t).ok_or_else(|| format!("{name}: return type `{t}` has no C mapping"))?,
        None => "void".to_string(),
    };
    let mut cparams = Vec::new();
    for p in params.split(", ").filter(|p| !p.trim().is_empty()) {
        let (pname, pty) = p.split_once(": ").ok_or_else(|| format!("{name}: cannot parse parameter `{p}`"))?;
        let cty = ctype(pty).ok_or_else(|| format!("{name}: parameter type `{pty}` has no C mapping"))?;
        cparams.push(declarator(&cty, pname.trim()));
    }
    let cparams = if cparams.is_empty() { "void".to_string() } else { cparams.join(", ") };
    Ok((name.trim().to_string(), ret, cparams))
}

/// The exports of `crates/boundary/src/native` as (name, C return type, C parameters).
fn rust_exports(root: &Path) -> Result<Vec<(String, String, String)>, String> {
    let dir = root.join("crates/boundary/src/native");
    let mut files: Vec<_> = fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?.flatten().map(|e| e.path()).collect();
    files.sort();
    let mut out = Vec::new();
    for f in files {
        let src = fs::read_to_string(&f).map_err(|e| e.to_string())?;
        for line in src.lines() {
            let Some(sig) = line.trim().strip_prefix("pub extern \"C\" fn ") else { continue };
            out.push(signature(sig, &c_type).map_err(|e| format!("{}: {e}", f.display()))?);
        }
    }
    Ok(out)
}

/// The facade `liella-native` (D-X.10, stage 2) declares the C ABI in this file.
const FACADE: &str = "crates/native/src/ffi.rs";

/// The C type of a parameter or return type of the facade's declarations: its structures are
/// passed by reference, under their own Rust names.
fn facade_c_type(rust: &str) -> Option<String> {
    match rust.trim() {
        "&mut Timespec" => Some("struct liella_timespec *".to_string()),
        "&mut Stat" => Some("struct liella_stat *".to_string()),
        "&mut Tm" => Some("struct liella_tm *".to_string()),
        "&mut NscacheInfo" => Some("struct liella_nscache_info *".to_string()),
        "&mut i64" => Some("int64_t *".to_string()),
        t => c_type(t),
    }
}

/// The facade's declarations (`safe fn liella_...;` in [`FACADE`]) as (name, C return type, C
/// parameters).
fn facade_decls(root: &Path) -> Result<Vec<(String, String, String)>, String> {
    let src = fs::read_to_string(root.join(FACADE)).map_err(|e| format!("{FACADE}: {e}"))?;
    let mut out = Vec::new();
    for line in src.lines() {
        let l = line.trim();
        let Some(sig) = l.strip_prefix("pub(crate) safe fn ").or_else(|| l.strip_prefix("safe fn ")) else { continue };
        out.push(signature(sig, &facade_c_type).map_err(|e| format!("{FACADE}: {e}"))?);
    }
    Ok(out)
}

/// The table's functions against a list of (name, C return type, C parameters): every one
/// present, with the same C signature.
fn compare_fns(rows: &[Row], have: &[(String, String, String)], what: &str) -> Vec<String> {
    let mut errors = Vec::new();
    let want: BTreeMap<&str, (&str, &str)> = rows.iter().filter(|r| r.kind == "fn").map(|r| (r.name.as_str(), (r.ty.as_str(), r.detail.as_str()))).collect();
    let mut seen = BTreeSet::new();
    for (name, ret, params) in have {
        seen.insert(name.as_str());
        match want.get(name.as_str()) {
            None => errors.push(format!("{name}: in {what} but not in {TABLE}")),
            Some((t, p)) => {
                if t != ret || p != params {
                    errors.push(format!("{name}: {what} gives `{ret} ({params})`, {TABLE} says `{t} ({p})`"));
                }
            }
        }
    }
    for name in want.keys() {
        if !seen.contains(name) {
            errors.push(format!("{name}: in {TABLE} but not in {what}"));
        }
    }
    errors
}

/// An integer literal as C (`0x40`, `0170000`, `16`) or Rust (`0o170000`, `1_000`) writes it.
fn int_literal(v: &str) -> Option<u64> {
    let v = v.trim().replace('_', "");
    if let Some(h) = v.strip_prefix("0x") {
        u64::from_str_radix(h, 16).ok()
    } else if let Some(o) = v.strip_prefix("0o") {
        u64::from_str_radix(o, 8).ok()
    } else if v.len() > 1 && v.starts_with('0') {
        u64::from_str_radix(&v[1..], 8).ok()
    } else {
        v.parse().ok()
    }
}

/// The facade's constants with literal values against the table: `NAME` in module `m` stands
/// for `LIELLA_NAME` or `LIELLA_<M>_NAME`. The errors and the number of constants compared.
fn check_facade_consts(root: &Path, rows: &[Row]) -> Result<(Vec<String>, usize), String> {
    let consts: BTreeMap<&str, &str> = rows.iter().filter(|r| r.kind == "const").map(|r| (r.name.as_str(), r.detail.as_str())).collect();
    let dir = root.join("crates/native/src");
    let mut files: Vec<_> = fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?.flatten().map(|e| e.path()).collect();
    files.sort();
    let (mut errors, mut compared) = (Vec::new(), 0);
    for f in files {
        let module = f.file_stem().and_then(|s| s.to_str()).unwrap_or_default().to_ascii_uppercase();
        let src = fs::read_to_string(&f).map_err(|e| e.to_string())?;
        for line in src.lines() {
            let l = line.trim();
            let l = l.strip_prefix("pub ").or_else(|| l.strip_prefix("pub(crate) ")).unwrap_or(l);
            let Some(decl) = l.strip_prefix("const ") else { continue };
            let (Some((name, _)), Some((_, value))) = (decl.split_once(':'), decl.split_once('=')) else { continue };
            let Some(have) = int_literal(value.trim().trim_end_matches(';')) else { continue };
            let name = name.trim();
            let Some((cname, want)) = [format!("LIELLA_{name}"), format!("LIELLA_{module}_{name}")].into_iter().find_map(|c| consts.get(c.as_str()).map(|w| (c.clone(), *w))) else { continue };
            compared += 1;
            if int_literal(want) != Some(have) {
                errors.push(format!("{}: {name} = {have}, but {TABLE} gives {cname} = {want}", f.display()));
            }
        }
    }
    Ok((errors, compared))
}

/// The table's functions against the boundary's exports and the facade's declarations, and
/// its constants against the facade's. The number of facade constants compared.
fn check_exports(root: &Path, rows: &[Row]) -> Result<(Vec<String>, usize), String> {
    let mut errors = compare_fns(rows, &rust_exports(root)?, "crates/boundary/src/native");
    errors.extend(compare_fns(rows, &facade_decls(root)?, FACADE));
    let (more, consts) = check_facade_consts(root, rows)?;
    errors.extend(more);
    Ok((errors, consts))
}

pub fn run(root: &Path, check: bool) -> Result<(), String> {
    let rows = table(root)?;
    let mut errors = check_layouts(&rows);
    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    let unique: BTreeSet<&str> = names.iter().copied().collect();
    if unique.len() != names.len() {
        errors.push(format!("{TABLE}: a name appears twice"));
    }
    let (more, consts) = check_exports(root, &rows)?;
    errors.extend(more);
    if !errors.is_empty() {
        for e in &errors {
            eprintln!("{e}");
        }
        return Err(format!("{} problems in the native interface", errors.len()));
    }
    let text = header(&rows);
    let path = root.join(HEADER);
    let fns = rows.iter().filter(|r| r.kind == "fn").count();
    if check {
        let current = fs::read_to_string(&path).unwrap_or_default();
        if current != text {
            return Err(format!("{HEADER} is stale: run `cargo xtask native-gen`"));
        }
        println!("native-gen: ok ({HEADER} current; {fns} functions match the exports and {FACADE}; {consts} facade constants match)");
    } else {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        fs::write(&path, text).map_err(|e| e.to_string())?;
        println!("native-gen: wrote {HEADER} ({fns} functions)");
    }
    Ok(())
}
