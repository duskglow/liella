/* probe.c - DL-17 black-box probe of dlclose (C-1..C-8): which objects stay loaded after dlclose
 * (RTLD_GLOBAL definers, TLS, unique symbols, NODELETE), dlclose's result and dlerror, dlclose from
 * a destructor, and the dl_iterate_phdr counters. argv[1]: the directory holding the objects.
 * Public interfaces only (dlopen(3), dlclose(3), dlinfo(3), dl_iterate_phdr(3)). */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <link.h>
#include <stdio.h>
#include <string.h>

static const char *dir;
static char buf[4096];

static const char *P(const char *n) {
    snprintf(buf, sizeof buf, "%s/%s", dir, n);
    return buf;
}

/* Loaded now? (an RTLD_NOLOAD open, closed again so the count is unchanged) */
static int loaded(const char *n) {
    void *h = dlopen(P(n), RTLD_NOW | RTLD_NOLOAD);
    if (h) dlclose(h);
    return h != NULL;
}

static unsigned long long adds, subs;
static int count;
static int cb(struct dl_phdr_info *i, size_t s, void *d) {
    (void)s; (void)d;
    adds = i->dlpi_adds; subs = i->dlpi_subs; count++;
    return 0;
}
static void counters(const char *tag) {
    count = 0;
    dl_iterate_phdr(cb, NULL);
    printf("%s: objects %d adds %llu subs %llu\n", tag, count, adds, subs);
}

static const char *err(void) {
    const char *e = dlerror();
    return e ? e : "(none)";
}

int main(int argc, char **argv) {
    (void)argc;
    dir = argv[1];
    setvbuf(stdout, NULL, _IONBF, 0);
    counters("start");

    /* C-1: plain close: result, dlerror after, counters. */
    void *a = dlopen(P("libP.so"), RTLD_NOW);
    counters("C-1 opened");
    dlerror();
    int r = dlclose(a);
    printf("C-1 dlclose=%d dlerror=%s loaded=%d\n", r, err(), loaded("libP.so"));
    counters("C-1 closed");

    /* C-2: a successful dlclose and a pending text. */
    dlopen(P("nonexistent.so"), RTLD_NOW);
    a = dlopen(P("libP.so"), RTLD_NOW);
    (void)dlerror();
    dlopen(P("nonexistent.so"), RTLD_NOW);
    r = dlclose(a);
    printf("C-2 dlclose=%d pending after=%s\n", r, err());

    /* C-3: RTLD_GLOBAL definer used by a later object without DT_NEEDED. */
    void *g = dlopen(P("libG.so"), RTLD_NOW | RTLD_GLOBAL);
    void *u = dlopen(P("libU.so"), RTLD_NOW);
    printf("C-3 open G=%d U=%d %s\n", g != NULL, u != NULL, u ? "" : err());
    r = dlclose(g);
    printf("C-3 close G=%d: G loaded=%d\n", r, loaded("libG.so"));
    if (u) {
        int (*f)(void) = (int (*)(void))dlsym(u, "use_g");
        printf("C-3 use_g()=%d\n", f ? f() : -1);
        r = dlclose(u);
        printf("C-3 close U=%d: U loaded=%d G loaded=%d\n", r, loaded("libU.so"), loaded("libG.so"));
    }

    /* C-3b: RTLD_LOCAL then promoted by an RTLD_GLOBAL re-open; two closes. */
    g = dlopen(P("libG.so"), RTLD_NOW);
    void *g2 = dlopen(P("libG.so"), RTLD_NOW | RTLD_GLOBAL | RTLD_NOLOAD);
    dlclose(g);
    printf("C-3b after 1 close: loaded=%d\n", loaded("libG.so"));
    dlclose(g2);
    printf("C-3b after 2 closes: loaded=%d\n", loaded("libG.so"));

    /* C-4: TLS (general dynamic), used and unused; the module id across a re-open. */
    for (int use = 0; use < 2; use++) {
        void *t = dlopen(P("libTG.so"), RTLD_NOW);
        size_t id = 0;
        dlinfo(t, RTLD_DI_TLS_MODID, &id);
        if (use) {
            int *(*f)(void) = (int *(*)(void))dlsym(t, "t_addr");
            printf("C-4 use=%d value=%d\n", use, *f());
        }
        r = dlclose(t);
        printf("C-4 use=%d modid=%zu dlclose=%d loaded=%d\n", use, id, r, loaded("libTG.so"));
    }
    counters("C-4 end");

    /* C-5: TLS (initial exec, static block). */
    for (int i = 0; i < 2; i++) {
        void *t = dlopen(P("libTI.so"), RTLD_NOW);
        size_t id = 0;
        if (t) dlinfo(t, RTLD_DI_TLS_MODID, &id);
        int *(*f)(void) = t ? (int *(*)(void))dlsym(t, "t_addr") : NULL;
        printf("C-5 open=%d modid=%zu value=%d\n", t != NULL, id, f ? *f() : -1);
        if (t) {
            r = dlclose(t);
            printf("C-5 dlclose=%d loaded=%d\n", r, loaded("libTI.so"));
        }
    }

    /* C-6: a bound unique symbol, and an unbound one. */
    void *q = dlopen(P("libQ.so"), RTLD_NOW);
    r = dlclose(q);
    printf("C-6 unique not looked up: dlclose=%d loaded=%d\n", r, loaded("libQ.so"));
    q = dlopen(P("libQ.so"), RTLD_NOW);
    void *uo = dlsym(q, "u_obj");
    r = dlclose(q);
    printf("C-6 unique looked up (%d): dlclose=%d loaded=%d\n", uo != NULL, r, loaded("libQ.so"));

    /* C-7: RTLD_NODELETE and the counters. */
    void *n = dlopen(P("libN.so"), RTLD_NOW | RTLD_NODELETE);
    r = dlclose(n);
    printf("C-7 nodelete dlclose=%d loaded=%d\n", r, loaded("libN.so"));
    counters("C-7");

    /* C-8: dlclose from a destructor (X opened Y in its constructor). */
    void *x = dlopen(P("libX.so"), RTLD_NOW);
    printf("C-8 open X=%d Y loaded=%d\n", x != NULL, loaded("libY.so"));
    r = dlclose(x);
    printf("C-8 dlclose(X)=%d X loaded=%d Y loaded=%d\n", r, loaded("libX.so"), loaded("libY.so"));

    /* C-9: the program's handle. */
    void *self = dlopen(NULL, RTLD_NOW);
    r = dlclose(self);
    printf("C-9 dlclose(program)=%d dlerror=%s\n", r, err());
    counters("end");
    return 0;
}
