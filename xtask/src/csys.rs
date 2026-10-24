//! `cargo xtask csys-gen [--check]`: generate the C syscall layer's class-*sys* wrappers and
//! their Rust declarations from `csys/syscalls.tsv` (rule K5: header and declarations cannot
//! drift, because both come from one table).
//!
//! Generated files:
//! * `csys/include/liella_sysnr.h`  syscall numbers, copied from the Linux UAPI
//!   `asm/unistd_64.h` (normative reference, OD-2) for the calls in the table;
//! * `csys/include/liella_sys.h`    wrapper declarations;
//! * `csys/src/sys/<area>.c`        one single-statement wrapper per call;
//! * `crates/boundary/src/ffi_sys.rs`  the matching `safe` foreign declarations.
//!
//! With `--check`, nothing is written; the command fails if any file would change.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

mod veneer;

const UAPI: &str = "/usr/include/x86_64-linux-gnu/asm/unistd_64.h";
/// The kernel's generic table, which aarch64 uses (ARCH-SYSCALLS, ledger A-3).
const UAPI_GENERIC: &str = "/usr/include/asm-generic/unistd.h";
/// The switches arm64's UAPI `asm/unistd.h` defines before it includes the generic table
/// (kernel 6.8). Checked against `arch/arm64/include/uapi/asm/unistd.h` when a kernel header
/// tree is installed under /usr/src.
const AARCH64_WANT: &[&str] = &[
    "__ARCH_WANT_RENAMEAT", "__ARCH_WANT_NEW_STAT", "__ARCH_WANT_SET_GET_RLIMIT",
    "__ARCH_WANT_TIME32_SYSCALLS", "__ARCH_WANT_SYS_CLONE3", "__ARCH_WANT_MEMFD_SECRET",
];

/// The table's optional fifth column: how a call exists on aarch64.
#[derive(Debug, PartialEq)]
enum Generic {
    /// `=` (or no column): the same kernel name is in the generic table.
    Same,
    /// `=backwards`: present, with the CONFIG_CLONE_BACKWARDS argument order
    /// (flags, newsp, parent_tid, tls, child_tid); only `clone` uses it.
    Backwards,
    /// `none:<call>`: no number on aarch64; the wrapper is built on `<call>` (a call of the table).
    /// `none:-`: no number and no replacement (x86-only; its export follows the reference, RL-8).
    Absent(Option<String>),
}

struct Call {
    name: String,
    nr: String,
    area: String,
    args: Vec<char>,
    aarch64: Generic,
}

fn generic_column(f: Option<&str>) -> Option<Generic> {
    match f {
        None | Some("=") => Some(Generic::Same),
        Some("=backwards") => Some(Generic::Backwards),
        Some("none:-") => Some(Generic::Absent(None)),
        Some(s) => s.strip_prefix("none:").filter(|r| !r.is_empty()).map(|r| Generic::Absent(Some(r.to_string()))),
    }
}

fn table(root: &Path) -> Result<Vec<Call>, String> {
    let text = fs::read_to_string(root.join("csys/syscalls.tsv")).map_err(|e| format!("syscalls.tsv: {e}"))?;
    let mut out = Vec::new();
    for (n, line) in text.lines().enumerate() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() != 4 && f.len() != 5 {
            return Err(format!("syscalls.tsv:{}: expected 4 or 5 columns", n + 1));
        }
        let aarch64 = generic_column(f.get(4).copied())
            .ok_or_else(|| format!("syscalls.tsv:{}: bad aarch64 column `{}` (=, =backwards, none:<call> or none:-)", n + 1, f[4]))?;
        let args: Vec<char> = if f[3] == "-" { Vec::new() } else { f[3].chars().collect() };
        // `*` as the kernel name: the call number is the first argument (`syscall()`, D-24.5).
        let max = if f[1] == "*" { 7 } else { 6 };
        if args.len() > max || args.iter().any(|c| !"icp".contains(*c)) || (f[1] == "*" && args.first() != Some(&'i')) {
            return Err(format!("syscalls.tsv:{}: bad argument kinds `{}`", n + 1, f[3]));
        }
        out.push(Call { name: f[0].into(), nr: f[1].into(), area: f[2].into(), args, aarch64 });
    }
    Ok(out)
}

fn c_type(k: char) -> &'static str {
    match k { 'c' => "const void *", 'p' => "void *", _ => "long" }
}

fn rust_type(k: char) -> &'static str {
    match k { 'c' => "*const c_void", 'p' => "*mut c_void", _ => "i64" }
}

fn c_params(c: &Call) -> String {
    if c.args.is_empty() {
        return "void".into();
    }
    c.args.iter().enumerate().map(|(i, k)| {
        let t = c_type(*k);
        if t.ends_with('*') { format!("{t}a{i}") } else { format!("{t} a{i}") }
    }).collect::<Vec<_>>().join(", ")
}

fn render(root: &Path) -> Result<BTreeMap<String, String>, String> {
    let calls = table(root)?;
    let uapi = fs::read_to_string(UAPI).map_err(|e| format!("{UAPI}: {e} (install linux-libc-dev)"))?;
    let mut numbers = BTreeMap::new();
    for line in uapi.lines() {
        let mut w = line.split_whitespace();
        if let (Some("#define"), Some(name), Some(val)) = (w.next(), w.next(), w.next()) {
            if let Some(n) = name.strip_prefix("__NR_") {
                if let Ok(v) = val.parse::<u32>() {
                    numbers.insert(n.to_string(), v);
                }
            }
        }
    }
    let mut files = BTreeMap::new();
    let banner = "/* @generated by `cargo xtask csys-gen` from csys/syscalls.tsv. Do not edit. */\n";

    let mut nr = format!("{banner}/* System-call numbers, x86_64 (Linux UAPI asm/unistd_64.h). */\n#ifndef LIELLA_SYSNR_H\n#define LIELLA_SYSNR_H\n\n");
    let mut seen = std::collections::BTreeSet::new();
    // The same numbers for Rust: the cancellation window (D-22.11) makes a system call by number.
    let mut rs = String::from("// @generated by `cargo xtask csys-gen` from csys/syscalls.tsv. Do not edit.\n//! System-call numbers, x86_64 (Linux UAPI asm/unistd_64.h), for the calls of the table.\n#![allow(dead_code, missing_docs)]\n\n");
    for c in calls.iter().filter(|c| c.nr != "*") {
        if seen.insert(c.nr.clone()) {
            let v = numbers.get(&c.nr).ok_or(format!("{} is not in {UAPI}", c.nr))?;
            let _ = writeln!(nr, "#define LIELLA_NR_{} {v}", c.nr);
            let _ = writeln!(rs, "pub(crate) const {}: i64 = {v};", c.nr.to_uppercase());
        }
    }
    nr.push_str("\n#endif\n");
    files.insert("csys/include/liella_sysnr.h".to_string(), nr);
    files.insert("crates/boundary/src/nr.rs".to_string(), rs);

    let mut decl = format!("{banner}/* Class sys: one wrapper per system call; each returns the raw kernel result. */\n#ifndef LIELLA_SYS_H\n#define LIELLA_SYS_H\n\n");
    let mut by_area: BTreeMap<&str, Vec<&Call>> = BTreeMap::new();
    for c in &calls {
        by_area.entry(c.area.as_str()).or_default().push(c);
    }
    for (area, list) in &by_area {
        let _ = writeln!(decl, "/* {area} */");
        for c in list {
            let _ = writeln!(decl, "long liella_sys_{}({});", c.name, c_params(c));
        }
        decl.push('\n');
        let mut src = format!("{banner}/* Class sys, area `{area}` (spec Appendix A.1). */\n#include \"liella_csys.h\"\n#include \"liella_sysnr.h\"\n#include \"liella_raw.h\"\n");
        for c in list {
            let args: String = c.args.iter().enumerate().map(|(i, k)| if *k == 'i' { format!(", a{i}") } else { format!(", (long)a{i}") }).collect();
            if c.nr == "*" {
                // The number is a0; the remaining arguments follow it.
                let rest = args.strip_prefix(", a0").unwrap_or(&args);
                let _ = write!(src, "\nlong liella_sys_{}({})\n{{\n    return liella_raw{}(a0{rest});\n}}\n", c.name, c_params(c), c.args.len() - 1);
            } else {
                let _ = write!(src, "\nlong liella_sys_{}({})\n{{\n    return liella_raw{}(LIELLA_NR_{}{args});\n}}\n", c.name, c_params(c), c.args.len(), c.nr);
            }
        }
        // The six veneer areas are written by `veneer` (COLD-REWRITE-W-far-08); the rest still by the loop above.
        let src = if veneer::AREAS.contains(area) { veneer::unit(area, list) } else { src };
        files.insert(format!("csys/src/sys/{area}.c"), src);
    }
    decl.push_str("#endif\n");
    files.insert("csys/include/liella_sys.h".to_string(), decl);

    let mut rs = String::from("// @generated by `cargo xtask csys-gen` from csys/syscalls.tsv. Do not edit.\n\
//! Category *extern*: declarations of the C layer's class-*sys* wrappers.\n\
//!\n\
//! Every item is `pub(crate)` and declared `safe`. Soundness does not come from the declaration\n\
//! alone: items taking addresses or pointers are reached only through `crate::sys::CSys` (which\n\
//! passes pointers derived from Rust references and slices it owns or borrows) and\n\
//! `crate::raw` (which passes addresses a C caller supplied under its own contract, decision\n\
//! D-X.9). Each returns the raw kernel result: a value, or `-errno`.\n//!\n\
//! A wrapper may go unused: the blocking calls that can be cancellation points are made by\n\
//! number (`crate::sys`, D-22.11), and the table lists every call the spec names.\n\n\
#![allow(dead_code)]\n\n\
use core::ffi::c_void;\n\n\
// UNSAFE(extern): Rust cannot call the C syscall layer without declaring its functions. The\n\
// signatures below are generated from the same table as the C definitions (rule K5), and every\n\
// item is crate-private so that only CSys and raw can pass addresses to the kernel.\n\
#[allow(unsafe_code)]\n\
unsafe extern \"C\" {\n");
    for c in &calls {
        let params = c.args.iter().enumerate().map(|(i, k)| format!("a{i}: {}", rust_type(*k))).collect::<Vec<_>>().join(", ");
        let _ = writeln!(rs, "    pub(crate) safe fn liella_sys_{}({params}) -> i64;", c.name);
    }
    rs.push_str("}\n");
    // aarch64 drops the x86-only wrappers (`none:-`); everything else keeps its declaration.
    let x86_only: Vec<String> = calls.iter().filter(|c| c.aarch64 == Generic::Absent(None))
        .map(|c| format!("    pub(crate) safe fn liella_sys_{}(", c.name)).collect();
    let rs_a64: String = rs.lines().filter(|l| !x86_only.iter().any(|p| l.starts_with(p.as_str())))
        .map(|l| format!("{l}\n")).collect::<String>()
        .replacen("declarations of the C layer's class-*sys* wrappers.", "declarations of the C layer's class-*sys* wrappers, aarch64 (not built yet: ledger A-6).", 1);
    files.insert("crates/boundary/src/ffi_sys.rs".to_string(), rs);
    files.insert("crates/boundary/src/ffi_sys_aarch64.rs".to_string(), rs_a64);
    let generic = fs::read_to_string(UAPI_GENERIC).map_err(|e| format!("{UAPI_GENERIC}: {e} (install linux-libc-dev)"))?;
    let numbers = generic_numbers(&generic, AARCH64_WANT, 64)?;
    check_arm64_tree(&calls)?;
    aarch64_numbers(&calls, &numbers, &mut files)?;
    Ok(files)
}

/// `liella_sysnr.h` and `nr.rs` for aarch64. Every call of the table must have a number in the
/// generic table or be marked absent (`none:`), and a call marked absent must have no number.
fn aarch64_numbers(calls: &[Call], numbers: &BTreeMap<String, u32>, files: &mut BTreeMap<String, String>) -> Result<(), String> {
    let banner = "/* @generated by `cargo xtask csys-gen` from csys/syscalls.tsv. Do not edit. */\n";
    let what = "aarch64 (Linux UAPI asm-generic/unistd.h with arm64's __ARCH_WANT_* switches)";
    let mut nr = format!("{banner}/* System-call numbers, {what}. Not built yet (ledger A-6). */\n#ifndef LIELLA_SYSNR_H\n#define LIELLA_SYSNR_H\n\n");
    let mut rs = format!("// @generated by `cargo xtask csys-gen` from csys/syscalls.tsv. Do not edit.\n//! System-call numbers, {what}, for the calls of the table. Not built yet (ledger A-6).\n#![allow(dead_code, missing_docs)]\n\n");
    let mut absent = String::new();
    let mut problems = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let kernel: BTreeMap<&str, &Call> = calls.iter().map(|c| (c.nr.as_str(), c)).collect();
    for c in calls.iter().filter(|c| c.nr != "*") {
        match (&c.aarch64, numbers.get(&c.nr)) {
            (Generic::Absent(r), None) => {
                if let Some(r) = r {
                    if !kernel.contains_key(r.as_str()) || !numbers.contains_key(r) {
                        problems.push(format!("{}: replacement `{r}` is not a call of the table with an aarch64 number", c.name));
                    }
                }
                let how = r.as_ref().map_or("x86-only, no wrapper".to_string(), |r| format!("liella_sys_{} is built on {r}", c.name));
                let _ = writeln!(absent, "/* {}: absent ({how}). */", c.nr);
                let _ = writeln!(rs, "// {}: absent ({how}).", c.nr.to_uppercase());
            }
            (Generic::Absent(_), Some(v)) => problems.push(format!("{}: marked absent, but {UAPI_GENERIC} gives __NR_{} = {v}", c.name, c.nr)),
            (_, None) => problems.push(format!("{}: __NR_{} is not in {UAPI_GENERIC} for aarch64 (mark it none:<replacement> or none:-)", c.name, c.nr)),
            (_, Some(v)) => {
                if seen.insert(c.nr.clone()) {
                    let _ = writeln!(nr, "#define LIELLA_NR_{} {v}", c.nr);
                    let _ = writeln!(rs, "pub(crate) const {}: i64 = {v};", c.nr.to_uppercase());
                }
            }
        }
        if c.aarch64 == Generic::Backwards && c.nr != "clone" {
            problems.push(format!("{}: =backwards is only for clone", c.name));
        }
    }
    if !problems.is_empty() {
        return Err(format!("syscalls.tsv aarch64 column:\n  {}", problems.join("\n  ")));
    }
    let backwards = calls.iter().any(|c| c.aarch64 == Generic::Backwards);
    nr.push('\n');
    nr.push_str(&absent);
    if backwards {
        nr.push_str("\n/* clone takes (flags, newsp, parent_tid, tls, child_tid) here (CONFIG_CLONE_BACKWARDS). */\n#define LIELLA_CLONE_BACKWARDS 1\n");
        rs.push_str("/// clone takes (flags, newsp, parent_tid, tls, child_tid) here (CONFIG_CLONE_BACKWARDS).\npub(crate) const CLONE_BACKWARDS: bool = true;\n");
    }
    nr.push_str("\n#endif\n");
    files.insert("csys/include/aarch64/liella_sysnr.h".to_string(), nr);
    files.insert("crates/boundary/src/nr_aarch64.rs".to_string(), rs);
    Ok(())
}

/// With a kernel header tree under /usr/src (kernel source, allowed): arm64's `__ARCH_WANT_*`
/// switches must equal [`AARCH64_WANT`], and its Kconfig selects CLONE_BACKWARDS exactly when the
/// table marks clone `=backwards`. Without a tree the constants stand as written.
fn check_arm64_tree(calls: &[Call]) -> Result<(), String> {
    let Ok(rd) = fs::read_dir("/usr/src") else { return Ok(()) };
    let mut trees: Vec<_> = rd.flatten().map(|e| e.path()).filter(|p| p.join("arch/arm64/include/uapi/asm/unistd.h").is_file()).collect();
    trees.sort();
    let Some(tree) = trees.last() else { return Ok(()) };
    let uapi = fs::read_to_string(tree.join("arch/arm64/include/uapi/asm/unistd.h")).map_err(|e| e.to_string())?;
    let mut want: Vec<&str> = uapi.lines().filter_map(|l| l.strip_prefix("#define ")).map(str::trim).filter(|w| w.starts_with("__ARCH_WANT_")).collect();
    let mut expect = AARCH64_WANT.to_vec();
    want.sort_unstable();
    expect.sort_unstable();
    if want != expect {
        return Err(format!("{}: arm64 defines {want:?}, csys.rs AARCH64_WANT has {expect:?}", tree.display()));
    }
    let kconfig = fs::read_to_string(tree.join("arch/arm64/Kconfig")).map_err(|e| e.to_string())?;
    let selects = kconfig.lines().any(|l| l.trim() == "select CLONE_BACKWARDS");
    let marked = calls.iter().any(|c| c.nr == "clone" && c.aarch64 == Generic::Backwards);
    if selects != marked {
        return Err(format!("{}: arm64 Kconfig CLONE_BACKWARDS is {selects}, syscalls.tsv clone =backwards is {marked}", tree.display()));
    }
    Ok(())
}

/// The `__NR_<name>` numbers of the kernel's generic table as an architecture with the switches
/// `defined` and `__BITS_PER_LONG == bits` sees them. Values naming another macro
/// (`__NR3264_fcntl`) are resolved; a condition this reader does not understand is an error.
fn generic_numbers(text: &str, defined: &[&str], bits: u32) -> Result<BTreeMap<String, u32>, String> {
    struct Branch { taken: bool, now: bool }
    let mut stack: Vec<Branch> = Vec::new();
    let mut local = std::collections::BTreeSet::new();
    let mut raw: BTreeMap<String, String> = BTreeMap::new();
    for (n, line) in text.lines().enumerate() {
        let Some(d) = line.trim_start().strip_prefix('#') else { continue };
        let d = d.split("/*").next().unwrap_or("").trim();
        let (word, rest) = d.split_once(char::is_whitespace).map_or((d, ""), |(w, r)| (w, r.trim()));
        let active = stack.iter().all(|b| b.now);
        let is_def = |name: &str| defined.contains(&name) || local.contains(name);
        let at = |e: String| format!("{UAPI_GENERIC}:{}: {e}", n + 1);
        match word {
            "if" | "ifdef" | "ifndef" => {
                let v = match word {
                    "ifdef" => is_def(rest),
                    "ifndef" => !is_def(rest),
                    _ => condition(rest, &is_def, bits).map_err(at)?,
                };
                stack.push(Branch { taken: v, now: v });
            }
            "elif" => {
                let v = condition(rest, &is_def, bits).map_err(at)?;
                let b = stack.last_mut().ok_or_else(|| at("#elif without #if".into()))?;
                b.now = !b.taken && v;
                b.taken |= v;
            }
            "else" => {
                let b = stack.last_mut().ok_or_else(|| at("#else without #if".into()))?;
                b.now = !b.taken;
                b.taken = true;
            }
            "endif" => {
                stack.pop().ok_or_else(|| at("#endif without #if".into()))?;
            }
            "define" if active => {
                let mut w = rest.split_whitespace();
                if let Some(name) = w.next() {
                    let name = name.split('(').next().unwrap_or(name);
                    local.insert(name.to_string());
                    if name.starts_with("__NR") {
                        raw.insert(name.to_string(), w.next().unwrap_or("").to_string());
                    }
                }
            }
            "undef" if active => {
                local.remove(rest);
                raw.remove(rest);
            }
            _ => {}
        }
    }
    if !stack.is_empty() {
        return Err(format!("{UAPI_GENERIC}: unterminated #if"));
    }
    let mut out = BTreeMap::new();
    for (name, val) in &raw {
        let Some(call) = name.strip_prefix("__NR_") else { continue };
        let mut v = val.as_str();
        for _ in 0..4 {
            if let Ok(x) = v.parse::<u32>() {
                out.insert(call.to_string(), x);
                break;
            }
            match raw.get(v) { Some(next) => v = next, None => break }
        }
    }
    Ok(out)
}

/// One `#if`/`#elif` expression: `||` of `&&` of `defined(X)`, `defined X`, `!...` and
/// `__BITS_PER_LONG ==|!= N`. Parentheses beyond `defined(...)` are not supported.
fn condition(expr: &str, is_def: &dyn Fn(&str) -> bool, bits: u32) -> Result<bool, String> {
    fn atom(a: &str, is_def: &dyn Fn(&str) -> bool, bits: u32) -> Result<bool, String> {
        if let Some(r) = a.strip_prefix('!') {
            return atom(r.trim(), is_def, bits).map(|v| !v);
        }
        if let Some(r) = a.strip_prefix("defined") {
            let name = r.trim();
            let name = name.strip_prefix('(').and_then(|n| n.strip_suffix(')')).unwrap_or(name).trim();
            if name.is_empty() || name.contains(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
                return Err(format!("unsupported condition `{a}`"));
            }
            return Ok(is_def(name));
        }
        match a.split_whitespace().collect::<Vec<_>>()[..] {
            ["__BITS_PER_LONG", op, n] => {
                let n: u32 = n.parse().map_err(|_| format!("unsupported condition `{a}`"))?;
                match op { "==" => Ok(bits == n), "!=" => Ok(bits != n), _ => Err(format!("unsupported condition `{a}`")) }
            }
            _ => Err(format!("unsupported condition `{a}`")),
        }
    }
    let mut any = false;
    for alt in expr.split("||") {
        let mut all = true;
        for a in alt.split("&&") {
            all &= atom(a.trim(), is_def, bits)?;
        }
        any |= all;
    }
    Ok(any)
}


/// K5 for the hand-written declarations: every `liella_*` function declared in
/// `crates/boundary/src/ffi.rs` must be declared in `csys/include/liella_csys.h` with the same
/// number of parameters.
pub fn machine_decls_match(root: &Path) -> Result<(), String> {
    fn arity(params: &str) -> usize {
        let p = params.trim();
        if p.is_empty() || p == "void" { 0 } else { p.split(',').count() }
    }
    let header = fs::read_to_string(root.join("csys/include/liella_csys.h")).map_err(|e| e.to_string())?;
    let ffi = fs::read_to_string(root.join("crates/boundary/src/ffi.rs")).map_err(|e| e.to_string())?;
    let mut c = BTreeMap::new();
    for line in header.lines() {
        if let Some(i) = line.find("liella_") {
            let rest = &line[i..];
            if let (Some(open), Some(close)) = (rest.find('('), rest.rfind(')')) {
                if rest.trim_end().ends_with(");") {
                    // The name is the identifier right before `(`: a return type may itself be a
                    // `liella_` type (`struct liella_strdiff liella_machine_strdiff(...)`).
                    let head = &rest[..open];
                    let name = head.rsplit(|ch: char| ch.is_whitespace() || ch == '*').next().unwrap_or(head);
                    c.insert(name.to_string(), arity(&rest[open + 1..close]));
                }
            }
        }
    }
    let mut problems = Vec::new();
    for line in ffi.lines() {
        let l = line.trim();
        if let Some(rest) = l.split("fn ").nth(1).filter(|_| l.contains("safe fn liella_")) {
            if let (Some(open), Some(close)) = (rest.find('('), rest.find(')')) {
                let name = &rest[..open];
                let n = arity(&rest[open + 1..close]);
                match c.get(name) {
                    Some(&m) if m == n => {}
                    Some(&m) => problems.push(format!("{name}: {n} parameters in ffi.rs, {m} in liella_csys.h")),
                    None => problems.push(format!("{name}: declared in ffi.rs but not in liella_csys.h")),
                }
            }
        }
    }
    if problems.is_empty() { Ok(()) } else { Err(format!("ffi.rs and liella_csys.h disagree (K5):
  {}", problems.join("
  "))) }
}

pub fn run(root: &Path, check: bool) -> Result<(), String> {
    machine_decls_match(root)?;
    let files = render(root)?;
    let mut stale = Vec::new();
    for (rel, content) in &files {
        let path = root.join(rel);
        let current = fs::read_to_string(&path).unwrap_or_default();
        if current != *content {
            if check {
                stale.push(rel.clone());
            } else {
                if let Some(d) = path.parent() {
                    fs::create_dir_all(d).map_err(|e| e.to_string())?;
                }
                fs::write(&path, content).map_err(|e| format!("{rel}: {e}"))?;
            }
        }
    }
    if !stale.is_empty() {
        return Err(format!("generated files are stale (run `cargo xtask csys-gen`): {}", stale.join(", ")));
    }
    let calls = files.keys().filter(|k| k.starts_with("csys/src/sys/")).count();
    println!("csys-gen: {} ({calls} area files, table csys/syscalls.tsv)", if check { "up to date" } else { "written" });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = "#ifndef __SYSCALL\n#define __SYSCALL(x, y)\n#endif\n\
#define __NR_io_setup 0\n\
#ifdef __ARCH_WANT_RENAMEAT\n#define __NR_renameat 38\n#endif /* __ARCH_WANT_RENAMEAT */\n\
#ifdef __ARCH_WANT_NOPE\n#define __NR_nope 1\n#else\n#define __NR_yes 2\n#endif\n\
#define __NR3264_fcntl 25\n\
#if defined(__ARCH_WANT_TIME32_SYSCALLS) || __BITS_PER_LONG != 32\n#define __NR_t32 3\n#endif\n\
#if __BITS_PER_LONG == 64 && !defined(__SYSCALL_COMPAT)\n#define __NR_fcntl __NR3264_fcntl\n#else\n#define __NR_fcntl64 __NR3264_fcntl\n#endif\n\
#if __BITS_PER_LONG == 32\n#define __NR_a 4\n#elif defined(__ARCH_WANT_RENAMEAT)\n#define __NR_b 5\n#else\n#define __NR_c 6\n#endif\n\
#define __NR_gone 7\n#undef __NR_gone\n";

    #[test]
    fn generic_reader_follows_conditions() {
        let m = generic_numbers(FIXTURE, &["__ARCH_WANT_RENAMEAT"], 64).unwrap();
        let got: Vec<(&str, u32)> = m.iter().map(|(k, v)| (k.as_str(), *v)).collect();
        assert_eq!(got, [("b", 5), ("fcntl", 25), ("io_setup", 0), ("renameat", 38), ("t32", 3), ("yes", 2)]);
        let m32 = generic_numbers(FIXTURE, &[], 32).unwrap();
        assert!(m32.contains_key("fcntl64") && m32.contains_key("a") && !m32.contains_key("t32") && !m32.contains_key("renameat"));
    }

    #[test]
    fn generic_reader_rejects_unknown_conditions() {
        assert!(generic_numbers("#if FOO > 1\n#endif\n", &[], 64).is_err());
        assert!(generic_numbers("#if (defined(A) || defined(B))\n#endif\n", &[], 64).is_err());
        assert!(generic_numbers("#ifdef A\n", &[], 64).is_err());
    }

    #[test]
    fn every_call_has_an_aarch64_entry() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let calls = table(root).unwrap();
        let names: std::collections::BTreeSet<&str> = calls.iter().map(|c| c.nr.as_str()).collect();
        for c in &calls {
            if let Generic::Absent(Some(r)) = &c.aarch64 {
                assert!(names.contains(r.as_str()), "{}: replacement {r} is not in the table", c.name);
            }
        }
        // The calls the generic table lacks (ledger A-3); the header check in csys-gen proves the list.
        let mut absent: Vec<&str> = calls.iter().filter(|c| matches!(c.aarch64, Generic::Absent(_))).map(|c| c.nr.as_str()).collect();
        absent.sort_unstable();
        assert_eq!(
            absent,
            [
                "alarm",
                "arch_prctl",
                "create_module",
                "dup2",
                "get_kernel_syms",
                "getpgrp",
                "ioperm",
                "iopl",
                "modify_ldt",
                "pause",
                "query_module",
                "time",
                "uselib",
                "ustat",
            ]
        );
        assert_eq!(generic_column(Some("none:")), None);
        assert_eq!(generic_column(Some("same")), None);
    }
}
