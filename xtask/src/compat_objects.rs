//! The compat data objects of `libc.so.6` (phase 15, LG-6 `LEGACY-OBJECTS`), generated into
//! `csys/src/machine/compat_aliases.S` next to LG-1's stubs (`compat.rs`, R15-1, R15-2).
//!
//! An old binary that names one of these objects gets a copy relocation of the object's `st_size`
//! bytes, so a wrong size is memory corruption. Every `libc.so.6` compat `OBJECT` row of [`NAMES`]
//! becomes one label with the manifest's size and the non-default version (single `@`); the
//! versions of one name share one table, since each older version is a prefix of the newest
//! (`spec/behaviour/compat-objects.md`, probe-settled). Contents:
//! * `sys_errlist`/`_sys_errlist`: the `strerror` texts of `spec/abi/errmsgs.tsv`, NULL where the
//!   text is "Unknown error n" and past the last errno; `sys_nerr`/`_sys_nerr`: the pointers in the
//!   same version of `sys_errlist`;
//! * `sys_siglist`/`_sys_siglist`/`sys_sigabbrev`: `spec/abi/sigmsgs.tsv`, NULL elsewhere;
//! * the inert malloc hooks, `__morecore`, `mallwatch` and `_null_auth`: zero bytes.
//!
//! `cargo xtask so` checks that every exported object of `libc.so.6` whose row `versions.tsv`
//! has carries the row's size ([`problems`]).

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Command;

/// The compat-only data objects this module generates.
pub const NAMES: &[&str] = &[
    "sys_errlist",
    "_sys_errlist",
    "sys_nerr",
    "_sys_nerr",
    "sys_siglist",
    "_sys_siglist",
    "sys_sigabbrev",
    "__malloc_hook",
    "__free_hook",
    "__realloc_hook",
    "__memalign_hook",
    "__after_morecore_hook",
    "__malloc_initialize_hook",
    "__morecore",
    "mallwatch",
    "_null_auth",
    "loc1",
    "loc2",
    "locs",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Errlist,
    Nerr,
    Siglist,
    Sigabbrev,
    Zero,
}

fn kind(name: &str) -> Kind {
    match name {
        "sys_errlist" | "_sys_errlist" => Kind::Errlist,
        "sys_nerr" | "_sys_nerr" => Kind::Nerr,
        "sys_siglist" | "_sys_siglist" => Kind::Siglist,
        "sys_sigabbrev" => Kind::Sigabbrev,
        _ => Kind::Zero,
    }
}

/// One generated object: `name@version`, `size` bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Object {
    pub name: String,
    pub version: String,
    pub size: usize,
    pub weak: bool,
}

impl Object {
    /// The object's own (local in `libc.so.6`) symbol.
    pub fn label(&self) -> String {
        format!("liella_compat_{}_{}", self.name, self.version.replace('.', "_"))
    }
}

/// The objects and the contents of the shared tables.
#[derive(Debug)]
pub struct Model {
    pub objects: Vec<Object>,
    pub errlist: Vec<Option<String>>,
    pub siglist: Vec<Option<String>>,
    pub sigabbrev: Vec<Option<String>>,
}

impl Model {
    /// The value of `sys_nerr@version`: the pointers in `sys_errlist@version`.
    pub fn nerr(&self, version: &str) -> Option<usize> {
        self.objects.iter().find(|o| kind(&o.name) == Kind::Errlist && o.version == version).map(|o| o.size / 8)
    }
}

fn version_key(v: &str) -> Vec<u32> {
    v.trim_start_matches("GLIBC_").split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

fn read(root: &Path, rel: &str) -> Result<String, String> {
    fs::read_to_string(root.join(rel)).map_err(|e| format!("{rel}: {e}"))
}

/// The model from the texts of `versions.tsv`, `errmsgs.tsv` and `sigmsgs.tsv`.
pub fn model_from(versions: &str, errmsgs: &str, sigmsgs: &str) -> Result<Model, String> {
    let mut objects = Vec::new();
    for line in versions.lines().skip(1) {
        let f: Vec<&str> = line.split('\t').collect();
        if let [lib, sym, ver, "compat", typ, scope, size] = f.as_slice() {
            if *lib != "libc.so.6" || !NAMES.contains(sym) {
                continue;
            }
            if *typ != "OBJECT" {
                return Err(format!("compat-gen: {sym}@{ver} is {typ}, not OBJECT"));
            }
            let size: usize = size.parse().map_err(|_| format!("compat-gen: {sym}@{ver} has no size"))?;
            objects.push(Object { name: sym.to_string(), version: ver.to_string(), size, weak: *scope == "WEAK" });
        }
    }
    for n in NAMES {
        if !objects.iter().any(|o| o.name == *n) {
            return Err(format!("compat-gen: {n} has no libc.so.6 compat row in spec/abi/versions.tsv"));
        }
    }
    objects.sort_by(|a, b| (a.name.as_str(), version_key(&a.version)).cmp(&(b.name.as_str(), version_key(&b.version))));
    let entries = |k: Kind| objects.iter().filter(|o| kind(&o.name) == k).map(|o| o.size / 8).max().unwrap_or(0);
    let mut errlist = vec![None; entries(Kind::Errlist)];
    for line in errmsgs.lines().skip(1) {
        let f: Vec<&str> = line.split('\t').collect();
        if let [n, _, msg] = f.as_slice() {
            let n: usize = n.parse().map_err(|_| format!("errmsgs.tsv: bad errno {n}"))?;
            if n < errlist.len() && *msg != format!("Unknown error {n}") {
                errlist[n] = Some(msg.to_string());
            }
        }
    }
    let (mut siglist, mut sigabbrev) = (vec![None; entries(Kind::Siglist)], vec![None; entries(Kind::Sigabbrev)]);
    for line in sigmsgs.lines().skip(1) {
        let f: Vec<&str> = line.split('\t').collect();
        if let [n, abbrev, descr] = f.as_slice() {
            let n: usize = n.parse().map_err(|_| format!("sigmsgs.tsv: bad signal {n}"))?;
            if n >= siglist.len() || n >= sigabbrev.len() {
                return Err(format!("sigmsgs.tsv: signal {n} does not fit the manifest's tables"));
            }
            siglist[n] = Some(descr.to_string());
            sigabbrev[n] = Some(abbrev.to_string());
        }
    }
    let model = Model { objects, errlist, siglist, sigabbrev };
    for o in &model.objects {
        match kind(&o.name) {
            Kind::Nerr if o.size != 4 || model.nerr(&o.version).is_none() => {
                return Err(format!("compat-gen: {}@{} needs 4 bytes and a sys_errlist of its version", o.name, o.version));
            }
            Kind::Errlist | Kind::Siglist | Kind::Sigabbrev if o.size % 8 != 0 => {
                return Err(format!("compat-gen: {}@{} is not a whole number of pointers", o.name, o.version));
            }
            _ => {}
        }
    }
    Ok(model)
}

/// The model of this tree.
pub fn model(root: &Path) -> Result<Model, String> {
    model_from(&read(root, "spec/abi/versions.tsv")?, &read(root, "spec/abi/errmsgs.tsv")?, &read(root, "spec/abi/sigmsgs.tsv")?)
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn declare(s: &mut String, objects: &[&Object]) {
    for o in objects {
        let l = o.label();
        let bind = if o.weak { "weak" } else { "globl" };
        s.push_str(&format!("\t.{bind} {l}\n\t.type {l}, @object\n\t.size {l}, {}\n", o.size));
    }
    for o in objects {
        s.push_str(&format!("{}:\n", o.label()));
    }
}

fn symver(s: &mut String, objects: &[&Object]) {
    for o in objects {
        s.push_str(&format!("\t.symver {}, {}@{}\n", o.label(), o.name, o.version));
    }
}

/// The assembly of the objects (directives only, K2).
pub fn render(m: &Model) -> String {
    let mut s = format!(
        "/* The compat data objects (LG-6): {} versions of {} names, each at the manifest's size; the versions of\n\
         * one name share one table (spec/behaviour/compat-objects.md). */\n",
        m.objects.len(),
        NAMES.len()
    );
    let tables = [(Kind::Errlist, "err", &m.errlist), (Kind::Siglist, "sig", &m.siglist), (Kind::Sigabbrev, "abbr", &m.sigabbrev)];
    s.push_str("\t.section .rodata.liella_compat_strings,\"aMS\",@progbits,1\n");
    for (_, tag, t) in tables {
        for (i, e) in t.iter().enumerate() {
            if let Some(text) = e {
                s.push_str(&format!(".Lliella_compat_{tag}_{i}:\n\t.asciz \"{}\"\n", escape(text)));
            }
        }
    }
    for (k, tag, t) in tables {
        let objs: Vec<&Object> = m.objects.iter().filter(|o| kind(&o.name) == k).collect();
        s.push_str(&format!("\t.section .data.rel.ro.liella_compat_{tag},\"aw\",@progbits\n\t.p2align 3\n"));
        declare(&mut s, &objs);
        for (i, e) in t.iter().enumerate() {
            match e {
                Some(_) => s.push_str(&format!("\t.quad .Lliella_compat_{tag}_{i}\n")),
                None => s.push_str("\t.quad 0\n"),
            }
        }
        symver(&mut s, &objs);
    }
    let mut nerr: BTreeMap<usize, Vec<&Object>> = BTreeMap::new();
    for o in m.objects.iter().filter(|o| kind(&o.name) == Kind::Nerr) {
        nerr.entry(m.nerr(&o.version).unwrap_or(0)).or_default().push(o);
    }
    for (v, objs) in nerr {
        s.push_str(&format!("\t.section .rodata.liella_compat_nerr_{v},\"a\",@progbits\n\t.p2align 2\n"));
        declare(&mut s, &objs);
        s.push_str(&format!("\t.long {v}\n"));
        symver(&mut s, &objs);
    }
    for o in m.objects.iter().filter(|o| kind(&o.name) == Kind::Zero) {
        s.push_str(&format!("\t.section .bss.{},\"aw\",@nobits\n\t.p2align 3\n", o.label()));
        declare(&mut s, &[o]);
        s.push_str(&format!("\t.zero {}\n", o.size));
        symver(&mut s, &[o]);
    }
    s
}

/// `text` (LG-1's rendering) with the objects before its closing GNU-stack note.
pub fn insert(text: &str, objects: &str) -> String {
    const NOTE: &str = "\t.section .note.GNU-stack";
    match text.rfind(NOTE) {
        Some(i) => format!("{}{objects}{}", &text[..i], &text[i..]),
        None => format!("{text}{objects}"),
    }
}

/// The problems of `readelf --dyn-syms -W` text of `libc.so.6`: every generated object exported as
/// `name@version` with type `OBJECT`, and every exported `OBJECT` (default or compat) that has a
/// row in `versions` with the row's size.
pub fn problems(readelf: &str, objects: &[Object], versions: &str) -> Vec<String> {
    let mut rows: BTreeMap<(String, String), usize> = BTreeMap::new();
    for line in versions.lines().skip(1) {
        let f: Vec<&str> = line.split('\t').collect();
        if let ["libc.so.6", sym, ver, _, "OBJECT", _, size] = f.as_slice() {
            if let Ok(size) = size.parse() {
                rows.insert((sym.to_string(), ver.to_string()), size);
            }
        }
    }
    let mut exported: BTreeMap<(String, String), (usize, String)> = BTreeMap::new();
    for line in readelf.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        let (Some(size), Some(typ), Some(ndx), Some(name)) = (f.get(2), f.get(3), f.get(6), f.get(7)) else { continue };
        if *ndx == "UND" {
            continue;
        }
        let size = match size.strip_prefix("0x") {
            Some(h) => usize::from_str_radix(h, 16).ok(),
            None => size.parse().ok(),
        };
        let (Some(size), Some((sym, ver))) = (size, name.split_once('@')) else { continue };
        exported.insert((sym.to_string(), ver.trim_start_matches('@').to_string()), (size, typ.to_string()));
    }
    let mut out = Vec::new();
    for o in objects {
        match exported.get(&(o.name.clone(), o.version.clone())) {
            None => out.push(format!("libc.so.6 does not export the compat object {}@{}", o.name, o.version)),
            Some((_, typ)) if typ != "OBJECT" => out.push(format!("{}@{} is {typ}, not OBJECT", o.name, o.version)),
            _ => {}
        }
    }
    for ((sym, ver), (size, typ)) in &exported {
        if let (Some(want), "OBJECT") = (rows.get(&(sym.clone(), ver.clone())), typ.as_str()) {
            if size != want {
                out.push(format!("{sym}@{ver}: st_size {size}, spec/abi/versions.tsv says {want} (a copy relocation copies the manifest's size)"));
            }
        }
    }
    out
}

/// `cargo xtask so`'s check of `so` (`libc.so.6`). The number of objects checked.
pub fn check(root: &Path, so: &Path) -> Result<usize, String> {
    let m = model(root)?;
    let out = Command::new("readelf").args(["--dyn-syms", "-W"]).arg(so).output().map_err(|e| format!("readelf: {e}"))?;
    let readelf = String::from_utf8_lossy(&out.stdout);
    let p = problems(&readelf, &m.objects, &read(root, "spec/abi/versions.tsv")?);
    let objects = readelf.lines().filter(|l| l.split_whitespace().nth(3) == Some("OBJECT")).count();
    if p.is_empty() { Ok(objects) } else { Err(format!("data objects of libc.so.6:\n  {}", p.join("\n  "))) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> &'static Path {
        Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
    }

    #[test]
    fn contents_equal_the_glibc_probe() {
        let m = model(root()).unwrap();
        let probe = read(root(), "spec/behaviour/compat-objects.glibc.txt").unwrap();
        let mut seen = BTreeMap::new();
        for line in probe.lines() {
            let Some((key, val)) = line.split_once("]=") else { continue };
            let (table, i) = key.split_once('[').unwrap();
            let t = match table {
                "sys_errlist@GLIBC_2.12" => &m.errlist,
                "sys_siglist@GLIBC_2.3.3" => &m.siglist,
                "sys_sigabbrev@GLIBC_2.3.3" => &m.sigabbrev,
                other => panic!("unexpected probe table {other}"),
            };
            let i: usize = i.parse().unwrap();
            let want = (val != "(null)").then(|| val.to_string());
            assert_eq!(t[i], want, "{table}[{i}]");
            *seen.entry(table.to_string()).or_insert(0) += 1;
        }
        assert_eq!(seen["sys_errlist@GLIBC_2.12"], m.errlist.len());
        assert_eq!(seen["sys_siglist@GLIBC_2.3.3"], m.siglist.len());
        assert_eq!(seen["sys_sigabbrev@GLIBC_2.3.3"], m.sigabbrev.len());
        // Older versions are prefixes, the zero objects are zero.
        assert!(probe.lines().filter(|l| l.contains(" vs ")).all(|l| l.contains(": 0 of ")));
        let nerr = |v: &str| m.nerr(v).unwrap();
        let line = format!(
            "sys_nerr 2.2.5={} 2.3={} 2.4={} 2.12={}; _sys_nerr 2.2.5={} 2.12={}",
            nerr("GLIBC_2.2.5"),
            nerr("GLIBC_2.3"),
            nerr("GLIBC_2.4"),
            nerr("GLIBC_2.12"),
            nerr("GLIBC_2.2.5"),
            nerr("GLIBC_2.12")
        );
        assert!(probe.lines().any(|l| l == line), "{line}");
        assert!(probe.lines().any(|l| l.starts_with("hooks null:") && !l.contains("=0")));
        assert!(probe.lines().any(|l| l == "_null_auth nonzero bytes=0"));
    }

    // LG-10 (spec/behaviour/regexp-h.md, s29_4_c_0): step/advance write loc1/loc2, so the three
    // `<regexp.h>` objects are writable zeroed storage, never a read-only table.
    #[test]
    fn the_regexp_h_objects_are_writable_zeroed_storage() {
        let s = render(&model(root()).unwrap());
        for o in ["loc1", "loc2", "locs"] {
            let l = format!("liella_compat_{o}_GLIBC_2_2_5");
            let want = format!("\t.section .bss.{l},\"aw\",@nobits\n\t.p2align 3\n\t.globl {l}\n\t.type {l}, @object\n\t.size {l}, 8\n{l}:\n\t.zero 8\n\t.symver {l}, {o}@GLIBC_2.2.5\n");
            assert!(s.contains(&want), "{o}");
        }
    }

    #[test]
    fn every_object_row_is_generated_at_its_size() {
        let m = model(root()).unwrap();
        assert_eq!(m.objects.len(), 34);
        let s = render(&m);
        for o in &m.objects {
            assert!(s.contains(&format!("\t.size {}, {}\n", o.label(), o.size)), "{o:?}");
            assert!(s.contains(&format!("\t.symver {}, {}@{}\n", o.label(), o.name, o.version)), "{o:?}");
        }
        assert!(!s.contains("@@"));
    }

    #[test]
    fn problems_name_a_wrong_size_a_missing_object_and_a_function() {
        let versions = "library\tsymbol\tversion\tbinding\ttype\tscope\tsize\nlibc.so.6\tsys_nerr\tGLIBC_2.12\tcompat\tOBJECT\tGLOBAL\t4\nlibc.so.6\tenviron\tGLIBC_2.2.5\tdefault\tOBJECT\tWEAK\t8\n";
        let o = vec![Object { name: "sys_nerr".into(), version: "GLIBC_2.12".into(), size: 4, weak: false }];
        let good = "   1: 0000000000002000     4 OBJECT  GLOBAL DEFAULT   14 sys_nerr@GLIBC_2.12\n   2: 0000000000003000     8 OBJECT  WEAK   DEFAULT   20 environ@@GLIBC_2.2.5\n";
        assert!(problems(good, &o, versions).is_empty());
        let short = good.replace("    4 OBJECT", "    2 OBJECT");
        assert_eq!(problems(&short, &o, versions).len(), 1);
        assert!(problems(&short, &o, versions)[0].contains("st_size 2"));
        assert_eq!(problems(&good.replace("     8 OBJECT", "    16 OBJECT"), &o, versions).len(), 1);
        assert_eq!(problems(&good.replace("4 OBJECT", "4 FUNC  "), &o, versions).len(), 1);
        assert_eq!(problems("", &o, versions).len(), 1);
    }
}
