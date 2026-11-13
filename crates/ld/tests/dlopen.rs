//! `dlopen` and `dlerror` under Liella's `ld.so` and `libc.so.6` (phase 14 DL-16a, plan/ph14/dl-16-dlopen.md
//! §1, §3, §4; spec/behaviour/dynlink.md "dlerror texts" and tools/probe/dl/gaps). One C program built
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

/// The scratch directory, its `ld.so` and `libc.so.6` (named by path: no `DT_SONAME`, as the
/// start-time loader needs), or `None` with the reason printed.
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
    let d = target().join(format!("ld-dlopen-{name}-{}", std::process::id()));
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

/// The helper objects: a plain one whose constructor counts in the program, one needing a missing
/// object, one with an undefined data reference.
const LIB_OK: &str = "extern int dl_ctor_count;
__attribute__((constructor)) static void c(void) { dl_ctor_count++; }
int dl_ok_fn(void) { return 42; }
";
const LIB_GONE: &str = "extern int gone_fn(void); int needs_gone(void) { return gone_fn(); }\n";
const LIB_UNDEF: &str = "extern int undefined_symbol_xyz; int uses_undef(void) { return undefined_symbol_xyz; }\n";
/// An IFUNC whose resolver writes one byte to fd 99 by a raw `write` (no relocation of its own),
/// and an internal call that makes binding it run the resolver.
const LIB_IFUNC: &str = "static const char mark = 'R';
static int impl(void) { return 7; }
static int (*res(void))(void) {
    long r;
    __asm__ volatile(\"syscall\" : \"=a\"(r) : \"a\"(1L), \"D\"(99L), \"S\"(&mark), \"d\"(1L) : \"rcx\", \"r11\", \"memory\");
    return impl;
}
int ifn(void) __attribute__((ifunc(\"res\")));
int use_ifn(void) { return ifn(); }
";

/// The program: argv[1] is the scratch directory.
const PROG: &str = r#"#include <dlfcn.h>
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>
int dl_ctor_count;
static char path[4096], want[8192], maps[1 << 16];
static int fail(int n, const char *what, const char *got) {
    fprintf(stderr, "check %d: %s: got [%s]\n", n, what, got ? got : "(null)");
    return n;
}
#define TEXT(n, call, expect) do { void *h_ = (call); const char *e_ = dlerror(); \
    if (h_ || !e_ || strcmp(e_, (expect)) != 0) return fail((n), (expect), e_); \
    if (dlerror() != NULL) return fail((n) + 100, "second dlerror NULL", "text"); } while (0)
static const char *at(const char *dir, const char *name) { snprintf(path, sizeof path, "%s/%s", dir, name); return path; }
static int mapped(const char *needle) {
    int fd = open("/proc/self/maps", O_RDONLY); ssize_t n, t = 0;
    while (fd >= 0 && (n = read(fd, maps + t, sizeof maps - 1 - t)) > 0) t += n;
    if (fd >= 0) close(fd);
    maps[t] = 0;
    return strstr(maps, needle) != NULL;
}
int main(int argc, char **argv) {
    const char *dir = argv[1];
    if (argc < 2) return 99;
    /* s27_1_c_1: the program's handle; no text. */
    void *self = dlopen(NULL, RTLD_NOW);
    if (!self || dlerror() || dlopen("", RTLD_LAZY) != self) return fail(1, "dlopen(NULL) handle", dlerror());
    /* A new object: constructor once, same handle again, l_name its path, NOLOAD hit. */
    void *h = dlopen(at(dir, "libok.so"), RTLD_NOW);
    if (!h) return fail(2, "dlopen libok.so", dlerror());
    if (dl_ctor_count != 1) return fail(3, "constructor ran once", "count");
    if (dlopen(at(dir, "libok.so"), RTLD_LAZY) != h || dl_ctor_count != 1) return fail(4, "re-open: same handle, no constructor", dlerror());
    if (dlopen(at(dir, "libok.so"), RTLD_NOW | RTLD_NOLOAD) != h) return fail(5, "NOLOAD hit", dlerror());
    if (strcmp(((const char *const *)h)[1], path) != 0) return fail(6, "l_name is the path", ((const char *const *)h)[1]);
    if (dlopen(at(dir, "libsym.so"), RTLD_NOW) != h) return fail(7, "symlink: same object", dlerror());
    /* s27_1_c_5: the texts (spec/behaviour/dynlink.md, tools/probe/dl/gaps). */
    snprintf(want, sizeof want, "%s: cannot open shared object file: No such file or directory", at(dir, "missing.so"));
    TEXT(10, dlopen(path, RTLD_NOW), want);
    snprintf(want, sizeof want, "%s: invalid mode for dlopen(): Invalid argument", at(dir, "libok.so"));
    TEXT(11, dlopen(path, 0), want);
    for (int bit = 0; bit < 32; bit++) {
        int m = (int)(1u << bit);
        if (m & (RTLD_LAZY | RTLD_NOW | RTLD_NOLOAD | RTLD_DEEPBIND | RTLD_GLOBAL | RTLD_NODELETE)) continue;
        TEXT(12, dlopen(at(dir, "libok.so"), RTLD_NOW | m), "invalid mode parameter");
    }
    snprintf(want, sizeof want, "%s: file too short", at(dir, "short.txt"));
    TEXT(13, dlopen(path, RTLD_NOW), want);
    snprintf(want, sizeof want, "%s: invalid ELF header", at(dir, "long.txt"));
    TEXT(14, dlopen(path, RTLD_NOW), want);
    snprintf(want, sizeof want, "%s: cannot read file data: Is a directory", at(dir, "subdir"));
    TEXT(15, dlopen(path, RTLD_NOW), want);
    snprintf(want, sizeof want, "%s: cannot dynamically load position-independent executable", argv[0]);
    TEXT(16, dlopen(argv[0], RTLD_NOW), want);
    snprintf(want, sizeof want, "%s: object file has no dynamic section", at(dir, "nodyn.so"));
    TEXT(17, dlopen(path, RTLD_NOW), want);
    /* A missing dependency: its DT_NEEDED name, and nothing of the attempt stays mapped. */
    TEXT(18, dlopen(at(dir, "libneedsgone.so"), RTLD_NOW), "libGone.so: cannot open shared object file: No such file or directory");
    if (mapped("libneedsgone.so")) return fail(19, "failed object unmapped", "still in /proc/self/maps");
    snprintf(want, sizeof want, "%s: undefined symbol: undefined_symbol_xyz", at(dir, "libundef.so"));
    TEXT(20, dlopen(path, RTLD_NOW), want);
    if (mapped("libundef.so")) return fail(21, "failed object unmapped", "still in /proc/self/maps");
    /* An RTLD_NOLOAD miss, and a success, clear a pending text (tools/probe/dl/gaps, #5127). */
    dlopen(at(dir, "missing.so"), RTLD_NOW);
    if (dlopen(at(dir, "libundef.so"), RTLD_NOW | RTLD_NOLOAD) != NULL || dlerror() != NULL) return fail(22, "NOLOAD miss: NULL, no text", "text");
    dlopen(at(dir, "missing.so"), RTLD_NOW);
    if (dlopen(at(dir, "libok.so"), RTLD_NOW) != h || dlerror() != NULL) return fail(23, "success clears the text", "text");
    if (!mapped("libok.so")) return fail(24, "loaded object mapped", "absent");
    /* A set that fails runs no IFUNC resolver, whichever order its objects come in (#5125, probe
       P-10); the resolver marks fd 99. Then the IFUNC object alone loads and its resolver runs. */
    int pfd[2]; char b = 0;
    if (pipe(pfd) != 0 || dup2(pfd[1], 99) != 99 || fcntl(pfd[0], F_SETFL, O_NONBLOCK) != 0) return fail(25, "pipe", "failed");
    const char *roots[2] = { "libroot_ui.so", "libroot_iu.so" };
    for (int i = 0; i < 2; i++) {
        void *r = dlopen(at(dir, roots[i]), RTLD_NOW);
        const char *e = dlerror();
        if (r || !e || !strstr(e, "undefined symbol: undefined_symbol_xyz")) return fail(26 + i, "failing set: undefined symbol", e);
    }
    if (read(pfd[0], &b, 1) != -1) return fail(28, "no resolver ran in a failing set", "marked");
    if (!dlopen(at(dir, "libifn.so"), RTLD_NOW) || read(pfd[0], &b, 1) != 1 || b != 'R') return fail(29, "the resolver runs on success", dlerror());
    return 0;
}
"#;

/// s27_1_c_1, s27_1_c_5 (plan/ph14/dl-16-dlopen.md §1, §3, §4; dynlink.md "dlerror texts", the gaps
/// probe; rulings #5125, #5126, #5127): `dlopen(NULL)`, a new object with its constructor run once, the same
/// handle for a second open, an `RTLD_NOLOAD` hit and a symlink, `l_name`; every refusal text with
/// one `dlerror` each, the sweep over every unused mode bit; a failed open (missing dependency,
/// undefined symbol) leaves nothing mapped and runs no IFUNC resolver in either dependency order (s27_1_f_0,
/// probe P-10); an `RTLD_NOLOAD` miss and a success clear a pending text.
#[test]
fn s27_1_c_1_dlopen_loads_and_refuses_with_texts() {
    let Some((d, ld, libc, crt1)) = setup("texts") else { return };
    let dir = d.to_str().unwrap();
    cc(&d, "libok.so", LIB_OK, &["-nostdlib", "-fPIC", "-shared"]);
    std::os::unix::fs::symlink(d.join("libok.so"), d.join("libsym.so")).unwrap();
    cc(&d, "libGone.so", "int gone_fn(void) { return 1; }\n", &["-nostdlib", "-fPIC", "-shared"]);
    cc(&d, "libneedsgone.so", LIB_GONE, &["-nostdlib", "-fPIC", "-shared", "-Wl,--no-as-needed", &format!("-L{dir}"), "-l:libGone.so"]);
    std::fs::remove_file(d.join("libGone.so")).unwrap();
    cc(&d, "libundef.so", LIB_UNDEF, &["-nostdlib", "-fPIC", "-shared"]);
    cc(&d, "libifn.so", LIB_IFUNC, &["-nostdlib", "-fPIC", "-shared"]);
    let rpath = format!("-Wl,-rpath,{dir}");
    for (root, first, second) in [("libroot_ui.so", "libundef.so", "libifn.so"), ("libroot_iu.so", "libifn.so", "libundef.so")] {
        let (a, b) = (format!("-l:{first}"), format!("-l:{second}"));
        cc(&d, root, "int root_fn(void) { return 0; }\n", &["-nostdlib", "-fPIC", "-shared", "-Wl,--no-as-needed", &format!("-L{dir}"), &rpath, &a, &b]);
    }
    // No PT_DYNAMIC: the valid object with that header retyped to PT_NULL.
    let mut nodyn = std::fs::read(d.join("libok.so")).unwrap();
    let elf = Elf::parse::<Native>(&nodyn).unwrap();
    let (phoff, phnum) = (elf.header().phoff as usize, elf.header().phnum as usize);
    let k = (0..phnum).find(|&i| nodyn[phoff + i * 56..phoff + i * 56 + 4] == 2u32.to_le_bytes()).unwrap();
    nodyn[phoff + k * 56..phoff + k * 56 + 4].copy_from_slice(&0u32.to_le_bytes());
    std::fs::write(d.join("nodyn.so"), nodyn).unwrap();
    std::fs::write(d.join("short.txt"), "not an object\n").unwrap();
    std::fs::write(d.join("long.txt"), "x".repeat(4096)).unwrap();
    std::fs::create_dir_all(d.join("subdir")).unwrap();
    let interp = format!("-Wl,--dynamic-linker={}", ld.display());
    let p = cc(&d, "prog", PROG, &["-nostdlib", "-fPIE", "-pie", "-Wl,--export-dynamic", &interp, crt1.to_str().unwrap(), libc.to_str().unwrap(), ld.to_str().unwrap(), "-lgcc"]);
    for (how, mut cmd) in [("kernel-started", Command::new(&p)), ("ld.so PROGRAM", { let mut c = Command::new(&ld); c.arg(&p); c })] {
        use std::os::unix::process::ExitStatusExt;
        let o = cmd.arg(&d).output().unwrap();
        let err = String::from_utf8_lossy(&o.stderr);
        assert_eq!((o.status.code(), o.status.signal()), (Some(0), None), "{how}: dlopen check {:?} failed; stderr: {err}", o.status.code());
    }
    std::fs::remove_dir_all(&d).ok();
}
