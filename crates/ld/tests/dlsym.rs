//! `dlsym` and `dlvsym` under Liella's `ld.so` and `libc.so.6` (phase 14 DL-16b, plan/ph14/dl-16-dlopen.md
//! §2; spec/behaviour/dynlink.md "Symbol binding" and probes P-3, P-4, P-5, P-6). One C program built
//! against the products checks every behaviour itself and exits with the number of the first check
//! that failed (its text on standard error). Needs `cargo xtask so` and a C compiler (else SKIP).
// ARCH(x86_64): the corpus is built with the host compiler for x86_64 and run under the x86_64 loader.
#![cfg(target_arch = "x86_64")]
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::arithmetic_side_effects, clippy::as_conversions, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Command;

use liella_ld::arch::{Arch, Native};
use liella_ld::elf::Elf;

fn target() -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| root.join("target"), PathBuf::from)
}

/// The scratch directory, its `ld.so`, `libc.so.6` (named by path: no `DT_SONAME`, as the
/// start-time loader needs) and `liella-crt1.o`, or `None` with the reason printed.
fn setup(name: &str) -> Option<(PathBuf, PathBuf, PathBuf, PathBuf)> {
    if !Command::new("cc").arg("--version").output().is_ok_and(|o| o.status.success()) {
        eprintln!("SKIP: no system C compiler to build the corpus");
        return None;
    }
    let lib = target().join("liella");
    let ld = lib.join(Native::INTERP);
    if !ld.exists() || !lib.join("libc.so.6").exists() || !lib.join("liella-crt1.o").exists() {
        eprintln!("SKIP: ld.so, libc.so.6 or liella-crt1.o not built (run cargo xtask so)");
        return None;
    }
    let d = target().join(format!("ld-dlsym-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    let d = std::fs::canonicalize(d).unwrap();
    let mut so = std::fs::read(lib.join("libc.so.6")).unwrap();
    let elf = Elf::parse::<Native>(&so).unwrap();
    let dynph = elf.find(liella_ld::elf::pt::DYNAMIC).unwrap();
    let (off, len) = (dynph.offset as usize, dynph.filesz as usize);
    let at = (off..off + len).step_by(16).find(|&o| so[o..o + 8] == 14u64.to_le_bytes()).expect("libc.so.6 has a DT_SONAME");
    so[at..at + 8].copy_from_slice(&0x6fff_fdf8u64.to_le_bytes());
    let libc = d.join("libc-dl.so");
    std::fs::write(&libc, &so).unwrap();
    Some((d, ld, libc, lib.join("liella-crt1.o")))
}

/// Build `out` from `src` with `flags` in `d`.
fn cc(d: &Path, out: &str, src: &str, flags: &[&str]) -> PathBuf {
    let c = d.join(format!("{out}.c"));
    std::fs::write(&c, src).unwrap();
    let o = d.join(out);
    let r = Command::new("cc").args(["-O1", "-fno-builtin", "-fno-stack-protector", "-fcf-protection=none"]).arg(&c).args(flags).arg("-o").arg(&o).output().unwrap();
    assert!(r.status.success(), "{out}: {}", String::from_utf8_lossy(&r.stderr));
    o
}

/// Helper 1 (linked, first): `f` = 1, its own `RTLD_NEXT` lookups, an initial-exec TLS variable.
const LIB_H1: &str = "#define _GNU_SOURCE
#include <dlfcn.h>
int f(void) { return 1; }
void *h1_next(const char *n) { return dlsym(RTLD_NEXT, n); }
void *h1_default(const char *n) { return dlsym(RTLD_DEFAULT, n); }
__attribute__((tls_model(\"initial-exec\"))) __thread int tv = 42;
int *h1_tv(void) { return &tv; }
";
/// Helper 2 (linked, second): `f` = 2.
const LIB_H2: &str = "int f(void) { return 2; }\n";
/// `libM5`: `who` = 5, `fn_M5`. `libL5` needs it; `libL5` and `libG5` look up from themselves.
const LIB_M5: &str = "int who(void) { return 5; } int fn_M5(void) { return 50; }\n";
const LIB_L5: &str = "#define _GNU_SOURCE
#include <dlfcn.h>
int fn_L5(void) { return 51; }
void *l5_next(const char *n) { return dlsym(RTLD_NEXT, n); }
void *l5_default(const char *n) { return dlsym(RTLD_DEFAULT, n); }
";
const LIB_G5: &str = "#define _GNU_SOURCE
#include <dlfcn.h>
int who(void) { return 7; }
void *g5_next(const char *n) { return dlsym(RTLD_NEXT, n); }
";
/// `libA3` needs `libB3`; `libZ3` references both without needing them.
const LIB_A3: &str = "int fn_A3(void) { return 3; }\n";
const LIB_B3: &str = "int fn_B3(void) { return 4; }\n";
const LIB_Z3: &str = "extern int fn_A3(void), fn_B3(void); int fn_Z3(void) { return fn_A3() + fn_B3(); }\n";
/// `dv` here and in the program: `call_dv` binds the program's, or its own under `RTLD_DEEPBIND`.
const LIB_DEEP: &str = "int dv(void) { return 'D'; } int call_dv(void) { return dv(); }\n";
/// A unique object `u_obj` with value `V` (`STB_GNU_UNIQUE`), and the address its own reference binds.
fn lib_unique(v: u32) -> String {
    format!(
        "__asm__(\".section .data.u_obj,\\\"aw\\\"\\n.globl u_obj\\n.type u_obj, @gnu_unique_object\\n.size u_obj, 4\\n.p2align 2\\nu_obj: .long {v}\\n.text\\n\");
extern int u_obj; int *u_addr(void) {{ return &u_obj; }}
"
    )
}
/// An IFUNC whose resolver picks `impl`.
const LIB_IFUNC: &str = "static int impl(void) { return 7; }
static int (*res(void))(void) { return impl; }
int ifn(void) __attribute__((ifunc(\"res\")));
";
/// The version table of dynlink.md "Symbol binding".
const LIB_V: &str = "int f1(void) { return 11; } int f2(void) { return 12; }
__asm__(\".symver f1,f@VER_1\"); __asm__(\".symver f2,f@@VER_2\");
int h1(void) { return 21; } int h2(void) { return 22; }
__asm__(\".symver h1,h@VER_1\"); __asm__(\".symver h2,h@VER_2\");
int b(void) { return 30; }
int g1(void) { return 41; } int g2(void) { return 42; }
__asm__(\".symver g2,g@VER_2\"); __asm__(\".symver g1,g@@VER_1\");
int m1(void) { return 51; } int m2(void) { return 52; } int m3(void) { return 53; }
__asm__(\".symver m1,m@VER_1\"); __asm__(\".symver m2,m@VER_2\"); __asm__(\".symver m3,m@@VER_3\");
";
const LIB_V_MAP: &str = "VER_1 { local: f1; f2; h1; h2; g1; g2; m1; m2; m3; };\nVER_2 { } VER_1;\nVER_3 { } VER_2;\n";

/// The program: argv[1] is the scratch directory.
const PROG: &str = r#"#define _GNU_SOURCE
#include <dlfcn.h>
#include <stdio.h>
#include <string.h>

int main_sym = 42;
int who(void) { return 0; }
int dv(void) { return 'P'; }
extern int f(void);
extern void *h1_next(const char *), *h1_default(const char *);
extern int *h1_tv(void);
typedef int fn(void);
typedef void *look(const char *);

static const char *dir, *prog;
static char bufs[8][4096];
static int nb;
static const char *at(const char *name) { char *b = bufs[nb++ % 8]; snprintf(b, 4096, "%s/%s", dir, name); return b; }
static int fail(int n, const char *what, const char *e) { fprintf(stderr, "check %d: %s [%s]\n", n, what, e ? e : "(null)"); return n; }
static int call(void *p) { return p ? ((fn *)p)() : -1; }

/* `ret` is NULL and the text is exactly OBJ ": undefined symbol: " WHAT, reported once. */
static int failed_with(const void *ret, const char *obj, const char *what) {
    char want[4400];
    snprintf(want, sizeof want, "%s: undefined symbol: %s", obj, what);
    const char *e = dlerror();
    int ok = ret == NULL && e != NULL && strcmp(e, want) == 0;
    if (!ok) fprintf(stderr, "want [%s], got %s [%s]\n", want, ret ? "a result" : "NULL", e ? e : "no text");
    return ok && dlerror() == NULL;
}

int main(int argc, char **argv) {
    if (argc < 2) return 99;
    dir = argv[1];
    prog = argv[0];
    /* RTLD_DEFAULT, RTLD_NEXT from a start-time object, the program's handle (s27_1_c_3, s27_1_c_1_3). */
    void *d = dlsym(RTLD_DEFAULT, "f");
    if (d != (void *)f || call(d) != 1) return fail(1, "RTLD_DEFAULT f is helper 1's", dlerror());
    if (call(h1_next("f")) != 2) return fail(2, "RTLD_NEXT f from helper 1 is helper 2's", dlerror());
    void *g = dlopen(NULL, RTLD_NOW);
    if (!g || dlsym(g, "main_sym") != (void *)&main_sym) return fail(3, "dlopen(NULL) finds the program's own symbol", dlerror());
    if (dlsym(g, "f") != d) return fail(4, "dlopen(NULL) f is the global scope's first", dlerror());
    void *k2 = dlopen(at("libh2.so"), RTLD_NOW);
    if (!k2 || call(dlsym(k2, "f")) != 2) return fail(5, "helper 2's handle finds its own f", dlerror());
    if (dlsym(RTLD_NEXT, "who") != NULL || dlerror() == NULL) return fail(6, "RTLD_NEXT from the program skips the program", "found");
    /* Texts (s27_1_c_5): the program, the caller, the handle's object. */
    if (!failed_with(dlsym(RTLD_DEFAULT, "no_such_sym"), prog, "no_such_sym")) return fail(7, "RTLD_DEFAULT text", "");
    if (!failed_with(h1_next("no_such_sym"), at("libh1.so"), "no_such_sym")) return fail(8, "RTLD_NEXT text names the caller", "");
    if (!failed_with(dlsym(k2, "no_such_sym"), at("libh2.so"), "no_such_sym")) return fail(9, "handle text", "");
    if (!failed_with(dlvsym(RTLD_DEFAULT, "f", "NOVER_1"), prog, "f, version NOVER_1")) return fail(10, "dlvsym text", "");
    if (!failed_with(dlsym(g, "no_such_sym"), prog, "no_such_sym")) return fail(11, "program handle text", "");
    /* Handles Liella did not return (D-27.32). */
    const char *e;
    if (dlsym((void *)&main_sym, "f") != NULL || !(e = dlerror()) || strcmp(e, "dlsym: invalid handle") != 0) return fail(12, "dlsym refuses a foreign handle", e);
    if (dlvsym((char *)g + 1, "f", "X") != NULL || !(e = dlerror()) || strcmp(e, "dlvsym: invalid handle") != 0) return fail(13, "dlvsym refuses a misaligned handle", e);
    /* A success clears a pending text (#5127). */
    if (dlopen(at("no_such_object.so"), RTLD_NOW) != NULL || dlsym(RTLD_DEFAULT, "f") != d || dlerror() != NULL) return fail(14, "a successful dlsym clears the text", "");
    /* STT_TLS of a start-time object: the calling thread's address. */
    int *tv = dlsym(RTLD_DEFAULT, "tv");
    if (tv != h1_tv() || *tv != 42) return fail(15, "dlsym of a TLS variable is this thread's", dlerror());
    /* P-5: RTLD_NEXT and RTLD_DEFAULT from an RTLD_LOCAL object and its dependency. */
    void *l5 = dlopen(at("libL5.so"), RTLD_NOW);
    if (!l5) return fail(16, "open libL5", dlerror());
    look *ln = (look *)dlsym(l5, "l5_next"), *ldf = (look *)dlsym(l5, "l5_default");
    if (!ln || !ldf) return fail(17, "libL5's functions", dlerror());
    if (call(ln("who")) != 5) return fail(18, "RTLD_NEXT from libL5 reaches libM5", dlerror());
    if (ln("puts") == NULL) return fail(19, "RTLD_NEXT from libL5 reaches the C library", dlerror());
    if (!failed_with(ln("fn_L5"), at("libL5.so"), "fn_L5")) return fail(20, "RTLD_NEXT leaves out the caller", "");
    if (call(ldf("who")) != 0) return fail(21, "RTLD_DEFAULT from libL5: the program's who first", dlerror());
    if (call(ldf("fn_M5")) != 50 || call(ldf("fn_L5")) != 51) return fail(22, "RTLD_DEFAULT from libL5 searches its local scope", dlerror());
    if (dlsym(RTLD_DEFAULT, "fn_L5") != NULL) return fail(23, "a local object is not in the program's RTLD_DEFAULT", "found");
    dlerror();
    void *g5 = dlopen(at("libG5.so"), RTLD_NOW | RTLD_GLOBAL);
    look *gn = g5 ? (look *)dlsym(g5, "g5_next") : NULL;
    if (!gn) return fail(24, "open libG5", dlerror());
    if (gn("puts") == NULL || gn("f") != NULL) return fail(25, "RTLD_NEXT from the last global object: its own scope", dlerror());
    dlerror();
    if (call(dlsym(RTLD_NEXT, "who")) != 7) return fail(26, "RTLD_NEXT from the program reaches a global object", dlerror());
    /* P-3: RTLD_GLOBAL promotion by NOLOAD; then an object needing both opens. */
    void *a3 = dlopen(at("libA3.so"), RTLD_NOW);
    if (!a3 || dlsym(RTLD_DEFAULT, "fn_A3") != NULL || dlsym(RTLD_DEFAULT, "fn_B3") != NULL) return fail(27, "RTLD_LOCAL keeps A3 and B3 out", "");
    dlerror();
    if (dlopen(at("libZ3.so"), RTLD_NOW) != NULL) return fail(28, "Z3 cannot bind before promotion", "opened");
    dlerror();
    if (dlopen(at("libA3.so"), RTLD_NOW | RTLD_NOLOAD | RTLD_GLOBAL) != a3) return fail(29, "NOLOAD|GLOBAL returns the handle", dlerror());
    if (call(dlsym(RTLD_DEFAULT, "fn_A3")) != 3 || call(dlsym(RTLD_DEFAULT, "fn_B3")) != 4) return fail(30, "promotion makes A3 and B3 global", dlerror());
    void *z3 = dlopen(at("libZ3.so"), RTLD_NOW);
    if (!z3 || call(dlsym(z3, "fn_Z3")) != 7) return fail(31, "Z3 binds after promotion", dlerror());
    /* RTLD_DEEPBIND: the new set's own definition before the global scope's. */
    void *dp = dlopen(at("libdeep.so"), RTLD_NOW), *db = dlopen(at("libdeep2.so"), RTLD_NOW | RTLD_DEEPBIND);
    if (!dp || !db) return fail(32, "open the DEEPBIND pair", dlerror());
    if (call(dlsym(dp, "call_dv")) != 'P' || call(dlsym(db, "call_dv")) != 'D') return fail(33, "DEEPBIND binds the object's own dv", "");
    /* STB_GNU_UNIQUE: one object for the name across two RTLD_LOCAL objects (s27_1_g_0). */
    void *u1 = dlopen(at("libu1.so"), RTLD_NOW), *u2 = dlopen(at("libu2.so"), RTLD_NOW);
    if (!u1 || !u2) return fail(34, "open the unique pair", dlerror());
    int *p1 = dlsym(u1, "u_obj"), *p2 = dlsym(u2, "u_obj");
    typedef int *addr(void);
    addr *a2 = (addr *)dlsym(u2, "u_addr");
    if (!p1 || p1 != p2 || *p1 != 11 || !a2 || a2() != p1) return fail(35, "the first unique definition serves both", dlerror());
    /* IFUNC: dlsym answers the resolver's choice. */
    void *fi = dlopen(at("libifn.so"), RTLD_NOW);
    if (!fi || call(dlsym(fi, "ifn")) != 7) return fail(36, "dlsym of an IFUNC calls the resolver", dlerror());
    /* The version table (dynlink.md "Symbol binding"). */
    void *v = dlopen(at("libv.so"), RTLD_NOW);
    if (!v) return fail(37, "open libv", dlerror());
    if (call(dlsym(v, "f")) != 12 || call(dlsym(v, "b")) != 30 || call(dlsym(v, "g")) != 41 || call(dlsym(v, "m")) != 53) return fail(38, "dlsym takes the base or default version", dlerror());
    if (!failed_with(dlsym(v, "h"), at("libv.so"), "h")) return fail(39, "dlsym of only hidden versions fails", "");
    if (call(dlvsym(v, "f", "VER_1")) != 11 || call(dlvsym(v, "f", "VER_2")) != 12 || call(dlvsym(v, "h", "VER_1")) != 21 || call(dlvsym(v, "h", "VER_2")) != 22) return fail(40, "dlvsym binds the exact version", dlerror());
    if (!failed_with(dlvsym(v, "f", "VER_9"), at("libv.so"), "f, version VER_9")) return fail(41, "dlvsym of a missing version", "");
    if (!failed_with(dlvsym(v, "b", "VER_1"), at("libv.so"), "b, version VER_1")) return fail(42, "dlvsym of a base definition by a version", "");
    if (!failed_with(dlvsym(v, "b", "libv.so"), at("libv.so"), "b, version libv.so")) return fail(43, "the base node's name is no version", "");
    if (call(h1_default("fn_A3")) != 3) return fail(44, "RTLD_DEFAULT from a start-time object", dlerror());
    return 0;
}
"#;

/// s27_1_c_3, s27_1_c_1_3, s27_1_g_0, s27_1_c_5 (plan/ph14/dl-16-dlopen.md §2; dynlink.md "Symbol
/// binding", P-3, P-5, P-6; D-27.32; rulings #5127, #5264): `RTLD_DEFAULT`, `RTLD_NEXT` from a
/// start-time and from a `dlopen`ed caller, the program's handle, an object's handle, `dlvsym`,
/// every failure text, foreign handles refused, a success clearing the text, `STT_TLS`,
/// `RTLD_GLOBAL` promotion, `RTLD_DEEPBIND`, the unique table, IFUNC, and the version table.
#[test]
fn s27_1_c_3_dlsym_scopes_handles_and_texts() {
    let Some((d, ld, libc, crt1)) = setup("scopes") else { return };
    let dir = d.to_str().unwrap();
    let libc_s = libc.to_str().unwrap();
    let rpath = format!("-Wl,-rpath,{dir}");
    let ldir = format!("-L{dir}");
    let shared = |name: &str, src: &str, extra: &[&str]| {
        let soname = format!("-Wl,-soname,{name}");
        let mut flags = vec!["-nostdlib", "-fPIC", "-shared", "-Wl,--no-as-needed", &soname, &ldir, &rpath];
        flags.extend_from_slice(extra);
        cc(&d, name, src, &flags);
    };
    // The two start-time helpers carry no DT_SONAME and the program names them by path: the
    // start-time loader opens DT_NEEDED entries by path only (no library search yet).
    let start_lib = |name: &str, src: &str, extra: &[&str]| {
        let mut flags = vec!["-nostdlib", "-fPIC", "-shared", "-Wl,--no-as-needed"];
        flags.extend_from_slice(extra);
        cc(&d, name, src, &flags)
    };
    let h1 = start_lib("libh1.so", LIB_H1, &[libc_s]);
    let h2 = start_lib("libh2.so", LIB_H2, &[]);
    shared("libM5.so", LIB_M5, &[]);
    shared("libL5.so", LIB_L5, &["-l:libM5.so", libc_s]);
    shared("libG5.so", LIB_G5, &[libc_s]);
    shared("libB3.so", LIB_B3, &[]);
    shared("libA3.so", LIB_A3, &["-l:libB3.so"]);
    shared("libZ3.so", LIB_Z3, &[]);
    shared("libdeep.so", LIB_DEEP, &[]);
    shared("libdeep2.so", LIB_DEEP, &[]);
    shared("libu1.so", &lib_unique(11), &[]);
    shared("libu2.so", &lib_unique(22), &[]);
    shared("libifn.so", LIB_IFUNC, &[]);
    std::fs::write(d.join("libv.map"), LIB_V_MAP).unwrap();
    let map = format!("-Wl,--version-script={dir}/libv.map");
    shared("libv.so", LIB_V, &[&map]);
    let interp = format!("-Wl,--dynamic-linker={}", ld.display());
    let p = cc(
        &d,
        "prog",
        PROG,
        &["-nostdlib", "-fPIE", "-pie", "-Wl,--export-dynamic", "-Wl,--no-as-needed", &interp, crt1.to_str().unwrap(), h1.to_str().unwrap(), h2.to_str().unwrap(), libc_s, ld.to_str().unwrap(), "-lgcc"],
    );
    for (how, mut cmd) in [("kernel-started", Command::new(&p)), ("ld.so PROGRAM", {
        let mut c = Command::new(&ld);
        c.arg(&p);
        c
    })] {
        use std::os::unix::process::ExitStatusExt;
        let o = cmd.arg(&d).output().unwrap();
        let err = String::from_utf8_lossy(&o.stderr);
        assert_eq!((o.status.code(), o.status.signal()), (Some(0), None), "{how}: dlsym check {:?} failed; stderr: {err}", o.status.code());
    }
    std::fs::remove_dir_all(&d).ok();
}

/// A statically linked program's `dlsym` and `dlvsym`: NULL, the text once, then NULL (ruling #5005,
/// D-27.33, #5264).
const STATIC_PROG: &str = r#"#define _GNU_SOURCE
#include <dlfcn.h>
#include <stdio.h>
#include <string.h>
int main(void) {
    const char *e;
    if (dlsym(RTLD_DEFAULT, "main") != NULL || !(e = dlerror()) || strcmp(e, "dlsym: no symbols can be looked up in a statically linked program") != 0) { fprintf(stderr, "static dlsym: [%s]\n", e ? e : "(null)"); return 1; }
    if (dlerror() != NULL) return 2;
    if (dlvsym(RTLD_NEXT, "main", "V") != NULL || !(e = dlerror()) || strcmp(e, "dlvsym: no symbols can be looked up in a statically linked program") != 0) { fprintf(stderr, "static dlvsym: [%s]\n", e ? e : "(null)"); return 3; }
    return 0;
}
"#;

/// s27_1_f_0 (ruling #5005, D-27.33; ruling #5264): in a program linked with `libc.a`, `dlsym` and
/// `dlvsym` fail with NULL and a text, `dlerror` gives it once, and nothing crashes.
#[test]
fn s27_1_f_0_static_program_dlsym_fails_with_text() {
    let Some((d, _ld, _libc, crt1)) = setup("static") else { return };
    let liba = target().join("liella/libc.a");
    let p = cc(&d, "sprog", STATIC_PROG, &["-static", "-nostdlib", crt1.to_str().unwrap(), liba.to_str().unwrap(), "-lgcc"]);
    use std::os::unix::process::ExitStatusExt;
    let o = Command::new(&p).output().unwrap();
    assert_eq!((o.status.code(), o.status.signal()), (Some(0), None), "static dlsym; stderr: {}", String::from_utf8_lossy(&o.stderr));
    std::fs::remove_dir_all(&d).ok();
}
