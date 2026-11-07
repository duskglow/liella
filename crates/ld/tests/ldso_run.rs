//! `ld.so` loads and enters a program (phase 14 DL-6, plan/ph14/design.md §1 steps 3 to 5, D-27.14):
//! the start decisions as pure logic (malformed auxiliary vectors refused, the `LD_*` settings, the
//! auxiliary-vector rewrite), the CPU-feature seam, and the built product running a `-nostdlib` PIE
//! with no dependencies 100 times each way (named in `PT_INTERP`, and as `ld.so PROGRAM ARGS`), half
//! with address randomisation off, plus a secure-mode (set-group-ID) child whose environment has
//! lost every `UNSECURE` name before the loader read any `LD_*` name.
//!
//! no spec clause: plan/ph14/design.md §1, spec/security/threat-model.md "Who clears, and when"
//! (owner approval #2661), phase 14 DL-6 (D-27.14).
//! Skips (with the reason) when `cc` is absent, the product is not built, or (secure test only) the
//! user has no supplementary group or the file system ignores the set-group-ID bit.
// ARCH(x86_64): the test program is x86-64 assembly and system call numbers; the product is x86_64's.
#![cfg(target_arch = "x86_64")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects, clippy::as_conversions, missing_docs)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use liella_kernel::auxv::Auxv;
use liella_ld::arch::{Arch, Native};
use liella_ld::start::{self, StartError};

/// The names a secure process must not see (spec/security/threat-model.md, the table: ld.so(8) and
/// the probe), written out here so a name dropped from `rt::start::UNSECURE` is caught.
const UNSECURE: [&str; 34] = [
    "GCONV_PATH", "GETCONF_DIR", "GLIBC_TUNABLES", "HOSTALIASES", "LD_AUDIT", "LD_BIND_NOT", "LD_BIND_NOW", "LD_DEBUG",
    "LD_DEBUG_OUTPUT", "LD_DYNAMIC_WEAK", "LD_HWCAP_MASK", "LD_LIBRARY_PATH", "LD_ORIGIN_PATH", "LD_PRELOAD", "LD_PROFILE",
    "LD_SHOW_AUXV", "LD_VERBOSE", "LD_WARN", "LOCALDOMAIN", "LOCPATH", "MALLOC_ARENA_MAX", "MALLOC_ARENA_TEST",
    "MALLOC_MMAP_MAX_", "MALLOC_MMAP_THRESHOLD_", "MALLOC_PERTURB_", "MALLOC_TOP_PAD_", "MALLOC_TRACE",
    "MALLOC_TRIM_THRESHOLD_", "NIS_PATH", "NLSPATH", "RESOLV_HOST_CONF", "RES_OPTIONS", "TMPDIR", "TZDIR",
];

/// The test program: prints `secure:<AT_SECURE>`, `reloc:ok` through a pointer the loader relocates,
/// `entry:ok|bad` (AT_ENTRY is its own `_start`), `phdr:ok|bad` (AT_PHDR inside its image), then one
/// line per argument and environment entry; exits 0. No libc, no dependencies.
const PROGRAM: &str = r#"
static long sys3(long n, long a, long b, long c) {
    long r;
    __asm__ volatile("syscall" : "=a"(r) : "a"(n), "D"(a), "S"(b), "d"(c) : "rcx", "r11", "memory");
    return r;
}
static unsigned long len(const char *s) { unsigned long n = 0; while (s[n]) n++; return n; }
static void put(const char *p, const char *s) { sys3(1, 1, (long)p, len(p)); sys3(1, 1, (long)s, len(s)); sys3(1, 1, (long)"\n", 1); }
static const char *volatile ok = "ok"; /* read through memory: a RELATIVE relocation */
extern char _start[], __ehdr_start[], _end[];
void start_c(long *sp) {
    long argc = sp[0];
    char **argv = (char **)(sp + 1), **envp = argv + argc + 1, **e = envp;
    while (*e) e++;
    long *aux = (long *)(e + 1);
    long secure = -1, entry = 0, phdr = 0;
    for (; aux[0]; aux += 2) {
        if (aux[0] == 23) secure = aux[1];
        if (aux[0] == 9) entry = aux[1];
        if (aux[0] == 3) phdr = aux[1];
    }
    put("secure:", secure == 1 ? "1" : secure == 0 ? "0" : "missing");
    put("reloc:", ok);
    put("entry:", entry == (long)_start ? "ok" : "bad");
    put("phdr:", phdr > (long)__ehdr_start && phdr < (long)_end ? "ok" : "bad");
    for (long i = 0; i < argc; i++) put("arg:", argv[i]);
    for (e = envp; *e; e++) put("env:", *e);
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

/// The built interpreter, or `None` (with the reason) before `cargo xtask so`.
fn product() -> Option<PathBuf> {
    let p = target().join("liella").join(Native::INTERP);
    if p.exists() {
        return Some(p);
    }
    eprintln!("SKIP: {} not built (run cargo xtask so)", p.display());
    None
}

/// A scratch directory under the target directory (not /tmp: the set-group-ID test needs a file
/// system that honours the bit).
fn dir(name: &str) -> PathBuf {
    let d = target().join(format!("ldso-run-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Link the test program with `ld` as its interpreter (plus `extra` flags), into `d`.
fn build(d: &Path, ld: &Path, extra: &[&str]) -> PathBuf {
    let c = d.join("prog.c");
    std::fs::write(&c, PROGRAM).unwrap();
    let exe = d.join("prog");
    let o = Command::new("cc")
        .args(["-nostdlib", "-fPIE", "-pie", "-O1", "-fno-stack-protector", "-fcf-protection=none", "-Wl,-z,relro"])
        .args(extra)
        .arg(format!("-Wl,--dynamic-linker={}", ld.display()))
        .arg(&c)
        .arg("-o")
        .arg(&exe)
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    exe
}

/// Run with a hang guard only (`timeout`; a regression could spin): the output.
fn run(cmd: &mut Command) -> Output {
    cmd.output().unwrap()
}

fn lines(o: &Output) -> Vec<String> {
    String::from_utf8_lossy(&o.stdout).lines().map(str::to_owned).collect()
}

/// The program's own report: started, relocated, and found its entry and headers in the vector.
fn assert_ran(o: &Output, what: &str) -> Vec<String> {
    assert_eq!(o.status.code(), Some(0), "{what}: {o:?}");
    let l = lines(o);
    for want in ["reloc:ok", "entry:ok", "phdr:ok"] {
        assert!(l.iter().any(|x| x == want), "{what}: no {want}: {l:?}");
    }
    l
}

fn randomised(i: usize, setarch: bool, prog: &Path) -> Command {
    let mut c = Command::new("timeout");
    c.arg("60");
    if setarch && i % 2 == 0 {
        c.args(["setarch", "-R"]);
    }
    c.arg(prog);
    c
}

fn setarch_ok() -> bool {
    let ok = Command::new("setarch").args(["-R", "true"]).status().is_ok_and(|s| s.success());
    if !ok {
        eprintln!("NOTE: setarch -R unavailable: all 100 starts randomised");
    }
    ok
}

/// (no spec clause: ld.so's start, plan/ph14/design.md §1 steps 3-5, D-27.14.)
#[test]
fn ldso_run_malformed_auxv_refused() {
    let v = |w: &[u64]| start::program(&Auxv::from_words(w));
    // AT_PHDR 3, AT_PHENT 4, AT_PHNUM 5, AT_ENTRY 9.
    assert_eq!(v(&[3, 0x1000, 4, 56, 5, 9, 9, 0x2000]), Ok(start::Program { phdr: 0x1000, phnum: 9, entry: 0x2000 }));
    assert_eq!(v(&[4, 56, 5, 9, 9, 0x2000]), Err(StartError::NoPhdr));
    assert_eq!(v(&[3, 0, 5, 9, 9, 0x2000]), Err(StartError::NoPhdr));
    assert_eq!(v(&[3, 0x1000, 5, 9]), Err(StartError::NoEntry));
    assert_eq!(v(&[3, 0x1000, 4, 32, 5, 9, 9, 0x2000]), Err(StartError::BadPhent));
    assert_eq!(v(&[3, 0x1000, 9, 0x2000]), Err(StartError::BadPhnum));
    for n in [0, start::PN_XNUM, u64::MAX] {
        assert_eq!(v(&[3, 0x1000, 5, n, 9, 0x2000]), Err(StartError::BadPhnum), "{n}");
    }
    assert!(!start::secure(&Auxv::from_words(&[])));
    assert!(!start::secure(&Auxv::from_words(&[23, 0])));
    assert!(start::secure(&Auxv::from_words(&[23, 1])));
}

/// (no spec clause: ld.so's start, plan/ph14/design.md §1 steps 3-5, D-27.14.)
#[test]
fn ldso_run_aux_rewrite_and_env() {
    let mut w = [3, 1, 4, 99, 5, 2, 7, 0, 9, 3, 6, 4096, 0, 0, 9, 7];
    start::rewrite(&mut w, &start::Program { phdr: 0x10, phnum: 11, entry: 0x20 }, 0x30);
    assert_eq!(w, [3, 0x10, 4, 56, 5, 11, 7, 0x30, 9, 0x20, 6, 4096, 0, 0, 9, 7]);
    let env: [&[u8]; 6] = [b"LD_PRELOADX=a", b"LD_PRELOAD=a  b:c", b"LD_PRELOAD=second", b"LD_BIND_NOW=", b"GLIBC_TUNABLES=x=1", b"LD_LIBRARY_PATH=/l"];
    let ld = start::ld_env(env);
    assert_eq!(ld.preload, Some(&b"a  b:c"[..]));
    assert_eq!(start::preload_names(ld.preload.unwrap()).collect::<Vec<_>>(), [&b"a"[..], b"b", b"c"]);
    assert!(!ld.bind_now);
    assert!(start::ld_env([&b"LD_BIND_NOW=1"[..]]).bind_now);
    assert_eq!((ld.tunables, ld.library_path), (Some(&b"x=1"[..]), Some(&b"/l"[..])));
    assert!(start::needs_objects(&[0x6ffffef5, 0, 1, 7, 0, 0]));
    assert!(!start::needs_objects(&[5, 0, 0, 0, 1, 7]));
}

/// The CPU-feature seam (design §0a item 5): outside `liella_ld::arch`, no loader source executes
/// CPUID or reads `AT_HWCAP*`/`AT_PLATFORM`, and the loader's start takes features from
/// `Native::features`.
/// (no spec clause: ld.so's start, plan/ph14/design.md §1 steps 3-5, D-27.14.)
#[test]
fn ldso_run_features_only_from_arch() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files: Vec<PathBuf> = std::fs::read_dir(root.join("src")).unwrap().map(|e| e.unwrap().path()).filter(|p| !p.ends_with("arch.rs")).collect();
    files.sort(); // FS-ORDER: every file is checked; sorted for a stable first failure
    let ld = root.join("../boundary/src/ld.rs");
    files.push(ld.clone());
    for f in &files {
        let s = std::fs::read_to_string(f).unwrap();
        for bad in ["Features::detect", "cpuid", "AT_HWCAP", "AT_PLATFORM", "xcr0"] {
            assert!(!s.contains(bad), "{}: `{bad}` outside the arch seam", f.display());
        }
    }
    assert!(std::fs::read_to_string(&ld).unwrap().contains("Native::features("), "ld.rs takes no features from the seam");
}

/// (no spec clause: ld.so's start, plan/ph14/design.md §1 steps 3-5, D-27.14.)
#[test]
fn ldso_run_by_interp_at_random_bases() {
    let (Some(ld), true) = (product(), cc_ok()) else { return eprintln!("SKIP: no product or no cc") };
    let d = dir("interp");
    let prog = build(&d, &ld, &[]);
    let setarch = setarch_ok();
    for i in 0..100 {
        let o = run(randomised(i, setarch, &prog).env("LIELLA_KEEP", "1").args(["one", "two"]));
        let l = assert_ran(&o, &format!("start {i}"));
        let args: Vec<_> = l.iter().filter(|x| x.starts_with("arg:")).cloned().collect();
        assert_eq!(args, [format!("arg:{}", prog.display()), "arg:one".into(), "arg:two".into()], "start {i}");
        assert!(l.iter().any(|x| x == "env:LIELLA_KEEP=1") && l.iter().any(|x| x == "secure:0"), "start {i}: {l:?}");
    }
    let _ = std::fs::remove_dir_all(&d);
}

/// (no spec clause: ld.so's start, plan/ph14/design.md §1 steps 3-5, D-27.14.)
#[test]
fn ldso_run_as_program_at_random_bases() {
    let (Some(ld), true) = (product(), cc_ok()) else { return eprintln!("SKIP: no product or no cc") };
    let d = dir("program");
    let prog = build(&d, &ld, &[]);
    let setarch = setarch_ok();
    for i in 0..100 {
        let mut c = randomised(i, setarch, &ld);
        let o = run(c.arg(&prog).arg("x"));
        let l = assert_ran(&o, &format!("start {i}"));
        let args: Vec<_> = l.iter().filter(|x| x.starts_with("arg:")).cloned().collect();
        assert_eq!(args, [format!("arg:{}", prog.display()), "arg:x".into()], "start {i}");
    }
    // Refused without a crash: not an ELF file, a program with a dependency, a missing file.
    let needs = d.join("needs");
    let o = Command::new("cc").args(["-fPIE", "-pie", "-O1"]).arg(d.join("prog.c")).args(["-nostartfiles", "-Wl,--no-as-needed", "-lm", "-o"]).arg(&needs).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    for (arg, msg) in [(d.join("prog.c"), "not a valid ELF"), (needs, "needs shared objects"), (d.join("absent"), "cannot open")] {
        let o = run(Command::new(&ld).arg(&arg));
        assert_eq!(o.status.code(), Some(127), "{}: {o:?}", arg.display());
        assert!(String::from_utf8_lossy(&o.stderr).contains(msg), "{}: {o:?}", arg.display());
    }
    let _ = std::fs::remove_dir_all(&d);
}

/// A non-secure start reads `LD_PRELOAD` (the diagnostic for each name) and parses the tunables;
/// the program still runs.
/// (no spec clause: ld.so's start, plan/ph14/design.md §1 steps 3-5, D-27.14.)
#[test]
fn ldso_run_reads_ld_settings() {
    let (Some(ld), true) = (product(), cc_ok()) else { return eprintln!("SKIP: no product or no cc") };
    let d = dir("settings");
    let prog = build(&d, &ld, &[]);
    let o = run(Command::new(&prog).env("LD_PRELOAD", "liba.so:libb.so").env("GLIBC_TUNABLES", "glibc.malloc.perturb=1"));
    assert_ran(&o, "settings");
    let err = String::from_utf8_lossy(&o.stderr);
    for n in ["liba.so", "libb.so"] {
        assert!(err.contains(&format!("ERROR: ld.so: object '{n}' from LD_PRELOAD cannot be preloaded")), "{err}");
    }
    let _ = std::fs::remove_dir_all(&d);
}

/// R14-18: a set-group-ID program (the kernel sets AT_SECURE; a supplementary group of the user, no
/// root) started through `ld.so` sees none of the UNSECURE names, keeps everything else (near
/// misses included), and the loader read no `LD_*` name before clearing (no LD_PRELOAD diagnostic).
/// (no spec clause: ld.so's start, plan/ph14/design.md §1 steps 3-5, D-27.14.)
#[test]
fn ldso_run_secure_child_sees_unsecure_cleared() {
    let (Some(ld), true) = (product(), cc_ok()) else { return eprintln!("SKIP: no product or no cc") };
    let id = |a: &str| String::from_utf8(Command::new("id").arg(a).output().unwrap().stdout).unwrap();
    let primary = id("-g").trim().to_owned();
    let Some(group) = id("-G").split_whitespace().find(|g| *g != primary).map(str::to_owned) else {
        return eprintln!("SKIP: the user has no supplementary group (a set-group-ID file needs one without root)");
    };
    let d = dir("secure");
    let prog = build(&d, &ld, &[]);
    assert!(Command::new("chgrp").arg(&group).arg(&prog).status().unwrap().success());
    assert!(Command::new("chmod").arg("g+s").arg(&prog).status().unwrap().success());
    let mut c = Command::new(&prog);
    c.env_clear();
    for n in UNSECURE {
        c.env(n, "x");
    }
    let keep = ["LD_", "LD_LIBRARY_PATH_X", "TMPDIRX", "PATH", "LANG", "MALLOC_CHECK_X"];
    for n in keep {
        c.env(n, "kept");
    }
    let o = run(&mut c);
    let l = assert_ran(&o, "secure");
    if l.iter().any(|x| x == "secure:0") {
        let _ = std::fs::remove_dir_all(&d);
        return eprintln!("SKIP: the kernel did not set AT_SECURE (the file system ignores set-group-ID here)");
    }
    assert!(l.iter().any(|x| x == "secure:1"), "{l:?}");
    let env: Vec<&str> = l.iter().filter_map(|x| x.strip_prefix("env:")).collect();
    for n in UNSECURE {
        assert!(!env.iter().any(|e| e.split('=').next() == Some(n)), "{n} reached the secure program: {env:?}");
    }
    for n in keep {
        assert!(env.contains(&format!("{n}=kept").as_str()), "{n} was removed: {env:?}");
    }
    assert!(o.stderr.is_empty(), "the loader read an LD_* name before clearing: {}", String::from_utf8_lossy(&o.stderr));
    let _ = std::fs::remove_dir_all(&d);
}
