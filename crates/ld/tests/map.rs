//! The general object mapper (phase 14 DL-7 `DL-MAP`, plan/ph14/design.md §3, D-27.15).
//!
//! Trace: s27_1_c_1 (loading an ELF object: segments mapped as the object asks); the malformed
//! corpus and the unmapping on failure: no spec clause (spec/security/threat-model.md "The dynamic
//! loader" rule 4, design §3 invariants).
//!
//! Three layers: (1) [`liella_ld::map::load`] against a model of an address space ([`Space`]) that
//! checks every call (inside the reservation, never writable and executable, stores only to
//! writable pages), for objects the system linker built (PIE, shared object, fixed-address program,
//! 2 MiB `p_align`, 16 KiB pages) and with each call failing in turn (nothing left mapped);
//! (2) the malformed corpus refused by the planner with the expected error and by the built
//! `ld.so` with status 127 and no signal; (3) the built `ld.so` running programs that print
//! their own `/proc/self/maps` (addresses, permissions, alignment, the `.bss` tail, the
//! inaccessible guard between segments). Skips (with the reason) when `cc` or the product is absent.
// ARCH(x86_64): the objects are built by the host's x86-64 linker and the program's system calls
// are x86-64's; the model and the corpus checks are arch-neutral but use those objects.
#![cfg(target_arch = "x86_64")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects, clippy::as_conversions, missing_docs)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use liella_kernel::auxv::Auxv;
use liella_ld::arch::{Arch, Native, AT_PAGESZ};
use liella_ld::map::{self, MapError, Mapper, PROT_EXEC, PROT_READ, PROT_WRITE};

/// The object every build uses: text, read-only data, initialised data and a `.bss` larger than a
/// page, so each segment kind and the `.bss` tail exist.
const OBJECT: &str = r#"
static long sys3(long n, long a, long b, long c) {
    long r;
    __asm__ volatile("syscall" : "=a"(r) : "a"(n), "D"(a), "S"(b), "d"(c) : "rcx", "r11", "memory");
    return r;
}
static unsigned long len(const char *s) { unsigned long n = 0; while (s[n]) n++; return n; }
static void put(const char *s) { sys3(1, 1, (long)s, len(s)); }
static void hex(const char *k, unsigned long v) {
    char b[19]; b[0] = '0'; b[1] = 'x';
    for (int i = 0; i < 16; i++) b[2 + i] = "0123456789abcdef"[(v >> (60 - 4 * i)) & 15];
    b[18] = 0; put(k); put(b); put("\n");
}
const char rodata[64] = "read-only";
char data[64] = "initialised";
char big[3 * 4096 + 100];
extern char __ehdr_start[], __bss_start[], _end[];
void start_c(long *sp) {
    (void)sp;
    int zero = 1;
    for (char *p = __bss_start; p < _end; p++) if (*p) zero = 0;
    put(zero ? "bss:ok\n" : "bss:bad\n");
    big[5] = 1; data[0] = 'I';
    hex("ehdr:", (unsigned long)__ehdr_start); hex("end:", (unsigned long)_end);
    hex("text:", (unsigned long)start_c); hex("rodata:", (unsigned long)rodata);
    hex("data:", (unsigned long)data); hex("big:", (unsigned long)big);
    static char maps[65536];
    long fd = sys3(2, (long)"/proc/self/maps", 0, 0), n = 0, r;
    while (fd >= 0 && (r = sys3(0, fd, (long)(maps + n), (long)sizeof maps - 1 - n)) > 0) n += r;
    sys3(1, 1, (long)maps, n);
    sys3(231, 0, 0, 0);
}
__asm__(".globl _start\n_start:\n\tmov %rsp, %rdi\n\tand $-16, %rsp\n\tcall start_c\n\thlt\n");
"#;

fn cc_ok() -> bool {
    Command::new("cc").arg("--version").output().is_ok_and(|o| o.status.success())
}

fn target() -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| root.join("target"), PathBuf::from)
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
    let d = target().join(format!("ld-map-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Build [`OBJECT`] as `name` with `flags` (the interpreter `ld` when given).
fn build(d: &Path, name: &str, flags: &[&str], ld: Option<&Path>) -> PathBuf {
    let c = d.join("obj.c");
    std::fs::write(&c, OBJECT).unwrap();
    let out = d.join(name);
    let mut cmd = Command::new("cc");
    cmd.args(["-nostdlib", "-O1", "-fno-stack-protector", "-fcf-protection=none", "-Wl,-z,relro", "-Wl,-z,separate-code"]).args(flags);
    if let Some(ld) = ld {
        cmd.arg(format!("-Wl,--dynamic-linker={}", ld.display()));
    }
    let o = cmd.arg(&c).arg("-o").arg(&out).output().unwrap();
    assert!(o.status.success(), "{name}: {}", String::from_utf8_lossy(&o.stderr));
    out
}

const PIE: &[&str] = &["-fPIE", "-pie"];
const SHARED: &[&str] = &["-fPIC", "-shared"];
const EXEC: &[&str] = &["-fno-pie", "-no-pie", "-static"];
const ALIGN_2M: &[&str] = &["-fPIE", "-pie", "-Wl,-z,max-page-size=0x200000", "-Wl,-z,common-page-size=0x200000"];
/// A movable (`ET_DYN`) object with 2 MiB `p_align` whose first `PT_LOAD` is at 0x600000, not 0:
/// GNU ld keeps `-shared` output `ET_DYN` under `-Ttext-segment` (with `-pie` it emits `ET_EXEC`);
/// `-Bsymbolic` binds its own symbols locally, so it has only `R_X86_64_RELATIVE` relocations.
const MOVED_2M: &[&str] = &["-fPIC", "-shared", "-Wl,-Bsymbolic", "-Wl,-e,_start","-Wl,-Ttext-segment=0x600000", "-Wl,-z,max-page-size=0x200000", "-Wl,-z,common-page-size=0x200000"];
const PAGE_16K: &[&str] = &["-fPIE", "-pie", "-Wl,-z,max-page-size=0x4000", "-Wl,-z,common-page-size=0x4000"];
/// One ELF64 program header.
fn ph(ty: u32, flags: u32, off: u64, at: u64, len: u64) -> Vec<u8> {
    let mut p = vec![0u8; 56];
    p[..4].copy_from_slice(&ty.to_le_bytes());
    p[4..8].copy_from_slice(&flags.to_le_bytes());
    for (o, v) in [(8, off), (16, at), (24, at), (32, len), (40, len), (48, 4096)] {
        put64(&mut p, o, v);
    }
    p
}

/// `f` (a linked PIE) rewritten to have `total` `PT_LOAD`s: the program headers move to a new page
/// at the end of the file, covered by a new read-only `PT_LOAD` one page after the last segment
/// (`PT_PHDR` follows them); the other added `PT_LOAD`s map that page again, one page each, at
/// ascending addresses after it.
fn add_loads(f: &[u8], total: usize) -> Vec<u8> {
    const PT_INTERP: u32 = 3;
    const PT_PHDR: u32 = 6;
    let hs = headers(f);
    let added = total - loads(f).len();
    let end = hs.iter().filter(|h| h.ty == PT_LOAD).map(|h| h.vaddr + h.memsz).max().unwrap();
    let base = end.div_ceil(4096) * 4096 + 4096;
    let size = 56 * (hs.len() + added) as u64;
    let (off, tlen) = ((f.len() as u64).div_ceil(4096) * 4096, size.div_ceil(4096) * 4096);
    let raw = |ty: u32| hs.iter().enumerate().filter(move |(_, h)| h.ty == ty).map(|(i, _)| f[ph_at(f, i)..ph_at(f, i) + 56].to_vec());
    let mut table: Vec<Vec<u8>> = hs.iter().filter(|h| h.ty == PT_PHDR).map(|_| ph(PT_PHDR, 4, off, base, size)).collect();
    table.extend(raw(PT_INTERP).chain(raw(PT_LOAD)));
    table.push(ph(PT_LOAD, 4, off, base, tlen));
    table.extend((1..added).map(|i| ph(PT_LOAD, 4, off, base + tlen + 4096 * (i as u64 - 1), 4096)));
    for ty in hs.iter().map(|h| h.ty).filter(|t| ![PT_PHDR, PT_INTERP, PT_LOAD].contains(t)).collect::<std::collections::BTreeSet<_>>() {
        table.extend(raw(ty));
    }
    let mut v = f.to_vec();
    v.resize((off + tlen) as usize, 0);
    v.splice(off as usize..(off + size) as usize, table.concat());
    put64(&mut v, 32, off);
    v[56..58].copy_from_slice(&(table.len() as u16).to_le_bytes());
    assert_eq!(loads(&v).len(), total);
    v
}

// ---------------------------------------------------------------- the address-space model

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Backing {
    Reserved,
    File(u64),
    Zero,
}

/// A model address space: page -> (protection, backing), plus stored bytes. Every call is checked.
struct Space<'f> {
    file: &'f [u8],
    page: u64,
    pages: BTreeMap<u64, (i32, Backing)>,
    stored: BTreeMap<u64, u8>,
    calls: usize,
    fail_at: Option<usize>,
    /// Where an anywhere-reservation lands: page-aligned, deliberately not 2 MiB-aligned.
    raw: u64,
}

impl<'f> Space<'f> {
    fn new(file: &'f [u8], page: u64) -> Self {
        Space { file, page, pages: BTreeMap::new(), stored: BTreeMap::new(), calls: 0, fail_at: None, raw: 0x5555_0000_0000 + 3 * page }
    }

    fn call(&mut self) -> Result<(), ()> {
        let n = self.calls;
        self.calls += 1;
        if self.fail_at == Some(n) { Err(()) } else { Ok(()) }
    }

    fn range(&self, at: u64, len: u64) -> impl Iterator<Item = u64> {
        assert_eq!(at % self.page, 0, "unaligned address {at:#x}");
        assert!(len > 0 && len % self.page == 0, "length {len:#x} not whole pages");
        (at..at + len).step_by(self.page as usize)
    }

    /// Replace pages inside the reservation (never outside it, never writable and executable).
    fn replace(&mut self, at: u64, len: u64, prot: i32, back: impl Fn(u64) -> Backing) {
        assert!(prot & PROT_WRITE == 0 || prot & PROT_EXEC == 0, "writable and executable at {at:#x}");
        for p in self.range(at, len).collect::<Vec<_>>() {
            assert!(self.pages.contains_key(&p), "mapping outside the reservation at {p:#x}");
            self.pages.insert(p, (prot, back(p - at)));
            self.stored.retain(|&a, _| a < p || a >= p + self.page);
        }
    }

    fn byte(&self, a: u64) -> u8 {
        if let Some(&b) = self.stored.get(&a) {
            return b;
        }
        let p = a - a % self.page;
        match self.pages.get(&p).expect("read of an unmapped byte").1 {
            Backing::File(off) => self.file.get((off + a - p) as usize).copied().unwrap_or(0),
            Backing::Zero => 0,
            Backing::Reserved => panic!("read of a reserved byte {a:#x}"),
        }
    }
}

impl Mapper for Space<'_> {
    fn reserve(&mut self, at: Option<u64>, len: u64) -> Result<u64, ()> {
        self.call()?;
        let start = at.unwrap_or(self.raw);
        if start == 0 || self.range(start, len).any(|p| self.pages.contains_key(&p)) {
            return Err(());
        }
        for p in self.range(start, len).collect::<Vec<_>>() {
            self.pages.insert(p, (0, Backing::Reserved));
        }
        Ok(start)
    }

    fn map_file(&mut self, at: u64, len: u64, prot: i32, offset: u64) -> Result<(), ()> {
        self.call()?;
        assert_eq!(offset % self.page, 0);
        assert!(offset < self.file.len() as u64, "file mapping past the end");
        self.replace(at, len, prot, |d| Backing::File(offset + d));
        Ok(())
    }

    fn map_zero(&mut self, at: u64, len: u64, prot: i32) -> Result<(), ()> {
        self.call()?;
        self.replace(at, len, prot, |_| Backing::Zero);
        Ok(())
    }

    fn store(&mut self, at: u64, len: u64, bytes: Option<&[u8]>) {
        for i in 0..len {
            let p = (at + i) - (at + i) % self.page;
            let prot = self.pages.get(&p).expect("store to an unmapped page").0;
            assert!(prot & PROT_WRITE != 0, "store to a page that is not writable at {:#x}", at + i);
            self.stored.insert(at + i, bytes.map_or(0, |b| b[i as usize]));
        }
    }

    fn protect(&mut self, at: u64, len: u64, prot: i32) -> Result<(), ()> {
        self.call()?;
        assert!(prot & PROT_WRITE == 0 || prot & PROT_EXEC == 0, "protect writable and executable");
        for p in self.range(at, len).collect::<Vec<_>>() {
            let e = self.pages.get_mut(&p).expect("protect of an unmapped page");
            e.0 = prot;
        }
        Ok(())
    }

    fn unmap(&mut self, at: u64, len: u64) {
        for p in self.range(at, len).collect::<Vec<_>>() {
            self.pages.remove(&p);
            self.stored.retain(|&a, _| a < p || a >= p + self.page);
        }
    }
}

// ---------------------------------------------------------------- program headers

const PT_LOAD: u32 = 1;
const PT_GNU_RELRO: u32 = 0x6474_e552;

#[derive(Clone, Copy, Debug)]
struct Ph {
    ty: u32,
    flags: u32,
    offset: u64,
    vaddr: u64,
    filesz: u64,
    memsz: u64,
    align: u64,
}

fn u16_at(f: &[u8], o: usize) -> u16 {
    u16::from_le_bytes(f[o..o + 2].try_into().unwrap())
}
fn u32_at(f: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(f[o..o + 4].try_into().unwrap())
}
fn u64_at(f: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(f[o..o + 8].try_into().unwrap())
}
fn put64(f: &mut [u8], o: usize, v: u64) {
    f[o..o + 8].copy_from_slice(&v.to_le_bytes());
}

/// The file offset of program header `i`.
fn ph_at(f: &[u8], i: usize) -> usize {
    u64_at(f, 32) as usize + 56 * i
}

fn headers(f: &[u8]) -> Vec<Ph> {
    (0..usize::from(u16_at(f, 56)))
        .map(|i| {
            let o = ph_at(f, i);
            Ph { ty: u32_at(f, o), flags: u32_at(f, o + 4), offset: u64_at(f, o + 8), vaddr: u64_at(f, o + 16), filesz: u64_at(f, o + 32), memsz: u64_at(f, o + 40), align: u64_at(f, o + 48) }
        })
        .collect()
}

/// Indexes of the `PT_LOAD` headers.
fn loads(f: &[u8]) -> Vec<usize> {
    headers(f).iter().enumerate().filter(|(_, h)| h.ty == PT_LOAD).map(|(i, _)| i).collect()
}

fn prot_of(flags: u32) -> i32 {
    (if flags & 4 != 0 { PROT_READ } else { 0 }) | (if flags & 2 != 0 { PROT_WRITE } else { 0 }) | (if flags & 1 != 0 { PROT_EXEC } else { 0 })
}

/// Load `f` into a fresh model with pages of `page` bytes and check the result against its headers.
fn check_model(name: &str, f: &[u8], page: u64) {
    let mut s = Space::new(f, page);
    let l = map::load::<Native, _>(&mut s, f, page).unwrap_or_else(|e| panic!("{name}: {e:?}"));
    let hs = headers(f);
    let align = hs.iter().filter(|h| h.ty == PT_LOAD).map(|h| h.align).max().unwrap().max(page);
    assert_eq!(l.bias % align, 0, "{name}: bias {:#x} not a multiple of p_align {align:#x}", l.bias);
    let calls = s.calls;
    let mut covered = BTreeMap::new();
    for h in hs.iter().filter(|h| h.ty == PT_LOAD) {
        let from = (h.vaddr + l.bias) / page * page;
        let to = (h.vaddr + l.bias + h.memsz).div_ceil(page) * page;
        for p in (from..to).step_by(page as usize) {
            assert_eq!(s.pages[&p].0, prot_of(h.flags), "{name}: page {p:#x} protection");
            covered.insert(p, ());
        }
        for i in (0..h.filesz).step_by(61) {
            assert_eq!(s.byte(h.vaddr + l.bias + i), f[(h.offset + i) as usize], "{name}: file byte {i:#x} of the segment at {:#x}", h.vaddr);
        }
        for i in h.filesz..h.memsz.min(h.filesz + 3 * page) {
            assert_eq!(s.byte(h.vaddr + l.bias + i), 0, "{name}: .bss byte {i:#x} of the segment at {:#x} not zero", h.vaddr);
        }
    }
    // Everything else in the reservation is the inaccessible guard; nothing lies outside it.
    for (&p, &(prot, back)) in &s.pages {
        assert!(p >= l.base && p < l.base + l.span, "{name}: page {p:#x} outside the reservation");
        if !covered.contains_key(&p) {
            assert_eq!((prot, back), (0, Backing::Reserved), "{name}: gap page {p:#x}");
        }
    }
    assert_eq!(s.pages.len() as u64, l.span / page, "{name}: reservation not whole");
    // The RELRO hook: recorded, then applied on request.
    if let Some(r) = hs.iter().find(|h| h.ty == PT_GNU_RELRO) {
        let (at, len) = l.relro.expect("relro recorded");
        assert_eq!(at, (r.vaddr + l.bias) / page * page, "{name}: relro start");
        assert_eq!(at + len, (r.vaddr + l.bias + r.memsz) / page * page, "{name}: relro end");
        l.protect_relro(&mut s).unwrap();
        for p in (at..at + len).step_by(page as usize) {
            assert_eq!(s.pages[&p].0, PROT_READ, "{name}: relro page {p:#x}");
        }
    }
    l.unmap(&mut s);
    assert!(s.pages.is_empty(), "{name}: unmap left pages");
    // Each call of the load failing in turn: an error, and nothing left mapped.
    assert!(calls >= 3, "{name}: {calls} calls");
    for n in 0..calls {
        let mut s = Space::new(f, page);
        s.fail_at = Some(n);
        let e = map::load::<Native, _>(&mut s, f, page).expect_err("a failed call must fail the load");
        assert!(matches!(e, MapError::System | MapError::Placement), "{name}: call {n}: {e:?}");
        assert!(s.pages.is_empty(), "{name}: call {n} failing left {} pages mapped", s.pages.len());
    }
}

/// s27_1_c_1: each segment mapped as the object asks (protection, file bytes, zero `.bss`, `p_align`);
/// nothing left mapped when a call fails (no spec clause: threat model loader rule 4).
#[test]
fn map_model_objects_from_the_linker() {
    if !cc_ok() {
        return eprintln!("SKIP: no cc");
    }
    let d = dir("model");
    for (name, flags, page) in [("pie", PIE, 4096), ("shared.so", SHARED, 4096), ("exec", EXEC, 4096), ("align2m", ALIGN_2M, 4096), ("moved2m.so", MOVED_2M, 4096), ("page16k", PAGE_16K, 0x4000)] {
        // Non-zero bytes after the sections: a `.bss` tail left unzeroed then shows as non-zero even
        // where the linker's file ends inside the last segment's page.
        let mut f = std::fs::read(build(&d, name, flags, None)).unwrap();
        if name == "moved2m.so" {
            let first = headers(&f)[loads(&f)[0]].vaddr;
            assert_eq!((u16::from_le_bytes([f[16], f[17]]), first), (3, 0x60_0000), "moved2m.so: not ET_DYN at 0x600000");
        }
        f.resize(f.len() + 0x1_0000, 0xaa);
        check_model(name, &f, page);
    }
    let f = std::fs::read(d.join("exec")).unwrap();
    let p = map::plan_file::<Native>(&f, 4096).unwrap();
    assert!(p.fixed && p.first == headers(&f)[loads(&f)[0]].vaddr / 4096 * 4096, "ET_EXEC plan: {p:?}");
    // A fixed-address program whose range is taken: Placement, and the occupant left alone.
    let mut s = Space::new(&f, 4096);
    s.reserve(Some(p.first), 4096).unwrap();
    assert_eq!(map::load::<Native, _>(&mut s, &f, 4096), Err(MapError::Placement));
    assert_eq!(s.pages.len(), 1, "the occupant's page");
    // A read-only segment with .bss sharing its last file page (the data segment made read-only):
    // the file bytes copied, the rest zero, the page left read-only.
    let mut f = std::fs::read(d.join("pie")).unwrap();
    f.resize(f.len() + 0x1_0000, 0xaa);
    let w = *loads(&f).iter().find(|&&i| headers(&f)[i].flags & 2 != 0).unwrap();
    let o = ph_at(&f, w) + 4;
    f[o..o + 4].copy_from_slice(&4u32.to_le_bytes());
    check_model("pie-readonly-bss", &f, 4096);
    std::fs::remove_dir_all(&d).ok();
}

/// no spec clause: the page size comes from `AT_PAGESZ` through the arch seam (design §0a, D-27.15).
#[test]
fn map_page_size_from_auxv() {
    let aux = |words: &[u64]| Native::page_size(&Auxv::from_words(words));
    assert_eq!(aux(&[AT_PAGESZ, 0x4000, 0, 0]), 0x4000);
    assert_eq!(aux(&[AT_PAGESZ, 0x1_0000, 0, 0]), 0x1_0000);
    assert_eq!(aux(&[AT_PAGESZ, 4096, 0, 0]), 4096);
    for bad in [0u64, 3000, 0x3000, 2048] {
        assert_eq!(aux(&[AT_PAGESZ, bad, 0, 0]), Native::BASE_PAGE, "AT_PAGESZ {bad:#x}");
    }
    assert_eq!(aux(&[0, 0]), Native::BASE_PAGE);
}

// ---------------------------------------------------------------- the malformed corpus

/// An object of `n` one-page read-only `PT_LOAD`s (`ET_DYN`, no sections), built here.
fn synthetic(n: u16) -> Vec<u8> {
    let phoff = 64u64;
    let size = (phoff + 56 * u64::from(n)).div_ceil(4096) * 4096;
    let mut f = vec![0u8; size as usize];
    f[..4].copy_from_slice(b"\x7fELF");
    f[4] = 2;
    f[5] = 1;
    f[6] = 1;
    f[16..18].copy_from_slice(&3u16.to_le_bytes());
    f[18..20].copy_from_slice(&62u16.to_le_bytes());
    f[20..24].copy_from_slice(&1u32.to_le_bytes());
    put64(&mut f, 32, phoff);
    f[52..54].copy_from_slice(&64u16.to_le_bytes());
    f[54..56].copy_from_slice(&56u16.to_le_bytes());
    f[56..58].copy_from_slice(&n.to_le_bytes());
    f[58..60].copy_from_slice(&64u16.to_le_bytes());
    for i in 0..usize::from(n) {
        let o = phoff as usize + 56 * i;
        f[o..o + 4].copy_from_slice(&PT_LOAD.to_le_bytes());
        f[o + 4..o + 8].copy_from_slice(&4u32.to_le_bytes());
        put64(&mut f, o + 16, 0x1000 * i as u64);
        put64(&mut f, o + 32, size.min(0x1000));
        put64(&mut f, o + 40, 0x1000);
        put64(&mut f, o + 48, 0x1000);
    }
    f
}

/// One corpus entry: name, file, the planner errors it may give.
type Case = (&'static str, Vec<u8>, fn(&MapError) -> bool);

/// The corpus. Edits of a linker-built PIE plus synthetic files.
fn corpus(pie: &[u8]) -> Vec<Case> {
    let l = loads(pie);
    let (a, b, last) = (l[0], l[1], *l.last().unwrap());
    let edit = |f: &dyn Fn(&mut Vec<u8>)| {
        let mut v = pie.to_vec();
        f(&mut v);
        v
    };
    let bad: fn(&MapError) -> bool = |e| *e == MapError::BadSegments;
    let len = pie.len() as u64;
    vec![
        ("overlapping", edit(&|v| {
            let (va, off) = (u64_at(v, ph_at(v, a) + 16), u64_at(v, ph_at(v, a) + 8));
            let o = ph_at(v, b);
            put64(v, o + 16, va);
            put64(v, o + 8, off);
        }), bad),
        ("huge", edit(&|v| { let o = ph_at(v, last) + 40; put64(v, o, 1 << 50) }), bad),
        ("huge-align", edit(&|v| { let o = ph_at(v, a) + 48; put64(v, o, 1 << 50) }), bad),
        ("align-not-power-of-two", edit(&|v| { let o = ph_at(v, a) + 48; put64(v, o, 0x3000) }), |e| matches!(e, MapError::BadSegments | MapError::Elf(_))),
        ("congruence-broken", edit(&|v| {
            let o = ph_at(v, b) + 16;
            let va = u64_at(v, o);
            put64(v, o, va + 0x10);
        }), bad),
        ("filesz-over-memsz", edit(&|v| {
            let o = ph_at(v, a);
            let m = u64_at(v, o + 40);
            put64(v, o + 32, m + 1);
        }), |e| matches!(e, MapError::BadSegments | MapError::Elf(_))),
        ("out-of-file", edit(&|v| {
            let o = ph_at(v, last);
            let va = u64_at(v, o + 16);
            put64(v, o + 8, len.div_ceil(4096) * 4096 + (va & 0xfff));
        }), |e| matches!(e, MapError::Elf(_))),
        ("write-exec", edit(&|v| {
            let o = ph_at(v, a) + 4;
            v[o..o + 4].copy_from_slice(&7u32.to_le_bytes());
        }), |e| *e == MapError::WriteExec),
        ("relro-outside", edit(&|v| {
            let i = headers(v).iter().position(|h| h.ty == PT_GNU_RELRO).expect("PT_GNU_RELRO");
            let o = ph_at(v, i) + 16;
            put64(v, o, 1 << 40);
        }), bad),
        ("not-loadable", edit(&|v| v[16..18].copy_from_slice(&1u16.to_le_bytes())), |e| matches!(e, MapError::NotLoadable | MapError::Elf(_))),
    ]
}

/// no spec clause: malformed objects refused before anything is mapped (threat model loader rule 4).
#[test]
fn map_malformed_corpus_refused() {
    if !cc_ok() {
        return eprintln!("SKIP: no cc");
    }
    let d = dir("corpus");
    let pie = std::fs::read(build(&d, "pie", PIE, None)).unwrap();
    // The positive controls: the unedited PIE and many PT_LOADs (no count bound, D-27.15; the
    // reference loads tens of thousands), mapped by the model too.
    map::plan_file::<Native>(&pie, 4096).unwrap();
    let many = synthetic(72);
    map::plan_file::<Native>(&many, 4096).unwrap();
    let mut s = Space::new(&many, 4096);
    map::load::<Native, _>(&mut s, &many, 4096).unwrap();
    assert!(s.calls > 72, "72 PT_LOADs: {} mapping calls", s.calls);
    for (name, f, want) in corpus(&pie) {
        let e = map::plan_file::<Native>(&f, 4096).expect_err(name);
        assert!(want(&e), "{name}: {e:?}");
        let mut s = Space::new(&f, 4096);
        assert_eq!(map::load::<Native, _>(&mut s, &f, 4096).err(), Some(e), "{name}: load");
        assert_eq!(s.calls, 0, "{name}: a refused object must map nothing");
    }
    // Every truncation of the file: an error or a plan, never a panic.
    for cut in (0..pie.len()).step_by(7) {
        let _ = map::plan_file::<Native>(&pie[..cut], 4096);
    }
    // A page size the PIE's segments share a page in (64 KiB pages, 4 KiB-linked): refused.
    assert!(map::plan_file::<Native>(&pie, 0x1_0000).is_err() || headers(&pie).iter().all(|h| h.ty != PT_LOAD || h.offset % 0x1_0000 == h.vaddr % 0x1_0000));
    std::fs::remove_dir_all(&d).ok();
}

/// no spec clause: `ld.so` refuses the malformed corpus with one line and status 127, no signal.
#[test]
fn map_malformed_corpus_refused_by_ldso() {
    let (Some(ld), true) = (product(), cc_ok()) else { return eprintln!("SKIP: no product or no cc") };
    let d = dir("ldso-corpus");
    let pie = std::fs::read(build(&d, "pie", PIE, Some(&ld))).unwrap();
    let mut files = corpus(&pie).into_iter().map(|(n, f, _)| (n, f)).collect::<Vec<_>>();
    // A fixed-address program at address 0: the reservation is refused (Placement).
    let exec = std::fs::read(build(&d, "exec0", &["-fno-pie", "-no-pie", "-Wl,-Ttext-segment=0"], Some(&ld))).unwrap();
    files.push(("exec-at-zero", exec));
    for (name, f) in files {
        let p = d.join(name);
        std::fs::write(&p, &f).unwrap();
        let o = Command::new("timeout").arg("60").arg(&ld).arg(&p).output().unwrap();
        let err = String::from_utf8_lossy(&o.stderr);
        assert_eq!(o.status.code(), Some(127), "{name}: {o:?}");
        assert!(err.starts_with("ld.so: ") && err.contains(name) && err.lines().count() == 1, "{name}: {err}");
    }
    std::fs::remove_dir_all(&d).ok();
}

// ---------------------------------------------------------------- /proc/self/maps

struct Line {
    start: u64,
    end: u64,
    perms: String,
}

fn hexval(l: &[String], k: &str) -> u64 {
    let v = l.iter().find_map(|x| x.strip_prefix(k)).unwrap_or_else(|| panic!("no {k}"));
    u64::from_str_radix(v.trim_start_matches("0x"), 16).unwrap()
}

/// Run `prog` through `ld.so PROG` (or directly) and check its own map.
fn check_maps(name: &str, cmd: &mut Command, align: u64, fixed: Option<u64>) {
    let o = cmd.output().unwrap();
    assert_eq!(o.status.code(), Some(0), "{name}: {o:?}");
    let out: Vec<String> = String::from_utf8_lossy(&o.stdout).lines().map(str::to_owned).collect();
    assert!(out.iter().any(|x| x == "bss:ok"), "{name}: .bss not zero: {out:?}");
    let (ehdr, end) = (hexval(&out, "ehdr:"), hexval(&out, "end:"));
    assert_eq!(ehdr % align, 0, "{name}: object at {ehdr:#x} not aligned to {align:#x}");
    if let Some(at) = fixed {
        assert_eq!(ehdr, at, "{name}: fixed-address program moved");
    }
    let maps: Vec<Line> = out
        .iter()
        .filter_map(|x| {
            let mut w = x.split_whitespace();
            let (r, perms) = (w.next()?, w.next()?);
            let (s, e) = r.split_once('-')?;
            Some(Line { start: u64::from_str_radix(s, 16).ok()?, end: u64::from_str_radix(e, 16).ok()?, perms: perms.to_owned() })
        })
        .collect();
    let at = |a: u64| maps.iter().find(|m| m.start <= a && a < m.end).unwrap_or_else(|| panic!("{name}: {a:#x} not mapped"));
    for (k, want) in [("text:", "r-xp"), ("rodata:", "r--p"), ("data:", "rw-p"), ("big:", "rw-p")] {
        assert_eq!(at(hexval(&out, k)).perms, want, "{name}: {k}");
    }
    // The object's range is covered without a hole, never writable and executable, and the gaps
    // between segments (if any) are inaccessible.
    let end = end.div_ceil(4096) * 4096;
    let mut a = ehdr;
    let mut guard = false;
    while a < end {
        let m = at(a);
        assert!(!(m.perms.contains('w') && m.perms.contains('x')), "{name}: writable and executable at {a:#x}");
        guard |= m.perms.starts_with("---");
        a = m.end;
    }
    if align > 0x1_0000 {
        assert!(guard, "{name}: no inaccessible guard between segments {align:#x} apart");
    }
}

/// s27_1_c_1: objects `ld.so` maps, as `/proc/self/maps` shows them.
#[test]
fn map_proc_self_maps_of_loaded_programs() {
    let (Some(ld), true) = (product(), cc_ok()) else { return eprintln!("SKIP: no product or no cc") };
    let d = dir("maps");
    for (name, flags, align, fixed) in [("pie", PIE, 4096, None), ("align2m", ALIGN_2M, 0x20_0000, None), ("moved2m", MOVED_2M, 0x20_0000, None), ("exec", &["-fno-pie", "-no-pie"][..], 4096, Some(0x40_0000))] {
        let prog = build(&d, name, flags, Some(&ld));
        for i in 0..4 {
            check_maps(&format!("{name} as ld.so PROG #{i}"), Command::new("timeout").arg("60").arg(&ld).arg(&prog), align, fixed);
        }
        if name == "exec" {
            check_maps("exec by PT_INTERP", Command::new("timeout").arg("60").arg(&prog), align, fixed);
        }
    }
    // 300 PT_LOADs (no count bound, D-27.15): the program runs and the last added page is mapped.
    let many = d.join("pie-300-loads");
    let f = add_loads(&std::fs::read(build(&d, "pie-loads", PIE, Some(&ld))).unwrap(), 300);
    std::fs::write(&many, &f).unwrap();
    check_maps("300 PT_LOADs as ld.so PROG", Command::new("timeout").arg("60").arg(&ld).arg(&many), 4096, None);
    let o = Command::new("timeout").arg("60").arg(&ld).arg(&many).output().unwrap();
    let out = String::from_utf8_lossy(&o.stdout);
    let last = headers(&f).into_iter().filter(|h| h.ty == PT_LOAD).map(|h| h.vaddr).max().unwrap();
    let bias = hexval(&out.lines().map(str::to_owned).collect::<Vec<_>>(), "ehdr:");
    let mapped = out.lines().filter_map(|l| l.split_once(' ')?.0.split_once('-')).any(|(s, e)| {
        let (s, e) = (u64::from_str_radix(s, 16).unwrap_or(0), u64::from_str_radix(e, 16).unwrap_or(0));
        s <= bias + last && bias + last < e
    });
    assert!(mapped, "300 PT_LOADs: the last segment ({last:#x} + bias {bias:#x}) is not mapped: {out}");
    std::fs::remove_dir_all(&d).ok();
}
