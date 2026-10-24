/* dlprobe.c - black-box probes of the dynamic loader's observable behaviour (DL-0b).
 *
 * Modes (argv[1]); DIR (argv[2]) is the corpus directory built by run.sh:
 *   errors DIR   dlerror text and return value for each failure (each case in a forked child)
 *   phdr DIR     dl_iterate_phdr object count, dlpi_adds, dlpi_subs across dlopen/dlclose
 *   tls DIR      static-TLS surplus: the largest initial-exec TLS block dlopen accepts, then how
 *                many 128-byte blocks fit one after the other
 *   subst DIR    $LIB, $PLATFORM, $ORIGIN as the loader expands them (RTLD_DI_SERINFO, dlerror)
 *   compat       the placeholder libraries: which names dlsym finds and whether they are libc's
 * Built with -DORDER_MAIN (and linked against the corpus) it is the order program instead.
 * Paths in the output are printed raw; run.sh replaces DIR with <dir>. Public interfaces only.
 */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <link.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>

static void out(const char *s) { (void)!write(1, s, strlen(s)); }

#ifdef ORDER_MAIN
/* ---- the order program: linked against libA (needs B, C) and libD (needs C) ---- */
static void say(const char *s) { out(s); out("\n"); }
static void pre(void) { say("preinit_array main"); }
static void ini(void) { say("init_array main"); }
static void fin(void) { say("fini_array main"); }
__attribute__((section(".preinit_array"), used)) static void (*pre_p)(void) = pre;
__attribute__((section(".init_array"), used)) static void (*ini_p)(void) = ini;
__attribute__((section(".fini_array"), used)) static void (*fin_p)(void) = fin;
__attribute__((constructor)) static void mctor(void) { say("ctor main"); }
__attribute__((destructor)) static void mdtor(void) { say("dtor main"); }
static void ax1(void) { say("atexit main 1"); }
static void ax2(void) { say("atexit main 2"); }
int fn_A(void);
int fn_D(void);

int main(int argc, char **argv)
{
    char p[4096];
    const char *dir = argc > 2 ? argv[2] : ".";
    const char *mode = argc > 1 ? argv[1] : "close";
    say("main");
    fn_A();
    fn_D();
    atexit(ax1);
    snprintf(p, sizeof p, "%s/libE.so", dir);
    say("dlopen E");
    void *h = dlopen(p, RTLD_NOW);
    if (!h) {
        say(dlerror());
        return 1;
    }
    atexit(ax2);
    if (strcmp(mode, "close") == 0) {
        say("dlclose E");
        dlclose(h);
        say("dlclose E done");
    } else if (strcmp(mode, "exit") == 0) {
        say("exit without dlclose");
        exit(0);
    } else if (strcmp(mode, "_exit") == 0) {
        say("_exit");
        _exit(0);
    } else if (strcmp(mode, "cycle") == 0) {
        /* libH needs libI and libI needs libH */
        snprintf(p, sizeof p, "%s/libH.so", dir);
        say("dlopen H (H<->I cycle)");
        void *hc = dlopen(p, RTLD_NOW);
        if (!hc)
            say(dlerror());
        say("dlclose H");
        if (hc)
            dlclose(hc);
        say("dlclose H done");
    }
    say("return from main");
    return 0;
}

#else
/* ---- the probe program ---- */
static const char *D;
static char pb[4096];
static const char *path(const char *f)
{
    snprintf(pb, sizeof pb, "%s/%s", D, f);
    return pb;
}

static void show(const char *label, const void *ret, int rc_is_int, long rc)
{
    char b[8192], eb[4096];
    /* copy: the next dlerror() call may release the string */
    const char *e0 = dlerror();
    const char *e = e0 ? (snprintf(eb, sizeof eb, "%s", e0), eb) : NULL;
    if (rc_is_int)
        snprintf(b, sizeof b, "%-34s | ret=%ld | dlerror=%s%s%s | again=%s\n", label, rc,
                 e ? "[" : "", e ? e : "NULL", e ? "]" : "", dlerror() ? "non-NULL" : "NULL");
    else
        snprintf(b, sizeof b, "%-34s | ret=%s | dlerror=%s%s%s | again=%s\n", label,
                 ret ? "handle" : "NULL", e ? "[" : "", e ? e : "NULL", e ? "]" : "",
                 dlerror() ? "non-NULL" : "NULL");
    out(b);
}

/* Each case runs in its own child so a load or a crash cannot affect the next. */
static void child(const char *label, void (*fn)(const char *), const char *arg)
{
    pid_t pid = fork();
    if (pid == 0) {
        alarm(10);
        fn(arg);
        _exit(0);
    }
    int st = 0;
    waitpid(pid, &st, 0);
    if (!WIFEXITED(st) || WEXITSTATUS(st) != 0) {
        char b[256];
        snprintf(b, sizeof b, "%-34s | child %s %d\n", label,
                 WIFSIGNALED(st) ? "signal" : "exit",
                 WIFSIGNALED(st) ? WTERMSIG(st) : WEXITSTATUS(st));
        out(b);
    }
}

static void c_open_now(const char *f) { void *h = dlopen(f, RTLD_NOW); show(f[0] ? f : "\"\" (empty name)", h, 0, 0); }
static void c_open_lazy(const char *f) { void *h = dlopen(f, RTLD_LAZY); show(f, h, 0, 0); }
static void c_open_mode0(const char *f) { void *h = dlopen(f, 0); show("mode 0", h, 0, 0); }
static void c_open_badmode(const char *f) { void *h = dlopen(f, RTLD_NOW | RTLD_LAZY); show("mode NOW|LAZY", h, 0, 0); }
static void c_noload(const char *f) { void *h = dlopen(f, RTLD_NOW | RTLD_NOLOAD); show("NOLOAD, not loaded", h, 0, 0); }
static void c_null(const char *f) { (void)f; void *h = dlopen(NULL, RTLD_NOW); show("dlopen(NULL)", h, 0, 0); }
static void c_sym_missing(const char *f)
{
    void *h = dlopen(f, RTLD_NOW);
    dlerror();
    void *s = dlsym(h, "no_such_symbol_xyz");
    show("dlsym(handle, missing)", s, 0, 0);
}
static void c_sym_default(const char *f) { (void)f; void *s = dlsym(RTLD_DEFAULT, "no_such_symbol_xyz"); show("dlsym(RTLD_DEFAULT, missing)", s, 0, 0); }
static void c_sym_next(const char *f) { (void)f; void *s = dlsym(RTLD_NEXT, "no_such_symbol_xyz"); show("dlsym(RTLD_NEXT, missing)", s, 0, 0); }
static void c_sym_ok(const char *f)
{
    void *h = dlopen(f, RTLD_NOW);
    void *s = dlsym(h, "fn_C");
    show("dlsym(handle, present)", s, 0, 0);
}
static void c_vsym_badver(const char *f) { (void)f; void *s = dlvsym(RTLD_DEFAULT, "printf", "GLIBC_9.99"); show("dlvsym(printf, GLIBC_9.99)", s, 0, 0); }
static void c_vsym_ok(const char *f) { (void)f; void *s = dlvsym(RTLD_DEFAULT, "printf", "GLIBC_2.2.5"); show("dlvsym(printf, GLIBC_2.2.5)", s, 0, 0); }
static void c_vsym_unver(const char *f)
{
    void *h = dlopen(f, RTLD_NOW);
    void *s = dlvsym(h, "fn_C", "NOVER_1");
    show("dlvsym(unversioned obj, NOVER_1)", s, 0, 0);
}
static void c_close_twice_ok(const char *f)
{
    void *h1 = dlopen(f, RTLD_NOW), *h2 = dlopen(f, RTLD_NOW);
    show("dlopen twice same handle", h1 == h2 ? h1 : NULL, 0, 0);
    int r = dlclose(h1);
    show("dlclose 1 of 2", NULL, 1, r);
    r = dlclose(h2);
    show("dlclose 2 of 2", NULL, 1, r);
}
static void c_close_null(const char *f) { (void)f; int r = dlclose(NULL); show("dlclose(NULL)", NULL, 1, r); }
static void c_info_bad(const char *f)
{
    void *h = dlopen(f, RTLD_NOW);
    long x = 0;
    int r = dlinfo(h, 9999, &x);
    show("dlinfo(request 9999)", NULL, 1, r);
}
static void c_info_origin(const char *f)
{
    void *h = dlopen(f, RTLD_NOW);
    char o[4096] = "";
    int r = dlinfo(h, RTLD_DI_ORIGIN, o);
    char b[4200];
    snprintf(b, sizeof b, "dlinfo ORIGIN = [%s]", o);
    show(b, NULL, 1, r);
}
static void c_lazy_then_sym(const char *f)
{
    void *h = dlopen(f, RTLD_LAZY);
    show("LAZY undefined, open", h, 0, 0);
    void *s = h ? dlsym(h, "use_undef") : NULL;
    show("LAZY undefined, dlsym use_undef", s, 0, 0);
    void *h2 = dlopen(f, RTLD_NOW | RTLD_NOLOAD);
    show("LAZY then NOW|NOLOAD", h2, 0, 0);
    void *h3 = dlopen(f, RTLD_NOW);
    show("LAZY then NOW", h3, 0, 0);
}

static void errors(void)
{
    static const char *files[] = {
        "nope.so", "libnope_xyz.so", "text.so", "empty.so", "short.so", "subdir",
        "class32.so", "machine.so", "msb.so", "osabi.so", "etype_rel.o", "trunc.so",
        "pie_exe", "nopie_exe", "libMissingDep.so", "libU.so", "libW.so", "libC.so", "",
    };
    out("== errors: dlopen(RTLD_NOW) of each file (path DIR/<name>; a name without DIR when noted)\n");
    for (size_t i = 0; i < sizeof files / sizeof *files; i++) {
        char lab[256];
        if (strcmp(files[i], "libnope_xyz.so") == 0 || !files[i][0]) {
            child(files[i], c_open_now, files[i]);
            continue;
        }
        snprintf(lab, sizeof lab, "%s", path(files[i]));
        child(lab, c_open_now, lab);
    }
    out("== errors: other calls\n");
    char libc[4096], libu[4096];
    snprintf(libc, sizeof libc, "%s", path("libC.so"));
    snprintf(libu, sizeof libu, "%s", path("libU.so"));
    child("lazy", c_open_lazy, libu);
    child("lazy+", c_lazy_then_sym, libu);
    child("mode0", c_open_mode0, libc);
    child("badmode", c_open_badmode, libc);
    child("noload", c_noload, libc);
    child("null", c_null, NULL);
    child("symok", c_sym_ok, libc);
    child("symmiss", c_sym_missing, libc);
    child("symdef", c_sym_default, NULL);
    child("symnext", c_sym_next, NULL);
    child("vsymok", c_vsym_ok, NULL);
    child("vsymbad", c_vsym_badver, NULL);
    child("vsymunver", c_vsym_unver, libc);
    child("close2", c_close_twice_ok, libc);
    child("closenull", c_close_null, NULL);
    child("infobad", c_info_bad, libc);
    child("infoorigin", c_info_origin, libc);
}

static int cb(struct dl_phdr_info *i, size_t sz, void *data)
{
    (void)sz;
    int *n = data;
    if (n[1]) {
        char b[4300];
        snprintf(b, sizeof b, "  [%d] name=[%s] phnum=%d tls_modid=%zu\n", n[0], i->dlpi_name,
                 i->dlpi_phnum, i->dlpi_tls_modid);
        out(b);
    }
    if (n[0] == 0) {
        n[2] = (int)i->dlpi_adds;
        n[3] = (int)i->dlpi_subs;
    }
    n[0]++;
    return 0;
}
static void phdr_at(const char *label, int names)
{
    int n[4] = {0, names, 0, 0};
    char b[256];
    dl_iterate_phdr(cb, n);
    snprintf(b, sizeof b, "%-36s objects=%d adds=%d subs=%d\n", label, n[0], n[2], n[3]);
    out(b);
}
static void phdr(void)
{
    char a[4096], c[4096], m[4096], t[4096];
    snprintf(a, sizeof a, "%s", path("libA.so"));
    snprintf(c, sizeof c, "%s", path("libC.so"));
    snprintf(m, sizeof m, "%s", path("libMissingDep.so"));
    snprintf(t, sizeof t, "%s", path("text.so"));
    out("== phdr: dl_iterate_phdr counters (adds/subs read from the first object)\n");
    phdr_at("start (names)", 1);
    void *hc = dlopen(c, RTLD_NOW);
    phdr_at("dlopen C", 0);
    void *hc2 = dlopen(c, RTLD_NOW);
    phdr_at("dlopen C again (same handle)", 0);
    void *ha = dlopen(a, RTLD_NOW);
    phdr_at("dlopen A (needs B, C)", 1);
    dlclose(ha);
    phdr_at("dlclose A", 0);
    dlclose(hc2);
    phdr_at("dlclose C (1 of 2)", 0);
    dlclose(hc);
    phdr_at("dlclose C (2 of 2)", 0);
    void *x = dlopen(t, RTLD_NOW);
    phdr_at(x ? "dlopen text.so ok?" : "dlopen text.so (fails)", 0);
    x = dlopen(m, RTLD_NOW);
    phdr_at(x ? "dlopen MissingDep ok?" : "dlopen MissingDep (fails)", 0);
    hc = dlopen(c, RTLD_NOW | RTLD_NODELETE);
    phdr_at("dlopen C NODELETE", 0);
    dlclose(hc);
    phdr_at("dlclose C NODELETE", 0);
}

static void tls_try(const char *f)
{
    void *h = dlopen(f, RTLD_NOW);
    if (h) {
        out("ok\n");
        _exit(0);
    }
    out("fail ");
    out(dlerror());
    out("\n");
    _exit(3);
}
static void tls(void)
{
    char f[4096], b[8400];
    int last_ok = -1;
    out("== tls: one dlopen of a library with an N-byte initial-exec TLS block, fresh process each\n");
    for (int n = 64; n <= 8192; n += 64) {
        int fd[2];
        if (pipe(fd))
            return;
        snprintf(f, sizeof f, "%s/tls/libtls%d.so", D, n);
        pid_t pid = fork();
        if (pid == 0) {
            dup2(fd[1], 1);
            tls_try(f);
        }
        close(fd[1]);
        char r[4096] = "";
        size_t got = 0;
        ssize_t k;
        while (got < sizeof r - 1 && (k = read(fd[0], r + got, sizeof r - 1 - got)) > 0)
            got += (size_t)k;
        r[got] = 0;
        close(fd[0]);
        int st;
        waitpid(pid, &st, 0);
        if (WIFEXITED(st) && WEXITSTATUS(st) == 0) {
            last_ok = n;
            continue;
        }
        const char *why = strstr(r, "fail ");
        snprintf(b, sizeof b, "largest N that loads: %d; first failing N: %d: %s", last_ok, n,
                 why ? why + 5 : r);
        out(b);
        break;
    }
    out("== tls: successive dlopens of distinct libraries with a 128-byte initial-exec TLS block\n");
    int i;
    for (i = 0; i < 64; i++) {
        snprintf(f, sizeof f, "%s/tls/libtlsc%d.so", D, i);
        if (!dlopen(f, RTLD_NOW))
            break;
    }
    snprintf(b, sizeof b, "loaded %d of 64; failure: %s\n", i, i < 64 ? dlerror() : "none");
    out(b);
}

static void subst(void)
{
    char b[8400];
    out("== subst: RTLD_DI_SERINFO of libR.so (DT_RUNPATH '/r/$LIB/$PLATFORM/$ORIGIN/${LIB}')\n");
    void *h = dlopen(path("libR.so"), RTLD_NOW);
    if (!h) {
        out(dlerror());
        out("\n");
        return;
    }
    Dl_serinfo sz;
    if (dlinfo(h, RTLD_DI_SERINFOSIZE, &sz) == 0) {
        Dl_serinfo *si = malloc(sz.dls_size);
        memcpy(si, &sz, sizeof sz);
        if (dlinfo(h, RTLD_DI_SERINFO, si) == 0)
            for (unsigned i = 0; i < si->dls_cnt; i++) {
                snprintf(b, sizeof b, "  path[%u] = [%s] flags=0x%x\n", i, si->dls_serpath[i].dls_name,
                         si->dls_serpath[i].dls_flags);
                out(b);
            }
    }
    out("== subst: dlerror of dlopen(\"DIR/$LIB/$PLATFORM/x.so\") (no expansion in dlopen names?)\n");
    snprintf(pb, sizeof pb, "%s/$LIB/$PLATFORM/x.so", D);
    void *x = dlopen(pb, RTLD_NOW);
    snprintf(b, sizeof b, "  ret=%s dlerror=[%s]\n", x ? "handle" : "NULL", x ? "" : dlerror());
    out(b);
    out("== subst: dlopen(\"$ORIGIN/libC.so\") from the program\n");
    x = dlopen("$ORIGIN/libC.so", RTLD_NOW);
    snprintf(b, sizeof b, "  ret=%s dlerror=[%s]\n", x ? "handle" : "NULL", x ? "" : dlerror());
    out(b);
}

static void compat(void)
{
    static const char *libs[] = {"libdl.so.2", "libpthread.so.0", "librt.so.1", "libutil.so.1",
                                 "libanl.so.1", "libBrokenLocale.so.1", "libresolv.so.2",
                                 "libthread_db.so.1"};
    static const char *names[] = {"dlopen", "dlsym", "dladdr", "pthread_create", "pthread_mutex_lock",
                                  "sem_open", "clock_gettime", "timer_create", "mq_open", "shm_open",
                                  "aio_read", "openpty", "forkpty", "login_tty", "getaddrinfo_a",
                                  "gai_suspend", "res_query", "__libc_freeres", "malloc", "no_such_xyz"};
    char b[8400];
    out("== compat: dlopen of each library, then dlsym(lib, name) (searches the library and its\n"
        "   dependencies); shown: the defining object's file name from dladdr, '-' = not found\n");
    for (size_t i = 0; i < sizeof libs / sizeof *libs; i++) {
        void *h = dlopen(libs[i], RTLD_NOW);
        snprintf(b, sizeof b, "%-24s %s", libs[i], h ? "loads:" : "FAILS: ");
        out(b);
        if (!h) {
            out(dlerror());
            out("\n");
            continue;
        }
        for (size_t j = 0; j < sizeof names / sizeof *names; j++) {
            void *s = dlsym(h, names[j]);
            dlerror();
            Dl_info di;
            const char *who = "-";
            if (s && dladdr(s, &di) && di.dli_fname) {
                who = strrchr(di.dli_fname, '/');
                who = who ? who + 1 : di.dli_fname;
            } else if (s)
                who = "?";
            snprintf(b, sizeof b, " %s=%s", names[j], who);
            out(b);
        }
        out("\n");
    }
}

int main(int argc, char **argv)
{
    if (argc < 2)
        return 2;
    D = argc > 2 ? argv[2] : ".";
    if (strcmp(argv[1], "errors") == 0)
        errors();
    else if (strcmp(argv[1], "phdr") == 0)
        phdr();
    else if (strcmp(argv[1], "tls") == 0)
        tls();
    else if (strcmp(argv[1], "subst") == 0)
        subst();
    else if (strcmp(argv[1], "compat") == 0)
        compat();
    else
        return 2;
    return 0;
}
#endif
