//! Constructor and destructor order (phase 14 DL-11 `DL-INIT-ORDER`, plan/ph14/design.md §9,
//! D-27.19).
//!
//! Trace: s1_1_c_5 (ELF constructors, `DT_PREINIT_ARRAY`, `DT_INIT`, `DT_INIT_ARRAY`, in dependency
//! order), s1_1_c_6 (destructors registered to run at normal exit), s27_1_c_1 (the constructors of a
//! set opened by `dlopen`: the order rule for its root).
//!
//! Two layers: (1) [`liella_ld::init`] over the dependency graphs of the order corpus, the expected
//! orders being the reference loader's (tools/probe/dl/order.sh, and DL-0b's `== order` record in
//! spec/behaviour/dynlink.glibc.txt for `dlopen` and `exit` with an object opened later); (2) the
//! corpus itself (`-nostdlib` objects printing what runs) under the built `ld.so`, compared line for
//! line with the reference's record below and, when the system loader runs the corpus, with the
//! reference live. The reference's termination function needs its C library, so the exit lines of
//! the record come from the same programs linked with the C library (`*_libc`).
// ARCH(x86_64): the corpus is built by the host toolchain with x86-64 inline assembly.
#![cfg(target_arch = "x86_64")]
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::arithmetic_side_effects, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Command;

use liella_ld::arch::{Arch, Native};
use liella_ld::init::{self, OrderError, OBJECTS_MAX};

fn init(n: usize, edges: &[(u8, u8)]) -> Vec<u8> {
    let mut out = [0u8; OBJECTS_MAX];
    let len = init::init_order(n, edges, &mut out).unwrap();
    out[..len].to_vec()
}

fn fini(n: usize, edges: &[(u8, u8)]) -> Vec<u8> {
    let mut out = [0u8; OBJECTS_MAX];
    let len = init::fini_order(n, edges, &mut out).unwrap();
    out[..len].to_vec()
}

/// s1_1_c_5, s1_1_c_6: the DL-0b graph (main -> A, D; A -> B, C; D -> C; load order main A D B C) initialises
/// C, B, D, A, then the program, and finalises in reverse.
#[test]
fn init_order_dependencies_first() {
    let e = [(0, 1), (0, 2), (1, 3), (1, 4), (2, 4)];
    assert_eq!(init(5, &e), [4, 3, 2, 1, 0], "dependencies are initialised before their dependents");
    assert_eq!(fini(5, &e), [0, 1, 2, 3, 4], "the finalisation order is the initialisation order reversed");
}

/// s1_1_c_5, s1_1_c_6: ties and a dependency loaded through a later sibling, as the reference orders them
/// (tools/probe/dl/order.sh `p_tie`, `p_sib`).
#[test]
fn init_order_ties_and_siblings() {
    // main -> R, Q, P; P -> R (load order main R Q P): R, P, Q.
    let tie = [(0, 1), (0, 2), (0, 3), (3, 1)];
    assert_eq!(init(4, &tie), [1, 3, 2, 0]);
    assert_eq!(fini(4, &tie), [0, 2, 3, 1]);
    // main -> L, K; L -> N; K -> M; N -> K (load order main L K N M): M, K, N, L.
    let sib = [(0, 1), (0, 2), (1, 3), (2, 4), (3, 2)];
    assert_eq!(init(5, &sib), [4, 2, 3, 1, 0]);
    assert_eq!(fini(5, &sib), [0, 1, 3, 2, 4]);
}

/// s1_1_c_5, s27_1_c_1: cycles as the reference orders them: at start (`p_cycle`, `p_cycle3`) and for a set
/// opened by `dlopen` (DL-0b: `dlopen("libH")`, H <-> I: I then H).
#[test]
fn init_order_cycles() {
    // main -> H; H -> I; I -> H: H, I.
    assert_eq!(init(3, &[(0, 1), (1, 2), (2, 1)]), [1, 2, 0]);
    // main -> X; X -> Y; Y -> Z, W; Z -> X (load order main X Y Z W): W, Y, X, Z.
    let c3 = [(0, 1), (1, 2), (2, 3), (2, 4), (3, 1)];
    assert_eq!(init(5, &c3), [4, 2, 1, 3, 0]);
    assert_eq!(fini(5, &c3), [0, 3, 1, 2, 4]);
    // The root of a dlopen set is never entered from another object: H (root) <-> I gives I, H.
    assert_eq!(init(2, &[(0, 1), (1, 0)]), [1, 0]);
}

/// s1_1_c_6: at exit an object opened later is placed by its dependencies (DL-0b `exit`: main A D B
/// E C F with E -> C, F opened after start).
#[test]
fn init_order_exit_with_object_opened_later() {
    let e = [(0, 1), (0, 2), (1, 3), (1, 4), (2, 4), (5, 4), (5, 6)];
    assert_eq!(fini(7, &e), [0, 1, 2, 3, 5, 4, 6]);
}

/// no spec clause: limits are refused, not walked past.
#[test]
fn init_order_refuses_bad_input() {
    let mut out = [0u8; OBJECTS_MAX];
    assert_eq!(init::init_order(OBJECTS_MAX + 1, &[], &mut out), Err(OrderError::TooMany));
    assert_eq!(init::init_order(2, &[(0, 2)], &mut out), Err(OrderError::BadEdge));
    assert_eq!(init::init_order(0, &[], &mut out), Ok(0));
}

/// s1_1_c_5, s1_1_c_6: one object's calls: `DT_INIT` before `DT_INIT_ARRAY` (in order); `DT_FINI_ARRAY` in
/// reverse before `DT_FINI`; 0 and all-ones entries left out (D-27.19).
#[test]
fn init_order_calls_of_one_object() {
    let init: Vec<u64> = init::init_calls(0x10, &[0x20, 0, 0x30, u64::MAX]).collect();
    assert_eq!(init, [0x10, 0x20, 0x30], "DT_INIT runs before DT_INIT_ARRAY, whose entries run in order");
    let fini: Vec<u64> = init::fini_calls(0x10, &[0x20, 0x30]).collect();
    assert_eq!(fini, [0x30, 0x20, 0x10], "DT_FINI_ARRAY runs in reverse, then DT_FINI");
    assert_eq!(init::init_calls(0, &[]).count(), 0);
}

// ------------------------------------------------------------------ (2) the corpus under ld.so

fn target() -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| root.join("target"), PathBuf::from)
}

fn tools_ok() -> bool {
    for (tool, arg) in [("cc", "--version"), ("perl", "-v"), ("bash", "--version")] {
        if !Command::new(tool).arg(arg).output().is_ok_and(|o| o.status.success()) {
            eprintln!("SKIP: no {tool} to build the order corpus");
            return false;
        }
    }
    true
}

/// The reference loader's record (tools/probe/dl/order.sh on the x86-64 reference host,
/// 2026-11-10): for each program, the start lines (the program's preinit array, then the shared
/// objects' initialisation; the program's own constructors are its C library's, so they do not
/// appear), and the exit lines of the same program linked with the C library.
const RECORD: &[(&str, &str, &str)] = &[
    (
        "p_order",
        "preinit_array main\nctor C\nctor B\nDT_INIT D\nctor D\nctor200 A\nctor300 A\nctor A\n",
        "fini_array main\ndtor main\ndtor A\ndtor300 A\ndtor200 A\ndtor D\nDT_FINI D\ndtor B\ndtor C\n",
    ),
    ("p_cycle", "preinit_array main\nctor H\nctor I\n", "fini_array main\ndtor main\ndtor I\ndtor H\n"),
    ("p_tie", "preinit_array main\nctor R\nctor P\nctor Q\n", "fini_array main\ndtor main\ndtor Q\ndtor P\ndtor R\n"),
    ("p_sib", "preinit_array main\nctor M\nctor K\nctor N\nctor L\n", "fini_array main\ndtor main\ndtor L\ndtor N\ndtor K\ndtor M\n"),
    ("p_cycle3", "preinit_array main\nctor W\nctor Y\nctor X\nctor Z\n", "fini_array main\ndtor main\ndtor Z\ndtor X\ndtor Y\ndtor W\n"),
    // libS's DT_INIT_ARRAY retagged DT_PREINIT_ARRAY: a shared object's preinit array is not run.
    ("p_spre", "preinit_array main\n", "fini_array main\ndtor main\ndtor S\n"),
];

/// The x86-64 psABI's program interpreter: the loader the corpus programs name in `PT_INTERP`.
const REFERENCE: &str = "/lib64/ld-linux-x86-64.so.2";

/// Run the corpus script; its output split into `== name` sections.
fn corpus(build: &Path, ldso: Option<&Path>) -> Vec<(String, String)> {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/probe/dl/order.sh");
    let mut c = Command::new("bash");
    c.arg(&script).arg(build);
    if let Some(l) = ldso {
        c.arg(l);
    }
    let o = c.output().unwrap();
    assert!(o.status.success(), "order.sh: {}", String::from_utf8_lossy(&o.stderr));
    let text = String::from_utf8(o.stdout).unwrap();
    let mut out: Vec<(String, String)> = Vec::new();
    for line in text.lines() {
        if let Some(name) = line.strip_prefix("== ") {
            out.push((name.to_owned(), String::new()));
        } else if let Some((_, body)) = out.last_mut() {
            body.push_str(line);
            body.push('\n');
        }
    }
    out
}

fn section<'a>(secs: &'a [(String, String)], name: &str) -> &'a str {
    secs.iter().find(|(n, _)| n == name).map_or_else(|| panic!("order.sh printed no section {name}"), |(_, b)| b.as_str())
}

/// s1_1_c_5, s1_1_c_6: the order corpus under the built `ld.so` prints, line for line,
/// the reference's start order, then `main`, then (through the termination function it got in
/// `%rdx`) the reference's exit order.
#[test]
fn init_order_corpus_matches_reference() {
    if !tools_ok() {
        return;
    }
    let ld = target().join("liella").join(Native::INTERP);
    if !ld.exists() {
        eprintln!("SKIP: {} not built (run cargo xtask so)", ld.display());
        return;
    }
    let d = target().join(format!("ld-init-order-{}", std::process::id()));
    let ours = corpus(&d, Some(&ld));
    for (name, start, exit) in RECORD {
        let want = format!("{start}main\ncall fini\n{exit}   rc=0\n");
        assert_eq!(section(&ours, name), want, "{name} under Liella's ld.so (left) differs from the reference's order (right)");
    }
    // The reference live: the same build started through its PT_INTERP (start lines), and the C
    // library variants (exit lines).
    if Path::new(REFERENCE).exists() {
        let theirs = corpus(&d, None);
        for (name, start, exit) in RECORD {
            let s = section(&theirs, name);
            assert!(s.starts_with(&format!("{start}main\n")), "{name}: the reference loader now prints\n{s}");
            let s = section(&theirs, &format!("{name}_libc"));
            assert!(s.ends_with(&format!("main\n{exit}   rc=0\n")), "{name}_libc: the reference now prints\n{s}");
        }
    } else {
        eprintln!("SKIP (live reference part): no {REFERENCE}");
    }
    std::fs::remove_dir_all(&d).ok();
}
