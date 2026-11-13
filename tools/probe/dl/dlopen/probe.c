/* probe.c - DL-16P black-box probes of dlopen/dlsym/dlerror/dlinfo (P-1..P-13 of
 * plan/ph14/dl-16-dlopen.md section 6).
 *   probe pN DIR    run probe N against the corpus run.sh built in DIR
 * Each case runs in its own forked child (alarm(30) in the child is a hang guard: a case it stops
 * prints "killed by signal 14 (hang guard)"). P-8 and P-9 also carry their own guard: a reader or a
 * fork that has not answered within 5 s is recorded as "blocked", then released, so nothing is left
 * waiting. Public interfaces only; output is written with write(2).
 */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <limits.h>
#include <link.h>
#include <poll.h>
#include <pthread.h>
#include <signal.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>

#define GUARD_MS 5000

static void pr(const char *fmt, ...)
{
    char b[2048];
    va_list ap;
    va_start(ap, fmt);
    int n = vsnprintf(b, sizeof b, fmt, ap);
    va_end(ap);
    if (n > 0)
        (void)!write(1, b, (size_t)n < sizeof b ? (size_t)n : sizeof b - 1);
}

static const char *D;
static const char *P(const char *f)
{
    static char ring[8][PATH_MAX];
    static int k;
    char *b = ring[k++ & 7];
    snprintf(b, PATH_MAX, "%s/%s", D, f);
    return b;
}
static const char *err(void)
{
    const char *e = dlerror();
    return e ? e : "(no dlerror text)";
}
static const char *lname(void *h)
{
    struct link_map *lm;
    if (!h)
        return "(null handle)";
    if (dlinfo(h, RTLD_DI_LINKMAP, &lm) != 0)
        return "(dlinfo failed)";
    return lm->l_name[0] ? lm->l_name : "\"\"";
}
static void show_open(const char *label, void *h)
{
    if (h)
        pr("   %s -> handle, l_name %s\n", label, lname(h));
    else
        pr("   %s -> NULL: %s\n", label, err());
}
static const char *cmp(void *a, void *b)
{
    if (!a)
        return "NULL";
    return a == b ? "same handle" : "different handle";
}

static void child(const char *label, void (*fn)(void))
{
    pid_t p = fork();
    if (p == 0) {
        alarm(30);
        pr("-- %s\n", label);
        fn();
        _exit(0);
    }
    int st;
    waitpid(p, &st, 0);
    if (WIFSIGNALED(st))
        pr("   killed by signal %d%s\n", WTERMSIG(st), WTERMSIG(st) == SIGALRM ? " (hang guard)" : "");
    else if (WEXITSTATUS(st) != 0)
        pr("   exit %d\n", WEXITSTATUS(st));
}

/* exported for P-5: the program's own who() */
const char *who(void) { return "main"; }

/* ---------------- P-1 mode bits ---------------- */
static int g_mode;
static void p1_now_lazy(void)
{
    void *h = dlopen(P("libU1.so"), RTLD_NOW | RTLD_LAZY);
    show_open("dlopen(libU1 [undefined function], RTLD_NOW|RTLD_LAZY)", h);
    h = dlopen(P("libU1.so"), RTLD_NOW);
    show_open("then dlopen(libU1, RTLD_NOW)", h);
}
static void p1_now_lazy_fresh_now(void)
{
    void *h = dlopen(P("libU1.so"), RTLD_NOW);
    show_open("dlopen(libU1 [undefined function], RTLD_NOW) alone", h);
}
static void p1_bit_now(void) { void *h = dlopen(P("libC1.so"), RTLD_NOW | g_mode); pr("   NOW|0x%x libC1: %s\n", (unsigned)g_mode, h ? "handle" : err()); }
static void p1_bit_lazy(void) { void *h = dlopen(P("libU1.so"), RTLD_LAZY | g_mode); pr("   LAZY|0x%x libU1: %s\n", (unsigned)g_mode, h ? "handle" : err()); }
static void p1_bit_alone(void) { void *h = dlopen(P("libC1.so"), g_mode); pr("   0x%x alone libC1: %s\n", (unsigned)g_mode, h ? "handle" : err()); }
static void p1_bad_missing(void)
{
    void *h = dlopen(P("no_such_file.so"), RTLD_NOW | g_mode);
    pr("   dlopen(missing file, NOW|0x%x): %s\n", (unsigned)g_mode, h ? "handle" : err());
}
static void p1(void)
{
    pr("== P-1 mode bits (libU1 references an undefined function; libC1 is plain)\n");
    child("NOW|LAZY on an object with an undefined function", p1_now_lazy);
    child("NOW on the same object (control)", p1_now_lazy_fresh_now);
    static const unsigned known = RTLD_LAZY | RTLD_NOW | RTLD_NOLOAD | RTLD_DEEPBIND | RTLD_GLOBAL | RTLD_NODELETE;
    /* Bit 0x40000000 is outside this sweep: Liella refuses it like every other unknown bit
     * (plan/ph14/dl-16-dlopen.md, Modes), and DL-16a's test sweeps every undefined bit. */
    static const unsigned unswept = 0x40000000u;
    pr("-- sweep: every bit outside RTLD_LAZY|NOW|NOLOAD|DEEPBIND|GLOBAL|NODELETE (0x%x) but 0x%x\n", known, unswept);
    for (int b = 0; b < 32; b++) {
        unsigned bit = 1u << b;
        if (bit & (known | unswept))
            continue;
        g_mode = (int)bit;
        pid_t p = fork();
        if (p == 0) {
            alarm(30);
            p1_bit_now();
            p1_bit_lazy();
            _exit(0);
        }
        int st;
        waitpid(p, &st, 0);
        if (WIFSIGNALED(st))
            pr("   bit 0x%x: killed by signal %d\n", bit, WTERMSIG(st));
    }
    g_mode = 0x10000;
    child("unknown bit without a binding mode (0x10000)", p1_bit_alone);
    g_mode = (int)0x80000000u;
    child("unknown bit without a binding mode (0x80000000)", p1_bit_alone);
    g_mode = 0x10000;
    child("refused bit on a file that does not exist (which check first)", p1_bad_missing);
}

/* ---------------- P-2 identity ---------------- */
static void p2_seq(void)
{
    void *h0 = dlopen("libId.so.1", RTLD_NOW | RTLD_NOLOAD);
    pr("   NOLOAD by DT_SONAME before any load -> %s\n", h0 ? "handle" : "NULL");
    void *h1 = dlopen(P("id/libId.so.1"), RTLD_NOW);
    show_open("by path id/libId.so.1", h1);
    void *h;
    h = dlopen(P("id/sym.so"), RTLD_NOW);
    pr("   symlink id/sym.so -> libId.so.1: %s, l_name %s\n", cmp(h, h1), lname(h));
    h = dlopen(P("id/hard.so"), RTLD_NOW);
    pr("   hard link id/hard.so: %s, l_name %s\n", cmp(h, h1), lname(h));
    h = dlopen(P("id/../id/libId.so.1"), RTLD_NOW);
    pr("   id/../id/libId.so.1: %s, l_name %s\n", cmp(h, h1), lname(h));
    if (chdir(P("id")) == 0) {
        h = dlopen("./libId.so.1", RTLD_NOW);
        pr("   ./libId.so.1 (cwd id): %s, l_name %s\n", cmp(h, h1), lname(h));
    }
    h = dlopen("libId.so.1", RTLD_NOW);
    pr("   by DT_SONAME libId.so.1 (no search path holds it): %s%s%s\n", cmp(h, h1), h ? "" : ": ", h ? "" : err());
    h = dlopen(P("id2/libId.so.1"), RTLD_NOW);
    pr("   a copy (other inode, same DT_SONAME) by path id2/libId.so.1: %s, l_name %s\n", cmp(h, h1), lname(h));
}
static void p2_nosoname(void)
{
    void *h = dlopen("libId.so.1", RTLD_NOW);
    pr("   by DT_SONAME before any load -> %s\n", h ? "handle" : err());
}
static void p2_hard_first(void)
{
    void *h1 = dlopen(P("id/hard.so"), RTLD_NOW);
    show_open("hard link first", h1);
    void *h = dlopen(P("id/libId.so.1"), RTLD_NOW);
    pr("   then the original path: %s, l_name %s\n", cmp(h, h1), lname(h));
    h = dlopen("hard.so", RTLD_NOW | RTLD_NOLOAD);
    pr("   NOLOAD by file name hard.so (no slash): %s\n", cmp(h, h1));
}
static void p2_copy_first(void)
{
    void *h1 = dlopen(P("id2/libId.so.1"), RTLD_NOW);
    show_open("the copy id2/libId.so.1 first", h1);
    void *h = dlopen("libId.so.1", RTLD_NOW);
    pr("   then by DT_SONAME: %s\n", cmp(h, h1));
    h = dlopen(P("id/libId.so.1"), RTLD_NOW);
    pr("   then the original by path: %s, l_name %s\n", cmp(h, h1), lname(h));
}
static void p2_needed(void)
{
    void *h1 = dlopen(P("id/libId.so.1"), RTLD_NOW);
    show_open("by path id/libId.so.1", h1);
    void *h = dlopen(P("libNeedId.so"), RTLD_NOW);
    show_open("libNeedId.so (DT_NEEDED libId.so.1, DT_RUNPATH id2 [a copy])", h);
    void *(*f)(void) = h ? (void *(*)(void))dlsym(h, "use_Id") : NULL;
    pr("   libNeedId's use_Id answers through %s\n", f ? "a definition" : "nothing");
    Dl_info di;
    void *s = h ? dlsym(h, "fn_Id") : NULL;
    if (s && dladdr(s, &di))
        pr("   dlsym(libNeedId, fn_Id) is in %s\n", di.dli_fname);
}
static void p2(void)
{
    pr("== P-2 identity (id/libId.so.1 has DT_SONAME libId.so.1; id/sym.so symlink, id/hard.so hard link, id2/libId.so.1 a copy)\n");
    child("by DT_SONAME, nothing loaded", p2_nosoname);
    child("original path first, then each other name", p2_seq);
    child("hard link first", p2_hard_first);
    child("copy first", p2_copy_first);
    child("DT_NEEDED by DT_SONAME while the original is loaded", p2_needed);
}

/* ---------------- P-3 RTLD_GLOBAL ---------------- */
static void p3_defaults(const char *when)
{
    void *a = dlsym(RTLD_DEFAULT, "fn_A3");
    void *b = dlsym(RTLD_DEFAULT, "fn_B3");
    pr("   %s: RTLD_DEFAULT fn_A3 %s, fn_B3 %s\n", when, a ? "found" : "NULL", b ? "found" : "NULL");
    (void)dlerror();
}
static void p3_z(void)
{
    void *z = dlopen(P("libZ3.so"), RTLD_NOW);
    pr("   dlopen(libZ3 [undefined refs to fn_A3, fn_B3], NOW): %s\n", z ? "handle" : err());
}
static void p3_global(void)
{
    void *h = dlopen(P("libA3.so"), RTLD_NOW | RTLD_GLOBAL);
    show_open("dlopen(libA3 [needs libB3], NOW|GLOBAL)", h);
    p3_defaults("after");
    p3_z();
}
static void p3_promote(void)
{
    void *h = dlopen(P("libA3.so"), RTLD_NOW);
    show_open("dlopen(libA3, NOW|LOCAL)", h);
    p3_defaults("after the local open");
    p3_z();
    void *g = dlopen(P("libA3.so"), RTLD_NOW | RTLD_NOLOAD | RTLD_GLOBAL);
    pr("   dlopen(libA3, NOW|NOLOAD|GLOBAL): %s\n", cmp(g, h));
    p3_defaults("after the NOLOAD|GLOBAL re-open");
    p3_z();
}
static void p3_promote_plain(void)
{
    void *h = dlopen(P("libA3.so"), RTLD_LAZY);
    show_open("dlopen(libA3, LAZY|LOCAL)", h);
    void *g = dlopen(P("libA3.so"), RTLD_NOW | RTLD_GLOBAL);
    pr("   dlopen(libA3, NOW|GLOBAL) without NOLOAD: %s\n", cmp(g, h));
    p3_defaults("after");
    p3_z();
}
static void p3_promote_dep(void)
{
    void *h = dlopen(P("libA3.so"), RTLD_NOW);
    show_open("dlopen(libA3, NOW|LOCAL)", h);
    void *b = dlopen(P("libB3.so"), RTLD_NOW | RTLD_NOLOAD | RTLD_GLOBAL);
    pr("   dlopen(libB3 [the dependency], NOW|NOLOAD|GLOBAL): %s\n", b ? "handle" : "NULL");
    p3_defaults("after");
    p3_z();
}
static void p3(void)
{
    pr("== P-3 RTLD_GLOBAL (libA3 needs libB3; libZ3 references fn_A3 and fn_B3 without DT_NEEDED)\n");
    child("GLOBAL open: root and dependency in the global scope?", p3_global);
    child("LOCAL open, then NOLOAD|GLOBAL re-open of the root", p3_promote);
    child("LAZY|LOCAL open, then NOW|GLOBAL re-open (no NOLOAD)", p3_promote_plain);
    child("LOCAL open, then NOLOAD|GLOBAL of the dependency only", p3_promote_dep);
}

/* ---------------- P-4 sibling dependencies ---------------- */
static void p4_mode(int mode, const char *m)
{
    void *h = dlopen(P("libA4.so"), mode);
    pr("   dlopen(libA4, %s): %s\n", m, h ? "handle" : err());
    if (!h)
        return;
    const char *(*c)(void) = (const char *(*)(void))dlsym(h, "use_C4");
    const char *(*a)(void) = (const char *(*)(void))dlsym(h, "use_A4");
    pr("   libB4's use_C4() -> %s, use_A4() -> %s\n", c ? c() : "(no use_C4)", a ? a() : "(no use_A4)");
}
static void p4_now(void) { p4_mode(RTLD_NOW, "RTLD_NOW"); }
static void p4_lazy(void) { p4_mode(RTLD_LAZY, "RTLD_LAZY"); }
static void p4(void)
{
    pr("== P-4 libA4 needs libB4 then libC4; libB4 references fn_C4 (its sibling) and fn_A4 (the root), linked with neither\n");
    child("RTLD_NOW", p4_now);
    child("RTLD_LAZY", p4_lazy);
}

/* ---------------- P-5 RTLD_NEXT / RTLD_DEFAULT from a dlopen'd object ---------------- */
static void p5_ask(void *(*f)(const char *), const char *how, const char *name)
{
    void *s = f(name);
    if (!s) {
        pr("   %s(%s) -> NULL: %s\n", how, name, err());
        return;
    }
    if (!strcmp(name, "who") || !strncmp(name, "fn_", 3))
        pr("   %s(%s) -> %s's\n", how, name, ((const char *(*)(void))s)());
    else
        pr("   %s(%s) -> %s\n", how, name, s == dlsym(RTLD_DEFAULT, name) ? "the RTLD_DEFAULT definition" : "another definition");
}
static void *prog_next(const char *n) { return dlsym(RTLD_NEXT, n); }
static void p5_run(const char *lib, int mode)
{
    void *h = dlopen(P(lib), mode);
    show_open(lib, h);
    if (!h)
        return;
    void *(*nx)(const char *) = (void *(*)(const char *))dlsym(h, "next_of");
    void *(*df)(const char *) = (void *(*)(const char *))dlsym(h, "default_of");
    static const char *names[] = {"who", "fn_M5", "fn_L5", "fn_G5", "puts", "no_such_symbol_xyz"};
    for (unsigned i = 0; i < sizeof names / sizeof *names; i++)
        p5_ask(nx, "RTLD_NEXT from the object", names[i]);
    for (unsigned i = 0; i < sizeof names / sizeof *names; i++)
        p5_ask(df, "RTLD_DEFAULT from the object", names[i]);
    p5_ask(prog_next, "RTLD_NEXT from the program", "who");
}
static void p5_local(void) { p5_run("libL5.so", RTLD_NOW); }
static void p5_last(void) { p5_run("libG5.so", RTLD_NOW | RTLD_GLOBAL); }
static void p5_lastdep(void) { p5_run("libL5.so", RTLD_NOW | RTLD_GLOBAL); }
static void p5(void)
{
    pr("== P-5 RTLD_NEXT/RTLD_DEFAULT from dlopen'd objects (who() in the program, libL5, libM5 and libG5; libL5 needs libM5)\n");
    child("libL5 RTLD_LOCAL", p5_local);
    child("libG5 RTLD_GLOBAL: the last object of the global scope", p5_last);
    child("libL5 RTLD_GLOBAL (its dependency libM5 after it)", p5_lastdep);
}

/* ---------------- P-6 dlerror ---------------- */
static void p6_fail(void) { (void)dlopen(P("no_such_file.so"), RTLD_NOW); }
static void p6_a(void)
{
    p6_fail();
    void *h = dlopen(P("libC1.so"), RTLD_NOW);
    pr("   failed dlopen, then a successful dlopen (%s), then dlerror(): %s\n", h ? "handle" : "NULL", err());
}
static void p6_b(void)
{
    p6_fail();
    void *s = dlsym(RTLD_DEFAULT, "puts");
    pr("   failed dlopen, then a successful dlsym (%s), then dlerror(): %s\n", s ? "found" : "NULL", err());
}
static void p6_b2(void)
{
    p6_fail();
    void *h = dlopen(NULL, RTLD_NOW);
    struct link_map *lm;
    int r = dlinfo(h, RTLD_DI_LINKMAP, &lm);
    pr("   failed dlopen, then a successful dlinfo (%d), then dlerror(): %s\n", r, err());
}
static void p6_b3(void)
{
    p6_fail();
    void *s = dlsym(RTLD_DEFAULT, "no_such_symbol_xyz");
    pr("   failed dlopen, then a failed dlsym (%s), then dlerror(): %s\n", s ? "found" : "NULL", err());
    pr("   and again: %s\n", err());
}
static int p6_pipe_go[2], p6_pipe_done[2];
static void *p6_thread_a(void *x)
{
    (void)x;
    p6_fail();
    char c = 'x';
    (void)!write(p6_pipe_done[1], &c, 1);
    (void)!read(p6_pipe_go[0], &c, 1);
    pr("   thread A, after the others looked: dlerror(): %s\n", err());
    return NULL;
}
static void *p6_thread_b(void *x)
{
    (void)x;
    pr("   thread B (created after A's failure): dlerror(): %s\n", err());
    return NULL;
}
static void p6_c(void)
{
    if (pipe(p6_pipe_go) || pipe(p6_pipe_done))
        return;
    pthread_t a, b;
    pthread_create(&a, NULL, p6_thread_a, NULL);
    char c;
    (void)!read(p6_pipe_done[0], &c, 1);
    pr("   main thread, after A's failed dlopen: dlerror(): %s\n", err());
    pthread_create(&b, NULL, p6_thread_b, NULL);
    pthread_join(b, NULL);
    (void)!write(p6_pipe_go[1], &c, 1);
    pthread_join(a, NULL);
}
static void *p6_thread_exit(void *x)
{
    (void)x;
    p6_fail();
    return NULL;
}
static void p6_d(void)
{
    pthread_t a;
    pthread_create(&a, NULL, p6_thread_exit, NULL);
    pthread_join(a, NULL);
    pr("   main thread, after a thread failed and exited: dlerror(): %s\n", err());
    p6_fail();
    pthread_create(&a, NULL, p6_thread_b, NULL);
    pthread_join(a, NULL);
    pr("   main thread, its own failure kept across the other thread: dlerror(): %s\n", err());
}
static void p6(void)
{
    pr("== P-6 dlerror\n");
    child("failure, success (dlopen), dlerror", p6_a);
    child("failure, success (dlsym), dlerror", p6_b);
    child("failure, success (dlinfo), dlerror", p6_b2);
    child("failure, second failure (dlsym), dlerror twice", p6_b3);
    child("failure in thread A, dlerror in main and in thread B", p6_c);
    child("failure in an exited thread; failure in main seen from a new thread", p6_d);
}

/* ---------------- P-7 constructor dlopens an object that needs it ---------------- */
static void p7_show(void *h)
{
    void **b = (void **)dlsym(h, "p7_b_handle");
    void **n = (void **)dlsym(h, "p7_self_noload");
    void **o = (void **)dlsym(h, "p7_self_open");
    if (b && *b)
        pr("   inner handle of libB7 vs dlopen(libB7, NOLOAD) now: %s\n", cmp(*b, dlopen(P("libB7.so"), RTLD_NOW | RTLD_NOLOAD)));
    if (n && *n)
        pr("   inner NOLOAD handle of self vs the outer handle: %s\n", cmp(*n, h));
    if (o && *o)
        pr("   inner plain handle of self vs the outer handle: %s\n", cmp(*o, h));
}
static void p7_b(void)
{
    setenv("P7_B", P("libB7.so"), 1);
    void *h = dlopen(P("libA7.so"), RTLD_NOW);
    show_open("outer dlopen(libA7) [its constructor dlopens libB7, which needs libA7]", h);
    if (h)
        p7_show(h);
}
static void p7_self(void)
{
    setenv("P7_SELF", P("libA7.so"), 1);
    void *h = dlopen(P("libA7.so"), RTLD_NOW);
    show_open("outer dlopen(libA7) [its constructor dlopens itself]", h);
    if (h)
        p7_show(h);
}
static void p7(void)
{
    pr("== P-7 a constructor of libA7 dlopens an object that needs libA7 (libB7: DT_NEEDED libA7.so, its constructor calls fn_A7)\n");
    child("constructor dlopens libB7", p7_b);
    child("constructor dlopens its own object", p7_self);
}

/* ---------------- P-8 readers in another thread during a slow constructor ---------------- */
static int p8_kind;
static int p8_started[2], p8_release[2], p8_done[2];
static int p8_cb(struct dl_phdr_info *i, size_t sz, void *data)
{
    (void)sz;
    if (strstr(i->dlpi_name, "libA8.so"))
        *(int *)data = 1;
    return 0;
}
static void *p8_reader(void *x)
{
    (void)x;
    char r[160];
    if (p8_kind == 0) {
        void *s = dlsym(RTLD_DEFAULT, "puts");
        snprintf(r, sizeof r, "dlsym(RTLD_DEFAULT, puts) %s", s ? "found" : "NULL");
    } else if (p8_kind == 1) {
        int seen = 0;
        dl_iterate_phdr(p8_cb, &seen);
        snprintf(r, sizeof r, "dl_iterate_phdr returned; libA8 %s", seen ? "listed" : "not listed");
    } else if (p8_kind == 2) {
        void *h = dlopen("libc.so.6", RTLD_NOW | RTLD_NOLOAD);
        snprintf(r, sizeof r, "dlopen(libc.so.6, NOLOAD) %s", h ? "handle" : "NULL");
    } else if (p8_kind == 3) {
        void *h = dlopen(P("libC1.so"), RTLD_NOW);
        snprintf(r, sizeof r, "dlopen(libC1) %s", h ? "handle" : "NULL");
    } else {
        void *s = dlsym(RTLD_DEFAULT, "fn_A8");
        snprintf(r, sizeof r, "dlsym(RTLD_DEFAULT, fn_A8 [the object being initialised, GLOBAL]) %s", s ? "found" : "NULL");
    }
    (void)!write(p8_done[1], r, strlen(r) + 1);
    return NULL;
}
static void *p8_watch(void *x)
{
    (void)x;
    char c, r[160];
    (void)!read(p8_started[0], &c, 1);
    pthread_t u;
    pthread_create(&u, NULL, p8_reader, NULL);
    struct pollfd pf = {p8_done[0], POLLIN, 0};
    if (poll(&pf, 1, GUARD_MS) == 1) {
        ssize_t n = read(p8_done[0], r, sizeof r);
        pr("   thread B: %s, during the constructor\n", n > 0 ? r : "?");
        (void)!write(p8_release[1], &c, 1);
    } else {
        pr("   thread B: blocked (no answer within the guard while the constructor waits)\n");
        (void)!write(p8_release[1], &c, 1);
        ssize_t n = read(p8_done[0], r, sizeof r);
        pr("   thread B: %s, after the constructor was released\n", n > 0 ? r : "?");
    }
    pthread_join(u, NULL);
    return NULL;
}
static void p8_run(void)
{
    if (pipe(p8_started) || pipe(p8_release) || pipe(p8_done))
        return;
    char b[16];
    snprintf(b, sizeof b, "%d", p8_started[1]);
    setenv("WAIT_STARTED", b, 1);
    snprintf(b, sizeof b, "%d", p8_release[0]);
    setenv("WAIT_RELEASE", b, 1);
    pthread_t t;
    pthread_create(&t, NULL, p8_watch, NULL);
    void *h = dlopen(P("libA8.so"), RTLD_NOW | (p8_kind == 4 ? RTLD_GLOBAL : 0));
    pr("   main: dlopen(libA8) returned %s\n", h ? "a handle" : err());
    pthread_join(t, NULL);
}
static void p8(void)
{
    pr("== P-8 thread B calls a dl function while thread A's dlopen runs a constructor that waits for B\n");
    static const char *what[] = {"dlsym(RTLD_DEFAULT, puts)", "dl_iterate_phdr", "dlopen(libc.so.6, NOLOAD)", "dlopen(libC1) [a new object]", "dlsym(RTLD_DEFAULT) of the object being initialised (GLOBAL)"};
    for (p8_kind = 0; p8_kind < 5; p8_kind++)
        child(what[p8_kind], p8_run);
}

/* ---------------- P-9 fork during another thread's dlopen ---------------- */
static int p9_started[2], p9_release[2], p9_forked[2], p9_first;
static int p9_cb(struct dl_phdr_info *i, size_t sz, void *data)
{
    (void)sz;
    int *d = data;
    d[0]++;
    if (strstr(i->dlpi_name, "libA9.so"))
        d[1] = 1;
    return 0;
}
static void *p9_loader(void *x)
{
    (void)x;
    void *h = dlopen(P("libA9.so"), RTLD_NOW);
    pr("   thread T: dlopen(libA9) returned %s\n", h ? "a handle" : "NULL");
    return NULL;
}
static void *p9_forkguard(void *x)
{
    (void)x;
    struct pollfd pf = {p9_forked[0], POLLIN, 0};
    if (poll(&pf, 1, GUARD_MS) != 1) {
        pr("   fork: blocked while the constructor waits (guard released the constructor)\n");
        char c = 'x';
        (void)!write(p9_release[1], &c, 1);
    }
    return NULL;
}
static void p9_child_step(int fd, const char *s) { (void)!write(fd, s, strlen(s)); (void)!write(fd, "\n", 1); }
static void p9_run(void)
{
    if (pipe(p9_started) || pipe(p9_release) || pipe(p9_forked))
        return;
    char b[16];
    snprintf(b, sizeof b, "%d", p9_started[1]);
    setenv("WAIT_STARTED", b, 1);
    snprintf(b, sizeof b, "%d", p9_release[0]);
    setenv("WAIT_RELEASE", b, 1);
    pthread_t t, g;
    pthread_create(&t, NULL, p9_loader, NULL);
    char c;
    (void)!read(p9_started[0], &c, 1);
    pthread_create(&g, NULL, p9_forkguard, NULL);
    int res[2];
    if (pipe(res))
        return;
    pid_t p = fork();
    if (p == 0) {
        close(res[0]);
        char r[256];
        int d[2] = {0, 0};
        p9_child_step(res[1], "child: started");
        if (p9_first <= 0) {
            dl_iterate_phdr(p9_cb, d);
            snprintf(r, sizeof r, "child: dl_iterate_phdr returned, %d objects, libA9 %s", d[0], d[1] ? "listed" : "not listed");
            p9_child_step(res[1], r);
        }
        if (p9_first <= 1) {
            void *s = dlsym(RTLD_DEFAULT, "puts");
            snprintf(r, sizeof r, "child: dlsym(RTLD_DEFAULT, puts) %s", s ? "found" : "NULL");
            p9_child_step(res[1], r);
        }
        if (p9_first <= 2) {
            void *h = dlopen(P("libA9.so"), RTLD_NOW | RTLD_NOLOAD);
            snprintf(r, sizeof r, "child: dlopen(libA9, NOLOAD) %s", h ? "handle" : "NULL");
            p9_child_step(res[1], r);
        }
        void *h = dlopen(P("libC1.so"), RTLD_NOW);
        snprintf(r, sizeof r, "child: dlopen(libC1) %s", h ? "handle" : dlerror());
        p9_child_step(res[1], r);
        p9_child_step(res[1], "child: done");
        _exit(0);
    }
    (void)!write(p9_forked[1], "x", 1);
    pthread_join(g, NULL);
    pr("   parent: fork returned\n");
    close(res[1]);
    char buf[4096];
    size_t len = 0;
    int done = 0;
    struct pollfd pf = {res[0], POLLIN, 0};
    while (!done) {
        if (poll(&pf, 1, GUARD_MS) != 1) {
            pr("   child: blocked (no further step within the guard); killed\n");
            kill(p, SIGKILL);
            break;
        }
        ssize_t n = read(res[0], buf + len, sizeof buf - 1 - len);
        if (n <= 0)
            break;
        len += (size_t)n;
        buf[len] = 0;
        char *nl;
        while ((nl = strchr(buf, '\n'))) {
            *nl = 0;
            pr("   %s\n", buf);
            if (!strcmp(buf, "child: done"))
                done = 1;
            size_t used = (size_t)(nl + 1 - buf);
            memmove(buf, nl + 1, len - used + 1);
            len -= used;
        }
    }
    close(res[0]);
    int st;
    waitpid(p, &st, 0);
    pr("   child status: %s %d\n", WIFSIGNALED(st) ? "signal" : "exit", WIFSIGNALED(st) ? WTERMSIG(st) : WEXITSTATUS(st));
    (void)!write(p9_release[1], "x", 1);
    pthread_join(t, NULL);
}
static void p9(void)
{
    pr("== P-9 fork while thread T is inside libA9's constructor (dlopen in progress); the child uses the loader\n");
    p9_first = 0;
    child("fork during a constructor; child: dl_iterate_phdr, dlsym, dlopen NOLOAD, dlopen", p9_run);
    p9_first = 1;
    child("fork during a constructor; child: dlsym first", p9_run);
    p9_first = 3;
    child("fork during a constructor; child: dlopen of a new object first", p9_run);
}

/* ---------------- P-10 IFUNC resolvers when the same dlopen fails ---------------- */
static int p10_mode;
static const char *p10_root;
static void p10_run(void)
{
    void *h = dlopen(P(p10_root), p10_mode);
    pr("   dlopen(%s, %s): %s\n", p10_root, p10_mode == RTLD_NOW ? "NOW" : "LAZY", h ? "handle" : err());
}
static void p10(void)
{
    pr("== P-10 libI10 defines the IFUNC ifn (resolver prints); libU10 references an undefined function\n");
    static const char *roots[] = {"libR10a.so", "libR10b.so", "libR10c.so"};
    static const char *what[] = {"libR10a needs libI10 then libU10; calls ifn", "libR10b needs libU10 then libI10; calls ifn",
                                 "libR10c needs libU10 only; defines and calls its own IFUNC"};
    for (int i = 0; i < 3; i++) {
        p10_root = roots[i];
        p10_mode = RTLD_NOW;
        char l[200];
        snprintf(l, sizeof l, "%s, RTLD_NOW", what[i]);
        child(l, p10_run);
        p10_mode = RTLD_LAZY;
        snprintf(l, sizeof l, "%s, RTLD_LAZY", what[i]);
        child(l, p10_run);
    }
}

/* ---------------- P-11 surplus TLS initial values in threads ---------------- */
static pthread_barrier_t p11_bar;
static void *p11_h;
static void p11_show(const char *who_)
{
    int *(*ie)(void) = (int *(*)(void))dlsym(p11_h, "addr_ie");
    int *(*gd)(void) = (int *(*)(void))dlsym(p11_h, "addr_gd");
    int *(*bs)(void) = (int *(*)(void))dlsym(p11_h, "addr_bss");
    if (!ie || !gd || !bs) {
        pr("   %s: getters missing\n", who_);
        return;
    }
    int *s = (int *)dlsym(p11_h, "tv_ie");
    pr("   %s: tv_ie=%d tv_gd=%d tv_bss=%d; dlsym(tv_ie) %s this thread's &tv_ie\n", who_, *ie(), *gd(), *bs(),
       s == ie() ? "is" : "is not");
}
static void *p11_before(void *x)
{
    (void)x;
    pthread_barrier_wait(&p11_bar);
    pthread_barrier_wait(&p11_bar);
    p11_show("thread created before the dlopen");
    int *(*ie)(void) = (int *(*)(void))dlsym(p11_h, "addr_ie");
    *ie() = 1042;
    pthread_barrier_wait(&p11_bar);
    pthread_barrier_wait(&p11_bar);
    return NULL;
}
static void *p11_after(void *x)
{
    (void)x;
    p11_show("thread created after the dlopen");
    return NULL;
}
static void p11_run(void)
{
    pthread_barrier_init(&p11_bar, NULL, 2);
    pthread_t t, u;
    pthread_create(&t, NULL, p11_before, NULL);
    pthread_barrier_wait(&p11_bar);
    p11_h = dlopen(P("libT11.so"), RTLD_NOW);
    show_open("dlopen(libT11)", p11_h);
    pthread_barrier_wait(&p11_bar);
    pthread_barrier_wait(&p11_bar);
    p11_show("main thread after the other thread set its tv_ie to 1042");
    pthread_barrier_wait(&p11_bar);
    pthread_join(t, NULL);
    pthread_create(&u, NULL, p11_after, NULL);
    pthread_join(u, NULL);
}
static void p11(void)
{
    pr("== P-11 libT11: tv_ie (initial-exec, 42), tv_gd (global-dynamic, 7), tv_bss (0)\n");
    child("threads before and after the dlopen", p11_run);
}

/* ---------------- P-12 dlinfo(RTLD_DI_LINKMAP) and l_name ---------------- */
static void p12_one(const char *label, void *h)
{
    struct link_map *lm = NULL;
    if (!h) {
        pr("   %s: NULL: %s\n", label, err());
        return;
    }
    int r = dlinfo(h, RTLD_DI_LINKMAP, &lm);
    pr("   %s: dlinfo %d, link map %s the handle, l_name %s%s%s\n", label, r, (void *)lm == h ? "==" : "!=", lm ? "\"" : "",
       lm ? lm->l_name : "-", lm ? "\"" : "");
}
static void p12_run(void)
{
    void *h0 = dlopen(NULL, RTLD_NOW);
    p12_one("dlopen(NULL)", h0);
    p12_one("dlopen(\"\")", dlopen("", RTLD_NOW));
    p12_one("dlopen(libC1 by path)", dlopen(P("libC1.so"), RTLD_NOW));
    if (chdir(D) == 0)
        p12_one("dlopen(./libC1b.so, relative)", dlopen("./libC1b.so", RTLD_NOW));
    p12_one("dlopen(libc.so.6)", dlopen("libc.so.6", RTLD_NOW));
    struct link_map *lm;
    if (dlinfo(h0, RTLD_DI_LINKMAP, &lm) == 0) {
        pr("   program link map: l_prev %s; chain by l_next:", lm->l_prev ? "non-NULL" : "NULL");
        for (struct link_map *m = lm; m; m = m->l_next)
            pr(" \"%s\"", m->l_name);
        pr("\n");
    }
}
static void p12(void)
{
    pr("== P-12 dlinfo(RTLD_DI_LINKMAP) and l_name\n");
    child("link maps", p12_run);
}

/* ---------------- P-13 dlopen of the program, ld.so, libc ---------------- */
static struct link_map *p13_find(const char *suffix)
{
    struct link_map *lm;
    if (dlinfo(dlopen(NULL, RTLD_NOW), RTLD_DI_LINKMAP, &lm) != 0)
        return NULL;
    for (; lm; lm = lm->l_next) {
        size_t a = strlen(lm->l_name), b = strlen(suffix);
        if (a >= b && !strcmp(lm->l_name + a - b, suffix))
            return lm;
    }
    return NULL;
}
static void p13_one(const char *label, const char *name, int mode, void *start)
{
    void *h = dlopen(name, mode);
    if (!h)
        pr("   %s: NULL: %s\n", label, err());
    else
        pr("   %s: %s the start-time one, l_name \"%s\"\n", label, h == start ? "same handle as" : "a different handle from",
           ((struct link_map *)h)->l_name);
}
static char p13_exe[PATH_MAX];
static void p13_run(void)
{
    void *prog = dlopen(NULL, RTLD_NOW);
    void *ld = p13_find("ld-linux-x86-64.so.2");
    void *libc = p13_find("libc.so.6");
    pr("   start-time l_name: ld.so \"%s\", libc \"%s\"\n", ld ? ((struct link_map *)ld)->l_name : "?",
       libc ? ((struct link_map *)libc)->l_name : "?");
    char ldreal[PATH_MAX], libcreal[PATH_MAX];
    if (!realpath("/lib64/ld-linux-x86-64.so.2", ldreal))
        strcpy(ldreal, "?");
    if (!realpath(libc ? ((struct link_map *)libc)->l_name : "/", libcreal))
        strcpy(libcreal, "?");
    p13_one("program by its real path", p13_exe, RTLD_NOW, prog);
    p13_one("program by its real path, NOLOAD", p13_exe, RTLD_NOW | RTLD_NOLOAD, prog);
    p13_one("program as /proc/self/exe", "/proc/self/exe", RTLD_NOW, prog);
    p13_one("ld.so by name ld-linux-x86-64.so.2", "ld-linux-x86-64.so.2", RTLD_NOW, ld);
    p13_one("ld.so by /lib64/ld-linux-x86-64.so.2", "/lib64/ld-linux-x86-64.so.2", RTLD_NOW, ld);
    p13_one("ld.so by its real path", ldreal, RTLD_NOW, ld);
    p13_one("libc by name libc.so.6", "libc.so.6", RTLD_NOW, libc);
    p13_one("libc by name libc.so.6, NOLOAD", "libc.so.6", RTLD_NOW | RTLD_NOLOAD, libc);
    p13_one("libc by its real path", libcreal, RTLD_NOW, libc);
    p13_one("libc as /lib/x86_64-linux-gnu/../x86_64-linux-gnu/libc.so.6", "/lib/x86_64-linux-gnu/../x86_64-linux-gnu/libc.so.6",
            RTLD_NOW, libc);
}
static void p13(void)
{
    pr("== P-13 dlopen of the program, of ld.so, of libc.so.6\n");
    child("program, ld.so, libc", p13_run);
}

int main(int argc, char **argv)
{
    if (argc < 3)
        return 2;
    D = argv[2];
    if (!realpath("/proc/self/exe", p13_exe))
        strcpy(p13_exe, argv[0]);
    static const struct {
        const char *n;
        void (*f)(void);
    } t[] = {{"p1", p1}, {"p2", p2}, {"p3", p3}, {"p4", p4}, {"p5", p5}, {"p6", p6}, {"p7", p7},
             {"p8", p8}, {"p9", p9}, {"p10", p10}, {"p11", p11}, {"p12", p12}, {"p13", p13}};
    for (unsigned i = 0; i < sizeof t / sizeof *t; i++)
        if (!strcmp(argv[1], t[i].n)) {
            t[i].f();
            return 0;
        }
    return 2;
}
