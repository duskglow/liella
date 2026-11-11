//! Eager relocation (phase 14 DL-10 `DL-RELOC`, plan/ph14/design.md §6, L14-6, L14-8, D-27.17).
//!
//! Trace: s27_1_c_1 (loading objects and their dependencies, relocation before the program runs),
//! s27_1_g_0 (IFUNC resolvers run at bind time, after their object is relocated), s27_1_f_0
//! (the version check). The executable-stack refusal: no spec clause (R14-13, threat model loader
//! rule 10, docs/differences-from-glibc.md). x86_64 only: the corpus is built by the system C
//! compiler and the arithmetic table is the x86-64 psABI's.
//!
//! Three layers: (1) `Arch::apply` for every non-TLS kind; (2) the relocator over a model of memory
//! for a corpus of objects the system linker built (one per relocation type), checking each relocated
//! field, that every resolver runs after its object and the objects it depends on are relocated, and
//! that a text-relocated segment is never writable and executable; (3) the built `ld.so` running a
//! `-nostdlib` program with several shared objects, a program needing a version against an unversioned
//! stub (status 127 and a message), executable-stack objects, and a write into RELRO.
// ARCH(x86_64): the corpus is built by the host toolchain with x86-64 inline assembly and relocation types.
#![cfg(target_arch = "x86_64")]
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::arithmetic_side_effects, clippy::as_conversions, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use liella_ld::arch::{ApplyError, Arch, Field, Native, Operands, RelocKind};
use liella_ld::elf::{Elf, Span, SpanKind};
use liella_ld::map::{PROT_EXEC, PROT_WRITE};
use liella_ld::reloc::{self, Image, Obj, Objects, RelocError, UniqueTable};
use liella_ld::symbols::{Object, Request, Want};

fn target() -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| root.join("target"), PathBuf::from)
}

fn cc_ok() -> bool {
    let ok = Command::new("cc").arg("--version").output().is_ok_and(|o| o.status.success());
    if !ok {
        eprintln!("SKIP: no system C compiler to build the corpus");
    }
    ok
}

fn product() -> Option<PathBuf> {
    let p = target().join("liella").join(Native::INTERP);
    if p.exists() {
        return Some(p);
    }
    eprintln!("SKIP: {} not built (run cargo xtask so)", p.display());
    None
}

fn dir(name: &str) -> PathBuf {
    let d = target().join(format!("ld-reloc-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Build `src` (C) into `out` in `d` with `flags`.
fn build(d: &Path, out: &str, src: &str, flags: &[&str]) -> PathBuf {
    let c = d.join(format!("{out}.c"));
    std::fs::write(&c, src).unwrap();
    let o = d.join(out);
    let r = Command::new("cc")
        .args(["-nostdlib", "-O1", "-fno-stack-protector", "-fcf-protection=none", "-Wl,-z,relro", "-Wl,-z,now", "-Wl,--hash-style=gnu"])
        .arg(&c)
        .args(flags)
        .arg("-o")
        .arg(&o)
        .output()
        .unwrap();
    assert!(r.status.success(), "{out}: {}", String::from_utf8_lossy(&r.stderr));
    o
}

const LIBB: &str = "int b_data = 7; int b_fn(void) { return 5; }\n";
const LIBA: &str = "extern int b_data; extern int b_fn(void);
int a_local = 3;
int *a_ptr = &b_data;                       /* R_X86_64_64 against another object */
int *a_lp = &a_local + 2;                   /* 64 with an addend (binds the program's COPY) */
static int a_s = 4;
int *a_sp = &a_s;                           /* RELATIVE */
int a_call(void) { return b_fn(); }         /* JUMP_SLOT */
int a_get(void) { return b_data; }          /* GLOB_DAT */
static int impl(void) { return a_get() + 1; }
static void *pick(void) { return (void *)impl; }
int a_ifunc(void) __attribute__((ifunc(\"pick\")));
int (*a_ifunc_ptr)(void) = a_ifunc;         /* 64 bound to an IFUNC (deferred) */
static int l_ifunc(void) __attribute__((ifunc(\"pick\")));
int (*a_lifunc_ptr)(void) = l_ifunc;        /* IRELATIVE (a local IFUNC) */
";
const LIBT: &str = "int t_target = 11;
__asm__(\".text\\n.globl t_word\\n.type t_word,@object\\nt_word: .quad t_target\\n.size t_word, 8\\n\");
";
/// The program: exits 0 when every relocated value is right, else the number of the first wrong one.
const PROG: &str = "extern int b_data; extern int b_fn(void); extern int a_call(void); extern int a_get(void);
extern int a_ifunc(void); extern int a_local; extern int *a_ptr; extern int *a_lp; extern int (*a_ifunc_ptr)(void);
extern int (*a_lifunc_ptr)(void); extern int t_target; extern long t_word; extern int *a_sp;
static void bye(int c) { __asm__ volatile(\"syscall\" :: \"a\"(231), \"D\"(c)); __builtin_unreachable(); }
static int check(void) {
  if (b_data != 7) return 1;                 /* COPY */
  if (a_get() != 7) return 2;                /* the copy is the one definition */
  if (a_call() != 5 || b_fn() != 5) return 3;
  if (a_ptr != &b_data) return 4;
  if (a_lp != &a_local + 2) return 5;
  if (a_ifunc() != 8 || a_ifunc_ptr() != 8 || a_lifunc_ptr() != 8) return 6;
  if (t_word != (long)&t_target) return 7;   /* TEXTREL */
  if (*a_sp != 4) return 8;                  /* RELATIVE */
  return 0;
}
void _start(void) { bye(check()); }
";

/// The corpus: (libb, liba needing libb, libt text-relocated, the program), DT_NEEDED by path.
fn corpus(d: &Path, prog_flags: &[&str]) -> [PathBuf; 4] {
    let b = build(d, "libb.so", LIBB, &["-fPIC", "-shared"]);
    let bs = b.to_str().unwrap();
    let a = build(d, "liba.so", LIBA, &["-fPIC", "-shared", bs]);
    let t = build(d, "libt.so", LIBT, &["-fPIC", "-shared", "-Wl,-z,notext"]);
    let mut f = vec!["-fPIE", "-pie", a.to_str().unwrap(), bs, t.to_str().unwrap()];
    f.extend_from_slice(prog_flags);
    let p = build(d, "prog", PROG, &f);
    [b, a, t, p]
}

// ------------------------------------------------------------------ (1) the arithmetic

/// no spec clause: x86-64 psABI §4.4.1 calculations through the arch seam (design §0a).
#[test]
fn reloc_apply_x86_64_arithmetic() {
    let o = Operands { s: 0x7000_0000_1000, a: -8, b: 0x5555_0000_0000, p: 0x5555_0000_2000 };
    let w = |value| Ok(Some(Field { value, width: 8 }));
    assert_eq!(Native::apply(RelocKind::Relative, o), w(0x5554_ffff_fff8), "RELATIVE is B + A");
    assert_eq!(Native::apply(RelocKind::Irelative, o), w(0x5554_ffff_fff8), "IRELATIVE's resolver is B + A");
    assert_eq!(Native::apply(RelocKind::Abs64, o), w(0x7000_0000_0ff8), "64 is S + A");
    assert_eq!(Native::apply(RelocKind::GlobDat, o), w(0x7000_0000_1000), "GLOB_DAT is S");
    assert_eq!(Native::apply(RelocKind::JumpSlot, o), w(0x7000_0000_1000), "JUMP_SLOT is S");
    assert_eq!(Native::apply(RelocKind::None, o), Ok(None));
    let near = Operands { s: 0x40_1000, a: 4, b: 0, p: 0x40_0000 };
    let pc32 = RelocKind::TextRel { width: 4, pc_relative: true, signed: true };
    assert_eq!(Native::apply(pc32, near), Ok(Some(Field { value: 0x1004, width: 4 })), "PC32 is S + A - P");
    let back = Operands { s: 0x40_0000, a: 0, b: 0, p: 0x40_1000 };
    assert_eq!(Native::apply(pc32, back), Ok(Some(Field { value: (-0x1000i64) as u64, width: 4 })), "PC32 negative fits");
    assert_eq!(Native::apply(pc32, o), Err(ApplyError::Overflow), "PC32 out of range");
    let r32 = RelocKind::TextRel { width: 4, pc_relative: false, signed: false };
    let r32s = RelocKind::TextRel { width: 4, pc_relative: false, signed: true };
    let at = |s| Operands { s, a: 0, b: 0, p: 0 };
    assert_eq!(Native::apply(r32, at(0xffff_ffff)), Ok(Some(Field { value: 0xffff_ffff, width: 4 })));
    assert_eq!(Native::apply(r32, at(0x1_0000_0000)), Err(ApplyError::Overflow), "32 is unsigned");
    assert_eq!(Native::apply(r32s, at(0xffff_ffff)), Err(ApplyError::Overflow), "32S is signed");
    assert_eq!(Native::apply(r32s, at(0xffff_ffff_8000_0000)), Ok(Some(Field { value: 0xffff_ffff_8000_0000, width: 4 })));
    for k in [RelocKind::Copy, RelocKind::TpOff, RelocKind::DtpMod, RelocKind::DtpOff, RelocKind::TlsDesc, RelocKind::Unsupported(99)] {
        assert_eq!(Native::apply(k, o), Err(ApplyError::NoArithmetic), "{k:?}");
    }
}

// ------------------------------------------------------------------ (2) the model

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum Ev {
    /// A store into object `.0`; `.1`: it is a resolver's result.
    Store(usize, bool),
    /// A resolver of object `.0` called.
    Call(usize),
}

/// Model memory: each object's segments at its bias; an event log; protections per page.
struct Model {
    mem: BTreeMap<u64, Vec<u8>>,
    spans: Vec<(u64, u64)>,
    log: Vec<Ev>,
    prot: BTreeMap<u64, i32>,
    after_call: bool,
}

impl Model {
    fn obj(&self, at: u64) -> usize {
        self.spans.iter().position(|&(s, e)| s <= at && at < e).expect("address in an object")
    }
    fn bytes(&mut self, at: u64, len: usize) -> &mut [u8] {
        let (&s, v) = self.mem.range_mut(..=at).next_back().unwrap();
        let o = (at - s) as usize;
        &mut v[o..o + len]
    }
}

impl Image for Model {
    fn store(&mut self, at: u64, bytes: &[u8]) {
        let o = self.obj(at);
        self.log.push(Ev::Store(o, std::mem::take(&mut self.after_call)));
        self.bytes(at, bytes.len()).copy_from_slice(bytes);
    }
    fn load(&mut self, at: u64, out: &mut [u8]) {
        out.copy_from_slice(self.bytes(at, out.len()));
    }
    fn copy(&mut self, to: u64, from: u64, len: u64) {
        let src = self.bytes(from, len as usize).to_vec();
        self.store(to, &src);
    }
    fn call(&mut self, resolver: u64) -> u64 {
        self.log.push(Ev::Call(self.obj(resolver)));
        self.after_call = true;
        0xabc0_0000_0000 + resolver
    }
    fn protect(&mut self, at: u64, len: u64, prot: i32) -> Result<(), ()> {
        assert!(prot & PROT_WRITE == 0 || prot & PROT_EXEC == 0, "W+X protection asked at {at:#x}");
        (0..len / 4096).for_each(|i| {
            self.prot.insert(at + i * 4096, prot);
        });
        Ok(())
    }
}

/// Bias of object `i` in the model.
fn bias(i: usize) -> u64 {
    0x1000_0000 * (i as u64 + 1)
}

fn sym_value(file: &[u8], name: &[u8]) -> u64 {
    let elf = Elf::parse::<Native>(file).unwrap();
    let o = Object::new(elf.dynamic().unwrap().unwrap()).unwrap();
    o.lookup(&Request::new(name, Want::Oldest)).unwrap().unwrap_or_else(|| panic!("{}", String::from_utf8_lossy(name))).st.value
}

fn word(m: &mut Model, at: u64) -> u64 {
    u64::from_le_bytes(m.bytes(at, 8).try_into().unwrap())
}

/// s27_1_c_1, s27_1_g_0: the corpus relocated in the model: each type's field holds its value, every
/// resolver runs after its object and its dependencies are relocated, TEXTREL is never W+X.
#[test]
fn reloc_model_corpus_one_object_per_type() {
    if !cc_ok() {
        return;
    }
    let d = dir("model");
    let paths = corpus(&d, &[]);
    // Load order: program, liba, libb, libt.
    let files: Vec<Vec<u8>> = [&paths[3], &paths[1], &paths[0], &paths[2]].iter().map(|p| std::fs::read(p).unwrap()).collect();
    let names: Vec<Vec<u8>> = [&paths[3], &paths[1], &paths[0], &paths[2]].iter().map(|p| p.to_str().unwrap().as_bytes().to_vec()).collect();
    let mut m = Model { mem: BTreeMap::new(), spans: vec![], log: vec![], prot: BTreeMap::new(), after_call: false };
    let mut objs = vec![];
    for (i, f) in files.iter().enumerate() {
        let elf = Elf::parse::<Native>(f).unwrap();
        let (mut lo, mut hi) = (u64::MAX, 0);
        for ph in elf.program_headers().filter(|p| p.p_type == 1) {
            let mut v = vec![0u8; ph.memsz as usize];
            v[..ph.filesz as usize].copy_from_slice(elf.segment_bytes(&ph).unwrap());
            m.mem.insert(bias(i) + ph.vaddr, v);
            (lo, hi) = (lo.min(bias(i) + ph.vaddr), hi.max(bias(i) + ph.vaddr + ph.memsz));
        }
        m.spans.push((lo, hi));
        objs.push(Obj { elf, dynm: elf.dynamic().unwrap().unwrap(), bias: bias(i), name: &names[i], tls: None });
    }
    let syms: Vec<Object> = objs.iter().map(|o| Object::new(o.dynm).unwrap()).collect();
    let global: Vec<usize> = (0..objs.len()).collect();
    let set = Objects { objs: &objs, syms: &syms, global: &global };
    reloc::check_versions(&objs).unwrap();
    let mut unique = UniqueTable::<8, 256>::default();
    reloc::relocate_all::<Native, _>(&set, &mut m, 4096, &mut unique).unwrap();

    let (pa, pb, pt) = (&files[1], &files[2], &files[3]);
    let a_ptr = bias(1) + sym_value(pa, b"a_ptr");
    let b_copy = bias(0) + sym_value(&files[0], b"b_data");
    assert_eq!(word(&mut m, a_ptr), b_copy, "64 against b_data binds the program's COPY (S + A)");
    let a_lp = bias(1) + sym_value(pa, b"a_lp");
    assert_eq!(word(&mut m, a_lp), bias(0) + sym_value(&files[0], b"a_local") + 8, "a_lp = &a_local + 2 (S + A, the COPY)");
    let a_sp = word(&mut m, bias(1) + sym_value(pa, b"a_sp"));
    assert_eq!(m.bytes(a_sp, 4), &4i32.to_le_bytes(), "RELATIVE: a_sp = B + A points at a_s");
    assert_eq!(&m.bytes(b_copy, 4)[..], &7i32.to_le_bytes(), "COPY copied b_data's bytes");
    let t_word = bias(3) + sym_value(pt, b"t_word");
    assert_eq!(word(&mut m, t_word), bias(0) + sym_value(&files[0], b"t_target"), "TEXTREL word = S + A (the COPY)");
    for (sym, what) in [(&b"a_lifunc_ptr"[..], "IRELATIVE"), (&b"a_ifunc_ptr"[..], "64 bound to an IFUNC")] {
        let ifp = word(&mut m, bias(1) + sym_value(pa, sym));
        assert!(ifp > 0xabc0_0000_0000 && (m.spans[1].0..m.spans[1].1).contains(&(ifp - 0xabc0_0000_0000)), "{what} holds its resolver's result: {ifp:#x}");
    }
    // GLOB_DAT / JUMP_SLOT of every object hold S of the definition the scope finds first. Count
    // the relocations that need a resolver: each IRELATIVE, and each symbol reference bound to an IFUNC.
    let (mut needs_resolver, mut irelative) = (0, 0);
    for (i, o) in objs.iter().enumerate() {
        for r in o.dynm.rela::<Native>().unwrap().into_iter().flatten().chain(o.dynm.jmprel::<Native>().unwrap().into_iter().flatten()) {
            if matches!(r.kind, RelocKind::Irelative) {
                needs_resolver += 1;
                irelative += 1;
            }
            if matches!(r.kind, RelocKind::Abs64 | RelocKind::GlobDat | RelocKind::JumpSlot | RelocKind::TextRel { .. }) && r.sym != 0 {
                let name = o.dynm.string(u64::from(syms[i].symtab().get(r.sym).unwrap().name)).unwrap();
                let def = (0..objs.len()).find_map(|j| syms[j].lookup(&Request::new(name, Want::Oldest)).unwrap());
                needs_resolver += usize::from(def.is_some_and(|d| d.st.kind() == 10));
            }
            if matches!(r.kind, RelocKind::GlobDat | RelocKind::JumpSlot) {
                let st = syms[i].symtab().get(r.sym).unwrap();
                let name = o.dynm.string(u64::from(st.name)).unwrap();
                let def = (0..objs.len()).find_map(|j| syms[j].lookup(&Request::new(name, Want::Oldest)).unwrap().map(|d| (j, d))).unwrap();
                let v = word(&mut m, o.bias + r.offset);
                if def.1.st.kind() == 10 {
                    assert!(v > 0xabc0_0000_0000, "{} bound to an IFUNC holds the resolver's result", String::from_utf8_lossy(name));
                } else {
                    assert_eq!(v, bias(def.0) + def.1.st.value, "{} in object {i}", String::from_utf8_lossy(name));
                }
            }
        }
    }
    // Ordering (threat rule 6, L14-8): no relocation store of the resolver's object or of any object
    // after it in load order happens after a resolver call, except resolver results.
    let calls: Vec<usize> = m.log.iter().enumerate().filter_map(|(k, e)| matches!(e, Ev::Call(_)).then_some(k)).collect();
    assert!(!calls.is_empty(), "the corpus calls resolvers");
    assert!(irelative > 0, "the corpus has an IRELATIVE (liba's local IFUNC)");
    // A resolver runs once per relocation that needs it, in the deferred pass only: a call in pass 1
    // as well would run it before its object's later entries are applied, and twice.
    assert_eq!(calls.len(), needs_resolver, "each IFUNC resolver runs once per relocation that needs it: {:?}", m.log);
    for &k in &calls {
        let Ev::Call(j) = m.log[k] else { unreachable!() };
        for e in &m.log[k..] {
            if let Ev::Store(o, false) = *e {
                assert!(o < j, "IFUNC resolver of object {j} ran before object {o} was relocated: {:?}", m.log);
            }
        }
    }
    // TEXTREL: libt's text pages were writable during pass 1 and are executable, not writable, after.
    let text = objs[3].elf.program_headers().find(|p| p.p_type == 1 && p.flags & 1 != 0).unwrap();
    let page = (bias(3) + text.vaddr) & !4095;
    assert_eq!(m.prot.get(&page), Some(&(1 | PROT_EXEC)), "libt's text restored to r-x");
    // RELRO ranges come after relocation and cover each object's PT_GNU_RELRO pages.
    reloc::protect_relro(&objs, &mut m, 4096).unwrap();
    let (at, _) = reloc::relro_range(&objs[1], 4096).expect("liba has RELRO");
    assert_eq!(m.prot.get(&at), Some(&1), "liba's RELRO read-only");
    let _ = pb;
    std::fs::remove_dir_all(&d).ok();
}

/// s27_1_f_0: a versioned reference to an object without version tables stops the load (D-27.11,
/// ruling #3655); a missing version of a versioned object too; a TLS relocation is refused.
#[test]
fn reloc_version_check_and_refusals_in_the_model() {
    if !cc_ok() {
        return;
    }
    let d = dir("ver");
    std::fs::write(d.join("v.map"), "VER_1 { global: v_fn; local: *; };\n").unwrap();
    let map = format!("-Wl,--version-script={}", d.join("v.map").display());
    let v = build(&d, "libv.so", "int v_fn(void) { return 1; }\n", &["-fPIC", "-shared", &map]);
    let p = build(&d, "pv", "extern int v_fn(void); void _start(void) { v_fn(); for (;;); }\n", &["-fPIE", "-pie", v.to_str().unwrap()]);
    let stub = std::fs::read(build(&d, "stub.so", "int v_fn(void) { return 1; }\n", &["-fPIC", "-shared"])).unwrap();
    let pf = std::fs::read(&p).unwrap();
    let vf = std::fs::read(&v).unwrap();
    let vname = v.to_str().unwrap().as_bytes();
    let obj = |f: &'static [u8], name: &'static [u8]| {
        let elf = Elf::parse::<Native>(f).unwrap();
        Obj { elf, dynm: elf.dynamic().unwrap().unwrap(), bias: 0, name, tls: None }
    };
    let leak = |b: Vec<u8>| -> &'static [u8] { Box::leak(b.into_boxed_slice()) };
    let (pf, vf, stub, vname) = (leak(pf), leak(vf), leak(stub), leak(vname.to_vec()));
    assert_eq!(reloc::check_versions(&[obj(pf, b"pv"), obj(vf, vname)]), Ok(()));
    assert_eq!(
        reloc::check_versions(&[obj(pf, b"pv"), obj(stub, vname)]),
        Err(reloc::VersionError::NoVersions { by: 0, file: vname, version: b"VER_1" }),
        "unversioned stub refused"
    );
    std::fs::write(d.join("w.map"), "VER_2 { global: v_fn; local: *; };\n").unwrap();
    let wmap = format!("-Wl,--version-script={}", d.join("w.map").display());
    let wf = leak(std::fs::read(build(&d, "libw.so", "int v_fn(void) { return 1; }\n", &["-fPIC", "-shared", &wmap])).unwrap());
    assert_eq!(
        reloc::check_versions(&[obj(pf, b"pv"), obj(wf, vname)]),
        Err(reloc::VersionError::Missing { by: 0, file: vname, version: b"VER_1" }),
        "missing version refused"
    );
    assert_eq!(RelocError::Tls(36).message(), b"TLS descriptor relocations are not available in this build\n");
    std::fs::remove_dir_all(&d).ok();
}

// ------------------------------------------------------------------ (3) the built ld.so

fn run(cmd: &mut Command) -> (Option<i32>, Option<i32>, String) {
    use std::os::unix::process::ExitStatusExt;
    let o = cmd.output().unwrap();
    (o.status.code(), o.status.signal(), String::from_utf8_lossy(&o.stderr).into_owned())
}

/// s27_1_c_1, s27_1_g_0: a `-nostdlib` program with three shared objects (64, RELATIVE, GLOB_DAT,
/// JUMP_SLOT, COPY, IRELATIVE, IFUNC-bound references, TEXTREL) runs and sees every value right, both
/// as `ld.so PROGRAM` and started by the kernel through `PT_INTERP`.
#[test]
fn reloc_ldso_runs_multi_object_program() {
    let (true, Some(ld)) = (cc_ok(), product()) else { return };
    let d = dir("run");
    let [.., p] = corpus(&d, &[]);
    for _ in 0..4 {
        let (code, sig, err) = run(Command::new(&ld).arg(&p));
        assert_eq!((code, sig), (Some(0), None), "ld.so PROG: check number {code:?} failed; stderr: {err}");
    }
    let interp = format!("-Wl,--dynamic-linker={}", ld.display());
    let [.., p2] = corpus(&d, &[&interp]);
    let (code, sig, err) = run(&mut Command::new(&p2));
    assert_eq!((code, sig), (Some(0), None), "PT_INTERP: check number {code:?} failed; stderr: {err}");
    std::fs::remove_dir_all(&d).ok();
}

/// no spec clause (L14-4, ruling #4231): a library reached by two paths (its own and a symlink) is one
/// object, loaded once: the program sees exactly one executable mapping of it.
#[test]
fn reloc_ldso_one_object_by_two_paths() {
    let (true, Some(ld)) = (cc_ok(), product()) else { return };
    let d = dir("ident");
    let b = build(&d, "libb.so", LIBB, &["-fPIC", "-shared"]);
    let alias = d.join("libalias.so");
    std::os::unix::fs::symlink(&b, &alias).unwrap();
    // Copies /proc/self/maps to standard output; exits 0 when libb.so's function answers.
    let src = "extern int b_fn(void);
static long sc(long n, long a, long b, long c) { long r; __asm__ volatile(\"syscall\" : \"=a\"(r) : \"a\"(n), \"D\"(a), \"S\"(b), \"d\"(c) : \"rcx\", \"r11\", \"memory\"); return r; }
static char buf[1 << 16];
void _start(void) {
  long fd = sc(2, (long)\"/proc/self/maps\", 0, 0), n = 0, r;
  while (fd >= 0 && (r = sc(0, fd, (long)buf + n, sizeof buf - n)) > 0) n += r;
  sc(1, 1, (long)buf, n);
  sc(60, b_fn() == 5 ? 0 : 99, 0, 0);
}
";
    // The executable mappings of the program run by ld.so (the loader maps without file names, so
    // count them all and compare with the same program naming libb.so once).
    let exec_maps = |p: &Path| {
        let o = Command::new(&ld).arg(p).output().unwrap();
        let maps = String::from_utf8_lossy(&o.stdout).into_owned();
        let n = maps.lines().filter(|l| l.split_whitespace().nth(1).is_some_and(|p| p.contains('x'))).count();
        (o.status.code(), n, maps)
    };
    let once = build(&d, "pone", src, &["-fPIE", "-pie", b.to_str().unwrap()]);
    let p = build(&d, "pid", src, &["-fPIE", "-pie", "-Wl,--no-as-needed", b.to_str().unwrap(), alias.to_str().unwrap()]);
    let file = std::fs::read(&p).unwrap();
    assert!(file.windows(12).any(|w| w == b"libalias.so\0"), "the program names the symlink too");
    let (c1, n1, _) = exec_maps(&once);
    let (c2, n2, maps) = exec_maps(&p);
    assert_eq!((c1, c2), (Some(0), Some(0)), "both programs run");
    assert_eq!(n2, n1, "libb.so must be mapped executable exactly once:\n{maps}");
    std::fs::remove_dir_all(&d).ok();
}

/// s27_1_f_0: a program needing VER_1 run against an unversioned stub stops with status 127 and a
/// message naming the object and the version (D-27.11, ruling #3655).
#[test]
fn reloc_ldso_version_needed_from_unversioned_stub() {
    let (true, Some(ld)) = (cc_ok(), product()) else { return };
    let d = dir("stub");
    std::fs::write(d.join("v.map"), "VER_1 { global: v_fn; local: *; };\n").unwrap();
    let map = format!("-Wl,--version-script={}", d.join("v.map").display());
    let v = build(&d, "libv.so", "int v_fn(void) { return 1; }\n", &["-fPIC", "-shared", &map]);
    let src = "extern int v_fn(void); void _start(void) { __asm__ volatile(\"syscall\" :: \"a\"(231), \"D\"(v_fn() - 1)); }\n";
    let p = build(&d, "pv", src, &["-fPIE", "-pie", v.to_str().unwrap()]);
    let (code, sig, err) = run(Command::new(&ld).arg(&p));
    assert_eq!((code, sig), (Some(0), None), "versioned library: {err}");
    build(&d, "libv.so", "int v_fn(void) { return 1; }\n", &["-fPIC", "-shared"]);
    let (code, sig, err) = run(Command::new(&ld).arg(&p));
    assert_eq!((code, sig), (Some(127), None), "unversioned stub must stop the load: {err}");
    assert!(err.contains("libv.so") && err.contains("VER_1") && err.contains("not found"), "message names object and version: {err}");
    std::fs::remove_dir_all(&d).ok();
}

/// no spec clause (R14-13, threat model loader rule 10): an object or a program asking for an
/// executable stack is refused with status 127, and nothing runs.
#[test]
fn reloc_ldso_refuses_executable_stack() {
    let (true, Some(ld)) = (cc_ok(), product()) else { return };
    let d = dir("xstack");
    let ok = "void _start(void) { __asm__ volatile(\"syscall\" :: \"a\"(231), \"D\"(0)); }\n";
    let x = build(&d, "libx.so", "int x_fn(void) { return 1; }\n", &["-fPIC", "-shared", "-Wl,-z,execstack"]);
    // The program references nothing in libx.so: keep the DT_NEEDED entry where the toolchain links
    // --as-needed by default.
    let p = build(&d, "px", ok, &["-fPIE", "-pie", "-Wl,--no-as-needed", x.to_str().unwrap()]);
    let (code, sig, err) = run(Command::new(&ld).arg(&p));
    assert_eq!((code, sig), (Some(127), None), "library with an executable stack: {err}");
    assert!(err.contains("libx.so") && err.contains("executable stack"), "{err}");
    let q = build(&d, "pq", ok, &["-fPIE", "-pie", "-Wl,-z,execstack"]);
    let (code, sig, err) = run(Command::new(&ld).arg(&q));
    assert_eq!((code, sig), (Some(127), None), "program with an executable stack: {err}");
    assert!(err.contains("executable stack"), "{err}");
    std::fs::remove_dir_all(&d).ok();
}

/// s27_1_c_1, design §6 (RELRO last): after relocation a dependency's RELRO data is read-only: a
/// program writing through it dies with SIGSEGV, while the same program reading it exits 0.
#[test]
fn reloc_ldso_relro_read_only_after_relocation() {
    let (true, Some(ld)) = (cc_ok(), product()) else { return };
    let d = dir("relro");
    let r = build(&d, "libr.so", "int r_x = 1; int *const r_p = &r_x; int **r_pp(void) { return (int **)&r_p; }\n", &["-fPIC", "-shared"]);
    let src = |write: bool| {
        format!(
            "extern int **r_pp(void); extern int r_x;
void _start(void) {{ int **pp = r_pp(); int c = *pp == &r_x ? 0 : 3; if ({}) *pp = 0; __asm__ volatile(\"syscall\" :: \"a\"(231), \"D\"(c)); }}\n",
            u8::from(write)
        )
    };
    let read = build(&d, "pread", &src(false), &["-fPIE", "-pie", r.to_str().unwrap()]);
    let (code, sig, err) = run(Command::new(&ld).arg(&read));
    assert_eq!((code, sig), (Some(0), None), "reading RELRO data: {err}");
    let write = build(&d, "pwrite", &src(true), &["-fPIE", "-pie", r.to_str().unwrap()]);
    let (code, sig, _) = run(Command::new(&ld).arg(&write));
    assert_eq!((code, sig), (None, Some(11)), "writing RELRO data after relocation must fault");
    std::fs::remove_dir_all(&d).ok();
}

/// s27_1_c_1, design §6 (RELRO last), D-27.25 (amends D-27.10): a dependency whose `PT_GNU_RELRO`
/// spans two writable `PT_LOAD`s (GNU ld keeps RELRO inside one segment, so `split_relro_load`
/// rewrites the linked object's program headers) is relocated in both segments and its RELRO pages
/// in each segment are read-only afterwards: reading both ends exits 0, writing either dies with
/// SIGSEGV. Catches RELRO protected through only one segment's range.
#[test]
fn reloc_ldso_relro_across_segments() {
    let (true, Some(ld)) = (cc_ok(), product()) else { return };
    let d = dir("relro-split");
    // 1100 relocated pointers in .data.rel.ro: RELRO covers at least two whole pages.
    let src = "int r_x = 1; int *const r_p[1100] = { [0 ... 1099] = &r_x };
int **r_first(void) { return (int **)&r_p[0]; } int **r_last(void) { return (int **)&r_p[1099]; }\n";
    let r = build(&d, "libr.so", src, &["-fPIC", "-shared"]);
    let (file, at) = split_relro_load(&std::fs::read(&r).unwrap());
    let p = sym_value(&file, b"r_p");
    assert!(p < at && p + 1099 * 8 >= at, "r_p[0] ({p:#x}) must lie in the first segment and r_p[1099] in the second (split at {at:#x})");
    // Written under another name and copied over the linked object by cp(1) (no ETXTBSY window).
    let tmp = d.join("libr.bytes");
    std::fs::write(&tmp, &file).unwrap();
    assert!(Command::new("cp").arg(&tmp).arg(&r).status().unwrap().success());
    let prog = |name: &str, write: &str| {
        let s = format!(
            "extern int **r_first(void); extern int **r_last(void); extern int r_x;
void _start(void) {{ int **f = r_first(), **l = r_last(); int c = *f == &r_x && *l == &r_x ? 0 : 3; {write}
__asm__ volatile(\"syscall\" :: \"a\"(231), \"D\"(c)); }}\n"
        );
        build(&d, name, &s, &["-fPIE", "-pie", r.to_str().unwrap()])
    };
    let (code, sig, err) = run(Command::new(&ld).arg(prog("pread", "")));
    assert_eq!((code, sig), (Some(0), None), "reading RELRO in both segments: {err}");
    for (name, write) in [("pfirst", "*f = 0;"), ("plast", "*l = 0;")] {
        let (code, sig, _) = run(Command::new(&ld).arg(prog(name, write)));
        assert_eq!((code, sig), (None, Some(11)), "{name}: writing RELRO in that segment after relocation must fault");
    }
    std::fs::remove_dir_all(&d).ok();
}

/// `f` with the writable `PT_LOAD` holding `PT_GNU_RELRO`'s last whole page split in two at that
/// page (`at`, returned): the same bytes at the same addresses, now two adjacent segments, so RELRO
/// spans a segment boundary. The table keeps its size: the `PT_NOTE` header (unused by the loader)
/// gives its slot, and the second half follows the first so the `PT_LOAD`s stay in address order.
fn split_relro_load(f: &[u8]) -> (Vec<u8>, u64) {
    const PAGE: u64 = 4096;
    let rd = |o: usize| u64::from_le_bytes(f[o..o + 8].try_into().unwrap());
    let (phoff, phnum) = (rd(32) as usize, usize::from(u16::from_le_bytes([f[56], f[57]])));
    let hs: Vec<&[u8]> = (0..phnum).map(|i| &f[phoff + i * 56..phoff + i * 56 + 56]).collect();
    let ty = |h: &[u8]| u32::from_le_bytes(h[..4].try_into().unwrap());
    let word = |h: &[u8], i: usize| u64::from_le_bytes(h[8 * i..8 * i + 8].try_into().unwrap());
    let relro = hs.iter().find(|h| ty(h) == 0x6474_e552).expect("PT_GNU_RELRO");
    let (r, m) = (word(relro, 2), word(relro, 5));
    let at = ((r + m) & !(PAGE - 1)) - PAGE;
    assert!(at > r & !(PAGE - 1), "RELRO must cover two whole pages");
    let load = hs.iter().position(|h| ty(h) == 1 && word(h, 2) < at && at < word(h, 2) + word(h, 4)).expect("the PT_LOAD holding RELRO");
    let note = hs.iter().position(|h| ty(h) == 4).expect("a PT_NOTE slot to reuse");
    let set = |h: &[u8], o: u64, v: u64, fsz: u64, msz: u64, align: u64| {
        let mut n = h[..8].to_vec();
        for w in [o, v, v, fsz, msz, align] {
            n.extend(w.to_le_bytes());
        }
        n
    };
    let l = hs[load];
    let cut = at - word(l, 2);
    let mut table = Vec::new();
    for (i, h) in hs.iter().enumerate() {
        if i == load {
            table.push(set(l, word(l, 1), word(l, 2), cut, cut, word(l, 6)));
            table.push(set(l, word(l, 1) + cut, at, word(l, 4) - cut, word(l, 5) - cut, PAGE));
        } else if i != note {
            table.push(h.to_vec());
        }
    }
    assert_eq!(table.len(), phnum);
    let mut v = f.to_vec();
    v[phoff..phoff + 56 * phnum].copy_from_slice(&table.concat());
    (v, at)
}

// ------------------------------------------------------------------ (4) the program's memory image

/// A program (TEXTREL with `-Wl,-z,notext`) with one dependency that copies nothing: exits 0 when the
/// text holding `_start` is mapped `r-x` (never writable) when its code first runs and its
/// text-relocated word is right; 10 when the text is writable or not executable, 20 when no mapping
/// holds it, 11 for a wrong word.
const TEXT_PROG: &str = "extern int b_fn(void); int p_target = 9;
__asm__(\".text\\n.globl p_word\\n.type p_word,@object\\np_word: .quad p_target\\n.size p_word, 8\\n\");
extern long p_word;
void _start(void);
static long sc(long n, long a, long b, long c) { long r; __asm__ volatile(\"syscall\" : \"=a\"(r) : \"a\"(n), \"D\"(a), \"S\"(b), \"d\"(c) : \"rcx\", \"r11\", \"memory\"); return r; }
static char buf[1 << 16];
static unsigned long hex(const char **p) { unsigned long v = 0; for (;;) { char c = **p; int d = c >= '0' && c <= '9' ? c - '0' : c >= 'a' && c <= 'f' ? c - 'a' + 10 : -1; if (d < 0) return v; v = v * 16 + d; (*p)++; } }
void _start(void) {
  long fd = sc(2, (long)\"/proc/self/maps\", 0, 0), n = 0, r;
  while (fd >= 0 && (r = sc(0, fd, (long)buf + n, sizeof buf - 1 - n)) > 0) n += r;
  buf[n] = 0;
  unsigned long me = (unsigned long)&_start; int code = 20;
  for (const char *p = buf; *p;) {
    unsigned long lo = hex(&p); p++; unsigned long hi = hex(&p); p++;
    if (lo <= me && me < hi) code = (p[0] == 'r' && p[1] == '-' && p[2] == 'x') ? 0 : 10;
    while (*p && *p != '\\n') p++;
    if (*p) p++;
  }
  if (code == 0 && (p_word != (long)&p_target || b_fn() != 5)) code = 11;
  sc(60, code, 0, 0);
}
";

/// The file bytes laid out at their link-time addresses (what the kernel maps, before relocation).
fn memory_of(file: &[u8], elf: &Elf<'_>) -> Vec<u8> {
    let loads: Vec<_> = elf.program_headers().filter(|p| p.p_type == 1).collect();
    let mut mem = vec![0u8; loads.iter().map(|p| (p.vaddr + p.memsz) as usize).max().unwrap()];
    for p in &loads {
        mem[p.vaddr as usize..(p.vaddr + p.filesz) as usize].copy_from_slice(&file[p.offset as usize..(p.offset + p.filesz) as usize]);
    }
    mem
}

/// Everything the relocator reads through the reader, as text (equal for two readers of one object).
fn reader_view(elf: &Elf<'_>) -> String {
    let d = elf.dynamic().unwrap().unwrap();
    let needed: Vec<_> = d.needed().map(|n| n.map(<[u8]>::to_vec)).collect();
    let count = d.symbol_count().unwrap().unwrap_or(0);
    let tab = d.symtab().unwrap().unwrap();
    let syms: Vec<_> = (0..count).map(|i| tab.get(i)).collect();
    let rela = d.rela::<Native>().unwrap().map(Iterator::collect::<Vec<_>>);
    let jmprel = d.jmprel::<Native>().unwrap().map(Iterator::collect::<Vec<_>>);
    format!("{:?}\n{needed:?}\n{syms:?}\n{rela:?}\n{jmprel:?}", d.entries().collect::<Vec<_>>())
}

/// no spec clause (ruling #4229, #4323, D-27.18): the image-mode reader slices in place only `PT_LOAD`s
/// whose bytes cannot change during loading (never a writable one, never any of a text-relocated
/// program), copies the others that hold tables, and reads exactly what the file reader reads, for a
/// program with a GOT and for a text-relocated one.
#[test]
fn reloc_image_reader_slices_only_unchanging_segments() {
    if !cc_ok() {
        return;
    }
    let d = dir("image");
    let [b, .., p] = corpus(&d, &[]);
    let t = build(&d, "ptext", TEXT_PROG, &["-fPIE", "-pie", "-Wl,-z,notext", b.to_str().unwrap()]);
    for (prog, textrel) in [(p, false), (t, true)] {
        let file = std::fs::read(&prog).unwrap();
        let elf = Elf::parse::<Native>(&file).unwrap();
        let dynph = elf.find(liella_ld::elf::pt::DYNAMIC).unwrap();
        let dynamic = elf.segment_bytes(&dynph).unwrap().to_vec();
        let mem = memory_of(&file, &elf);
        let loads: Vec<_> = elf.program_headers().filter(|p| p.p_type == 1 && p.filesz != 0).collect();
        let kinds: Vec<_> = loads.iter().map(|p| elf.image_span_kind(p, &dynamic)).collect();
        for (p, k) in loads.iter().zip(&kinds) {
            assert!(
                !(*k == SpanKind::Live && (textrel || p.flags & 2 != 0)),
                "{}: a segment the relocator may store into is never sliced in place (flags {:#x}): {kinds:?}",
                prog.display(),
                p.flags
            );
        }
        assert_eq!(kinds.contains(&SpanKind::Copy), textrel, "{}: only the text-relocated program needs whole-segment copies: {kinds:?}", prog.display());
        let range = |p: &liella_ld::elf::ProgramHeader| p.vaddr as usize..(p.vaddr + p.filesz) as usize;
        let copies: Vec<(u64, Vec<u8>)> = loads.iter().zip(&kinds).filter(|(_, k)| **k == SpanKind::Copy).map(|(p, _)| (p.vaddr, mem[range(p)].to_vec())).collect();
        let mut spans: Vec<Span<'_>> = copies.iter().map(|(v, b)| Span { vaddr: *v, bytes: b }).collect();
        spans.push(Span { vaddr: dynph.vaddr, bytes: &dynamic });
        spans.extend(loads.iter().zip(&kinds).filter(|(_, k)| **k == SpanKind::Live).map(|(p, _)| Span { vaddr: p.vaddr, bytes: &mem[range(p)] }));
        let h = elf.header();
        let phdrs = &file[h.phoff as usize..h.phoff as usize + usize::from(h.phnum) * 56];
        let image = Elf::from_image::<Native>(phdrs, &spans).unwrap();
        assert_eq!(reader_view(&image), reader_view(&elf), "{}: the image reader reads what the file reader reads", prog.display());
    }
    std::fs::remove_dir_all(&d).ok();
}

/// s27_1_c_1 (ruling #4229): a program started by the kernel through `PT_INTERP` loads its
/// dependencies with no `/proc` at all (a chroot holding only the loader, the objects and the program).
#[test]
fn reloc_ldso_kernel_started_without_proc() {
    let (true, Some(ld)) = (cc_ok(), product()) else { return };
    if !Command::new("unshare").args(["-r", "true"]).status().is_ok_and(|s| s.success()) {
        eprintln!("SKIP: unshare -r (an unprivileged user namespace for chroot) is not available here");
        return;
    }
    let d = std::fs::canonicalize(dir("noproc")).unwrap();
    let ld = std::fs::canonicalize(ld).unwrap();
    let interp = format!("-Wl,--dynamic-linker={}", ld.display());
    let files = corpus(&d, &[&interp]);
    let root = d.join("root");
    let inside = |p: &Path| root.join(p.strip_prefix("/").unwrap());
    for f in files.iter().chain([&ld]) {
        std::fs::create_dir_all(inside(f).parent().unwrap()).unwrap();
        std::fs::copy(f, inside(f)).unwrap();
    }
    assert!(!root.join("proc").exists());
    // `unshare --root` changes the root itself: no chroot(8), which sits outside many PATHs.
    let (code, sig, err) = run(Command::new("unshare").arg("-r").arg(format!("--root={}", root.display())).arg(&files[3]));
    assert_eq!((code, sig), (Some(0), None), "kernel-started program without /proc: check number {code:?} failed; stderr: {err}");
    std::fs::remove_dir_all(&d).ok();
}

/// s27_1_c_1 (ruling #4371, D-27.18): a position-independent program without `PT_PHDR` runs, started
/// by the kernel and as `ld.so PROGRAM`: its load bias comes from its ELF header and the `PT_LOAD` that
/// holds its program headers (never 0 for want of `PT_PHDR`). Two programs: one with dependencies
/// (relocated and protected by `link`) and one without (RELATIVE relocations and its RELRO pages
/// handled in `run`, which then protects the pages at that bias).
#[test]
fn reloc_ldso_program_without_pt_phdr() {
    let (true, Some(ld)) = (cc_ok(), product()) else { return };
    let d = dir("nophdr");
    let interp = format!("-Wl,--dynamic-linker={}", ld.display());
    let [.., p] = corpus(&d, &[&interp]);
    let alone = build(
        &d,
        "palone",
        "int x = 1; int *const p = &x;
void _start(void) { int *q = *(int *const volatile *)&p; int c = q == &x ? 0 : 3; __asm__ volatile(\"syscall\" :: \"a\"(231), \"D\"(c)); }\n",
        &["-fPIE", "-pie", &interp],
    );
    for (i, p) in [p, alone].iter().enumerate() {
        without_pt_phdr(&ld, &d, p, &format!("prog{i}-nophdr"));
    }
    std::fs::remove_dir_all(&d).ok();
}

/// Run `p` with its `PT_PHDR` made `PT_NULL` (written to `d/name`), kernel-started and as `ld.so PROGRAM`.
fn without_pt_phdr(ld: &Path, d: &Path, p: &Path, name: &str) {
    let mut file = std::fs::read(p).unwrap();
    let elf = Elf::parse::<Native>(&file).unwrap();
    let (phoff, phnum) = (elf.header().phoff as usize, usize::from(elf.header().phnum));
    let at = (0..phnum).map(|i| phoff + i * 56).find(|&o| file[o..o + 4] == 6u32.to_le_bytes()).expect("the linker gave the program a PT_PHDR");
    file[at..at + 4].copy_from_slice(&0u32.to_le_bytes()); // PT_PHDR -> PT_NULL
    // Written under another name and copied by cp(1): the program run below is never open for
    // writing in this process, so a fork by another test thread cannot make its exec fail (ETXTBSY).
    let (tmp, q) = (d.join(format!("{name}.bytes")), d.join(name));
    std::fs::write(&tmp, &file).unwrap();
    std::fs::set_permissions(&tmp, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    assert!(Command::new("cp").arg(&tmp).arg(&q).status().unwrap().success());
    assert!(Elf::parse::<Native>(&file).unwrap().find(liella_ld::elf::pt::PHDR).is_none());
    let (code, sig, err) = run(&mut Command::new(&q));
    assert_eq!((code, sig), (Some(0), None), "{name}: kernel-started program without PT_PHDR: check number {code:?} failed; stderr: {err}");
    let (code, sig, err) = run(Command::new(ld).arg(&q));
    assert_eq!((code, sig), (Some(0), None), "{name}: ld.so PROGRAM without PT_PHDR: check number {code:?} failed; stderr: {err}");
}

/// A copy of `lib/libc.so.6` at `d/name` whose `DT_SONAME` is retagged `DT_CHECKSUM` (so a program
/// linked against it names it by path: `ld.so` has no library search yet), changed by `edit`.
fn libc_by_path(lib: &Path, d: &Path, name: &str, edit: impl FnOnce(&mut Vec<u8>)) -> std::path::PathBuf {
    let mut so = std::fs::read(lib.join("libc.so.6")).unwrap();
    let elf = Elf::parse::<Native>(&so).unwrap();
    let dynph = elf.find(liella_ld::elf::pt::DYNAMIC).unwrap();
    let (off, len) = (dynph.offset as usize, dynph.filesz as usize);
    let at = (off..off + len).step_by(16).find(|&o| so[o..o + 8] == 14u64.to_le_bytes()).expect("libc.so.6 has a DT_SONAME");
    so[at..at + 8].copy_from_slice(&0x6fff_fdf8u64.to_le_bytes());
    edit(&mut so);
    let out = d.join(name);
    std::fs::write(&out, &so).unwrap();
    out
}

/// The program of [`reloc_libc_early_init_before_preinit`]: its `.preinit_array` function, the first
/// program code to run, records 1 when the C library has no environment yet (`getenv` of a variable
/// the test sets), 2 when the NULL-folding copy and fill with a zero length do not return; `main`
/// exits with what it recorded (0: initialised).
const EARLY_PROG: &str = "#include <stdlib.h>
#include <string.h>
static int seen = 3;
static void pre(int argc, char **argv, char **envp) {
  (void)argc; (void)argv; (void)envp;
  if (getenv(\"LIELLA_EARLY_PROBE\") == 0) { seen = 1; return; }
  seen = 2;
  memcpy(0, 0, 0);
  memset(0, 0, 0);
  seen = 0;
}
__attribute__((section(\".preinit_array\"), used)) static void (*pre_entry)(int, char **, char **) = pre;
int main(void) { return seen; }
";

/// Link [`EARLY_PROG`] as a PIE against `libc` (by path) with `ld` as its interpreter.
fn early_prog(ld: &Path, d: &Path, libc: &Path, out: &str) -> std::path::PathBuf {
    let crt1 = target().join("liella").join("liella-crt1.o");
    let c = d.join("early.c");
    std::fs::write(&c, EARLY_PROG).unwrap();
    let o = d.join(out);
    let r = Command::new("cc").args(["-O1", "-fno-builtin", "-nostdlib", "-fPIE", "-pie"]).arg(format!("-Wl,--dynamic-linker={}", ld.display())).arg("-o").arg(&o).arg(&crt1).arg(&c).arg(libc).arg("-lgcc").output().unwrap();
    assert!(r.status.success(), "{out}: {}", String::from_utf8_lossy(&r.stderr));
    o
}

/// s27_1_c_8, D-27.22 (design §14): `ld.so` calls libc's `__liella_libc_early_init` before any
/// constructor, so the program's `.preinit_array` function finds the environment published and
/// `memcpy(NULL, NULL, 0)`/`memset(NULL, 0, 0)` return; kernel-started and as `ld.so PROGRAM`.
#[test]
fn reloc_libc_early_init_before_preinit() {
    let (true, Some(ld)) = (cc_ok(), product()) else { return };
    let lib = target().join("liella");
    if !lib.join("liella-crt1.o").exists() || !lib.join("libc.so.6").exists() {
        eprintln!("SKIP: liella-crt1.o or libc.so.6 not built (run cargo xtask so)");
        return;
    }
    let d = std::fs::canonicalize(dir("libcearly")).unwrap();
    let libc = libc_by_path(&lib, &d, "libc-early.so", |_| {});
    let p = early_prog(&ld, &d, &libc, "early");
    for (how, mut cmd) in [("kernel-started", Command::new(&p)), ("ld.so PROGRAM", { let mut c = Command::new(&ld); c.arg(&p); c })] {
        let (code, sig, err) = run(cmd.env("LIELLA_EARLY_PROBE", "1"));
        assert_eq!((code, sig), (Some(0), None), "{how}: early init before .preinit_array: check number {code:?} failed; stderr: {err}");
    }
    std::fs::remove_dir_all(&d).ok();
}

/// s27_1_c_8, L14-10, D-27.22: a `libc.so.6` whose `LIELLA_PRIVATE` build tag differs from the
/// loader's (another build) is refused at load with the diagnostic, before any of its code runs.
#[test]
fn reloc_libc_build_tag_mismatch_refused() {
    let (true, Some(ld)) = (cc_ok(), product()) else { return };
    let lib = target().join("liella");
    if !lib.join("liella-crt1.o").exists() || !lib.join("libc.so.6").exists() {
        eprintln!("SKIP: liella-crt1.o or libc.so.6 not built (run cargo xtask so)");
        return;
    }
    let d = std::fs::canonicalize(dir("libctag")).unwrap();
    let prefix = liella_ld::start::private::TAG_PREFIX;
    let libc = libc_by_path(&lib, &d, "libc-other.so", |so| {
        let at = so.windows(prefix.len()).position(|w| w == prefix).expect("libc.so.6 carries a build tag") + prefix.len();
        so[at] ^= 0x20;
    });
    let p = early_prog(&ld, &d, &libc, "other");
    for (how, mut cmd) in [("kernel-started", Command::new(&p)), ("ld.so PROGRAM", { let mut c = Command::new(&ld); c.arg(&p); c })] {
        let (code, sig, err) = run(cmd.env("LIELLA_EARLY_PROBE", "1"));
        assert_eq!((code, sig), (Some(127), None), "{how}: a libc from another build ran; stderr: {err}");
        assert!(err.contains("libc-other.so: LIELLA_PRIVATE build tag differs"), "{how}: the build-tag diagnostic is missing: {err}");
    }
    std::fs::remove_dir_all(&d).ok();
}

/// The program of [`reloc_libc_bias_without_pt_phdr`]: exits 0 when libc's view of the executable's
/// load bias is the real one (`AT_ENTRY` minus the header's `e_entry`, found without libc's help):
/// 1 when `dl_iterate_phdr`'s first object has another `dlpi_addr`; a `%n` in a read-only format
/// passed to `__printf_chk` aborts (the fortify check places the format by that bias, D-12.4) when
/// the bias is wrong.
const LIBC_BIAS_PROG: &str = "#define _GNU_SOURCE
#include <elf.h>
#include <link.h>
#include <stdint.h>
#include <sys/auxv.h>
extern const ElfW(Ehdr) __ehdr_start;
extern int __printf_chk(int, const char *, ...);
static const char ro[] = \"%n\";
static int first(struct dl_phdr_info *i, size_t n, void *d) { (void)n; *(uintptr_t *)d = i->dlpi_addr; return 1; }
int main(void) {
  uintptr_t want = getauxval(AT_ENTRY) - __ehdr_start.e_entry, got = 1;
  int w = -1;
  dl_iterate_phdr(first, &got);
  if (got != want) return 1;
  __printf_chk(1, ro, &w);
  return w == 0 ? 0 : 2;
}
";

/// s27_1_c_8, D-27.18, D-27.12 (DL-LIBC-START-BIAS-3): libc's own lookups of the executable
/// (`dl_iterate_phdr`, the `%n` check) see its real load bias when it has no `PT_PHDR`: a PIE and
/// an ET_EXEC linked against `libc.so.6`, started by the kernel through `PT_INTERP` and as
/// `ld.so PROGRAM`, and a static ET_EXEC (which the linker gives no `PT_PHDR`). `libc.so.6` is
/// named by path (a copy whose `DT_SONAME` is retagged `DT_CHECKSUM`): `ld.so` has no library search
/// yet. Keep naming it by path until phase 14's library search lands, then link by soname.
#[test]
fn reloc_libc_bias_without_pt_phdr() {
    let (true, Some(ld)) = (cc_ok(), product()) else { return };
    let lib = target().join("liella");
    let (crt1, libc_a) = (lib.join("liella-crt1.o"), lib.join("libc.a"));
    if !crt1.exists() || !libc_a.exists() {
        eprintln!("SKIP: {} or libc.a not built (run cargo xtask so)", crt1.display());
        return;
    }
    let d = std::fs::canonicalize(dir("libcbias")).unwrap();
    let libc_so = libc_by_path(&lib, &d, "libc-by-path.so", |_| {});
    let c = d.join("bias.c");
    std::fs::write(&c, LIBC_BIAS_PROG).unwrap();
    let interp = format!("-Wl,--dynamic-linker={}", ld.display());
    let link = |out: &str, flags: &[&str], libc: &Path| {
        let o = d.join(out);
        let r = Command::new("cc").args(["-O1", "-fno-builtin", "-nostdlib", "-D_FORTIFY_SOURCE=0"]).args(flags).arg("-o").arg(&o).arg(&crt1).arg(&c).arg(libc).arg("-lgcc").output().unwrap();
        assert!(r.status.success(), "{out}: {}", String::from_utf8_lossy(&r.stderr));
        o
    };
    let pie = link("pie", &["-fPIE", "-pie", &interp], &libc_so);
    let exe = link("exe", &["-fno-pie", "-no-pie", &interp], &libc_so);
    for (i, p) in [pie, exe].iter().enumerate() {
        without_pt_phdr(&ld, &d, p, &format!("libc{i}-nophdr"));
    }
    let stat = link("static", &["-static", "-fno-pie", "-no-pie"], &libc_a);
    let file = std::fs::read(&stat).unwrap();
    assert!(Elf::parse::<Native>(&file).unwrap().find(liella_ld::elf::pt::PHDR).is_none(), "the static ET_EXEC has no PT_PHDR");
    let (code, sig, err) = run(&mut Command::new(&stat));
    assert_eq!((code, sig), (Some(0), None), "static ET_EXEC without PT_PHDR: check number {code:?} failed; stderr: {err}");
    std::fs::remove_dir_all(&d).ok();
}

/// s27_1_c_1, D-27.15, D-27.18: a program with dependencies and 300 `PT_LOAD`s (no count bound) runs
/// as `ld.so PROGRAM`. `link` reads it through the image reader, whose span list
/// holds one span per `PT_LOAD` (the `-nostdlib` 300-segment program of the map tests never links).
#[test]
fn reloc_ldso_program_with_many_loads() {
    let (true, Some(ld)) = (cc_ok(), product()) else { return };
    let d = dir("manyloads");
    let interp = format!("-Wl,--dynamic-linker={}", ld.display());
    let [.., p] = corpus(&d, &[&interp]);
    let file = with_loads(&std::fs::read(&p).unwrap(), 300);
    // Written under another name and copied by cp(1), as in `without_pt_phdr` (no ETXTBSY window).
    let (tmp, q) = (d.join("many.bytes"), d.join("many"));
    std::fs::write(&tmp, &file).unwrap();
    std::fs::set_permissions(&tmp, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    assert!(Command::new("cp").arg(&tmp).arg(&q).status().unwrap().success());
    // As `ld.so PROGRAM` only: the kernel refuses (ENOEXEC) a program header table larger than a page,
    // so a kernel-started program has at most 73 headers; `link` and the reader are the same code.
    let (code, sig, err) = run(Command::new(&ld).arg(&q));
    assert_eq!((code, sig), (Some(0), None), "ld.so PROGRAM with 300 PT_LOADs: check number {code:?} failed; stderr: {err}");
    std::fs::remove_dir_all(&d).ok();
}

/// `f` with its program header table moved to a new page at the end of the file, mapped by a new
/// read-only `PT_LOAD` after the last segment, at the first `PT_LOAD`'s address-minus-offset (so the
/// kernel's `AT_PHDR` agrees with the moved `PT_PHDR`); the other added `PT_LOAD`s map that page again,
/// one page each, at ascending addresses, until there are `total`.
fn with_loads(f: &[u8], total: usize) -> Vec<u8> {
    const PAGE: u64 = 4096;
    let rd = |o: usize| u64::from_le_bytes(f[o..o + 8].try_into().unwrap());
    let (phoff, phnum) = (rd(32) as usize, usize::from(u16::from_le_bytes([f[56], f[57]])));
    let hs: Vec<&[u8]> = (0..phnum).map(|i| &f[phoff + i * 56..phoff + i * 56 + 56]).collect();
    let ty = |h: &[u8]| u32::from_le_bytes(h[..4].try_into().unwrap());
    let word = |h: &[u8], i: usize| u64::from_le_bytes(h[8 * i..8 * i + 8].try_into().unwrap());
    let loads: Vec<&[u8]> = hs.iter().copied().filter(|h| ty(h) == 1).collect();
    let delta = word(loads[0], 2) - word(loads[0], 1);
    let end = loads.iter().map(|h| word(h, 2) + word(h, 5)).max().unwrap();
    let base = end.max(f.len() as u64 + delta).div_ceil(PAGE) * PAGE + PAGE;
    let off = base - delta;
    let added = total - loads.len();
    let n = phnum + added;
    let (size, tlen) = (56 * n as u64, (56 * n as u64).div_ceil(PAGE) * PAGE);
    let ph = |t: u32, o: u64, v: u64, len: u64, align: u64| {
        let mut h = [t.to_le_bytes(), 4u32.to_le_bytes()].concat();
        for w in [o, v, v, len, len, align] {
            h.extend(w.to_le_bytes());
        }
        h
    };
    let mut table: Vec<Vec<u8>> = hs.iter().filter(|h| ty(h) == 6).map(|_| ph(6, off, base, size, 8)).collect();
    table.extend(hs.iter().filter(|h| ty(h) == 3).map(|h| h.to_vec()));
    table.extend(loads.iter().map(|h| h.to_vec()));
    table.push(ph(1, off, base, tlen, PAGE));
    table.extend((1..added).map(|i| ph(1, off, base + tlen + PAGE * (i as u64 - 1), PAGE, PAGE)));
    table.extend(hs.iter().filter(|h| ![1, 3, 6].contains(&ty(h))).map(|h| h.to_vec()));
    assert_eq!(table.len(), n);
    let mut v = f.to_vec();
    v.resize((off + tlen) as usize, 0);
    v[off as usize..(off + size) as usize].copy_from_slice(&table.concat());
    v[32..40].copy_from_slice(&off.to_le_bytes());
    v[56..58].copy_from_slice(&(n as u16).to_le_bytes());
    v
}

/// A program's data in three or more writable `PT_LOAD`s (as conformance
/// s01_static_pie_multi_writable.c): three small 16 KiB-aligned pointer sections back to back, each
/// beginning about 16 KiB past the one before, so GNU ld starts a new writable segment for at least
/// two of them. `{decl}` declares what `{init}` (each section's three pointers) and `{check}` (the
/// test of one section `s`, true when right) use. Exits 0 when every section is right, else the
/// section's number (1 `.liella_seg2`, 2 `.data1`, 3 `.liella_seg3`).
fn multi_writable_src(decl: &str, init: &str, check: &str) -> String {
    format!(
        "{decl}
static int m_a = 1, m_b = 2;
void *m_seg2[] __attribute__((section(\".liella_seg2\"), aligned(16384))) = {{ {init} }};
void *m_data1[] __attribute__((section(\".data1\"), aligned(16384))) = {{ {init} }};
void *m_seg3[] __attribute__((section(\".liella_seg3\"), aligned(16384))) = {{ {init} }};
static int ok(void **s) {{ return *(int *)s[0] == 1 && *(int *)s[1] == 2 && ({check}); }}
void _start(void) {{
  int c = !ok(m_seg2) ? 1 : !ok(m_data1) ? 2 : !ok(m_seg3) ? 3 : 0;
  __asm__ volatile(\"syscall\" :: \"a\"(231), \"D\"(c)); __builtin_unreachable();
}}\n"
    )
}

/// The number of writable `PT_LOAD`s in the linked file `p`.
fn writable_loads(p: &Path) -> usize {
    let f = std::fs::read(p).unwrap();
    let elf = Elf::parse::<Native>(&f).unwrap();
    elf.program_headers().filter(|h| h.p_type == 1 && h.flags & 2 != 0).count()
}

/// Runs `p` as `ld.so PROGRAM`, then `q` (the same program naming `ld` in `PT_INTERP`) started by
/// the kernel; both must exit 0 (`what` names the case in the messages). Each file must have at
/// least three writable `PT_LOAD`s, so a linker that stops splitting fails here, not vacuously.
fn run_multi_writable(ld: &Path, p: &Path, q: &Path, what: &str) {
    for f in [p, q] {
        assert!(writable_loads(f) >= 3, "{what}: {} must have three or more writable PT_LOADs", f.display());
    }
    let (code, sig, err) = run(Command::new(ld).arg(p));
    assert_eq!((code, sig), (Some(0), None), "{what}, ld.so PROGRAM: section {code:?} has a wrong pointer; stderr: {err}");
    let (code, sig, err) = run(&mut Command::new(q));
    assert_eq!((code, sig), (Some(0), None), "{what}, PT_INTERP: section {code:?} has a wrong pointer; stderr: {err}");
}

/// s27_1_c_1, D-27.25 (amends D-27.10): a `-nostdlib` PIE without dependencies whose RELATIVE
/// targets lie in three writable `PT_LOAD`s is relocated by ld.so's self-relocator (`boot::image`,
/// `relocate_self`) in every segment, as `ld.so PROGRAM` and through `PT_INTERP`. Catches a relocator
/// that writes only through one writable segment or refuses a target in a later one.
#[test]
fn reloc_ldso_program_multi_writable_no_deps() {
    let (true, Some(ld)) = (cc_ok(), product()) else { return };
    let d = dir("multiw-self");
    let src = multi_writable_src("", "&m_a, &m_b, 0", "s[2] == 0");
    let interp = format!("-Wl,--dynamic-linker={}", ld.display());
    let p = build(&d, "pself", &src, &["-fPIE", "-pie"]);
    let q = build(&d, "pself-interp", &src, &["-fPIE", "-pie", &interp]);
    run_multi_writable(&ld, &p, &q, "no dependencies");
    std::fs::remove_dir_all(&d).ok();
}

/// no spec clause (D-27.25, D-27.10 refusals): the program of
/// `reloc_ldso_program_multi_writable_no_deps` with `PF_W` taken off its last writable `PT_LOAD`
/// (header patch: the segment is mapped read-only, its pointers are then in no writable segment)
/// is refused by ld.so with status 127 and a message, before any store: neither a fault on the
/// read-only page nor a run.
#[test]
fn reloc_ldso_program_target_outside_writable_refused() {
    let (true, Some(ld)) = (cc_ok(), product()) else { return };
    let d = dir("multiw-ro");
    let p = build(&d, "pself", &multi_writable_src("", "&m_a, &m_b, 0", "s[2] == 0"), &["-fPIE", "-pie"]);
    let mut f = std::fs::read(&p).unwrap();
    let (phoff, phnum) = (u64::from_le_bytes(f[32..40].try_into().unwrap()) as usize, usize::from(u16::from_le_bytes([f[56], f[57]])));
    let at = |i: usize| phoff + i * 56;
    let rd = |f: &[u8], o: usize| u32::from_le_bytes(f[o..o + 4].try_into().unwrap());
    let vaddr = |f: &[u8], i: usize| u64::from_le_bytes(f[at(i) + 16..at(i) + 24].try_into().unwrap());
    let last = (0..phnum).filter(|&i| rd(&f, at(i)) == 1 && rd(&f, at(i) + 4) & 2 != 0).max_by_key(|&i| vaddr(&f, i)).unwrap();
    let flags = rd(&f, at(last) + 4) & !2;
    f[at(last) + 4..at(last) + 8].copy_from_slice(&flags.to_le_bytes());
    // Written under another name and copied by cp(1), as in `without_pt_phdr` (no ETXTBSY window).
    let (tmp, q) = (d.join("ro.bytes"), d.join("pro"));
    std::fs::write(&tmp, &f).unwrap();
    std::fs::set_permissions(&tmp, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    assert!(Command::new("cp").arg(&tmp).arg(&q).status().unwrap().success());
    let (code, sig, err) = run(Command::new(&ld).arg(&q));
    assert_eq!((code, sig), (Some(127), None), "a RELATIVE target in a read-only segment must be refused; stderr: {err}");
    assert!(err.contains("cannot relocate the program"), "the refusal names the cause: {err}");
    std::fs::remove_dir_all(&d).ok();
}

/// s27_1_c_1, D-27.25: a PIE with a dependency (libb.so) whose targets in three writable `PT_LOAD`s
/// are RELATIVE and symbol relocations (`b_fn`, `b_data`) is relocated by ld.so's object path
/// (`link`) in every segment, as `ld.so PROGRAM` and through `PT_INTERP`.
#[test]
fn reloc_ldso_program_multi_writable_with_deps() {
    let (true, Some(ld)) = (cc_ok(), product()) else { return };
    let d = dir("multiw-link");
    let b = build(&d, "libb.so", LIBB, &["-fPIC", "-shared"]);
    let src = multi_writable_src(
        "extern int b_data; extern int b_fn(void);",
        "&m_a, &m_b, (void *)b_fn, &b_data",
        "((int (*)(void))s[2])() == 5 && s[3] == (void *)&b_data && b_data == 7",
    );
    let interp = format!("-Wl,--dynamic-linker={}", ld.display());
    let p = build(&d, "plink", &src, &["-fPIE", "-pie", b.to_str().unwrap()]);
    let q = build(&d, "plink-interp", &src, &["-fPIE", "-pie", b.to_str().unwrap(), &interp]);
    run_multi_writable(&ld, &p, &q, "with libb.so");
    std::fs::remove_dir_all(&d).ok();
}

/// s27_1_c_1, L14-6 (ruling #4323): a text-relocated program with a dependency runs, started by the
/// kernel and as `ld.so PROGRAM`, and its text is `r-x` again before any of its code runs.
#[test]
fn reloc_ldso_textrel_program_text_rx_before_it_runs() {
    let (true, Some(ld)) = (cc_ok(), product()) else { return };
    let d = dir("textprog");
    let b = build(&d, "libb.so", LIBB, &["-fPIC", "-shared"]);
    let interp = format!("-Wl,--dynamic-linker={}", ld.display());
    let t = build(&d, "ptext", TEXT_PROG, &["-fPIE", "-pie", "-Wl,-z,notext", &interp, b.to_str().unwrap()]);
    let elf_bytes = std::fs::read(&t).unwrap();
    let elf = Elf::parse::<Native>(&elf_bytes).unwrap();
    assert!(elf.dynamic().unwrap().unwrap().get(liella_ld::elf::dt::TEXTREL).is_some() || elf.dynamic().unwrap().unwrap().get(liella_ld::elf::dt::FLAGS).is_some_and(|f| f & 4 != 0), "the program is text-relocated");
    for (how, mut cmd) in [("kernel-started", Command::new(&t)), ("ld.so PROGRAM", Command::new(&ld))] {
        if how != "kernel-started" {
            cmd.arg(&t);
        }
        let (code, sig, err) = run(&mut cmd);
        assert_eq!((code, sig), (Some(0), None), "text-relocated program ({how}): 10 = text not r-x when it runs, 11 = wrong word; stderr: {err}");
    }
    std::fs::remove_dir_all(&d).ok();
}
