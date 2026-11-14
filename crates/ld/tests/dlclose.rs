//! `dlclose` under Liella's `ld.so` and `libc.so.6` (phase 14 DL-17 `DL-DLCLOSE`, handoff/DL-DLCLOSE.md;
//! plan/ph14/design.md §7-§10, plan/ph14/dl-16-dlopen.md §1 and §4; spec/behaviour/dynlink.md `== close`,
//! `== order`, `== phdr`; probe tools/probe/dl/close). One C program built against the products checks
//! every behaviour itself and exits with the number of the first check that failed (its text on
//! standard error); `alarm` is only a hang guard (a deadlock), never a verdict on speed; loops have
//! iteration budgets. Needs `cargo xtask so` and a C compiler (else SKIP).
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
/// with the reason printed (as crates/ld/tests/dlopen_concurrent.rs).
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
    let d = target().join(format!("ld-dlclose-{name}-{}", std::process::id()));
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

/// Build `out` from `src` (C, or C++ when `out` ends in `cxx` or `cxx.so`) with `flags` in `d`.
fn build(d: &Path, out: &str, src: &str, flags: &[&str]) -> PathBuf {
    let cxx = out.ends_with("cxx") || out.ends_with("cxx.so");
    let c = d.join(format!("{out}.{}", if cxx { "cc" } else { "c" }));
    std::fs::write(&c, src).unwrap();
    let o = d.join(out);
    let r = Command::new(if cxx { "c++" } else { "cc" }).args(["-O1", "-fno-stack-protector", "-fcf-protection=none"]).arg(&c).args(flags).arg("-o").arg(&o).output().unwrap();
    assert!(r.status.success(), "{out}: {}", String::from_utf8_lossy(&r.stderr));
    o
}

/// One helper source, built as several `-nostdlib` objects (`-DKIND_<name>`). Each records its
/// destructor (`DT_FINI_ARRAY`) and `DT_FINI` in the program's event log (`ev`, exported).
const LIB: &str = r#"#include <dlfcn.h>
extern void ev(const char *);
extern char dir[];
extern int pctor, pdtor;
#define DTOR(t) __attribute__((destructor)) static void dtor(void) { ev("d" t); } void fini_hook(void) { ev("f" t); }
#ifdef KIND_A
DTOR("A") int a_fn(void) { return 1; }
#endif
#ifdef KIND_B
DTOR("B") int b_fn(void) { return 2; }
#endif
#ifdef KIND_C
DTOR("C") int c_fn(void) { return 3; }
#endif
#ifdef KIND_G
DTOR("G") int g_val = 42;
#endif
#ifdef KIND_U
DTOR("U") extern int g_val; int use_g(void) { return g_val; }
#endif
#ifdef KIND_N
DTOR("N") int n_fn(void) { return 4; }
#endif
#ifdef KIND_Y
DTOR("Y") int y_fn(void) { return 5; }
#endif
#ifdef KIND_X
static void *y;
static char path[4200];
__attribute__((constructor)) static void ctor(void) {
    int i = 0;
    for (const char *s = dir; *s; s++) path[i++] = *s;
    for (const char *s = "/libY.so"; *s; s++) path[i++] = *s;
    path[i] = 0;
    y = dlopen(path, RTLD_NOW);
}
__attribute__((destructor)) static void dtor(void) { ev(y ? "dX1" : "dX-noY"); ev(dlclose(y) == 0 ? "dX2" : "dX-fail"); }
void fini_hook(void) { ev("fX"); }
#endif
#ifdef KIND_T
__attribute__((tls_model("initial-exec"))) __thread int t_var = 7;
int *t_addr(void) { return &t_var; }
#endif
#ifdef KIND_T2
__attribute__((tls_model("initial-exec"))) __thread long t_var[4] = {11, 12, 13, 14};
long *t_addr(void) { return t_var; }
#endif
#ifdef KIND_T3
__attribute__((tls_model("initial-exec"))) __thread long t_var[16] = {21};
long *t_addr(void) { return t_var; }
#endif
#ifdef KIND_R
int r_fn(void) { return 6; }
#endif
#ifdef KIND_K
extern void k_ctor(void);
__attribute__((constructor)) static void kc(void) { k_ctor(); }
#endif
#ifdef KIND_P
__attribute__((constructor)) static void pc(void) { pctor++; }
__attribute__((destructor)) static void pd(void) { pdtor++; }
int p_fn(void) { return 7; }
#endif
"#;

/// The program: `prog DIR MODE` (`basic`, `concurrent`, `room`); exit 0 or the failed check's number.
const PROG: &str = r#"#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <fcntl.h>
#include <link.h>
#include <pthread.h>
#include <sched.h>
#include <semaphore.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/syscall.h>
#include <sys/wait.h>
#include <unistd.h>
char dir[4096];
int pctor, pdtor;
static char evlog[4096];
void ev(const char *s) { strcat(evlog, s); strcat(evlog, " "); }
#define CHECK(n, c, ...) do { if (!(c)) { fprintf(stderr, "check %d: ", n); fprintf(stderr, __VA_ARGS__); fprintf(stderr, "\n"); exit(n); } } while (0)
static char pbuf[4200];
static const char *P(const char *n) { snprintf(pbuf, sizeof pbuf, "%s/%s", dir, n); return pbuf; }
static int loaded(const char *n) { void *h = dlopen(P(n), RTLD_NOW | RTLD_NOLOAD); if (h) dlclose(h); return h != NULL; }
static int mapped(void *a) { long pg = sysconf(_SC_PAGESIZE); return msync((void *)((unsigned long)a & -(unsigned long)pg), pg, MS_ASYNC) == 0 || errno != ENOMEM; }
struct cnt { int n; unsigned long long adds, subs; const char *want; size_t modid; void *close_first, *close; long sum; int seen; };
static int cb(struct dl_phdr_info *i, size_t s, void *d) {
    struct cnt *c = d; (void)s;
    c->n++; c->adds = i->dlpi_adds; c->subs = i->dlpi_subs;
    if (c->want && i->dlpi_name && strstr(i->dlpi_name, c->want)) {
        c->modid = i->dlpi_tls_modid;
        c->seen = 1;
        if (c->close_first) { dlclose(c->close_first); c->close_first = NULL; }
        if (c->close) { dlclose(c->close); c->close = NULL; }
        /* Read the object's program headers after a dlclose in this callback: still mapped. */
        for (int k = 0; k < i->dlpi_phnum; k++) c->sum += i->dlpi_phdr[k].p_type + i->dlpi_phdr[k].p_memsz;
    }
    return 0;
}
static struct cnt walk(const char *want) { struct cnt c = {0}; c.want = want; dl_iterate_phdr(cb, &c); return c; }
static const char *err(void) { const char *e = dlerror(); return e ? e : "(none)"; }

static void basic(void) {
    struct cnt c0 = walk(NULL);
    /* s27_1_c_4: A needs B; one close unloads both, A's destructor then DT_FINI, then B's. */
    void *a = dlopen(P("libA.so"), RTLD_NOW);
    CHECK(1, a != NULL, "dlopen libA: %s", err());
    void *afn = dlsym(a, "a_fn");
    CHECK(2, walk(NULL).n == c0.n + 2, "A and B listed");
    evlog[0] = 0;
    CHECK(3, dlclose(a) == 0, "dlclose(A): %s", err());
    CHECK(4, strcmp(evlog, "dA fA dB fB ") == 0, "finalisers: %s", evlog);
    CHECK(5, !loaded("libA.so") && !loaded("libB.so"), "A or B still loaded");
    struct cnt c1 = walk(NULL);
    CHECK(6, c1.n == c0.n && c1.subs == c0.subs + 2 && c1.adds == c0.adds + 2, "counters n %d subs %llu adds %llu", c1.n, c1.subs, c1.adds);
    CHECK(7, afn && !mapped(afn), "A's text still mapped after dlclose");
    /* Invalid handles: -1 and a text, never a crash (D-27.32). */
    int local = 0;
    CHECK(8, dlclose(a) == -1 && strstr(err(), "invalid handle"), "double dlclose");
    CHECK(9, dlclose(NULL) == -1 && strstr(err(), "invalid handle"), "dlclose(NULL)");
    CHECK(10, dlclose(&local) == -1 && strstr(err(), "invalid handle"), "foreign handle");
    /* Counts: two opens need two closes. */
    a = dlopen(P("libA.so"), RTLD_NOW);
    CHECK(11, dlopen(P("libA.so"), RTLD_NOW) == a, "same handle");
    evlog[0] = 0;
    CHECK(12, dlclose(a) == 0 && loaded("libA.so") && evlog[0] == 0, "first of two closes unloaded: %s", evlog);
    CHECK(13, dlclose(a) == 0 && !loaded("libA.so") && strcmp(evlog, "dA fA dB fB ") == 0, "second close: %s", evlog);
    /* A shared dependency stays while another object needs it. */
    a = dlopen(P("libA.so"), RTLD_NOW);
    void *cc = dlopen(P("libC.so"), RTLD_NOW);
    evlog[0] = 0;
    CHECK(14, dlclose(a) == 0 && !loaded("libA.so") && loaded("libB.so") && strcmp(evlog, "dA fA ") == 0, "B with C open: %s", evlog);
    evlog[0] = 0;
    CHECK(15, dlclose(cc) == 0 && !loaded("libB.so") && strcmp(evlog, "dC fC dB fB ") == 0, "C then B: %s", evlog);
    /* C-3: a global definer bound by a later object stays until that object is closed. */
    void *g = dlopen(P("libG.so"), RTLD_NOW | RTLD_GLOBAL);
    void *u = dlopen(P("libU.so"), RTLD_NOW);
    CHECK(16, g && u, "G/U: %s", err());
    evlog[0] = 0;
    CHECK(17, dlclose(g) == 0 && loaded("libG.so") && evlog[0] == 0, "G unloaded under U: %s", evlog);
    int (*use_g)(void) = (int (*)(void))dlsym(u, "use_g");
    CHECK(18, use_g && use_g() == 42, "use_g");
    CHECK(19, dlclose(u) == 0 && !loaded("libU.so") && !loaded("libG.so") && strcmp(evlog, "dU fU dG fG ") == 0, "U then G: %s", evlog);
    /* RTLD_NODELETE: stays, no destructor. */
    void *n = dlopen(P("libN.so"), RTLD_NOW | RTLD_NODELETE);
    evlog[0] = 0;
    CHECK(20, dlclose(n) == 0 && loaded("libN.so") && evlog[0] == 0, "nodelete: %s", evlog);
    /* C-8: dlclose from a destructor. */
    void *x = dlopen(P("libX.so"), RTLD_NOW);
    CHECK(21, x && loaded("libY.so"), "X opened Y: %s", err());
    evlog[0] = 0;
    CHECK(22, dlclose(x) == 0 && !loaded("libX.so") && !loaded("libY.so"), "X/Y loaded after close");
    CHECK(23, strcmp(evlog, "dX1 dY fY dX2 fX ") == 0, "destructor's dlclose: %s", evlog);
    /* C-2, C-9: a success clears a pending text; the program's handle closes with 0. */
    dlopen(P("missing.so"), RTLD_NOW);
    void *self = dlopen(NULL, RTLD_NOW);
    dlopen(P("missing.so"), RTLD_NOW);
    CHECK(24, dlclose(self) == 0 && dlerror() == NULL, "program handle / pending text");
    /* TLS (C-4/C-5): the module ID comes back, the block is fresh; a freed place is reused. */
    void *t = dlopen(P("libT.so"), RTLD_NOW);
    CHECK(25, t != NULL, "libT: %s", err());
    size_t id1 = walk("libT.so").modid;
    int *(*ta)(void) = (int *(*)(void))dlsym(t, "t_addr");
    CHECK(26, id1 > 0 && *ta() == 7, "libT id %zu", id1);
    *ta() = 99;
    void *t2 = dlopen(P("libT2.so"), RTLD_NOW);
    size_t id2 = walk("libT2.so").modid;
    CHECK(27, t2 && id2 > 0 && id2 != id1, "libT2 id %zu", id2);
    CHECK(28, dlclose(t) == 0 && !loaded("libT.so"), "libT closed");
    void *t3 = dlopen(P("libT3.so"), RTLD_NOW);
    long *(*t3a)(void) = t3 ? (long *(*)(void))dlsym(t3, "t_addr") : NULL;
    CHECK(29, t3a && walk("libT3.so").modid == id1 && t3a()[0] == 21 && t3a()[15] == 0, "libT3 reuses id %zu: %zu", id1, walk("libT3.so").modid);
    long *(*t2a)(void) = (long *(*)(void))dlsym(t2, "t_addr");
    CHECK(30, t2a()[0] == 11 && t2a()[3] == 14, "libT2's block intact");
    CHECK(31, dlclose(t3) == 0 && dlclose(t2) == 0, "close T2/T3");
    t = dlopen(P("libT.so"), RTLD_NOW);
    ta = (int *(*)(void))dlsym(t, "t_addr");
    CHECK(32, walk("libT.so").modid == id1 && *ta() == 7, "re-open: id %zu value %d", walk("libT.so").modid, *ta());
    dlclose(t);
    /* A dl_iterate_phdr callback closes another object, then the object it is visiting, and reads
       the visited object's headers after: two unloads inside one walk, and the visited object stays
       mapped until the walk has left; a later call unmaps it. */
    void *r = dlopen(P("libR.so"), RTLD_NOW);
    void *rfn = dlsym(r, "r_fn");
    struct cnt c = {0};
    c.want = "libR.so";
    c.close_first = dlopen(P("libY.so"), RTLD_NOW);
    c.close = r;
    dl_iterate_phdr(cb, &c);
    CHECK(33, c.seen && c.close_first == NULL && c.close == NULL && c.sum > 0, "callback close");
    CHECK(34, !loaded("libR.so"), "R still loaded");
    dlopen(P("libR.so"), RTLD_NOW | RTLD_NOLOAD);
    CHECK(35, !mapped(rfn), "R never unmapped after the walk left");
    /* Slot and arena reuse: more opens than the 512-object table. */
    for (int i = 0; i < 600; i++) {
        void *p = dlopen(P("libP.so"), RTLD_NOW);
        CHECK(36, p != NULL, "open %d: %s", i, err());
        CHECK(37, dlclose(p) == 0, "close %d", i);
    }
    CHECK(38, pctor == 600 && pdtor == 600, "ctors %d dtors %d", pctor, pdtor);
    CHECK(39, walk(NULL).n == c0.n + 1, "listed at the end: %d (N stays)", walk(NULL).n);
}

static volatile int done;
static void *reader(void *arg) {
    long k = 0, budget = (long)arg;
    while (!done && k < budget) {
        struct cnt c = {0};
        c.want = "libP";
        dl_iterate_phdr(cb, &c);
        void *f = dlsym(RTLD_DEFAULT, "p_fn");
        if (f) { struct dl_find_object o; _dl_find_object(f, &o); }
        k++;
    }
    return NULL;
}

static void concurrent(void) {
    pthread_t th[3];
    for (int i = 0; i < 3; i++) CHECK(50, pthread_create(&th[i], NULL, reader, (void *)200000L) == 0, "pthread_create");
    for (int i = 0; i < 3000; i++) {
        evlog[0] = 0; /* A's and B's constructors and destructors log here (this thread only). */
        void *p = dlopen(P("libP.so"), RTLD_NOW | RTLD_GLOBAL);
        void *a = dlopen(P("libA.so"), RTLD_NOW);
        CHECK(51, p && a, "open %d: %s", i, err());
        CHECK(52, dlclose(p) == 0 && dlclose(a) == 0, "close %d", i);
    }
    done = 1;
    for (int i = 0; i < 3; i++) pthread_join(th[i], NULL);
    CHECK(53, !loaded("libP.so") && !loaded("libA.so"), "still loaded");
}

/* Ruling #5651: past a retired threshold (a quarter of the 512-object table) dlopen and dlclose
   wait at entry for the readers to drain, never from inside a read section or a constructor. */
static int fill_n = -1, fill_full, fill_close;
static int fill_cb(struct dl_phdr_info *i, size_t s, void *d) {
    (void)i; (void)s; (void)d;
    for (int k = 0; k < 600; k++) {
        void *p = dlopen(P("libP.so"), RTLD_NOW);
        if (!p) { fill_n = k; fill_full = strstr(err(), "cannot load more objects") != NULL; return 1; }
        if (dlclose(p) != 0) { fill_close = k + 1; return 1; }
    }
    fill_n = 600;
    return 1;
}
static sem_t in_cb, go;
static volatile int progress, released_at = -1;
static pid_t main_tid;
/* Stay inside the walk until the main thread is seen asleep (state S) with its progress frozen
   over 20 polls, or has finished: only a wait for this reader puts it to sleep. */
static int watch_cb(struct dl_phdr_info *i, size_t s, void *d) {
    (void)i; (void)s; (void)d;
    char path[64], buf[512];
    snprintf(path, sizeof path, "/proc/self/task/%d/stat", (int)main_tid);
    int last = -1, still = 0;
    sem_post(&in_cb);
    while (progress < 600) {
        int p = progress, fd = open(path, O_RDONLY);
        ssize_t n = fd >= 0 ? read(fd, buf, sizeof buf - 1) : -1;
        if (fd >= 0) close(fd);
        char st = 0;
        if (n > 0) { buf[n] = 0; char *e = strrchr(buf, ')'); if (e && e[1] && e[2]) st = e[2]; }
        if (p == last && st == 'S') { if (++still >= 20) { released_at = p; break; } } else still = 0;
        last = p;
        sched_yield();
    }
    return 1;
}
static void *watcher(void *a) { (void)a; dl_iterate_phdr(watch_cb, NULL); return NULL; }
static void *r_open;
static int k_pairs, k_fail;
/* libK's constructor (holding the loader's writer lock): lets the stalled reader call dlopen (it
   then blocks on that lock) and passes the threshold itself. */
void k_ctor(void) {
    sem_post(&go);
    for (int i = 0; i < 40; i++) {
        void *p = dlopen(P("libP.so"), RTLD_NOW);
        if (!p || dlclose(p) != 0) { k_fail = i + 1; return; }
        k_pairs++;
    }
}
static int stall_cb(struct dl_phdr_info *i, size_t s, void *d) {
    (void)i; (void)s; (void)d;
    char path[4200];
    snprintf(path, sizeof path, "%s/libR.so", dir);
    sem_post(&in_cb);
    while (sem_wait(&go) != 0) {}
    r_open = dlopen(path, RTLD_NOW);
    return 1;
}
static void *staller(void *a) { (void)a; dl_iterate_phdr(stall_cb, NULL); return NULL; }
static pid_t fork_pid = -1;
static int fork_cb(struct dl_phdr_info *i, size_t s, void *d) { (void)i; (void)s; (void)d; fork_pid = fork(); return 1; }

static void room(void) {
    /* A callback opening and closing: each close stays retired (its own thread is the reader), the
       threshold passes and its calls never wait on themselves; dlopen fails once the table is full. */
    dl_iterate_phdr(fill_cb, NULL);
    CHECK(60, fill_full && fill_n > 128 && fill_n < 512 && !fill_close, "callback opens: %d (full %d, close failed at %d)", fill_n, fill_full, fill_close);
    void *p = dlopen(P("libP.so"), RTLD_NOW);
    CHECK(61, p && dlclose(p) == 0, "open after the walk: %s", err());
    /* A reader stalled inside a walk: the main thread's opens wait for it past the threshold
       instead of filling the table. */
    sem_init(&in_cb, 0, 0);
    sem_init(&go, 0, 0);
    main_tid = (pid_t)syscall(SYS_gettid);
    pthread_t th;
    CHECK(62, pthread_create(&th, NULL, watcher, NULL) == 0, "pthread_create");
    while (sem_wait(&in_cb) != 0) {}
    for (int i = 0; i < 600; i++) {
        void *q = dlopen(P("libP.so"), RTLD_NOW);
        CHECK(63, q && dlclose(q) == 0, "open %d behind a stalled reader: %s", i, err());
        progress = i + 1;
    }
    pthread_join(th, NULL);
    CHECK(64, released_at >= 100 && released_at <= 200, "waited after %d opens", released_at);
    /* A constructor (holding the writer lock) passes the threshold behind a stalled reader whose
       callback calls dlopen, which blocks on that lock: the constructor's calls must not wait. */
    CHECK(65, pthread_create(&th, NULL, staller, NULL) == 0, "pthread_create");
    while (sem_wait(&in_cb) != 0) {}
    /* fork from inside a walk while the other thread is stalled in its own: the child, alone, must
       neither wait for the gone reader nor lose its own count; 600 pairs after its walk ends. */
    dl_iterate_phdr(fork_cb, NULL);
    if (fork_pid == 0) {
        alarm(120);
        for (int i = 0; i < 600; i++) {
            void *q = dlopen(P("libP.so"), RTLD_NOW);
            if (!q || dlclose(q) != 0) _exit(1);
        }
        _exit(0);
    }
    int st = -1;
    CHECK(69, fork_pid > 0 && waitpid(fork_pid, &st, 0) == fork_pid && WIFEXITED(st) && WEXITSTATUS(st) == 0, "fork child: status %#x", st);
    for (int i = 0; i < 100; i++) {
        void *q = dlopen(P("libP.so"), RTLD_NOW);
        CHECK(66, q && dlclose(q) == 0, "open %d: %s", i, err());
    }
    void *k = dlopen(P("libK.so"), RTLD_NOW);
    CHECK(67, k && k_pairs == 40 && !k_fail, "constructor's opens: %d done, failed at %d: %s", k_pairs, k_fail, err());
    pthread_join(th, NULL);
    CHECK(68, r_open && dlclose(r_open) == 0 && dlclose(k) == 0, "the reader's dlopen: %s", err());
}

int main(int argc, char **argv) {
    if (argc < 3) return 2;
    alarm(300);
    strncpy(dir, argv[1], sizeof dir - 1);
    if (!strcmp(argv[2], "basic")) basic();
    else if (!strcmp(argv[2], "concurrent")) concurrent();
    else if (!strcmp(argv[2], "room")) room();
    else return 2;
    return 0;
}
"#;

/// The C++ plugin: a static object whose destructor counts, an exception thrown and caught inside
/// the plugin, an `atexit` handler registered with its DSO handle (run at `dlclose`) and a fork
/// handler (dropped at `dlclose`: a later `fork` must not call into the unmapped object).
const PLUGIN: &str = r#"#include <stddef.h>
extern "C" { extern int cxx_ctors, cxx_dtors, cxx_atexit, cxx_caught, cxx_forks; extern void *__dso_handle;
int __cxa_atexit(void (*)(void *), void *, void *); int __register_atfork(void (*)(void), void (*)(void), void (*)(void), void *); }
struct S { int v; S() : v(1) { cxx_ctors++; } ~S() { cxx_dtors++; } };
static S s;
static void at(void *) { cxx_atexit++; }
static void child() { cxx_forks++; }
static int thrower(int v) { if (v) throw v; return 0; }
extern "C" int plugin_run(int v) {
    __cxa_atexit(at, NULL, &__dso_handle);
    __register_atfork(NULL, NULL, child, &__dso_handle);
    try { thrower(v); } catch (int e) { cxx_caught += e; }
    return s.v;
}
"#;

const CXX_PROG: &str = r#"#include <dlfcn.h>
#include <stdio.h>
#include <sys/wait.h>
#include <unistd.h>
extern "C" { int cxx_ctors, cxx_dtors, cxx_atexit, cxx_caught, cxx_forks; }
int main(int argc, char **argv) {
    if (argc < 2) return 2;
    alarm(300);
    char p[4200];
    snprintf(p, sizeof p, "%s/libplugincxx.so", argv[1]);
    for (int i = 0; i < 300; i++) {
        void *h = dlopen(p, RTLD_NOW);
        if (!h) { fprintf(stderr, "dlopen %d: %s\n", i, dlerror()); return 3; }
        int (*run)(int) = (int (*)(int))dlsym(h, "plugin_run");
        if (!run || run(1) != 1) { fprintf(stderr, "plugin_run %d\n", i); return 4; }
        if (dlclose(h) != 0) { fprintf(stderr, "dlclose %d: %s\n", i, dlerror()); return 5; }
    }
    if (cxx_ctors != 300 || cxx_dtors != 300 || cxx_atexit != 300 || cxx_caught != 300) {
        fprintf(stderr, "ctors %d dtors %d atexit %d caught %d\n", cxx_ctors, cxx_dtors, cxx_atexit, cxx_caught);
        return 6;
    }
    pid_t c = fork();
    if (c == 0) _exit(cxx_forks == 0 ? 0 : 1);
    int st = 0;
    if (c < 0 || waitpid(c, &st, 0) != c || !WIFEXITED(st) || WEXITSTATUS(st) != 0) { fprintf(stderr, "fork after dlclose: %d\n", st); return 7; }
    return 0;
}
"#;

/// Build the C corpus in `d`: the helpers and the program.
fn corpus(d: &Path, ld: &Path, libc: &Path, crt1: &Path) -> PathBuf {
    let dir = d.to_str().unwrap();
    let lib = |out: &str, kind: &str, extra: &[&str]| {
        let mut f = vec!["-nostdlib", "-fPIC", "-shared", "-Wl,-fini=fini_hook", kind];
        f.extend_from_slice(extra);
        build(d, out, LIB, &f);
    };
    let (ldir, rpath) = (format!("-L{dir}"), format!("-Wl,-rpath,{dir}"));
    lib("libB.so", "-DKIND_B", &[]);
    lib("libA.so", "-DKIND_A", &["-Wl,--no-as-needed", &ldir, &rpath, "-l:libB.so"]);
    lib("libC.so", "-DKIND_C", &["-Wl,--no-as-needed", &ldir, &rpath, "-l:libB.so"]);
    for (out, kind) in [("libG.so", "-DKIND_G"), ("libU.so", "-DKIND_U"), ("libN.so", "-DKIND_N"), ("libY.so", "-DKIND_Y"), ("libX.so", "-DKIND_X")] {
        lib(out, kind, &[]);
    }
    for (out, kind) in [("libT.so", "-DKIND_T"), ("libT2.so", "-DKIND_T2"), ("libT3.so", "-DKIND_T3"), ("libR.so", "-DKIND_R"), ("libP.so", "-DKIND_P"), ("libK.so", "-DKIND_K")] {
        build(d, out, LIB, &["-nostdlib", "-fPIC", "-shared", kind]);
    }
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

/// s27_1_c_4 (probe `== close` C-1..C-9, `== order`, `== phdr`): `dlclose` unloads an object and the
/// dependencies nothing else holds (destructor, then `DT_FINI`, the dependent first), moves
/// `dlpi_subs` and unmaps; two opens need two closes; a shared dependency stays while another object
/// holds it; a global definer bound by a later object stays until that object closes; `RTLD_NODELETE`
/// stays; a destructor may call `dlclose`; NULL, a closed and a foreign handle give -1 and a text
/// (D-27.32); a success clears a pending text; the program's handle closes with 0; a
/// TLS module's ID is reused with a fresh block and its surplus place freed; a `dl_iterate_phdr`
/// callback that closes the object it visits can still read its headers, and the object is unmapped
/// after the walk; 600 open/close pairs reuse the 512-object table.
#[test]
fn s27_1_c_4_dlclose_unloads_finalises_and_reclaims() {
    let Some((d, ld, libc, crt1)) = setup("basic") else { return };
    let prog = corpus(&d, &ld, &libc, &crt1);
    run(&d, &ld, &prog, "basic");
    std::fs::remove_dir_all(&d).ok();
}

/// s27_1_c_8, s27_1_c_4 (D-27.1, plan §4): three reader threads walk `dl_iterate_phdr` (reading every
/// visited object's headers), `dlsym` and `_dl_find_object` while the main thread opens and closes
/// two objects 3000 times (iteration budgets): no reader faults and nothing stays loaded.
#[test]
fn s27_1_c_4_dlclose_concurrent_with_readers() {
    let Some((d, ld, libc, crt1)) = setup("concurrent") else { return };
    let prog = corpus(&d, &ld, &libc, &crt1);
    run(&d, &ld, &prog, "concurrent");
    std::fs::remove_dir_all(&d).ok();
}

/// s27_1_c_4, s27_1_c_8 (ruling #5651, the D-27 dlclose record's amendment): past a quarter of the
/// table in retired objects, `dlopen`/`dlclose` wait at entry for the readers to drain. A
/// `dl_iterate_phdr` callback that opens and closes in a loop never waits on itself: its `dlopen`
/// fails with `cannot load more objects` once the table is full, and the next call outside the walk
/// reclaims everything. A reader stalled inside a walk makes the main thread's 600 open/close
/// pairs wait (seen asleep after 100..200 pairs) instead of filling the table. A constructor
/// (holding the writer lock) that passes the threshold while a stalled reader's callback is
/// blocked on that lock never waits (a deadlock otherwise; `alarm` is the hang guard). A child
/// forked from inside a walk while another thread is stalled in its own runs 600 pairs: the gone
/// thread's count is cleared and the child's own is kept until its walk ends.
#[test]
fn s27_1_c_4_dlopen_waits_for_readers_only_outside_them() {
    let Some((d, ld, libc, crt1)) = setup("room") else { return };
    let prog = corpus(&d, &ld, &libc, &crt1);
    run(&d, &ld, &prog, "room");
    std::fs::remove_dir_all(&d).ok();
}

/// s27_1_c_4 (plan/ph14/design.md §9; dynlink.md `__cxa_finalize`, P2): a C++ plugin with a static
/// object, an exception thrown and caught inside it, an `atexit` handler and a fork handler, loaded
/// and unloaded 300 times: every static destructor and `atexit` handler runs at its `dlclose`, every
/// exception is caught, and a later `fork` calls no dropped fork handler (DL-ATFORK-FINALIZE).
#[test]
fn s27_1_c_4_cxx_plugin_load_unload_loop() {
    let Some((d, ld, libc, crt1)) = setup("cxx") else { return };
    if !Command::new("c++").arg("--version").output().is_ok_and(|o| o.status.success()) {
        eprintln!("SKIP: no system C++ compiler");
        return;
    }
    let file = |f: &str| String::from_utf8(Command::new("c++").arg(format!("-print-file-name={f}")).output().unwrap().stdout).unwrap().trim().to_owned();
    let (begin, end) = (file("crtbeginS.o"), file("crtendS.o"));
    // libgcc_eh's unwinder symbols are hidden: the plugin carries its own copy (as `-static-libgcc`).
    build(&d, "libplugincxx.so", PLUGIN, &["-nostdlib", "-fPIC", "-shared", "-fexceptions", &begin, "-lgcc_eh", "-lgcc", &end]);
    let interp = format!("-Wl,--dynamic-linker={}", ld.display());
    // The program throws nothing itself: `-u` pulls the C++ exception runtime the plugin needs out of
    // the static archives, and `--export-dynamic` lets the plugin bind to it.
    let rt = "-Wl,-u,_ZTIi,-u,__cxa_allocate_exception,-u,__cxa_free_exception,-u,__cxa_throw,-u,__cxa_begin_catch,-u,__cxa_end_catch,-u,__gxx_personality_v0";
    let prog = build(&d, "prog_cxx", CXX_PROG, &["-nostdlib", "-fPIE", "-pie", "-Wl,--export-dynamic", "-Wl,--eh-frame-hdr", rt, &interp, crt1.to_str().unwrap(), &begin, libc.to_str().unwrap(), ld.to_str().unwrap(), "-Wl,-Bstatic", "-lsupc++", "-lgcc_eh", "-lgcc", "-Wl,-Bdynamic", libc.to_str().unwrap(), ld.to_str().unwrap(), &end]);
    use std::os::unix::process::ExitStatusExt;
    for (how, mut cmd) in [("kernel-started", Command::new(&prog)), ("ld.so PROGRAM", { let mut c = Command::new(&ld); c.arg(&prog); c })] {
        let o = cmd.arg(&d).output().unwrap();
        assert_eq!((o.status.code(), o.status.signal()), (Some(0), None), "{how}: C++ plugin loop; stderr: {}", String::from_utf8_lossy(&o.stderr));
    }
    std::fs::remove_dir_all(&d).ok();
}
