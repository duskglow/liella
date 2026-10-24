//! `cargo xtask compat-gen`: the compat versions of `libc.so.6` as `.symver` aliases (phase 15,
//! LG-1 `COMPAT-SYMVER-GEN`, ruling R15-1).
//!
//! One generated file, `csys/src/machine/compat_aliases.S`, holds one stub for each compat version
//! Liella exports: a frame-free `jmp` to the function that carries the contract, given the
//! non-default version with `.symver stub, name@VERSION` (single `@`, D-27.4). A program linked
//! anew always binds the default version (`name@@...`); only old binaries that ask for the old
//! version reach a stub, so no current caller pays anything.
//!
//! The entries come from `spec/abi/compat-plan.tsv` (LG-0a), the single source, joined with
//! `spec/abi/versions.tsv` for the binding (weak or global) of each name. Of the table's `libc.so.6`
//! rows:
//! * an **alias** row (the default version's contract, or the function named by `target=` in its
//!   note) gets a stub when its story is `LG-1` (this generator's scope) or its target function is
//!   already defined in the tree. The libdl names wait in [`DEFERRED`] for phase 14;
//! * a **behaviour**, **subsystem** or **stub** row (its own contract, R15-1; the LG-5 removed
//!   syscalls and STREAMS names are `stub` rows) gets a stub when the tree defines
//!   `liella_compat_<name>` (`stime`, the LG-5 names, the `xdr_*` primitives); the stub's target
//!   is that function. Rows whose function has not landed yet are skipped, and appear as soon as it
//!   does, with no change to this file;
//! * `object` rows are other stories' (LG-6: data needs a copy with the manifest's size, not a jump).
//!
//! The object is assembled by `cargo xtask so` and linked into `libc.so.6` only; it is not in
//! `libc.a` (R15-2: a static link cannot bind a non-default version, and compat-only names do not
//! belong in the archive). `cargo xtask so` checks that every target is defined by `libc.a` and that
//! the shared object exports exactly these `name@VERSION` symbols; `cargo xtask static` checks that
//! the generated file is current.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

/// The generated file.
pub const FILE: &str = "csys/src/machine/compat_aliases.S";

/// Aliases held back because `libc.a` does not define their target yet: the libdl merge, whose
/// functions arrive with dynamic loading (phase 14). `cargo xtask so` fails on any other alias whose
/// target is missing, so a name leaves this list when its default version lands.
pub const DEFERRED: &[&str] = &["dladdr", "dladdr1", "dlclose", "dlerror", "dlinfo", "dlmopen", "dlopen", "dlsym", "dlvsym"];

/// Table `behaviour` rows whose own function has not landed, still exported as an alias of the
/// default version's function because the previous rule exported them and the `so` ratchet
/// (compat-missing.tsv) forbids losing an export. Each leaves this list when its
/// `liella_compat_<name>` lands (`__isnanf128`: LG-4, `__libc_start_main`: LG-2).
pub const HELD: &[(&str, &str)] = &[("__isnanf128", "GLIBC_2.26"), ("__libc_start_main", "GLIBC_2.2.5")];

/// One generated stub: `name@version` jumps to `target`; `weak` follows glibc's binding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub version: String,
    pub target: String,
    pub weak: bool,
}

impl Entry {
    /// The stub's own (local in `libc.so.6`) symbol.
    pub fn stub(&self) -> String {
        format!("liella_compat_{}_{}", self.name, self.version.replace('.', "_"))
    }
}

/// Numeric order of `GLIBC_x.y.z` versions.
fn version_key(v: &str) -> Vec<u32> {
    v.strip_prefix("GLIBC_").unwrap_or(v).split('.').map(|p| p.parse().unwrap_or(u32::MAX)).collect()
}

/// The code of a Rust source with its comments (line, doc and nested block comments) blanked, so
/// a name mentioned only in a comment defines nothing (#1794). String literals are kept; char
/// literals are blanked so a `'"'` opens no string.
fn code_only(text: &str) -> String {
    let b: Vec<char> = text.chars().collect();
    let (mut out, mut i) = (String::with_capacity(text.len()), 0);
    while i < b.len() {
        let next = b.get(i + 1).copied();
        if b[i] == '/' && next == Some('/') {
            while i < b.len() && b[i] != '\n' {
                i += 1;
            }
        } else if b[i] == '/' && next == Some('*') {
            let (mut depth, mut j) = (1, i + 2);
            while j < b.len() && depth > 0 {
                if b[j] == '/' && b.get(j + 1) == Some(&'*') {
                    depth += 1;
                    j += 1;
                } else if b[j] == '*' && b.get(j + 1) == Some(&'/') {
                    depth -= 1;
                    j += 1;
                }
                j += 1;
            }
            out.push(' ');
            i = j;
        } else if b[i] == '"' {
            let mut j = i + 1;
            while j < b.len() && b[j] != '"' {
                j += if b[j] == '\\' { 2 } else { 1 };
            }
            // kept verbatim (an `export_name = "liella_compat_.."` names a definition), but a `//`
            // inside it starts no comment
            out.extend(&b[i..(j + 1).min(b.len())]);
            i = j + 1;
        } else if b[i] == '\'' && (next == Some('\\') || b.get(i + 2) == Some(&'\'')) {
            // a char literal ('x', '\n', '\''); a lifetime ('a) has no closing quote two on
            let mut j = i + 2;
            while j < b.len() && b[j] != '\'' {
                j += 1;
            }
            out.push_str("' '");
            i = j + 1;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    out
}

/// Adds to `out` the names `text` defines: every `fn <name>` and every `liella_compat_*` token
/// of its code (the xdr layer defines its entries through a macro, so the name only occurs as a
/// token); comments and literals do not count (#1794).
fn scan_defined(text: &str, out: &mut BTreeSet<String>) {
    let code = code_only(text);
    let mut prev = "";
    for tok in code.split(|c: char| !(c.is_alphanumeric() || c == '_')).filter(|t| !t.is_empty()) {
        if prev == "fn" || tok.starts_with("liella_compat_") {
            out.insert(tok.to_string());
        }
        prev = tok;
    }
}

/// Names defined in the tree's Rust sources (see [`scan_defined`]).
fn defined_names(root: &Path) -> Result<BTreeSet<String>, String> {
    fn walk(dir: &Path, out: &mut BTreeSet<String>) -> Result<(), String> {
        let mut paths: Vec<_> = fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?.flatten().map(|e| e.path()).collect();
        paths.sort();
        for p in paths {
            if p.is_dir() {
                if p.file_name().is_some_and(|n| n == "tests" || n == "target") {
                    continue;
                }
                walk(&p, out)?;
            } else if p.extension().is_some_and(|e| e == "rs") {
                let text = fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
                scan_defined(&text, out);
            }
        }
        Ok(())
    }
    let mut out = BTreeSet::new();
    walk(&root.join("crates"), &mut out)?;
    Ok(out)
}

/// The entries chosen from the text of `spec/abi/versions.tsv` (`versions`, for each name's binding)
/// and `spec/abi/compat-plan.tsv` (`plan`); `defined` says whether the tree defines a function.
/// Sorted by name and version.
pub fn entries_from(versions: &str, plan: &str, defined: &dyn Fn(&str) -> bool) -> Result<Vec<Entry>, String> {
    let mut weak: BTreeMap<(&str, &str), bool> = BTreeMap::new();
    for line in versions.lines().skip(1) {
        let f: Vec<&str> = line.split('\t').collect();
        if let [lib, sym, ver, "compat", typ, scope, ..] = f.as_slice() {
            if *lib == "libc.so.6" && *typ == "FUNC" {
                weak.insert((sym, ver), *scope == "WEAK");
            }
        }
    }
    let mut out = Vec::new();
    for line in plan.lines().skip(1) {
        let f: Vec<&str> = line.split('\t').collect();
        let [lib, sym, ver, kind, story, _drow, note] = f.as_slice() else { continue };
        if *lib != "libc.so.6" || DEFERRED.contains(sym) {
            continue;
        }
        let own = format!("liella_compat_{sym}");
        let target = match *kind {
            "alias" => {
                let target = note.strip_prefix("target=").map_or(*sym, |t| t.split_whitespace().next().unwrap_or(sym));
                if *story != "LG-1" && !defined(target) {
                    continue;
                }
                target.to_string()
            }
            "behaviour" | "subsystem" | "stub" if defined(&own) => own,
            "behaviour" if HELD.contains(&(*sym, *ver)) => sym.to_string(),
            _ => continue,
        };
        let Some(&w) = weak.get(&(*sym, *ver)) else {
            return Err(format!("compat-gen: {sym}@{ver} is a plan row without a FUNC compat row in spec/abi/versions.tsv"));
        };
        out.push(Entry { name: sym.to_string(), version: ver.to_string(), target, weak: w });
    }
    out.sort_by(|a, b| (a.name.as_str(), version_key(&a.version)).cmp(&(b.name.as_str(), version_key(&b.version))));
    Ok(out)
}

/// The entries for this tree.
pub fn entries(root: &Path) -> Result<Vec<Entry>, String> {
    let read = |p: &str| fs::read_to_string(root.join(p)).map_err(|e| format!("{p}: {e}"));
    let names = defined_names(root)?;
    entries_from(&read("spec/abi/versions.tsv")?, &read("spec/abi/compat-plan.tsv")?, &|n| names.contains(n))
}

/// The generated file of `libm.so.6`'s compat versions (LG-4). It lives with the libm product, not
/// in `csys/src/machine/`, so no archive build picks it up: `cargo xtask so` links it into
/// `libm.so.6` only (R15-2).
pub const LIBM_FILE: &str = "crates/libm-so/compat_aliases.S";

/// The `libm.so.6` entries (LG-4 `COMPAT-LIBM`): every `alias` row of `spec/abi/compat-plan.tsv`
/// (`plan`) for `libm.so.6` - the `__*_finite` names (D-10.2: the finite-math entry points carry
/// the contract of the plain function) and `pow10{,f,l}` - jumping to the function named by the
/// row's `target=<name>` note, or to the default version of the same name. The binding follows
/// glibc's (`versions.tsv` `scope`); a row that is not a `libm.so.6` compat row of
/// `versions.tsv` (`table`) is an error.
pub fn libm_entries_from(table: &str, plan: &str) -> Result<Vec<Entry>, String> {
    let mut scope: BTreeMap<(&str, &str), &str> = BTreeMap::new();
    for line in table.lines().skip(1) {
        if let ["libm.so.6", sym, ver, "compat", _, sc, ..] = line.split('\t').collect::<Vec<_>>().as_slice() {
            scope.insert((sym, ver), sc);
        }
    }
    let mut out = Vec::new();
    for line in plan.lines().skip(1) {
        let f: Vec<&str> = line.split('\t').collect();
        let ["libm.so.6", sym, ver, "alias", _, _, rest @ ..] = f.as_slice() else { continue };
        let Some(sc) = scope.get(&(*sym, *ver)) else {
            return Err(format!("compat-gen: {sym}@{ver} is a libm.so.6 alias of spec/abi/compat-plan.tsv but no libm.so.6 compat row of spec/abi/versions.tsv"));
        };
        let note = rest.first().copied().unwrap_or("");
        let target = note.split_whitespace().find_map(|w| w.strip_prefix("target=")).unwrap_or(sym);
        out.push(Entry { name: sym.to_string(), version: ver.to_string(), target: target.to_string(), weak: *sc == "WEAK" });
    }
    out.sort_by(|a, b| (a.name.as_str(), version_key(&a.version)).cmp(&(b.name.as_str(), version_key(&b.version))));
    Ok(out)
}

/// The `libm.so.6` entries for this tree.
pub fn libm_entries(root: &Path) -> Result<Vec<Entry>, String> {
    let read = |p: &str| fs::read_to_string(root.join(p)).map_err(|e| format!("{p}: {e}"));
    libm_entries_from(&read("spec/abi/versions.tsv")?, &read("spec/abi/compat-plan.tsv")?)
}

/// The text of [`LIBM_FILE`] for `entries`.
pub fn render_libm(entries: &[Entry]) -> String {
    let body = render(entries);
    let stubs = body.split_once("*/\n").map_or(body.as_str(), |(_, b)| b);
    format!(
        "/* @generated by `cargo xtask compat-gen` from the libm.so.6 `alias` rows of spec/abi/compat-plan.tsv;\n\
         * do not edit (xtask/src/compat.rs). The compat versions of libm.so.6 (LG-4, R15-1, D-10.2): {} aliases\n\
         * of a current function. Each stub jumps, frame-free, to the function with the contract and is given\n\
         * the non-default version (single `@`), so only old binaries bind it. Linked into libm.so.6 only,\n\
         * never into libm.a (R15-2). */\n{stubs}",
        entries.len()
    )
}

/// The text of [`FILE`] for `entries`.
pub fn render(entries: &[Entry]) -> String {
    let aliases = entries.iter().filter(|e| !e.target.starts_with("liella_compat_")).count();
    let mut s = format!(
        "/* @generated by `cargo xtask compat-gen` from spec/abi/compat-plan.tsv; do not edit (xtask/src/compat.rs).\n\
         * Class machine: the compat versions of libc.so.6 (LG-1, R15-1, D-27.4): {aliases} aliases of the default\n\
         * contract and {} entries with their own contract. Each stub jumps, frame-free, to the function with the contract\n\
         * and is given the non-default version (single `@`), so only old binaries bind it. Linked into\n\
         * libc.so.6 only, never into libc.a (R15-2). */\n",
        entries.len() - aliases
    );
    let mut refs: BTreeSet<(&str, &str)> = BTreeSet::new();
    for e in entries {
        let stub = e.stub();
        let bind = if e.weak { "weak" } else { "globl" };
        let mut loads = String::new();
        if let Some((.., nargs, objs)) = LOADS.iter().find(|(n, v, ..)| *n == e.name && *v == e.version) {
            for (i, &(o, ov)) in objs.iter().enumerate() {
                loads.push_str(&format!("\tmovq {}@GOTPCREL(%rip), %{}\n", object_ref(o), ARG_REGS[nargs + i]));
                refs.insert((o, ov));
            }
        }
        s.push_str(&format!(
            "\t.section .text.{stub},\"ax\",@progbits\n\t.{bind} {stub}\n\t.type {stub}, @function\n\t.p2align 6\n{stub}:\n{loads}\tjmp {}@PLT\n\t.size {stub}, .-{stub}\n\t.symver {stub}, {}@{}\n",
            e.target, e.name, e.version
        ));
    }
    // The references the loads use: undefined here, each bound to the object's versioned name
    // (the object itself is compat_objects' label with the same `.symver`).
    for (o, ov) in refs {
        s.push_str(&format!("\t.symver {}, {o}@{ov}\n", object_ref(o)));
    }
    s.push_str("\t.section .note.GNU-stack,\"\",@progbits\n");
    s
}

/// Behaviour rows whose stub first loads the addresses of compat data objects (`compat_objects`)
/// into the argument registers after the caller's `nargs` arguments, then jumps to the target:
/// `(name, version, nargs, [(object, object version)])` (LG-10 `<regexp.h>`, ruling #1811). The load
/// goes through the GOT of the object's versioned name, so the address is the copy an old non-PIE
/// binary made by `R_X86_64_COPY` (the one it reads), not libc.so.6's own storage.
pub const LOADS: &[Load] = &[
    ("advance", "GLIBC_2.2.5", 2, &[("loc1", "GLIBC_2.2.5"), ("loc2", "GLIBC_2.2.5")]),
    ("step", "GLIBC_2.2.5", 2, &[("loc1", "GLIBC_2.2.5"), ("loc2", "GLIBC_2.2.5")]),
];

/// One [`LOADS`] row: the behaviour row's name and version, its argument count, the objects.
pub type Load = (&'static str, &'static str, usize, &'static [(&'static str, &'static str)]);

/// The System V x86-64 integer argument registers.
const ARG_REGS: [&str; 6] = ["rdi", "rsi", "rdx", "rcx", "r8", "r9"];

/// The undefined symbol a [`LOADS`] stub loads the address of compat object `name` through.
fn object_ref(name: &str) -> String {
    format!("liella_compat_{name}_ref")
}

/// `cargo xtask compat-gen [--check]`.
pub fn run(root: &Path, check: bool) -> Result<(), String> {
    let libc = entries(root)?;
    let libm = libm_entries(root)?;
    // the LG-6 data objects (compat_objects) are inserted into the libc.so.6 file's text
    let libc_text = crate::compat_objects::insert(&render(&libc), &crate::compat_objects::render(&crate::compat_objects::model(root)?));
    for (file, text, n) in [(FILE, libc_text, libc.len()), (LIBM_FILE, render_libm(&libm), libm.len())] {
        let path = root.join(file);
        if check {
            if fs::read_to_string(&path).unwrap_or_default() != text {
                return Err(format!("{file} is stale: run `cargo xtask compat-gen`"));
            }
            println!("compat-gen: ok ({file} current, {n} compat versions)");
        } else {
            fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
            println!("compat-gen: wrote {file} ({n} compat versions)");
        }
    }
    Ok(())
}

/// The problems of `readelf --dyn-syms -W` text of `lib` (`libc.so.6`, `libm.so.6`) against `entries` and the compat rows
/// of `spec/abi/versions.tsv` (`compat_rows`, `(name, version)`): every entry exported as
/// `name@version` (single `@`), and no compat version exported that the manifest lacks.
pub fn export_problems(lib: &str, readelf: &str, entries: &[Entry], compat_rows: &[(String, String)]) -> Vec<String> {
    let mut exported = std::collections::BTreeSet::new();
    for line in readelf.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        let (Some(ndx), Some(name)) = (f.get(6), f.get(7)) else { continue };
        if *ndx == "UND" || name.contains("@@") {
            continue;
        }
        if let Some((sym, ver)) = name.split_once('@') {
            exported.insert((sym.to_string(), ver.to_string()));
        }
    }
    let mut problems = Vec::new();
    for e in entries {
        if !exported.contains(&(e.name.clone(), e.version.clone())) {
            problems.push(format!("{lib} does not export {}@{} ({})", e.name, e.version, if lib == "libm.so.6" { LIBM_FILE } else { FILE }));
        }
    }
    for (sym, ver) in &exported {
        if !compat_rows.iter().any(|(s, v)| s == sym && v == ver) {
            problems.push(format!("{lib} exports {sym}@{ver}, which spec/abi/versions.tsv has no compat row for"));
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    const VHEAD: &str = "library\tsymbol\tversion\tbinding\ttype\tscope\tsize\n";
    const PHEAD: &str = "library\tsymbol\tversion\tkind\tstory\tdrow\tnote\n";

    fn join(head: &str, rows: &[&str]) -> String {
        rows.iter().fold(head.to_string(), |t, r| t + r + "\n")
    }

    fn got(v: &[&str], p: &[&str], defined: &[&str]) -> Vec<(String, String, String, bool)> {
        let f = |n: &str| defined.contains(&n);
        entries_from(&join(VHEAD, v), &join(PHEAD, p), &f).unwrap().into_iter().map(|e| (e.name, e.version, e.target, e.weak)).collect()
    }

    fn row(name: &str, ver: &str, weak: bool) -> String {
        format!("libc.so.6\t{name}\t{ver}\tcompat\tFUNC\t{}\t", if weak { "WEAK" } else { "GLOBAL" })
    }

    #[test]
    fn the_plan_rows_choose_the_entries() {
        let v = [row("pthread_create", "GLIBC_2.2.5", false), row("clock_gettime", "GLIBC_2.2.5", true), row("xdr_int", "GLIBC_2.2.5", false), row("stime", "GLIBC_2.2.5", false), row("xdr_bytes", "GLIBC_2.2.5", false), row("__dn_comp", "GLIBC_2.2.5", false), row("__dn_skipname", "GLIBC_2.2.5", false), row("memcpy", "GLIBC_2.2.5", false)];
        let v: Vec<&str> = v.iter().map(String::as_str).collect();
        let p = [
            "libc.so.6\t__dn_comp\tGLIBC_2.2.5\talias\tLG-7\tD-16.6\ttarget=dn_comp",
            "libc.so.6\t__dn_skipname\tGLIBC_2.2.5\talias\tLG-7\tD-16.6\ttarget=dn_skipname",
            "libc.so.6\tclock_gettime\tGLIBC_2.2.5\talias\tLG-1\t-\t",
            "libc.so.6\tmemcpy\tGLIBC_2.2.5\tbehaviour\tLG-2\tD-1.1\tnot landed",
            "libc.so.6\tpthread_create\tGLIBC_2.2.5\talias\tLG-1\t-\t",
            "libc.so.6\tstime\tGLIBC_2.2.5\tbehaviour\tLG-1\tD-27.4\t",
            "libc.so.6\txdr_bytes\tGLIBC_2.2.5\tsubsystem\tLG-13\tD-29.1\tnot landed",
            "libc.so.6\txdr_int\tGLIBC_2.2.5\tsubsystem\tLG-13\tD-29.1\t",
            "libpthread.so.0\tpthread_create\tGLIBC_2.2.5\tstub\tLG-6\t-\t",
        ];
        let want = [
            ("__dn_comp", "GLIBC_2.2.5", "dn_comp", false),
            ("clock_gettime", "GLIBC_2.2.5", "clock_gettime", true),
            ("pthread_create", "GLIBC_2.2.5", "pthread_create", false),
            ("stime", "GLIBC_2.2.5", "liella_compat_stime", false),
            ("xdr_int", "GLIBC_2.2.5", "liella_compat_xdr_int", false),
        ];
        let defined = ["dn_comp", "liella_compat_stime", "liella_compat_xdr_int"];
        assert_eq!(got(&v, &p, &defined), want.map(|(a, b, c, d)| (a.into(), b.into(), c.into(), d)));
    }

    #[test]
    fn the_nss_switch_internals_are_own_entries() {
        // LG-7 COMPAT-NSS-RESOLV: five behaviour rows, each with its own `liella_compat_<name>`.
        let names = ["__nss_database_lookup", "__nss_group_lookup", "__nss_hosts_lookup", "__nss_next", "__nss_passwd_lookup"];
        let v = names.map(|n| row(n, "GLIBC_2.2.5", false));
        let v: Vec<&str> = v.iter().map(String::as_str).collect();
        let p = names.map(|n| format!("libc.so.6\t{n}\tGLIBC_2.2.5\tbehaviour\tLG-7\tD-16.6\t"));
        let p: Vec<&str> = p.iter().map(String::as_str).collect();
        let own = names.map(|n| format!("liella_compat_{n}"));
        let defined: Vec<&str> = own.iter().map(String::as_str).collect();
        let got: Vec<(String, String)> = got(&v, &p, &defined).into_iter().map(|(n, _, t, _)| (n, t)).collect();
        assert_eq!(got, names.iter().zip(own.iter()).map(|(n, t)| (n.to_string(), t.clone())).collect::<Vec<_>>());
    }

    #[test]
    fn a_name_only_in_a_comment_is_not_defined() {
        // #1794 (no spec clause: generator input): comments define nothing; code, macro tokens and strings do.
        let src = "/// see liella_compat_doc\n// liella_compat_line\n/* a /* nested */ liella_compat_block */\nfn liella_compat_real() {}\nm!(liella_compat_macro);\nconst Q: char = '\"';\nconst U: &str = \"http://x\"; #[export_name = \"liella_compat_named\"] fn f<'a>() {}\n";
        let mut out = BTreeSet::new();
        scan_defined(src, &mut out);
        let want: BTreeSet<String> = ["f", "liella_compat_macro", "liella_compat_named", "liella_compat_real"].iter().map(|s| s.to_string()).collect();
        assert_eq!(out, want);
    }

    #[test]
    fn the_malloc_compat_stubs_are_own_entries() {
        // LG-11 MALLOC-COMPAT (D-3.5): `stub` rows, each with its own `liella_compat_<name>`.
        let names = ["__default_morecore", "tr_break"];
        let v = names.map(|n| row(n, "GLIBC_2.2.5", false));
        let v: Vec<&str> = v.iter().map(String::as_str).collect();
        let p = names.map(|n| format!("libc.so.6\t{n}\tGLIBC_2.2.5\tstub\tLG-11\tD-3.5\t"));
        let p: Vec<&str> = p.iter().map(String::as_str).collect();
        let own = names.map(|n| format!("liella_compat_{n}"));
        let defined: Vec<&str> = own.iter().map(String::as_str).collect();
        let got: Vec<(String, String)> = got(&v, &p, &defined).into_iter().map(|(n, _, t, _)| (n, t)).collect();
        assert_eq!(got, names.iter().zip(own.iter()).map(|(n, t)| (n.to_string(), t.clone())).collect::<Vec<_>>());
    }

    #[test]
    fn a_plan_row_without_a_versions_row_is_an_error() {
        let p = join(PHEAD, &["libc.so.6\tfgetc\tGLIBC_2.2.5\talias\tLG-1\t-\t"]);
        assert!(entries_from(&join(VHEAD, &[]), &p, &|_| true).is_err());
    }

    #[test]
    fn the_real_tables_give_the_stubs_and_the_xdr_directives() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let e = entries(root).unwrap();
        // every LG-1 alias row of the table but the deferred libdl names, plus stime
        let plan = fs::read_to_string(root.join("spec/abi/compat-plan.tsv")).unwrap();
        let lg1 = plan.lines().filter(|l| l.starts_with("libc.so.6\t") && l.contains("\talias\tLG-1\t")).count();
        let lg1_deferred = DEFERRED.iter().filter(|d| plan.contains(&format!("libc.so.6\t{d}\t"))).count();
        assert!(e.iter().filter(|e| !e.target.starts_with("liella_compat_")).count() >= lg1 - lg1_deferred);
        assert!(e.iter().any(|e| e.name == "stime" && e.target == "liella_compat_stime"));
        assert!(!e.iter().any(|e| e.name == "timer_create" && e.version == "GLIBC_2.2.5"));
    }

    /// s10_0_c2_0 (`__<name>_finite` keeps the contract of `<name>`), s0_6_c0_6_0 (`pow10`), LG-4.
    #[test]
    fn libm_aliases_come_from_the_plan_with_their_targets() {
        let t = join(VHEAD, &[
            "libm.so.6\t__exp_finite\tGLIBC_2.15\tcompat\tFUNC\tGLOBAL\t",
            "libm.so.6\tpow10\tGLIBC_2.2.5\tcompat\tFUNC\tWEAK\t",
            "libm.so.6\texp\tGLIBC_2.2.5\tcompat\tFUNC\tGLOBAL\t",
        ]);
        let plan = "library\tsymbol\tversion\tkind\tstory\tdrow\tnote\n\
                    libm.so.6\t__exp_finite\tGLIBC_2.15\talias\tLG-4\tD-10.2\ttarget=exp\n\
                    libm.so.6\texp\tGLIBC_2.2.5\tbehaviour\tLG-4\tD-10.2\tSVID\n\
                    libm.so.6\tpow10\tGLIBC_2.2.5\talias\tLG-4\tD-10.2\ttarget=exp10\n\
                    libc.so.6\tpthread_join\tGLIBC_2.2.5\talias\tLG-1\t-\t\n";
        let got: Vec<(String, String, String, bool)> = libm_entries_from(&t, plan).unwrap().into_iter().map(|e| (e.name, e.version, e.target, e.weak)).collect();
        let want = [("__exp_finite", "GLIBC_2.15", "exp", false), ("pow10", "GLIBC_2.2.5", "exp10", true)];
        assert_eq!(got, want.map(|(a, b, c, d)| (a.into(), b.into(), c.into(), d)));
        // An alias row without its versions.tsv compat row is an error, not a silent skip.
        assert!(libm_entries_from(&join(VHEAD, &[]), plan).is_err());
    }

    /// s10_0_c2_0: the real tables give every `__*_finite` name and `pow10{,f,l}`, each aimed at a
    /// different (current) function, and no `behaviour` row (those are not aliases).
    #[test]
    fn the_real_tables_give_the_libm_scope() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let e = libm_entries(root).unwrap();
        assert_eq!(e.len(), 110);
        assert_eq!(e.iter().filter(|e| e.name.starts_with("__") && e.name.ends_with("_finite")).count(), 107);
        assert!(e.iter().all(|e| e.name != e.target && !e.target.is_empty()));
        assert!(e.iter().any(|e| e.name == "__exp_finite" && e.version == "GLIBC_2.15" && e.target == "exp"));
        assert!(!e.iter().any(|e| e.name == "exp" || e.name.starts_with("totalorder") || e.name == "matherr"));
        let text = render_libm(&e);
        assert!(text.contains("\t.symver liella_compat___exp_finite_GLIBC_2_15, __exp_finite@GLIBC_2.15\n"));
        assert!(text.contains("\tjmp exp@PLT\n") && !text.contains("@@"));
    }

    #[test]
    fn rendered_stubs_are_aligned_jumps_with_one_version_each() {
        let e = Entry { name: "pthread_join".into(), version: "GLIBC_2.2.5".into(), target: "pthread_join".into(), weak: false };
        let s = render(std::slice::from_ref(&e));
        assert!(s.contains("\t.globl liella_compat_pthread_join_GLIBC_2_2_5\n"));
        assert!(s.contains("\t.p2align 6\nliella_compat_pthread_join_GLIBC_2_2_5:\n\tjmp pthread_join@PLT\n"));
        assert!(s.contains("\t.symver liella_compat_pthread_join_GLIBC_2_2_5, pthread_join@GLIBC_2.2.5\n"));
        assert!(!s.contains("@@"));
    }

    // LG-10 (spec/behaviour/regexp-h.md, s29_4_c_0): the LOADS rows reach the generated stubs.
    #[test]
    fn the_regexp_h_stubs_pass_the_got_addresses_of_loc1_and_loc2() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let s = render(&entries(root).unwrap());
        for f in ["advance", "step"] {
            let want = format!("liella_compat_{f}_GLIBC_2_2_5:\n\tmovq liella_compat_loc1_ref@GOTPCREL(%rip), %rdx\n\tmovq liella_compat_loc2_ref@GOTPCREL(%rip), %rcx\n\tjmp liella_compat_{f}@PLT\n");
            assert!(s.contains(&want), "{f}");
            assert!(s.contains(&format!("\t.symver liella_compat_{f}_GLIBC_2_2_5, {f}@GLIBC_2.2.5\n")), "{f}");
        }
        for o in ["loc1", "loc2"] {
            assert_eq!(s.matches(&format!("\t.symver liella_compat_{o}_ref, {o}@GLIBC_2.2.5\n")).count(), 1, "{o}");
        }
        assert!(!s.contains("@@"));
    }

    #[test]
    fn export_problems_name_a_dropped_alias_and_an_unknown_version() {
        let e = vec![Entry { name: "pthread_join".into(), version: "GLIBC_2.2.5".into(), target: "pthread_join".into(), weak: false }];
        let rows = vec![("pthread_join".to_string(), "GLIBC_2.2.5".to_string())];
        let good = "   1: 0000000000001000     5 FUNC    GLOBAL DEFAULT   12 pthread_join@GLIBC_2.2.5\n   2: 0000000000001040    10 FUNC    GLOBAL DEFAULT   12 pthread_join@@GLIBC_2.34\n";
        assert!(export_problems("libc.so.6", good, &e, &rows).is_empty());
        let dropped = "   2: 0000000000001040    10 FUNC    GLOBAL DEFAULT   12 pthread_join@@GLIBC_2.34\n";
        assert_eq!(export_problems("libc.so.6", dropped, &e, &rows), vec![format!("libc.so.6 does not export pthread_join@GLIBC_2.2.5 ({FILE})")]);
        let extra = format!("{good}   3: 0000000000001080     5 FUNC    GLOBAL DEFAULT   12 fgetc@GLIBC_2.1\n");
        assert_eq!(export_problems("libc.so.6", &extra, &e, &rows).len(), 1);
    }
}
