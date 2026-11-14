/* Spec §27.1 (s27_1_c_7 dlinfo; its failure text, s27_1_c_5): the requests the reference answers,
 * as probed (tools/probe/dl/dladdr/; spec/behaviour/dynlink.md "dladdr and dlinfo"), on helpers
 * opened by dlopen and on the program (dlopen(NULL)):
 *   - RTLD_DI_LMID 0; RTLD_DI_LINKMAP the handle; RTLD_DI_ORIGIN the directory of the path the
 *     helper was opened by;
 *   - RTLD_DI_TLS_MODID: 0 without PT_TLS; else the module id __tls_get_addr takes for the block;
 *     RTLD_DI_TLS_DATA: NULL without PT_TLS; the calling thread's block once that thread used it
 *     (before that the reference gives NULL and Liella the block: docs/differences-from-glibc.md);
 *   - RTLD_DI_PHDR: returns the program header count, stores the run-time table (the file's headers);
 *   - RTLD_DI_SERINFOSIZE / RTLD_DI_SERINFO: the search path, DT_RPATH, LD_LIBRARY_PATH, DT_RUNPATH
 *     ($ORIGIN expanded), then the system directories; dls_size is the header, the entries and the
 *     strings; strings follow the entries, flags 0; a buffer one entry short still succeeds;
 *   - other requests (0, 3, 7, 8, 12, 99): -1, "unsupported dlinfo request", argument untouched;
 *     a success leaves no dlerror text.
 * Each failed check prints its line; the exit status is the number of failed checks. */
/* DLOPEN-HELPERS: dla_syms dla_run dla_rp */
/* HELPER-FLAGS: dla_run -Wl,--enable-new-dtags,-rpath,/r1:$ORIGIN/x:/r2 */
/* HELPER-FLAGS: dla_rp -Wl,--disable-new-dtags,-rpath,/rp */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <elf.h>
#include <link.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/auxv.h>

static int fails;
#define CHECK(c) do { if (!(c)) { fprintf(stderr, "s27_dlinfo: line %d: %s\n", __LINE__, #c); fails++; } } while (0)

static const char *dir;

static void *open_helper(const char *name)
{
    char p[4096];
    snprintf(p, sizeof p, "%s/lib%s.so", dir, name);
    void *h = dlopen(p, RTLD_NOW);
    if (h == NULL)
        fprintf(stderr, "s27_dlinfo: dlopen %s: %s\n", p, dlerror());
    return h;
}

/* RTLD_DI_PHDR on helper NAME (handle H): the count is the file's e_phnum and the table is the
 * file's program headers, mapped at the load bias plus e_phoff (the first PT_LOAD maps offset 0
 * at address 0 in these helpers). */
static int phdr_ok(const char *name, void *h)
{
    struct link_map *lm = NULL;
    const void *t = NULL;
    char p[4096];
    snprintf(p, sizeof p, "%s/lib%s.so", dir, name);
    FILE *f = fopen(p, "rb");
    Elf64_Ehdr eh;
    Elf64_Phdr ph[32];
    int ok = f != NULL && fread(&eh, sizeof eh, 1, f) == 1 && eh.e_phnum > 0 && eh.e_phnum <= 32 &&
             fseek(f, (long)eh.e_phoff, SEEK_SET) == 0 && fread(ph, sizeof ph[0], eh.e_phnum, f) == eh.e_phnum;
    if (f != NULL)
        fclose(f);
    ok = ok && dlinfo(h, RTLD_DI_LINKMAP, &lm) == 0 && lm != NULL;
    int n = ok ? dlinfo(h, RTLD_DI_PHDR, &t) : -1;
    ok = ok && n == eh.e_phnum && t == (const void *)(lm->l_addr + eh.e_phoff) &&
         memcmp(t, ph, eh.e_phnum * sizeof ph[0]) == 0;
    if (!ok)
        fprintf(stderr, "s27_dlinfo: PHDR of %s: %d headers at %p\n", name, n, t);
    return ok;
}

/* The search path of H: dls_size, the strings after the entries, and its first WANT entries. */
static int serinfo_ok(const char *what, void *h, const char *const *want, int nwant)
{
    Dl_serinfo sz;
    memset(&sz, 0x5a, sizeof sz);
    if (dlinfo(h, RTLD_DI_SERINFOSIZE, &sz) != 0 || sz.dls_cnt < (unsigned)nwant || sz.dls_size > 65536) {
        fprintf(stderr, "s27_dlinfo: %s: SERINFOSIZE failed\n", what);
        return 0;
    }
    Dl_serinfo *si = malloc(sz.dls_size);
    if (si == NULL)
        return 0;
    si->dls_size = sz.dls_size;
    si->dls_cnt = sz.dls_cnt;
    int ok = dlinfo(h, RTLD_DI_SERINFO, si) == 0 && si->dls_cnt == sz.dls_cnt && si->dls_size == sz.dls_size;
    size_t text = sizeof(Dl_serinfo) - sizeof(Dl_serpath) + sz.dls_cnt * sizeof(Dl_serpath);
    size_t total = text;
    char *lo = (char *)si + text, *hi = (char *)si + sz.dls_size;
    for (unsigned k = 0; ok && k < sz.dls_cnt; k++) {
        const char *s = si->dls_serpath[k].dls_name;
        ok = s >= lo && s < hi && memchr(s, 0, (size_t)(hi - s)) != NULL && si->dls_serpath[k].dls_flags == 0 && s[0] == '/';
        if (ok)
            total += strlen(s) + 1;
        if (ok && (int)k < nwant && strcmp(s, want[k]) != 0) {
            fprintf(stderr, "s27_dlinfo: %s: entry %u [%s], want [%s]\n", what, k, s, want[k]);
            ok = 0;
        }
    }
    ok = ok && total == sz.dls_size;
    free(si);
    /* A buffer sized for one entry fewer: still a success. */
    if (ok && sz.dls_cnt > 1) {
        si = malloc(sz.dls_size);
        if (si == NULL)
            return 0;
        si->dls_size = sz.dls_size;
        si->dls_cnt = sz.dls_cnt - 1;
        ok = dlinfo(h, RTLD_DI_SERINFO, si) == 0 && dlerror() == NULL;
        free(si);
    }
    if (!ok)
        fprintf(stderr, "s27_dlinfo: %s: SERINFO (size %zu, cnt %u) does not hold together\n", what, sz.dls_size, sz.dls_cnt);
    return ok;
}

static void *syms;
static long *(*tls_addr)(void);
static void *(*tls_get_addr)(void *);

/* In a thread: TLS_DATA is its own block once it used it, and the module id finds that block. */
static void *thread(void *arg)
{
    void *main_block = arg, *data = (void *)1;
    long *mine = tls_addr();
    size_t modid = 0;
    int ok = dlinfo(syms, RTLD_DI_TLS_DATA, &data) == 0 && data == mine && data != main_block &&
             dlinfo(syms, RTLD_DI_TLS_MODID, &modid) == 0;
    size_t ti[2] = {modid, 0};
    ok = ok && tls_get_addr(ti) == mine;
    return ok ? (void *)1 : NULL;
}

int main(void)
{
    dir = getenv("CONF_HELPERS");
    if (dir == NULL)
        return 100;
    syms = open_helper("dla_syms");
    void *run = open_helper("dla_run"), *rp = open_helper("dla_rp"), *g = dlopen(NULL, RTLD_LAZY);
    if (syms == NULL || run == NULL || rp == NULL || g == NULL)
        return 100;
    tls_addr = (long *(*)(void))dlsym(syms, "lib_tls_addr");
    tls_get_addr = (void *(*)(void *))dlsym(syms, "dla_tls_get_addr");
    CHECK(tls_addr != NULL && tls_get_addr != NULL);
    if (fails)
        return fails;

    /* LMID, LINKMAP, ORIGIN. */
    Lmid_t lmid = 7;
    CHECK(dlinfo(syms, RTLD_DI_LMID, &lmid) == 0 && lmid == LM_ID_BASE);
    lmid = 7;
    CHECK(dlinfo(g, RTLD_DI_LMID, &lmid) == 0 && lmid == LM_ID_BASE);
    struct link_map *lm = NULL;
    CHECK(dlinfo(syms, RTLD_DI_LINKMAP, &lm) == 0 && (void *)lm == syms);
    lm = NULL;
    CHECK(dlinfo(g, RTLD_DI_LINKMAP, &lm) == 0 && (void *)lm == g);
    char origin[4096];
    memset(origin, 0x5a, sizeof origin);
    CHECK(dlinfo(syms, RTLD_DI_ORIGIN, origin) == 0 && strcmp(origin, dir) == 0);
    CHECK(dlinfo(run, RTLD_DI_ORIGIN, origin) == 0 && strcmp(origin, dir) == 0);
    CHECK(dlerror() == NULL);

    /* TLS: the module id is the one __tls_get_addr takes; the block is the thread's. */
    size_t modid = 99;
    CHECK(dlinfo(syms, RTLD_DI_TLS_MODID, &modid) == 0 && modid != 0);
    void *data = (void *)1;
    CHECK(dlinfo(syms, RTLD_DI_TLS_DATA, &data) == 0);
    long *mine = tls_addr();
    CHECK(data == NULL || data == mine); /* before first use: the reference NULL, Liella the block */
    data = (void *)1;
    CHECK(dlinfo(syms, RTLD_DI_TLS_DATA, &data) == 0 && data == mine);
    size_t ti[2] = {modid, 0};
    CHECK(tls_get_addr(ti) == mine);
    pthread_t t;
    void *tr = NULL;
    CHECK(pthread_create(&t, NULL, thread, mine) == 0 && pthread_join(t, &tr) == 0 && tr == (void *)1);
    size_t m2 = 99;
    CHECK(dlinfo(run, RTLD_DI_TLS_MODID, &m2) == 0 && m2 == 0);
    data = (void *)1;
    CHECK(dlinfo(run, RTLD_DI_TLS_DATA, &data) == 0 && data == NULL);

    /* PHDR. */
    CHECK(phdr_ok("dla_syms", syms));
    CHECK(phdr_ok("dla_run", run));
    const void *pt = NULL;
    CHECK(dlinfo(g, RTLD_DI_PHDR, &pt) == (int)getauxval(AT_PHNUM) && pt == (const void *)getauxval(AT_PHDR));

    /* SERINFO: DT_RUNPATH after LD_LIBRARY_PATH (the helper directory); DT_RPATH before it. */
    char xdir[4096];
    snprintf(xdir, sizeof xdir, "%s/x", dir);
    const char *const want_run[] = {dir, "/r1", xdir, "/r2"};
    const char *const want_rp[] = {"/rp", dir};
    const char *const want_syms[] = {dir};
    CHECK(serinfo_ok("dla_run", run, want_run, 4));
    CHECK(serinfo_ok("dla_rp", rp, want_rp, 2));
    CHECK(serinfo_ok("dla_syms", syms, want_syms, 1));

    /* Requests the reference does not answer. */
    static const int bad[] = {0, 3, 7, 8, 12, 99};
    for (unsigned k = 0; k < sizeof bad / sizeof bad[0]; k++) {
        unsigned char arg[64];
        memset(arg, 0x5a, sizeof arg);
        int r = dlinfo(syms, bad[k], arg);
        const char *e = dlerror();
        int untouched = 1;
        for (unsigned b = 0; b < sizeof arg; b++)
            untouched &= arg[b] == 0x5a;
        if (!(r == -1 && e != NULL && strcmp(e, "unsupported dlinfo request") == 0 && untouched && dlerror() == NULL)) {
            fprintf(stderr, "s27_dlinfo: request %d: ret %d [%s]\n", bad[k], r, e ? e : "no text");
            fails++;
        }
    }
    /* A success clears a pending text. */
    CHECK(dlinfo(syms, 99, &lmid) == -1);
    CHECK(dlinfo(syms, RTLD_DI_LMID, &lmid) == 0 && dlerror() == NULL);
    return fails;
}
