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
use liella_ld::elf::Elf;
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
    assert_eq!(RelocError::Tls(18).message(), b"thread-local storage relocations are not available in this build\n");
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
