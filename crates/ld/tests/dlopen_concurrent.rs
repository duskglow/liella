//! The loader's lock-free readers and `dlopen` from many threads, constructors and `fork` under
//! Liella's `ld.so` and `libc.so.6` (phase 14 DL-16c `DL-DLOPEN-CONCURRENT`, plan/ph14/dl-16-dlopen.md
//! §3, §4; spec/behaviour/dynlink.md `== phdr`, P-7, P-8, P-9; probe tools/probe/dl/readers). One C
//! program built against the products checks every behaviour itself and exits with the number of
//! the first check that failed (its text on standard error); `alarm` is only a hang guard (a
//! deadlock), never a verdict on speed. Needs `cargo xtask so` and a C compiler (else SKIP).
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

/// The scratch directory, `ld.so`, `libc.so.6` without `DT_SONAME` and the start object, or `None`
/// with the reason printed (as crates/ld/tests/dlopen.rs).
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
    let d = target().join(format!("ld-dlconc-{name}-{}", std::process::id()));
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

/// Build `out` from `src` (C, or C++ when `out` ends in `cxx`) with `flags` in `d`.
fn build(d: &Path, out: &str, src: &str, flags: &[&str]) -> PathBuf {
    let cxx = out.ends_with("cxx");
    let c = d.join(format!("{out}.{}", if cxx { "cc" } else { "c" }));
    std::fs::write(&c, src).unwrap();
    let o = d.join(out);
    let r = Command::new(if cxx { "c++" } else { "cc" }).args(["-O1", "-fno-stack-protector", "-fcf-protection=none"]).arg(&c).args(flags).arg("-o").arg(&o).output().unwrap();
    assert!(r.status.success(), "{out}: {}", String::from_utf8_lossy(&r.stderr));
    o
}

/// One helper source, built as several objects (`-DKIND=`, `-DK=`).
/// KIND 1 (`libpaK`): an IFUNC bound by its own relocation, so its resolver runs in pass 2; the
/// resolver records whether `dl_iterate_phdr` lists the object already, the constructor whether it
/// does then and what `_dl_find_object` says. KIND 2 (`libca`): the constructor corpus. KIND 3
/// (`libslow`): a constructor that waits on a pipe the program names. KIND 4 (`libplain*`): a
/// constructor that counts. KIND 5 (`libthrough`): calls back the program (the C++ case).
const LIB: &str = r#"#define _GNU_SOURCE
#include <dlfcn.h>
#include <link.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
extern int resolver_saw[16], ctor_saw[16], ctor_log[64], n_log, slow_in, slow_out;
extern char dir[];
static int listed_cb(struct dl_phdr_info *i, size_t s, void *d) { (void)s; return strstr(i->dlpi_name, (const char *)d) != NULL; }
#if KIND == 1
#define S2(x) #x
#define S(x) S2(x)
static int impl(void) { return K; }
static void *resolve(void) { resolver_saw[K] = 1 + dl_iterate_phdr(listed_cb, "/libpa" S(K) ".so"); return (void *)impl; }
int pa_fn(void) __attribute__((ifunc("resolve")));
int (*pa_ptr)(void) = pa_fn;
__attribute__((constructor)) static void c(void) {
    struct dl_find_object fo;
    int found = _dl_find_object((void *)impl, &fo) == 0 && (char *)fo.dlfo_map_start <= (char *)impl && (char *)impl < (char *)fo.dlfo_map_end && fo.dlfo_eh_frame && fo.dlfo_link_map;
    ctor_saw[K] = 1 + dl_iterate_phdr(listed_cb, "/libpa" S(K) ".so") + 2 * found + 4 * (pa_ptr() == K);
}
#elif KIND == 2
static void *reader(void *a) { (void)a; struct dl_find_object fo; return (void *)(long)(dl_iterate_phdr(listed_cb, "/libca.so") + 2 * (_dl_find_object((void *)reader, &fo) == 0)); }
__attribute__((constructor)) static void c(void) {
    char p[4200], buf[64];
    ctor_log[n_log++] = 20;
    char *m = malloc(100); if (m) { strcpy(m, "heap"); ctor_log[n_log++] = 21 + (strcmp(m, "heap") != 0); free(m); }
    if (snprintf(buf, sizeof buf, "ctor %d", 7) == 6 && fputs("", stdout) >= 0) ctor_log[n_log++] = 23;
    snprintf(p, sizeof p, "%s/libca.so", dir);
    if (dlopen(p, RTLD_NOW | RTLD_NOLOAD)) ctor_log[n_log++] = 24;
    snprintf(p, sizeof p, "%s/libplain1.so", dir);
    if (dlopen(p, RTLD_NOW)) ctor_log[n_log++] = 25;
    if (dl_iterate_phdr(listed_cb, "/libca.so")) ctor_log[n_log++] = 26;
    pthread_t t; void *r = 0;
    if (pthread_create(&t, 0, reader, 0) == 0 && pthread_join(t, &r) == 0 && (long)r == 3) ctor_log[n_log++] = 27;
    ctor_log[n_log++] = 29;
}
#elif KIND == 3
extern int slow_in, slow_out;
__attribute__((constructor)) static void c(void) { char b = 1; if (write(slow_in, &b, 1) != 1 || read(slow_out, &b, 1) != 1) abort(); }
#elif KIND == 4
__attribute__((constructor)) static void c(void) { ctor_log[n_log++] = 40 + K; }
#elif KIND == 5
extern void (*through_fn)(void (*)(void));
extern int through_calls;
void through(void (*f)(void)) { through_calls++; f(); __asm__ volatile("" ::: "memory"); through_calls++; }
__attribute__((constructor)) static void c(void) { through_fn = through; }
#endif
"#;

/// The checking program: `prog DIR MODE` (MODE readers, concurrent or fork).
const PROG: &str = r#"#define _GNU_SOURCE
#include <dlfcn.h>
#include <link.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>
int resolver_saw[16], ctor_saw[16], ctor_log[64], n_log, slow_in, slow_out;
char dir[4096];
#define FAIL(n, ...) do { fprintf(stderr, __VA_ARGS__); fputc('\n', stderr); return n; } while (0)
static const char *at(const char *f) { static char b[4][4200]; static int i; i = (i + 1) % 4; snprintf(b[i], sizeof b[i], "%s/%s", dir, f); return b[i]; }

struct walk { int n; unsigned long long adds, subs; const char *names[64]; int new_seen; };
static int walk_cb(struct dl_phdr_info *i, size_t s, void *d) {
    struct walk *w = d; (void)s;
    if (w->n < 64) w->names[w->n] = i->dlpi_name;
    w->n++; w->adds = i->dlpi_adds; w->subs = i->dlpi_subs;
    return 0;
}
static struct walk walk(void) { struct walk w; memset(&w, 0, sizeof w); dl_iterate_phdr(walk_cb, &w); return w; }
static int opening_cb(struct dl_phdr_info *i, size_t s, void *d) {
    struct walk *w = d; (void)s;
    if (w->n++ == 0) { w->adds = i->dlpi_adds; if (!dlopen(at("libplain2.so"), RTLD_NOW)) return -1; }
    if (strstr(i->dlpi_name, "/libplain2.so") && i->dlpi_adds == w->adds + 1) w->new_seen = 1;
    return 0;
}
static int in_main(void) { return 1; }

static int readers(void) {
    struct walk w = walk();
    if (w.n < 4 || strcmp(w.names[0], "") != 0 || strcmp(w.names[1], "linux-vdso.so.1") != 0) FAIL(10, "start walk: %d objects, first [%s] [%s]", w.n, w.n ? w.names[0] : "", w.n > 1 ? w.names[1] : "");
    if (w.adds != (unsigned long long)w.n || w.subs != 0) FAIL(11, "start counters adds %llu subs %llu for %d objects", w.adds, w.subs, w.n);
    struct dl_find_object fo;
    if (_dl_find_object((void *)in_main, &fo) != 0 || !fo.dlfo_eh_frame || (char *)fo.dlfo_map_start > (char *)in_main || (char *)in_main >= (char *)fo.dlfo_map_end) FAIL(12, "_dl_find_object of the program before any dlopen");
    if (_dl_find_object((void *)&fo, &fo) != -1) FAIL(13, "_dl_find_object of a stack address");
    /* IFUNC resolvers run in pass 2, before publication: the object is not listed yet; its
       constructor runs after publication: listed, found, relocated. */
    void *h = dlopen(at("libpa1.so"), RTLD_NOW);
    if (!h) FAIL(14, "dlopen libpa1: %s", dlerror());
    if (resolver_saw[1] != 1) FAIL(15, "libpa1's IFUNC resolver (pass 2) saw the object listed (%d): published before pass 2", resolver_saw[1]);
    if (ctor_saw[1] != 8) FAIL(16, "libpa1's constructor: listed+found+relocated code %d, want 8", ctor_saw[1]);
    struct walk w2 = walk();
    if (w2.n != w.n + 1 || strcmp(w2.names[w2.n - 1], at("libpa1.so")) != 0 || w2.adds != w.adds + 1 || w2.subs != 0) FAIL(17, "after dlopen: %d objects, last [%s], adds %llu subs %llu", w2.n, w2.names[w2.n - 1], w2.adds, w2.subs);
    if (_dl_find_object((void *)in_main, &fo) != 0 || fo.dlfo_link_map != dlopen(NULL, RTLD_NOW)) FAIL(18, "_dl_find_object of the program: link map is not the program's handle");
    struct link_map *lm = h;
    if (_dl_find_object((void *)(lm->l_addr + 0x1000), &fo) != 0 || fo.dlfo_link_map != h) FAIL(19, "_dl_find_object in libpa1: link map is not its handle");
    /* A failed dlopen that mapped one object moves both counters by one (dynlink.md == phdr). */
    if (dlopen(at("libneedsgone.so"), RTLD_NOW)) FAIL(20, "libneedsgone opened");
    struct walk w3 = walk();
    if (w3.n != w2.n || w3.adds != w2.adds + 1 || w3.subs != w2.subs + 1) FAIL(21, "failed dlopen: %d objects adds %llu subs %llu (before %d, %llu, %llu)", w3.n, w3.adds, w3.subs, w2.n, w2.adds, w2.subs);
    /* A callback calls dlopen: no lock is held, and the running walk visits the new object (R-2). */
    struct walk w4; memset(&w4, 0, sizeof w4);
    if (dl_iterate_phdr(opening_cb, &w4) != 0 || !w4.new_seen || w4.n != w3.n + 1) FAIL(22, "dlopen in a callback: rc, seen %d, walked %d of %d", w4.new_seen, w4.n, w3.n + 1);
    /* Constructors call malloc, stdio, dlopen (a new object; themselves with NOLOAD while
       initialising), dl_iterate_phdr, and start and join a thread that reads (lock-free). */
    n_log = 0;
    if (!dlopen(at("libca.so"), RTLD_NOW)) FAIL(23, "dlopen libca: %s", dlerror());
    static const int want[] = { 20, 21, 23, 24, 41, 25, 26, 27, 29 };
    if (n_log != 9 || memcmp(ctor_log, want, sizeof want) != 0) { for (int k = 0; k < n_log; k++) fprintf(stderr, " %d", ctor_log[k]); FAIL(24, " <- constructor corpus log (want 20 21 23 24 41 25 26 27 29)"); }
    return 0;
}

/* A reader thread walks ITER times while the main thread opens eight objects: the listed count
   and the counters never go back, and every listed libpaK ran its pass-2 resolver already. */
#define ITER 20000
static int conc_rc;
static void *conc_reader(void *a) {
    (void)a; int last = 0; unsigned long long last_adds = 0;
    for (int it = 0; it < ITER; it++) {
        struct walk w = walk();
        if (w.n < last || w.adds < last_adds) { conc_rc = 31; return 0; }
        last = w.n; last_adds = w.adds;
        for (int k = 0; k < w.n && k < 64; k++) {
            const char *p = strstr(w.names[k], "/libpa");
            if (p && resolver_saw[p[6] - '0'] == 0) { conc_rc = 32; return 0; }
        }
        struct dl_find_object fo;
        if (_dl_find_object((void *)in_main, &fo) != 0) { conc_rc = 33; return 0; }
    }
    return 0;
}
static int concurrent(void) {
    pthread_t t;
    if (pthread_create(&t, 0, conc_reader, 0) != 0) FAIL(30, "pthread_create");
    for (int k = 2; k < 10; k++) { char n[32]; snprintf(n, sizeof n, "libpa%d.so", k); if (!dlopen(at(n), RTLD_NOW)) FAIL(34, "dlopen %s: %s", n, dlerror()); }
    pthread_join(t, 0);
    if (conc_rc) FAIL(conc_rc, "reader thread check %d failed", conc_rc);
    for (int k = 2; k < 10; k++) if (ctor_saw[k] != 8) FAIL(35, "libpa%d constructor code %d", k, ctor_saw[k]);
    return 0;
}

/* fork while another thread is inside a constructor (probe P-9): fork returns, and the child
   lists the object, finds it with RTLD_NOLOAD, and opens a new one. */
static void *slow_opener(void *a) { (void)a; return dlopen(at("libslow.so"), RTLD_NOW); }
static int forking(void) {
    int in[2], out[2];
    if (pipe(in) || pipe(out)) FAIL(40, "pipe");
    slow_in = in[1]; slow_out = out[0];
    pthread_t t; char b;
    if (pthread_create(&t, 0, slow_opener, 0) != 0) FAIL(41, "pthread_create");
    if (read(in[0], &b, 1) != 1) FAIL(42, "the slow constructor did not start");
    pid_t pid = fork();
    if (pid == 0) {
        alarm(60); /* a child does not inherit the parent's alarm: its own hang guard */
        struct walk w = walk(); int listed = 0;
        for (int k = 0; k < w.n && k < 64; k++) listed |= strstr(w.names[k], "/libslow.so") != NULL;
        n_log = 0;
        int ok = listed && dlopen(at("libslow.so"), RTLD_NOW | RTLD_NOLOAD) && dlopen(at("libplain3.so"), RTLD_NOW) && n_log == 1 && ctor_log[0] == 43;
        _exit(ok ? 0 : 1 + listed);
    }
    if (pid < 0) FAIL(43, "fork");
    int st = 0;
    if (waitpid(pid, &st, 0) != pid || !WIFEXITED(st) || WEXITSTATUS(st) != 0) FAIL(44, "the child could not use the loader (status %#x)", st);
    b = 1;
    if (write(out[1], &b, 1) != 1) FAIL(45, "release");
    void *h = 0;
    pthread_join(t, &h);
    if (!h) FAIL(46, "the slow dlopen failed");
    return 0;
}

int main(int argc, char **argv) {
    if (argc < 3) return 2;
    snprintf(dir, sizeof dir, "%s", argv[1]);
    alarm(120); /* hang guard only: a deadlock ends the run */
    if (!strcmp(argv[2], "readers")) return readers();
    if (!strcmp(argv[2], "concurrent")) return concurrent();
    if (!strcmp(argv[2], "fork")) return forking();
    return 3;
}
"#;

/// The C++ case: an exception thrown by the program's callback unwinds through a frame of a
/// `dlopen`'d C object (found by `_dl_find_object`) and is caught in the program, both destructors run.
const CXX: &str = r#"#include <dlfcn.h>
#include <stdio.h>
extern "C" { void (*through_fn)(void (*)(void)); int through_calls; int resolver_saw[16], ctor_saw[16], ctor_log[64], n_log, slow_in, slow_out; char dir[4096]; }
static int dtors;
struct D { ~D() { dtors++; } };
static void thrower() { D d; throw 42; }
int main(int argc, char **argv) {
    if (argc < 2) return 2;
    char p[4200];
    snprintf(p, sizeof p, "%s/libthrough.so", argv[1]);
    if (!dlopen(p, RTLD_NOW) || !through_fn) { fprintf(stderr, "dlopen libthrough: %s\n", dlerror()); return 3; }
    try { D outer; through_fn(thrower); } catch (int v) {
        if (v == 42 && dtors == 2 && through_calls == 1) return 0;
        fprintf(stderr, "caught %d, dtors %d, calls %d\n", v, dtors, through_calls);
        return 4;
    }
    fprintf(stderr, "not thrown\n");
    return 5;
}
"#;

/// Build the corpus in `d`: the helpers and the program.
fn corpus(d: &Path, ld: &Path, libc: &Path, crt1: &Path) -> PathBuf {
    let shared = ["-nostdlib", "-fPIC", "-shared", "-fexceptions"];
    let mut kinds: Vec<(String, String)> = (1..10).map(|k| (format!("libpa{k}.so"), format!("-DKIND=1 -DK={k}"))).collect();
    kinds.push(("libca.so".into(), "-DKIND=2".into()));
    kinds.push(("libslow.so".into(), "-DKIND=3".into()));
    for k in 1..4 {
        kinds.push((format!("libplain{k}.so"), format!("-DKIND=4 -DK={k}")));
    }
    kinds.push(("libthrough.so".into(), "-DKIND=5".into()));
    for (out, defs) in &kinds {
        let mut flags: Vec<&str> = shared.to_vec();
        flags.extend(defs.split(' '));
        build(d, out, LIB, &flags);
    }
    let dir = d.to_str().unwrap();
    build(d, "libGone.so", "int gone_fn(void) { return 1; }\n", &shared);
    build(d, "libneedsgone.so", "extern int gone_fn(void); int ng(void) { return gone_fn(); }\n", &["-nostdlib", "-fPIC", "-shared", "-Wl,--no-as-needed", &format!("-L{dir}"), "-l:libGone.so"]);
    std::fs::remove_file(d.join("libGone.so")).unwrap();
    let interp = format!("-Wl,--dynamic-linker={}", ld.display());
    build(d, "prog", PROG, &["-nostdlib", "-fPIE", "-pie", "-Wl,--export-dynamic", &interp, crt1.to_str().unwrap(), libc.to_str().unwrap(), ld.to_str().unwrap(), "-lgcc"])
}

/// Run `prog DIR MODE` kernel-started and as `ld.so PROGRAM`; both must exit 0.
fn run(d: &Path, ld: &Path, prog: &Path, mode: &str) {
    use std::os::unix::process::ExitStatusExt;
    for (how, mut cmd) in [("kernel-started", Command::new(prog)), ("ld.so PROGRAM", { let mut c = Command::new(ld); c.arg(prog); c })] {
        let o = cmd.arg(d).arg(mode).output().unwrap();
        let err = String::from_utf8_lossy(&o.stderr);
        assert_eq!((o.status.code(), o.status.signal()), (Some(0), None), "{how}: {mode} check {:?} failed; stderr: {err}", o.status.code());
    }
}

/// s27_1_c_8 (ruling #2607, plan §4, probes `== phdr` and R-1..R-4): `dl_iterate_phdr` lists the
/// program, the vDSO, its dependencies and each `dlopen`'d object in load order with the probed
/// counters (a failed open that mapped one object moves both); an object is published after pass 2
/// (its IFUNC resolver does not see it listed) and before its constructors (they do, and
/// `_dl_find_object` finds it with its link map); a callback may call `dlopen` and the running walk
/// visits the new object; constructors call malloc, stdio, `dlopen` (P-7) and start and join a
/// thread that reads.
#[test]
fn s27_1_c_8_readers_see_published_objects_only() {
    let Some((d, ld, libc, crt1)) = setup("readers") else { return };
    let prog = corpus(&d, &ld, &libc, &crt1);
    run(&d, &ld, &prog, "readers");
    std::fs::remove_dir_all(&d).ok();
}

/// s27_1_c_8 (plan §4, D-27.1): a reader thread walks 20000 times (an iteration budget) while the
/// main thread opens eight objects: counts never go back and no listed object is half published.
#[test]
fn s27_1_c_8_concurrent_dlopen_and_walk() {
    let Some((d, ld, libc, crt1)) = setup("concurrent") else { return };
    let prog = corpus(&d, &ld, &libc, &crt1);
    run(&d, &ld, &prog, "concurrent");
    std::fs::remove_dir_all(&d).ok();
}

/// s27_1_c_1 (probe P-9, plan §4 lock order): `fork` while another thread runs a `dlopen`'d
/// constructor returns, and the child lists the object, finds it with `RTLD_NOLOAD` and opens a
/// new object whose constructor runs.
#[test]
fn s27_1_c_1_fork_during_dlopen_child_can_load() {
    let Some((d, ld, libc, crt1)) = setup("fork") else { return };
    let prog = corpus(&d, &ld, &libc, &crt1);
    run(&d, &ld, &prog, "fork");
    std::fs::remove_dir_all(&d).ok();
}

/// s27_1_c_8 (`_dl_find_object` for unwinders, plan §4): a C++ exception thrown by the program's
/// callback unwinds through a frame of a `dlopen`'d object and is caught in the program with both
/// destructors run (libgcc's unwinder linked statically, libsupc++ for the C++ runtime).
#[test]
fn s27_1_c_8_cxx_exception_through_dlopened_object() {
    let Some((d, ld, libc, crt1)) = setup("cxx") else { return };
    if !Command::new("c++").arg("--version").output().is_ok_and(|o| o.status.success()) {
        eprintln!("SKIP: no system C++ compiler");
        return;
    }
    let file = |f: &str| String::from_utf8(Command::new("c++").arg(format!("-print-file-name={f}")).output().unwrap().stdout).unwrap().trim().to_owned();
    let (begin, end) = (file("crtbeginS.o"), file("crtendS.o"));
    build(&d, "libthrough.so", LIB, &["-nostdlib", "-fPIC", "-shared", "-fexceptions", "-DKIND=5"]);
    let interp = format!("-Wl,--dynamic-linker={}", ld.display());
    let prog = build(&d, "prog_cxx", CXX, &["-nostdlib", "-fPIE", "-pie", "-Wl,--export-dynamic", "-Wl,--eh-frame-hdr", &interp, crt1.to_str().unwrap(), &begin, libc.to_str().unwrap(), ld.to_str().unwrap(), "-Wl,-Bstatic", "-lsupc++", "-lgcc_eh", "-lgcc", "-Wl,-Bdynamic", libc.to_str().unwrap(), ld.to_str().unwrap(), &end]);
    use std::os::unix::process::ExitStatusExt;
    let o = Command::new(&prog).arg(&d).output().unwrap();
    assert_eq!((o.status.code(), o.status.signal()), (Some(0), None), "C++ throw through a dlopen'd object; stderr: {}", String::from_utf8_lossy(&o.stderr));
    std::fs::remove_dir_all(&d).ok();
}
