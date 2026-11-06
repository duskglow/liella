//! Symbol lookup (`liella_ld::symbols`, phase 14 DL-9, decision D-27.11) over a corpus this test builds
//! with the system compiler and linker: a library with several versions of one name (hidden, default,
//! base, three versions), built with GNU, SysV and both hash styles; an unversioned stub of it; weak and
//! strong definitions; `STB_GNU_UNIQUE` objects; and two programs whose references carry versions or none.
//! The expected bindings are the reference loader's, recorded by black-box probe in
//! spec/behaviour/dynlink.md ("Symbol binding"); the malformed variants return typed errors.
//!
//! Trace: s27_1_g_0 (global scope order, symbol versioning, `STB_GNU_UNIQUE`), s27_1_c_3 (`dlsym` by name,
//! `dlvsym` by version), s27_1_f_0 (undefined symbol, version mismatch); the hash and malformed-table tests:
//! no spec clause (gABI hash functions; spec/security/threat-model.md rule 4).
//! Skips (with the reason) when `cc` is absent.
// ARCH(x86_64): the corpus is built by the host toolchain and parsed as x86-64 objects.
#![cfg(target_arch = "x86_64")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects, clippy::as_conversions, missing_docs)]

use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;

use liella_ld::arch::X86_64;
use liella_ld::elf::{dt, pt, Dynamic, Elf, ElfError, Table, STB_GNU_UNIQUE, STB_WEAK};
use liella_ld::symbols::{
    gnu_hash, lookup, resolve, sysv_hash, Binding, Found, HashStyle, LookupError, Object, Request, Scope, UniqueRegistry, Want,
};

const LIB_C: &str = r#"
#ifdef LIBV
int f_1(void) { return 1; }
__asm__(".symver f_1,f@VER_1");
int f_2(void) { return 2; }
__asm__(".symver f_2,f@@VER_2");
int h_1(void) { return 11; }
__asm__(".symver h_1,h@VER_1");
int h_2(void) { return 12; }
__asm__(".symver h_2,h@VER_2");
int b(void) { return 31; }
int g_1(void) { return 41; }
__asm__(".symver g_1,g@@VER_1");
int g_2(void) { return 42; }
__asm__(".symver g_2,g@VER_2");
int m_1(void) { return 51; }
__asm__(".symver m_1,m@VER_1");
int m_2(void) { return 52; }
__asm__(".symver m_2,m@VER_2");
int m_3(void) { return 53; }
__asm__(".symver m_3,m@@VER_3");
#endif
#ifdef LIBV_STUB
int f(void) { return -1; } int h(void) { return -1; } int b(void) { return -1; } int g(void) { return -1; } int m(void) { return -1; }
#endif
#ifdef LIBW1
__attribute__((weak)) int w(void) { return 21; }
#endif
#ifdef LIBW2
int w(void) { return 22; }
#endif
#ifdef LIBU
__asm__(".globl u_obj\n.type u_obj, @gnu_unique_object\n.size u_obj, 4\n.data\n.balign 4\nu_obj: .long 7\n.text");
extern int u_obj;
int *U_GET(void) { return &u_obj; }
#endif
#ifdef LIBU_PLAIN
int u_obj = 99;
int *u_get3(void) { return &u_obj; }
#endif
"#;

const PROG_C: &str = r#"
#include <stdio.h>
#ifdef PROG_UNVER
int f(void); int h(void); int b(void); int g(void); int m(void);
int main(void) { printf("%d %d %d %d %d\n", f(), h(), b(), g(), m()); return 0; }
#endif
#ifdef PROG_VER
int f(void);
extern int f_v1(void);
__asm__(".symver f_v1,f@VER_1");
extern int h_v2(void);
__asm__(".symver h_v2,h@VER_2");
int main(void) { printf("%d %d %d\n", f(), f_v1(), h_v2()); return 0; }
#endif
#ifdef PROG_WEAK
int w(void);
extern int uw __attribute__((weak));
extern int uwf(void) __attribute__((weak));
int main(void) { printf("%d %d %d\n", w(), &uw != 0, uwf != 0); return 0; }
#endif
"#;

fn cc(dir: &Path, args: &[&str]) {
    let o = Command::new("cc").current_dir(dir).args(args).output().unwrap();
    assert!(o.status.success(), "cc {args:?}: {}", String::from_utf8_lossy(&o.stderr));
}

fn corpus() -> Option<&'static HashMap<&'static str, Vec<u8>>> {
    static C: OnceLock<Option<HashMap<&'static str, Vec<u8>>>> = OnceLock::new();
    C.get_or_init(|| {
        if !Command::new("cc").arg("--version").output().is_ok_and(|o| o.status.success()) {
            println!("SKIP: `cc` is not available, the symbol corpus cannot be built");
            return None;
        }
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("ld_symbols_{}", std::process::id()));
        for d in ["v", "vs", "vb", "stub"] {
            std::fs::create_dir_all(dir.join(d)).unwrap();
        }
        std::fs::write(dir.join("lib.c"), LIB_C).unwrap();
        std::fs::write(dir.join("prog.c"), PROG_C).unwrap();
        std::fs::write(dir.join("v.map"), "VER_1 { global: f; h; g; m; };\nVER_2 { global: f; h; g; m; } VER_1;\nVER_3 { global: m; } VER_2;\n").unwrap();
        let so = ["-O0", "-fPIC", "-shared", "lib.c"];
        let v = ["-DLIBV", "-Wl,--version-script=v.map", "-Wl,-soname,libv.so"];
        for (out, style) in [("v/libv.so", "gnu"), ("vs/libv.so", "sysv"), ("vb/libv.so", "both")] {
            let hs = format!("-Wl,--hash-style={style}");
            cc(&dir, &[&so[..], &v[..], &[hs.as_str(), "-o", out]].concat());
        }
        cc(&dir, &[&so[..], &["-DLIBV_STUB", "-Wl,-soname,libv.so", "-o", "stub/libv.so"]].concat());
        cc(&dir, &[&so[..], &["-DLIBW1", "-Wl,-soname,libw1.so", "-o", "libw1.so"]].concat());
        cc(&dir, &[&so[..], &["-DLIBW2", "-Wl,-soname,libw2.so", "-o", "libw2.so"]].concat());
        cc(&dir, &[&so[..], &["-DLIBU", "-DU_GET=u_get1", "-o", "libu1.so"]].concat());
        cc(&dir, &[&so[..], &["-DLIBU", "-DU_GET=u_get2", "-o", "libu2.so"]].concat());
        cc(&dir, &[&so[..], &["-DLIBU_PLAIN", "-o", "libu3.so"]].concat());
        cc(&dir, &["-O0", "-DPROG_UNVER", "-o", "unver", "prog.c", "-Lstub", "-lv"]);
        cc(&dir, &["-O0", "-DPROG_VER", "-o", "ver", "prog.c", "-Lv", "-lv"]);
        cc(&dir, &["-O0", "-DPROG_WEAK", "-o", "weak", "prog.c", "-L.", "-lw1", "-lw2"]);
        let names = ["v/libv.so", "vs/libv.so", "vb/libv.so", "stub/libv.so", "libw1.so", "libw2.so", "libu1.so", "libu2.so", "libu3.so", "unver", "ver", "weak"];
        Some(names.iter().map(|n| (*n, std::fs::read(dir.join(n)).unwrap())).collect())
    })
    .as_ref()
}

fn dynm(b: &[u8]) -> Result<Dynamic<'_>, ElfError> {
    Ok(Elf::parse::<X86_64>(b)?.dynamic()?.expect("no PT_DYNAMIC"))
}

fn obj(b: &[u8], style: HashStyle) -> Object<'_> {
    Object::with_style(dynm(b).unwrap(), style).unwrap()
}

/// The version name of what `want` finds for `name`: "-" when nothing, "<base>" for index 1.
fn found(o: &Object<'_>, name: &str, want: Want<'_>) -> String {
    match o.lookup(&Request::new(name.as_bytes(), want)).unwrap() {
        None => "-".into(),
        Some(d) => o.version_name(d.version).unwrap().map_or("<base>".into(), |n| String::from_utf8(n.to_vec()).unwrap()),
    }
}

#[derive(Default)]
struct Reg(Vec<(Vec<u8>, Vec<u8>, Found)>);
impl UniqueRegistry for Reg {
    fn find(&self, name: &[u8], version: &[u8]) -> Option<Found> {
        self.0.iter().find(|e| e.0 == name && e.1 == version).map(|e| e.2)
    }
    fn record(&mut self, name: &[u8], version: &[u8], found: Found) -> Result<(), LookupError> {
        self.0.push((name.to_vec(), version.to_vec(), found));
        Ok(())
    }
}

/// The libv variants with the style each is walked with.
fn libvs(c: &'static HashMap<&'static str, Vec<u8>>) -> Vec<(&'static str, Object<'static>)> {
    vec![
        ("gnu", obj(&c["v/libv.so"], HashStyle::Gnu)),
        ("sysv", obj(&c["vs/libv.so"], HashStyle::Sysv)),
        ("both/gnu", obj(&c["vb/libv.so"], HashStyle::Gnu)),
        ("both/sysv", obj(&c["vb/libv.so"], HashStyle::Sysv)),
    ]
}

#[test]
fn hash_functions_match_the_linker() {
    // no spec clause: gABI SysV hash and the GNU hash; every symbol sits in the chain its hash selects.
    let Some(c) = corpus() else { return };
    for file in ["vb/libv.so", "unver", "libu1.so"] {
        let d = dynm(&c[file]).unwrap();
        let (sv, gh, st) = (d.sysv_hash().unwrap(), d.gnu_hash().unwrap().unwrap(), d.symtab().unwrap().unwrap());
        assert_eq!(gnu_hash(b""), 5381);
        for i in 1..st.count().unwrap() {
            let name = d.string(u64::from(st.get(i).unwrap().name)).unwrap();
            if let Some(sv) = sv {
                assert!(sv.chain(sysv_hash(name)).any(|e| e.unwrap() == i), "{file}: {} not in its SysV chain", String::from_utf8_lossy(name));
            }
            if i >= gh.symoffset() {
                let h = gnu_hash(name);
                assert!(gh.chain(h).any(|e| e.is_ok_and(|e| e.sym == i && e.hash | 1 == h | 1)), "{file}: {} not in its GNU chain", String::from_utf8_lossy(name));
            }
        }
    }
}

#[test]
fn versions_bind_like_the_reference() {
    // s27_1_g_0, s27_1_c_3: spec/behaviour/dynlink.md "Symbol binding", every hash style.
    let Some(c) = corpus() else { return };
    let rows: [(&str, Want<'_>, &str); 20] = [
        ("f", Want::Oldest, "VER_1"),
        ("h", Want::Oldest, "VER_1"),
        ("b", Want::Oldest, "<base>"),
        ("g", Want::Oldest, "VER_1"),
        ("m", Want::Oldest, "VER_1"),
        ("f", Want::Default, "VER_2"),
        ("h", Want::Default, "-"),
        ("b", Want::Default, "<base>"),
        ("g", Want::Default, "VER_1"),
        ("m", Want::Default, "VER_3"),
        ("f", Want::Version(b"VER_1"), "VER_1"),
        ("f", Want::Version(b"VER_2"), "VER_2"),
        ("h", Want::Version(b"VER_1"), "VER_1"),
        ("h", Want::Version(b"VER_2"), "VER_2"),
        ("m", Want::Version(b"VER_2"), "VER_2"),
        ("f", Want::Version(b"VER_9"), "-"),
        ("b", Want::Version(b"VER_1"), "-"),
        ("b", Want::Version(b"libv.so"), "-"),
        ("nope", Want::Oldest, "-"),
        ("nope", Want::Default, "-"),
    ];
    for (style, o) in libvs(c) {
        for (name, want, expect) in rows {
            assert_eq!(found(&o, name, want), expect, "{style}: {name} {want:?}");
        }
    }
    // An object without version tables: every definition matches every request.
    let stub = obj(&c["stub/libv.so"], HashStyle::Gnu);
    for want in [Want::Oldest, Want::Default, Want::Version(b"VER_1")] {
        assert_eq!(found(&stub, "f", want), "<base>");
    }
}

#[test]
fn gnu_and_sysv_give_the_same_answers() {
    // no spec clause: design §5, the two hash tables of one object give the same definitions.
    let Some(c) = corpus() else { return };
    let (g, s) = (obj(&c["vb/libv.so"], HashStyle::Gnu), obj(&c["vb/libv.so"], HashStyle::Sysv));
    let st = g.symtab();
    for i in 0..st.count().unwrap() {
        let name = g.dynamic().string(u64::from(st.get(i).unwrap().name)).unwrap();
        for want in [Want::Oldest, Want::Default, Want::Version(b"VER_1"), Want::Version(b"VER_2"), Want::Version(b"VER_3"), Want::Version(b"libv.so")] {
            let r = Request::new(name, want);
            assert_eq!(g.lookup(&r).unwrap(), s.lookup(&r).unwrap(), "{} {want:?}", String::from_utf8_lossy(name));
        }
    }
}

/// (name, version found) for every undefined reference of program `prog` named in `names`, against `lib`.
fn bind_program(prog: &[u8], lib: &[u8], names: &[&str]) -> Vec<(String, String)> {
    let (p, l) = (obj(prog, HashStyle::Gnu), obj(lib, HashStyle::Gnu));
    let mut out = Vec::new();
    for i in 1..p.symtab().count().unwrap() {
        let st = p.symtab().get(i).unwrap();
        let name = String::from_utf8(p.dynamic().string(u64::from(st.name)).unwrap().to_vec()).unwrap();
        if st.shndx == 0 && names.contains(&name.as_str()) {
            out.push((name.clone(), found(&l, &name, p.reference_want(i).unwrap())));
        }
    }
    out.sort();
    out
}

#[test]
fn programs_bind_like_the_reference() {
    // s27_1_g_0, s27_1_f_0: the probe's programs; a reference's version comes from its versym/verneed entry.
    let Some(c) = corpus() else { return };
    let s = |v: &[(&str, &str)]| v.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect::<Vec<_>>();
    for lib in ["v/libv.so", "vs/libv.so"] {
        let got = bind_program(&c["ver"], &c[lib], &["f", "h"]);
        assert_eq!(got, s(&[("f", "VER_1"), ("f", "VER_2"), ("h", "VER_2")]), "{lib}");
        let got = bind_program(&c["unver"], &c[lib], &["f", "h", "b", "g", "m"]);
        assert_eq!(got, s(&[("b", "<base>"), ("f", "VER_1"), ("g", "VER_1"), ("h", "VER_1"), ("m", "VER_1")]), "{lib}");
    }
    // The program's own undefined entries are not definitions.
    assert_eq!(found(&obj(&c["ver"], HashStyle::Gnu), "f", Want::Oldest), "-");
}

#[test]
fn weak_definitions_are_ordinary_and_undefined_weak_is_zero() {
    // s27_1_g_0, s27_1_f_0: the first object in scope order wins, weak or strong; an undefined weak
    // reference binds to 0, an undefined strong one fails.
    let Some(c) = corpus() else { return };
    let objs = [obj(&c["libw1.so"], HashStyle::Gnu), obj(&c["libw2.so"], HashStyle::Gnu), obj(&c["weak"], HashStyle::Gnu)];
    let mut reg = Reg::default();
    let w = Request::new(b"w", Want::Oldest);
    let first = lookup(&objs, Scope::global(&[0, 1]), &w, &mut reg).unwrap().unwrap();
    assert_eq!((first.obj, first.def.st.bind()), (0, STB_WEAK));
    assert_eq!(lookup(&objs, Scope::global(&[1, 0]), &w, &mut reg).unwrap().unwrap().obj, 1);
    let p = &objs[2];
    let mut weak_refs = 0;
    for i in 1..p.symtab().count().unwrap() {
        let st = p.symtab().get(i).unwrap();
        let name = p.dynamic().string(u64::from(st.name)).unwrap();
        if name == b"uw" || name == b"uwf" {
            assert_eq!((st.shndx, st.bind()), (0, STB_WEAK));
            weak_refs += 1;
            let r = Request::new(name, p.reference_want(i).unwrap());
            assert_eq!(resolve(&objs, Scope::global(&[2, 0, 1]), &r, true, &mut reg), Ok(Binding::WeakUndefined));
            assert_eq!(resolve(&objs, Scope::global(&[2, 0, 1]), &r, false, &mut reg), Err(LookupError::Undefined));
        }
    }
    assert_eq!(weak_refs, 2);
}

#[test]
fn unique_definitions_are_shared_in_a_namespace() {
    // s27_1_g_0: STB_GNU_UNIQUE, one definition per namespace across separate local scopes.
    let Some(c) = corpus() else { return };
    let objs = [obj(&c["libu1.so"], HashStyle::Gnu), obj(&c["libu2.so"], HashStyle::Gnu), obj(&c["libu3.so"], HashStyle::Gnu)];
    let mut reg = Reg::default();
    let u = Request::new(b"u_obj", Want::Oldest);
    let a = lookup(&objs, Scope::group(&[], &[0], false), &u, &mut reg).unwrap().unwrap();
    assert_eq!((a.obj, a.def.st.bind()), (0, STB_GNU_UNIQUE));
    assert_eq!(lookup(&objs, Scope::group(&[], &[1], false), &u, &mut reg).unwrap().unwrap(), a);
    assert_eq!(lookup(&objs, Scope::group(&[], &[2], false), &u, &mut reg).unwrap().unwrap().obj, 2);
    assert_eq!(reg.0.len(), 1);
}

#[test]
fn scopes_order_and_exclusion() {
    // s27_1_g_0, s27_1_c_3: global then local, RTLD_DEEPBIND local first, RTLD_NEXT after the caller,
    // a COPY lookup without the program.
    let Some(c) = corpus() else { return };
    let objs = [obj(&c["libw1.so"], HashStyle::Gnu), obj(&c["libw2.so"], HashStyle::Gnu)];
    let mut reg = Reg::default();
    let w = Request::new(b"w", Want::Default);
    let at = |s: Scope<'_>, reg: &mut Reg| lookup(&objs, s, &w, reg).unwrap().map(|f| f.obj);
    assert_eq!(at(Scope::group(&[1], &[0], false), &mut reg), Some(1));
    assert_eq!(at(Scope::group(&[1], &[0], true), &mut reg), Some(0));
    assert_eq!(at(Scope::next(&[0, 1], &[], 0).unwrap(), &mut reg), Some(1));
    assert_eq!(at(Scope::next(&[0, 1], &[], 1).unwrap(), &mut reg), None);
    assert_eq!(at(Scope::next(&[], &[1, 0], 1).unwrap(), &mut reg), Some(0));
    assert!(Scope::next(&[0], &[], 1).is_none());
    assert_eq!(at(Scope::global(&[0, 1]).excluding(0), &mut reg), Some(1));
    assert_eq!(lookup(&objs, Scope::global(&[5]), &w, &mut reg), Err(LookupError::NoObject(5)));
}

/// The file offset of the table at `vaddr` (the corpus libraries map their first PT_LOAD at 0).
fn off(b: &[u8], vaddr: u64) -> usize {
    let e = Elf::parse::<X86_64>(b).unwrap();
    let ph = e.program_headers().find(|p| p.p_type == pt::LOAD).unwrap();
    assert_eq!((ph.offset, ph.vaddr), (0, 0));
    assert!(vaddr < ph.filesz);
    vaddr as usize
}

fn put(b: &mut [u8], at: usize, v: u32) {
    b[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

fn get(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

fn err_of(b: &[u8], style: HashStyle, name: &str, want: Want<'_>) -> LookupError {
    let r = Object::with_style(dynm(b).unwrap(), style).and_then(|o| o.lookup(&Request::new(name.as_bytes(), want)));
    r.expect_err("malformed table accepted")
}

#[test]
fn malformed_tables_give_typed_errors() {
    // no spec clause: threat-model rule 4; chain loops, out-of-range indices, bad entry sizes.
    let Some(c) = corpus() else { return };
    // SysV: a bucket past nchain; a chain that loops on a symbol of another name.
    let base = &c["vs/libv.so"];
    let h = off(base, dynm(base).unwrap().get(dt::HASH).unwrap());
    let (nbucket, nchain) = (get(base, h), get(base, h + 4));
    let bucket = h + 8 + 4 * (sysv_hash(b"f") % nbucket) as usize;
    let mut b = base.clone();
    put(&mut b, bucket, nchain + 3);
    assert_eq!(err_of(&b, HashStyle::Sysv, "f", Want::Oldest), LookupError::Elf(ElfError::HashIndex(Table::SysvHash, nchain + 3)));
    let mut b = base.clone();
    put(&mut b, bucket, 1);
    put(&mut b, h + 8 + 4 * (nbucket as usize + 1), 1);
    assert!(matches!(err_of(&b, HashStyle::Sysv, "f", Want::Oldest), LookupError::Elf(ElfError::HashChainLoop(1))));
    // GNU: a bucket below symoffset, a bucket past the mapped chain.
    let base = &c["v/libv.so"];
    let g = off(base, dynm(base).unwrap().get(dt::GNU_HASH).unwrap());
    let (nb, symoff, bloom) = (get(base, g), get(base, g + 4), get(base, g + 8));
    let bucket = g + 16 + 8 * bloom as usize + 4 * (gnu_hash(b"f") % nb) as usize;
    assert!(symoff > 1);
    for bad in [symoff - 1, 0x7fff_0000] {
        let mut b = base.clone();
        put(&mut b, bucket, bad);
        assert_eq!(err_of(&b, HashStyle::Gnu, "f", Want::Default), LookupError::Elf(ElfError::HashIndex(Table::GnuHash, bad)), "bucket {bad}");
    }
    // The symbol table: an index at the count; a DT_SYMENT other than 24.
    let st = dynm(base).unwrap().symtab().unwrap().unwrap();
    let n = st.count().unwrap();
    assert_eq!(st.get(n), Err(ElfError::SymbolIndex(n)));
    assert!(st.get(n - 1).is_ok());
    let d = Elf::parse::<X86_64>(base).unwrap().find(pt::DYNAMIC).unwrap();
    let mut b = base.clone();
    let e = (0..d.filesz as usize / 16).map(|i| d.offset as usize + 16 * i).find(|&e| get(base, e) == dt::SYMENT as u32).expect("DT_SYMENT");
    put(&mut b, e + 8, 16);
    assert_eq!(Object::new(dynm(&b).unwrap()).err(), Some(LookupError::Elf(ElfError::BadDynamicValue(dt::SYMENT))));
    // versym: index 0 (local) hides every definition of the name; an index above the definitions is refused.
    let o = obj(base, HashStyle::Gnu);
    let vs = off(base, dynm(base).unwrap().get(dt::VERSYM).unwrap());
    let fs: Vec<u32> = (1..n).filter(|&i| o.dynamic().string(u64::from(st.get(i).unwrap().name)).unwrap() == b"f").collect();
    assert_eq!(fs.len(), 2);
    let mut b = base.clone();
    for &i in &fs {
        b[vs + 2 * i as usize..vs + 2 * i as usize + 2].copy_from_slice(&0u16.to_le_bytes());
    }
    assert_eq!(found(&obj(&b, HashStyle::Gnu), "f", Want::Oldest), "-");
    let mut b = base.clone();
    b[vs + 2 * fs[0] as usize..vs + 2 * fs[0] as usize + 2].copy_from_slice(&0x7ff0u16.to_le_bytes());
    assert_eq!(err_of(&b, HashStyle::Gnu, "f", Want::Oldest), LookupError::Elf(ElfError::VersionIndex(fs[0], 0x7ff0)));
}

#[test]
fn any_byte_changed_never_panics() {
    // no spec clause: threat-model rule 4; each byte of the lookup tables set to 0xff or 0x80 gives a
    // result or a typed error, never a panic.
    let Some(c) = corpus() else { return };
    let base = &c["vb/libv.so"];
    let d = dynm(base).unwrap();
    let tabs: Vec<usize> = [dt::HASH, dt::GNU_HASH, dt::SYMTAB, dt::STRTAB, dt::VERSYM, dt::VERDEF].iter().map(|&t| off(base, d.get(t).unwrap())).collect();
    let (lo, hi) = (*tabs.iter().min().unwrap(), (*tabs.iter().max().unwrap() + 256).min(base.len()));
    let names = ["f", "h", "b", "g", "m", "nope"];
    for at in lo..hi {
        for v in [0xff, 0x80] {
            let mut b = base.clone();
            b[at] = v;
            let Ok(d) = dynm(&b) else { continue };
            for style in [HashStyle::Gnu, HashStyle::Sysv] {
                let Ok(o) = Object::with_style(d, style) else { continue };
                for n in names {
                    for want in [Want::Oldest, Want::Default, Want::Version(b"VER_2")] {
                        let _ = o.lookup(&Request::new(n.as_bytes(), want));
                    }
                }
            }
        }
    }
}
