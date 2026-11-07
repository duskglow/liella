//! The interpreter's link invariant (L14-1, `liella_ld::link`) over objects this test links with the
//! system compiler: a clean shared object (RELATIVE as RELA, and packed as RELR), and one object for
//! each violation: an undefined symbol, a `DT_NEEDED`, a non-relative relocation, a text relocation.
//! Then the built `ld.so` product itself (`cargo xtask so`): the check holds, and it runs as a
//! program (`--version`, `--help`, refusals) 100 times, half with address randomisation off
//! (`setarch -R`): an ET_DYN image is mapped at a non-zero base either way, so a start that read
//! an unrelocated pointer would fault (design §1 "ASLR is the oracle").
//!
//! no spec clause: plan/ph14/design.md §1 and §21 L14-1, phase 14 DL-5 (D-27.13).
//! Skips (with the reason) when `cc` is absent, and the product part when `ld.so` is not built.
// ARCH(x86_64): the corpus is built by the host toolchain; the product is the x86_64 interpreter.
#![cfg(target_arch = "x86_64")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects, clippy::as_conversions, missing_docs)]

use std::path::{Path, PathBuf};
use std::process::Command;

use liella_ld::arch::{Arch, Native, RelocKind};
use liella_ld::link::{check, LinkReport};
use liella_ld::program::{self, Action};

fn cc_ok() -> bool {
    Command::new("cc").arg("--version").output().is_ok_and(|o| o.status.success())
}

fn dir() -> PathBuf {
    let d = std::env::temp_dir().join(format!("liella-ldso-link-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Link `src` (C) into a shared object with `flags`; the file's bytes.
fn link(d: &Path, name: &str, src: &str, flags: &[&str]) -> Vec<u8> {
    let c = d.join(format!("{name}.c"));
    let so = d.join(format!("{name}.so"));
    std::fs::write(&c, src).unwrap();
    let o = Command::new("cc").args(["-shared", "-nostdlib", "-fPIC", "-O1", "-Wl,-z,now"]).args(flags).arg(&c).arg("-o").arg(&so).output().unwrap();
    assert!(o.status.success(), "cc {name}: {}", String::from_utf8_lossy(&o.stderr));
    std::fs::read(&so).unwrap()
}

fn report(bytes: &[u8]) -> LinkReport<'_> {
    check::<Native>(bytes).unwrap()
}

/// A local object and a pointer to it: RELATIVE only; hidden visibility keeps both local.
const CLEAN: &str = "static int v = 1;\n__attribute__((visibility(\"hidden\"))) int *p = &v;\nint get(void) { return *p; }\n";

#[test]
fn ldso_link_clean_object_holds() {
    if !cc_ok() {
        eprintln!("SKIP: no cc");
        return;
    }
    let d = dir();
    let rela = link(&d, "clean", CLEAN, &[]);
    let r = report(&rela);
    assert!(r.holds(), "clean object: {r:?}");
    assert!(r.dynamic && r.relative >= 1, "clean object has its RELATIVE entry: {r:?}");
    let relr = link(&d, "clean_relr", CLEAN, &["-Wl,-z,pack-relative-relocs"]);
    let r = report(&relr);
    assert!(r.holds(), "clean RELR object: {r:?}");
}

#[test]
fn ldso_link_undefined_symbol_refused() {
    if !cc_ok() {
        eprintln!("SKIP: no cc");
        return;
    }
    let r_bytes = link(&dir(), "undef", "extern int liella_missing(void);\nint get(void) { return liella_missing(); }\n", &[]);
    let r = report(&r_bytes);
    assert_eq!(r.undefined, 1, "one undefined symbol: {r:?}");
    assert_eq!(r.first_undefined, Some(&b"liella_missing"[..]));
    assert!(!r.holds());
}

#[test]
fn ldso_link_needed_refused() {
    if !cc_ok() {
        eprintln!("SKIP: no cc");
        return;
    }
    let d = dir();
    link(&d, "dep", "int dep_value;\n", &["-Wl,-soname,libliella-dep.so"]);
    let bytes = link(&d, "needs", CLEAN, &["-Wl,--no-as-needed", d.join("dep.so").to_str().unwrap()]);
    let r = report(&bytes);
    assert_eq!(r.needed, 1, "one DT_NEEDED: {r:?}");
    assert_eq!((r.undefined, r.other, r.textrel), (0, 0, false), "only DT_NEEDED is wrong: {r:?}");
    assert!(!r.holds());
}

#[test]
fn ldso_link_non_relative_refused() {
    if !cc_ok() {
        eprintln!("SKIP: no cc");
        return;
    }
    // A pointer to a preemptible (default-visibility) definition: R_X86_64_64 against the symbol.
    let bytes = link(&dir(), "abs", "int g = 1;\nint *p = &g;\n", &[]);
    let r = report(&bytes);
    assert_eq!(r.first_other, Some(RelocKind::Abs64), "{r:?}");
    assert_eq!(r.undefined, 0, "only the relocation kind is wrong: {r:?}");
    assert!(!r.holds());
}

#[test]
fn ldso_link_textrel_refused() {
    if !cc_ok() {
        eprintln!("SKIP: no cc");
        return;
    }
    // An absolute address of a local object in text: a RELATIVE entry against a read-only segment.
    let src = "__asm__(\".text\\n.globl get\\nget: movabs $v, %rax\\nret\\n.data\\nv: .quad 0\\n\");\n";
    let bytes = link(&dir(), "textrel", src, &["-Wl,-z,notext"]);
    let r = report(&bytes);
    assert!(r.textrel, "DT_TEXTREL or DF_TEXTREL: {r:?}");
    assert_eq!((r.undefined, r.needed, r.other), (0, 0, 0), "only the text relocation is wrong: {r:?}");
    assert!(!r.holds());
}

#[test]
fn ldso_program_arguments() {
    let a = |v: &[&'static [u8]]| program::action(v.iter().copied());
    assert_eq!(a(&[b"--version"]), Action::Version);
    assert_eq!(a(&[b"--help", b"--version"]), Action::Help);
    assert_eq!(a(&[]), Action::Missing);
    assert_eq!(a(&[b"--frob"]), Action::Unknown(b"--frob"));
    assert_eq!(a(&[b"/bin/true", b"--version"]), Action::Program(b"/bin/true"));
    assert!(program::version::<Native>("1").concat().contains(Native::INTERP));
    assert!(program::help::<Native>().concat().contains("--version"));
}

/// The built interpreter, or `None` (with the reason) before `cargo xtask so`.
fn product() -> Option<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let target = std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| root.join("target"), PathBuf::from);
    let p = target.join("liella").join(Native::INTERP);
    if p.exists() {
        Some(p)
    } else {
        eprintln!("SKIP: {} not built (run cargo xtask so)", p.display());
        None
    }
}

#[test]
fn ldso_product_link_invariant_holds() {
    let Some(p) = product() else { return };
    let bytes = std::fs::read(&p).unwrap();
    let r = report(&bytes);
    assert!(r.holds(), "{}: {r:?}", p.display());
    assert!(r.relative + r.relr_words > 0, "the interpreter has relocations to apply: {r:?}");
}

#[test]
fn ldso_product_runs_at_random_bases() {
    let Some(p) = product() else { return };
    let setarch = Command::new("setarch").args(["-R", "true"]).status().is_ok_and(|s| s.success());
    if !setarch {
        eprintln!("NOTE: setarch -R unavailable: all 100 starts randomised");
    }
    for i in 0..100 {
        let mut cmd = if setarch && i % 2 == 0 {
            let mut c = Command::new("setarch");
            c.arg("-R").arg(&p);
            c
        } else {
            Command::new(&p)
        };
        let o = cmd.arg("--version").output().unwrap();
        assert_eq!(o.status.code(), Some(0), "start {i}: {o:?}");
        let out = String::from_utf8_lossy(&o.stdout);
        assert!(out.starts_with("ld.so (Liella) ") && out.contains(Native::INTERP), "start {i}: {out}");
    }
    let o = Command::new(&p).arg("--help").output().unwrap();
    assert_eq!(o.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&o.stdout).contains("--version"));
    for args in [&["--frob"][..], &[][..], &["/bin/true"][..]] {
        let o = Command::new(&p).args(args).output().unwrap();
        assert_eq!(o.status.code(), Some(program::FAILURE), "{args:?}: {o:?}");
        assert!(o.stdout.is_empty() && !o.stderr.is_empty(), "{args:?}: {o:?}");
    }
}

#[test]
fn ldso_product_refuses_as_interpreter() {
    let Some(p) = product() else { return };
    if !cc_ok() {
        eprintln!("SKIP: no cc");
        return;
    }
    // A program naming this interpreter in PT_INTERP: the kernel starts ld.so for it (AT_ENTRY is the
    // program's), and DL-5 refuses with 127 before reaching the program's entry.
    let d = dir();
    let c = d.join("interp.c");
    std::fs::write(&c, "void _start(void) { for (;;) {} }\n").unwrap();
    let exe = d.join("interp");
    let o = Command::new("cc").args(["-nostdlib", "-fPIE", "-pie", "-O1"]).arg(format!("-Wl,--dynamic-linker={}", p.display())).arg(&c).arg("-o").arg(&exe).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    // Hang guard only (a regression would spin in _start): `timeout` kills it, and the status says so.
    let o = Command::new("timeout").arg("60").arg(&exe).output().unwrap();
    assert_eq!(o.status.code(), Some(program::FAILURE), "{o:?}");
    assert_eq!(o.stderr, program::AS_INTERPRETER);
}
