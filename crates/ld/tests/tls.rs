//! Static TLS of the initially loaded objects (phase 14 DL-12 `DL-TLS-STATIC`, plan/ph14/design.md §7,
//! R14-4, D-27.20).
//!
//! Trace: s22_6_c_4 (language-level TLS: each thread its own instance), s27_3_c_0 (`__tls_get_addr`:
//! the address of a TLS variable from a module ID and an offset). The layout, the surplus and the
//! thread-block slots: no spec clause (Drepper, "ELF Handling For Thread-Local Storage" §3.4; x86-64
//! psABI §3.4.6 and §4.4.1; R14-4).
//!
//! Two layers: (1) pure logic over the arch table (layout with the surplus and alignments, the TLS
//! relocation arithmetic, the dtv and `__tls_get_addr`'s lookup with its generation check); (2) the
//! built `ld.so` running a `-nostdlib` program and two libraries with TLS of different alignments
//! (4 to 64 bytes) through every model (LE, IE, GD, LD), checking `.tdata` contents, zero `.tbss`,
//! alignment, that the models agree on one variable's address, and the canary and pointer guard.
// ARCH(x86_64): the corpus is built by the host toolchain; the arithmetic is the x86-64 psABI's.
#![cfg(target_arch = "x86_64")]
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::arithmetic_side_effects, clippy::as_conversions, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Command;

use liella_ld::arch::{Arch, Native, RelocKind, TlsOperands};
use liella_ld::tls::{self, Module, Placed};

fn target() -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| root.join("target"), PathBuf::from)
}

fn tools_ok() -> bool {
    let ok = ["cc", "readelf"].iter().all(|t| Command::new(t).arg("--version").output().is_ok_and(|o| o.status.success()));
    if !ok {
        eprintln!("SKIP: no system C compiler or readelf to build and inspect the corpus");
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

// ------------------------------------------------------------------ (1) pure logic

/// no spec clause (Drepper §3.4.2, R14-4): variant II offsets in load order, each block aligned, the
/// program's block ending at TP, the 64 KiB surplus below the last block, module IDs skipping
/// objects without TLS.
#[test]
fn tls_layout_variant_ii_offsets_alignment_and_surplus() {
    let m = |memsz, align, vaddr| Some(Module { vaddr, filesz: 0, memsz, align });
    let mods = [m(12, 4, 0x2000), m(100, 64, 0x3000), None, m(8, 16, 0x4008), m(1, 0, 0x5000)];
    let mut out = [None; 5];
    let a = tls::layout::<Native>(&mods, &mut out, Native::TCB_SIZE, tls::SURPLUS).unwrap();
    let p = |module, tp_offset| Some(Placed { module, tp_offset });
    // 12 -> 12; round_up(12 + 100, 64) = 128; r = 8: round_up(128 + 8 + 8, 16) - 8 = 136; 137.
    assert_eq!(out, [p(1, -12), p(2, -128), None, p(3, -136), p(4, -137)]);
    for (o, md) in out.iter().zip(&mods) {
        if let (Some(o), Some(md)) = (o, md) {
            let al = md.align.max(1) as i64;
            assert_eq!((o.tp_offset - (md.vaddr as i64 % al)).rem_euclid(al), 0, "module {} keeps its alignment and remainder", o.module);
        }
    }
    assert_eq!(a.align, 64);
    assert_eq!(a.modules, 4);
    assert!(a.tp >= 137 + tls::SURPLUS, "the surplus lies below the last block (tp {})", a.tp);
    assert_eq!(a.tp % 64, 0);
    assert_eq!(a.dtv, a.tp + Native::TCB_SIZE);
    assert_eq!(a.dtv_words, tls::DTV_HEADER + 4);
    assert_eq!(a.len, a.dtv + 8 * a.dtv_words);
    // No TLS anywhere: no module, still a thread block and a dtv.
    let none = tls::layout::<Native>(&[None, None], &mut [None; 2], Native::TCB_SIZE, 0).unwrap();
    assert_eq!((none.tp, none.modules, none.dtv_words), (0, 0, tls::DTV_HEADER));
    assert_eq!(tls::layout::<Native>(&[m(8, 24, 0)], &mut [None], Native::TCB_SIZE, 0), Err(tls::TlsError::Align(0)));
    let bad = [Some(Module { vaddr: 0, filesz: 9, memsz: 8, align: 8 })];
    assert_eq!(tls::layout::<Native>(&bad, &mut [None], Native::TCB_SIZE, 0), Err(tls::TlsError::Size(0)));
    assert_eq!(tls::layout::<Native>(&[m(u64::MAX, 8, 0)], &mut [None], Native::TCB_SIZE, 0), Err(tls::TlsError::Overflow));
    assert_eq!(tls::layout::<Native>(&mods, &mut [None; 2], Native::TCB_SIZE, 0), Err(tls::TlsError::Args));
}

/// no spec clause (plan/ph14/dl-16-dlopen.md §5, R14-4; DL-16d): places in the static surplus for
/// `dlopen`ed modules. First fit nearest the thread pointer; each block keeps its alignment and
/// `p_vaddr` remainder; a gap an alignment left is used by a later block; blocks never overlap and
/// stay inside the surplus; an alignment above the thread pointer's (or not a power of two), a
/// block larger than its image's file part allows, and an exhausted table are refused; `rollback`
/// gives back the places made after its mark.
#[test]
fn tls_surplus_first_fit_alignment_gap_reuse_and_rollback() {
    let m = |memsz, align, vaddr| Module { vaddr, filesz: 0, memsz, align };
    let (lo, hi, tpa) = (100u64, 1100u64, 64u64);
    let mut s = tls::Surplus::<8>::default();
    let put = |s: &mut tls::Surplus<8>, md: Module| s.place::<Native>(lo, hi, tpa, &md);
    assert!(s.is_empty());
    assert_eq!(put(&mut s, m(10, 1, 0)), Some(-110), "at the surplus's start");
    // Remainder 8 modulo 64: TP - d == 8 (mod 64) for the nearest d >= 110 + 8, d = 120.
    assert_eq!(put(&mut s, m(8, 64, 0x1008)), Some(-120), "alignment and p_vaddr remainder");
    assert_eq!(put(&mut s, m(2, 2, 0)), Some(-112), "the gap [110, 112) the alignment left");
    assert_eq!(put(&mut s, m(8, 128, 0)), None, "an alignment above the thread pointer's");
    assert_eq!(put(&mut s, m(8, 24, 0)), None, "an alignment that is not a power of two");
    assert_eq!(put(&mut s, Module { vaddr: 0, filesz: 9, memsz: 8, align: 1 }), None, "p_filesz > p_memsz");
    let mark = s.len();
    assert_eq!(mark, 3);
    assert_eq!(put(&mut s, m(980, 1, 0)), Some(-1100), "up to the surplus's end");
    assert_eq!(put(&mut s, m(1, 1, 0)), None, "no byte left");
    s.rollback(mark);
    assert_eq!(s.len(), 3);
    assert_eq!(put(&mut s, m(500, 1, 0)), Some(-620), "a place taken back is used again");
    let mut two = tls::Surplus::<2>::default();
    assert!(two.place::<Native>(lo, hi, tpa, &m(1, 1, 0)).is_some() && two.place::<Native>(lo, hi, tpa, &m(1, 1, 0)).is_some());
    assert_eq!(two.place::<Native>(lo, hi, tpa, &m(1, 1, 0)), None, "every place taken");
}

/// no spec clause (x86-64 psABI §4.4.1): `TPOFF64` is S + A + the block's (negative) offset from TP,
/// `DTPOFF64` is S + A, `DTPMOD64` the module ID; `TLSDESC` has no single field: its descriptor
/// (DL-15; Oliva, "Thread-Local Storage Descriptors for IA32 and AMD64/EM64T") is the function, then
/// the `TPOFF64` value as its argument.
#[test]
fn tls_relocation_arithmetic_from_the_table() {
    let o = TlsOperands { s: 0x10, a: 4, module: 3, tp_offset: -0x80 };
    let v = |k| Native::apply_tls(k, o).map(|f| (f.value, f.width));
    assert_eq!(v(RelocKind::TpOff), Ok(((-0x80i64 + 0x14) as u64, 8)));
    assert_eq!(v(RelocKind::DtpOff), Ok((0x14, 8)));
    assert_eq!(v(RelocKind::DtpMod), Ok((3, 8)));
    assert!(v(RelocKind::TlsDesc).is_err() && v(RelocKind::Abs64).is_err());
    assert_eq!(Native::tls_descriptor(0xabc0, o), Ok([0xabc0, (-0x80i64 + 0x14) as u64]));
    assert_eq!((Native::STACK_GUARD_SLOT, Native::POINTER_GUARD_SLOT, Native::DTV_SLOT), (Some(0x28), Some(0x30), 0x08));
}

/// s27_3_c_0: the dtv a thread starts with gives each static module's block; a vector built for an
/// older generation, module 0 and a module past the count take the slow path.
#[test]
fn tls_dtv_lookup_checks_generation_and_module() {
    let placed = [Some(Placed { module: 1, tp_offset: -16 }), None, Some(Placed { module: 2, tp_offset: -192 })];
    let mut dtv = [0u64; 4];
    let tp = 0x7000_1000u64;
    tls::fill_dtv(&mut dtv, 5, tp, &placed);
    assert_eq!(dtv, [5, 2, tp - 16, tp - 192]);
    let get = |generation, module, offset| tls::lookup::<Native>(|i| dtv[i as usize], generation, module, offset);
    assert_eq!(get(5, 1, 4), Some(tp - 12));
    assert_eq!(get(5, 2, 0x20), Some(tp - 192 + 0x20));
    assert_eq!(get(6, 1, 4), None, "a vector built for an older generation is not current");
    assert_eq!(get(5, 0, 0), None);
    assert_eq!(get(5, 3, 0), None);
}

// ------------------------------------------------------------------ (2) the model matrix

const LIBB: &str = "__thread long b_v = 0x1234;
__thread char b_z[100] __attribute__((aligned(16)));
long *b_gd(void) { return &b_v; }
char *b_zp(void) { return b_z; }
";
const LIBA: &str = "extern __thread long b_v;
__thread int a_d = 7;
__thread char a_al[3] __attribute__((aligned(64))) = {1, 2, 3};
static __thread int a_s = 9;
__attribute__((tls_model(\"initial-exec\"))) __thread int a_ie_v = 11;
int *a_gd(void) { return &a_d; }               /* GD on its own global: DTPMOD64 + DTPOFF64 */
int *a_ld(void) { return &a_s; }               /* LD: DTPMOD64 without a symbol */
int *a_ie(void) { return &a_ie_v; }            /* IE in a library: TPOFF64 */
char *a_alp(void) { return a_al; }
long *a_b(void) { return &b_v; }               /* GD on another library's variable */
";
/// The program: exits 0 when every check holds, else the number of the first that failed.
const PROG: &str = "extern __thread int a_d; extern __thread long b_v;
extern int *a_gd(void); extern int *a_ld(void); extern int *a_ie(void); extern char *a_alp(void);
extern long *a_b(void); extern long *b_gd(void); extern char *b_zp(void);
__thread int p_d = 5;                          /* LE, .tdata */
__thread long p_z[4] __attribute__((aligned(32)));  /* LE, .tbss */
static void bye(int c) { __asm__ volatile(\"syscall\" :: \"a\"(231), \"D\"(c)); __builtin_unreachable(); }
static unsigned long fs(unsigned long off) { unsigned long v; __asm__ volatile(\"movq %%fs:(%1), %0\" : \"=r\"(v) : \"r\"(off)); return v; }
static int check(void) {
  if (p_d != 5) return 1;
  for (int i = 0; i < 4; i++) if (p_z[i] != 0) return 2;
  if ((unsigned long)p_z % 32 != 0) return 3;
  if (a_d != 7 || b_v != 0x1234) return 4;     /* IE from the program: TPOFF64 */
  if (a_gd() != &a_d || *a_gd() != 7) return 5; /* GD and IE agree */
  if (*a_ld() != 9) return 6;
  if (*a_ie() != 11) return 7;
  char *al = a_alp();
  if ((unsigned long)al % 64 != 0 || al[0] != 1 || al[1] != 2 || al[2] != 3) return 8;
  if (a_b() != &b_v || b_gd() != &b_v) return 9;
  char *z = b_zp();
  if ((unsigned long)z % 16 != 0) return 10;
  for (int i = 0; i < 100; i++) if (z[i] != 0) return 11;
  p_d = 6; *a_gd() = 70; b_v = 71;
  if (p_d != 6 || a_d != 70 || *a_b() != 71) return 12;
  unsigned long tp = fs(0);
  if ((unsigned long)&p_d >= tp || (unsigned long)&a_d >= tp) return 13;  /* variant II: below TP */
  unsigned long canary = fs(0x28), guard = fs(0x30);
  if (canary == 0 || (canary & 0xff) != 0 || guard == 0 || guard == canary) return 14;
  if (fs(0x10) != tp) return 15;
  return 0;
}
void _start(void) { bye(check()); }
";

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

fn relocs(f: &Path) -> String {
    String::from_utf8_lossy(&Command::new("readelf").arg("-rW").arg(f).output().unwrap().stdout).into_owned()
}

/// s22_6_c_4, s27_3_c_0: the LE/IE/GD/LD matrix over a program and two libraries (TLS aligned 4, 16,
/// 32 and 64), run as `ld.so PROGRAM` and started by the kernel through `PT_INTERP`.
#[test]
fn tls_model_matrix_over_initially_loaded_objects() {
    let (true, Some(ld)) = (tools_ok(), product()) else { return };
    let d = target().join(format!("ld-tls-matrix-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    let lds = ld.to_str().unwrap();
    let b = build(&d, "libb.so", LIBB, &["-fPIC", "-shared", lds]);
    let bs = b.to_str().unwrap();
    let a = build(&d, "liba.so", LIBA, &["-fPIC", "-shared", bs, lds]);
    let ra = relocs(&a);
    for kind in ["R_X86_64_DTPMOD64", "R_X86_64_DTPOFF64", "R_X86_64_TPOFF64"] {
        assert!(ra.contains(kind), "liba has no {kind}: the matrix would not cover it\n{ra}");
    }
    let interp = format!("-Wl,--dynamic-linker={lds}");
    for (name, extra) in [("prog", None), ("prog-interp", Some(interp.as_str()))] {
        let mut f = vec!["-fPIE", "-pie", a.to_str().unwrap(), bs, lds];
        f.extend(extra);
        let p = build(&d, name, PROG, &f);
        assert!(relocs(&p).contains("R_X86_64_TPOFF64"), "{name}: no TPOFF64 (IE from the program)");
        for _ in 0..3 {
            let o = if extra.is_some() { Command::new(&p).output() } else { Command::new(&ld).arg(&p).output() }.unwrap();
            assert_eq!(o.status.code(), Some(0), "{name}: TLS check number {:?} failed; stderr: {}", o.status.code(), String::from_utf8_lossy(&o.stderr));
        }
    }
    std::fs::remove_dir_all(&d).ok();
}

/// s27_3_c_0 (ruling #4229, D-27.23): `__tls_get_addr` binds to the interpreter's own scope entry,
/// which `ld.so` takes from its mapped image: it never reads its own file. The interpreter here may
/// be executed but not read (mode 0111), so neither its `PT_INTERP` path nor `/proc/self/exe` can be
/// opened; the GD/LD libraries of the matrix still run, started by the kernel and as `ld.so PROGRAM`.
#[test]
fn tls_interpreter_scope_entry_without_reading_its_file() {
    use std::os::unix::fs::PermissionsExt;
    let (true, Some(ld)) = (tools_ok(), product()) else { return };
    let d = target().join(format!("ld-tls-noread-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    let own = d.join(Native::INTERP);
    std::fs::copy(&ld, &own).unwrap();
    std::fs::set_permissions(&own, std::fs::Permissions::from_mode(0o111)).unwrap();
    if std::fs::File::open(&own).is_ok() {
        eprintln!("SKIP: an execute-only file is still readable here (the tests run with file-permission override)");
        std::fs::remove_dir_all(&d).ok();
        return;
    }
    // Linked against the readable product (same soname); started through the execute-only copy.
    let lds = ld.to_str().unwrap();
    let b = build(&d, "libb.so", LIBB, &["-fPIC", "-shared", lds]);
    let bs = b.to_str().unwrap();
    let a = build(&d, "liba.so", LIBA, &["-fPIC", "-shared", bs, lds]);
    let interp = format!("-Wl,--dynamic-linker={}", own.display());
    let p = build(&d, "prog", PROG, &["-fPIE", "-pie", a.to_str().unwrap(), bs, lds, &interp]);
    for (how, o) in [("PT_INTERP", Command::new(&p).output()), ("ld.so PROGRAM", Command::new(&own).arg(&p).output())] {
        let o = o.unwrap();
        assert_eq!(o.status.code(), Some(0), "{how} with an unreadable interpreter: TLS check number {:?} failed; stderr: {}", o.status.code(), String::from_utf8_lossy(&o.stderr));
    }
    std::fs::remove_dir_all(&d).ok();
}
