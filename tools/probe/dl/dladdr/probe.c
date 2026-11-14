/* probe.c - DL-18 (DL-DLADDR) black-box probe of dladdr, dladdr1 and dlinfo; built and run by
 * run.sh. Public interfaces only: the reference loader is executed, the objects' own files are
 * read for their section headers (to find .plt) and dynamic-symbol order. Each case prints one
 * line; addresses print relative to the containing object's base (dli_fbase) or as names. */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <elf.h>
#include <fcntl.h>
#include <link.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/auxv.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <unistd.h>

static const char *D; /* the scratch directory, printed as <dir> */

static void line(const char *s)
{
    char b[8192];
    size_t n = strlen(D);
    const char *p;
    size_t o = 0;
    while ((p = strstr(s, D)) != NULL && o + (size_t)(p - s) + 6 < sizeof b) {
        memcpy(b + o, s, (size_t)(p - s));
        o += (size_t)(p - s);
        memcpy(b + o, "<dir>", 5);
        o += 5;
        s = p + n;
    }
    snprintf(b + o, sizeof b - o, "%s\n", s);
    write(1, b, strlen(b));
}

/* One dladdr answer, relative to the object's base. */
static void show(const char *what, const void *addr)
{
    Dl_info i;
    memset(&i, 0x5a, sizeof i);
    int r = dladdr(addr, &i);
    char b[8192];
    if (r == 0)
        snprintf(b, sizeof b, "dladdr %-28s ret=0 (info %s)", what,
                 i.dli_fname == (void *)0x5a5a5a5a5a5a5a5aul ? "untouched" : "written");
    else
        snprintf(b, sizeof b, "dladdr %-28s ret=%d fname=[%s] fbase=%s sname=[%s] saddr=%s%+ld", what, r,
                 i.dli_fname ? i.dli_fname : "(null)", i.dli_fbase ? "base" : "NULL",
                 i.dli_sname ? i.dli_sname : "(null)", i.dli_saddr ? "base" : "NULL",
                 i.dli_saddr ? (long)((char *)i.dli_saddr - (char *)i.dli_fbase) : 0L);
    line(b);
}

/* [lo, hi) of section NAME of the object at BASE read from FILE (ELF64 section headers). */
static int section(const char *file, uintptr_t base, const char *name, uintptr_t *lo, uintptr_t *hi)
{
    int fd = open(file, O_RDONLY);
    struct stat st;
    if (fd < 0 || fstat(fd, &st) != 0)
        return -1;
    unsigned char *m = mmap(NULL, (size_t)st.st_size, PROT_READ, MAP_PRIVATE, fd, 0);
    close(fd);
    if (m == MAP_FAILED)
        return -1;
    Elf64_Ehdr *e = (Elf64_Ehdr *)m;
    Elf64_Shdr *s = (Elf64_Shdr *)(m + e->e_shoff);
    const char *str = (const char *)(m + s[e->e_shstrndx].sh_offset);
    int ok = -1;
    for (int k = 0; k < e->e_shnum; k++)
        if (strcmp(str + s[k].sh_name, name) == 0) {
            *lo = base + s[k].sh_addr;
            *hi = *lo + s[k].sh_size;
            ok = 0;
        }
    munmap(m, (size_t)st.st_size);
    return ok;
}

/* The .dynsym index and binding of each named symbol of FILE (the tie-break order). */
static void dynsym_order(const char *file, const char *const *names, int n)
{
    int fd = open(file, O_RDONLY);
    struct stat st;
    if (fd < 0 || fstat(fd, &st) != 0)
        return;
    unsigned char *m = mmap(NULL, (size_t)st.st_size, PROT_READ, MAP_PRIVATE, fd, 0);
    close(fd);
    Elf64_Ehdr *e = (Elf64_Ehdr *)m;
    Elf64_Shdr *s = (Elf64_Shdr *)(m + e->e_shoff);
    for (int k = 0; k < e->e_shnum; k++) {
        if (s[k].sh_type != SHT_DYNSYM)
            continue;
        Elf64_Sym *sym = (Elf64_Sym *)(m + s[k].sh_offset);
        const char *str = (const char *)(m + s[s[k].sh_link].sh_offset);
        size_t cnt = s[k].sh_size / sizeof *sym;
        for (int j = 0; j < n; j++)
            for (size_t x = 0; x < cnt; x++)
                if (strcmp(str + sym[x].st_name, names[j]) == 0) {
                    char b[256];
                    snprintf(b, sizeof b, "dynsym %-10s index=%zu bind=%d type=%d size=%lu", names[j], x,
                             ELF64_ST_BIND(sym[x].st_info), ELF64_ST_TYPE(sym[x].st_info),
                             (unsigned long)sym[x].st_size);
                    line(b);
                }
    }
    munmap(m, (size_t)st.st_size);
}

static void *lib_handle;
static char *lib_base;
static void *sym(const char *n) { return dlsym(lib_handle, n); }

static void *tls_thread(void *arg)
{
    (void)arg;
    void *d = (void *)1;
    int r = dlinfo(lib_handle, RTLD_DI_TLS_DATA, &d);
    char b[256];
    snprintf(b, sizeof b, "dlinfo TLS_DATA new thread, before use: ret=%d data=%s", r, d ? "non-NULL" : "NULL");
    line(b);
    long *(*a)(void) = (long *(*)(void))sym("lib_tls_addr");
    long *mine = a();
    r = dlinfo(lib_handle, RTLD_DI_TLS_DATA, &d);
    snprintf(b, sizeof b, "dlinfo TLS_DATA new thread, after use: ret=%d data==&s_tls %d", r, d == (void *)mine);
    line(b);
    return NULL;
}

static void info_req(const char *what, void *h, int req)
{
    unsigned char buf[8192];
    memset(buf, 0x5a, sizeof buf);
    dlerror();
    int r = dlinfo(h, req, buf);
    const char *e = dlerror();
    char b[512];
    snprintf(b, sizeof b, "dlinfo %-12s req=%2d ret=%d dlerror=[%s] arg0=%016lx", what, req, r, e ? e : "NULL",
             *(unsigned long *)buf);
    line(b);
}

static void serinfo(const char *what, void *h)
{
    Dl_serinfo sz;
    char b[8192];
    memset(&sz, 0x5a, sizeof sz);
    int r = dlinfo(h, RTLD_DI_SERINFOSIZE, &sz);
    snprintf(b, sizeof b, "dlinfo %s SERINFOSIZE ret=%d size=%zu cnt=%u", what, r, sz.dls_size, sz.dls_cnt);
    line(b);
    if (r != 0)
        return;
    Dl_serinfo *si = malloc(sz.dls_size);
    memset(si, 0x5a, sz.dls_size);
    memcpy(si, &sz, sizeof sz);
    r = dlinfo(h, RTLD_DI_SERINFO, si);
    snprintf(b, sizeof b, "dlinfo %s SERINFO ret=%d size=%zu cnt=%u", what, r, si->dls_size, si->dls_cnt);
    line(b);
    for (unsigned i = 0; r == 0 && i < si->dls_cnt; i++) {
        snprintf(b, sizeof b, "  [%u] [%s] flags=0x%x offset=%ld", i, si->dls_serpath[i].dls_name,
                 si->dls_serpath[i].dls_flags, (long)(si->dls_serpath[i].dls_name - (char *)si));
        line(b);
    }
    /* A buffer whose dls_size/dls_cnt disagree with the sizing call. */
    memcpy(si, &sz, sizeof sz);
    si->dls_cnt = sz.dls_cnt ? sz.dls_cnt - 1 : 0;
    pid_t p = fork();
    if (p == 0) {
        alarm(10);
        dlerror();
        r = dlinfo(h, RTLD_DI_SERINFO, si);
        const char *e = dlerror();
        snprintf(b, sizeof b, "dlinfo %s SERINFO cnt-1 ret=%d dlerror=[%s]", what, r, e ? e : "NULL");
        line(b);
        _exit(0);
    }
    int st;
    waitpid(p, &st, 0);
    if (WIFSIGNALED(st))
        line("  (child ended by a signal)");
    free(si);
}

int main(int argc, char **argv)
{
    if (argc < 3)
        return 2;
    D = argv[1];
    const char *libpath = argv[2];
    lib_handle = dlopen(libpath, RTLD_NOW);
    if (!lib_handle) {
        line(dlerror());
        return 1;
    }
    Dl_info i0;
    dladdr(sym("s_a"), &i0);
    lib_base = i0.dli_fbase;
    char b[8192];

    line("== dladdr in the probe object");
    show("base", lib_base);
    show("base+1", lib_base + 1);
    show("base+64", lib_base + 64);
    char *a = sym("s_a");
    show("s_a", a);
    show("s_a+15", a + 15);
    show("s_a+16 (local gap)", a + 16);
    show("s_a+47 (local gap end)", a + 47);
    show("s_zero", a + 48);
    show("s_zero+8 (size 0)", a + 56);
    show("s_b (ties)", a + 64);
    show("s_b+8", a + 72);
    show("s_w1/s_w2 (weak first)", a + 80);
    show("s_nt (notype)", a + 96);
    show("ifres (IFUNC value)", a + 112);
    show("ifres+12", a + 124);
    show("s_nt+8 (in s_outer)", a + 104);
    show("s_impl+4 (in s_outer)", a + 132);
    show("s_impl", a + 128);
    show("s_tail", a + 144);
    show("s_tail+15", a + 159);
    show("s_tail+16", a + 160);
    snprintf(b, sizeof b, "dlsym s_if == s_impl %d, dlsym s_impl == base+%ld", sym("s_if") == sym("s_impl"),
             (long)((char *)sym("s_impl") - lib_base));
    line(b);
    show("s_obj", sym("s_obj"));
    show("s_obj+4", (char *)sym("s_obj") + 4);
    show("s_obj+8", (char *)sym("s_obj") + 8);
    uintptr_t lo, hi;
    if (section(libpath, (uintptr_t)lib_base, ".plt", &lo, &hi) == 0) {
        show(".plt start", (void *)lo);
        show(".plt last byte", (void *)(hi - 1));
    }
    if (section(libpath, (uintptr_t)lib_base, ".plt.sec", &lo, &hi) == 0)
        show(".plt.sec start", (void *)lo);
    if (section(libpath, (uintptr_t)lib_base, ".dynsym", &lo, &hi) == 0)
        show(".dynsym", (void *)lo);
    if (section(libpath, (uintptr_t)lib_base, ".eh_frame", &lo, &hi) == 0)
        show(".eh_frame", (void *)lo);
    if (section(libpath, (uintptr_t)lib_base, ".bss", &lo, &hi) == 0)
        show(".bss start", (void *)lo);
    struct link_map *lm = lib_handle;
    snprintf(b, sizeof b, "link_map l_addr == fbase %d", (char *)lm->l_addr == lib_base);
    line(b);
    static const char *const ties[] = {"s_a", "s_zero", "s_b", "s_b_alias", "s_b_weak", "s_w1", "s_w2", "s_nt", "s_if", "s_impl", "s_tls", "s_outer"};
    dynsym_order(libpath, ties, (int)(sizeof ties / sizeof *ties));

    line("== dladdr1");
    const ElfW(Sym) *se = (void *)1;
    int r = dladdr1(a + 4, &i0, (void **)&se, RTLD_DL_SYMENT);
    snprintf(b, sizeof b, "dladdr1 SYMENT s_a+4 ret=%d sym=%s value=base+%lu size=%lu info=0x%x", r,
             se ? "non-NULL" : "NULL", se ? (unsigned long)se->st_value : 0ul, se ? (unsigned long)se->st_size : 0ul,
             se ? se->st_info : 0);
    line(b);
    se = (void *)1;
    r = dladdr1(lib_base + 1, &i0, (void **)&se, RTLD_DL_SYMENT);
    snprintf(b, sizeof b, "dladdr1 SYMENT base+1 ret=%d sym=%s sname=%s", r, se == (void *)1 ? "untouched" : se ? "non-NULL" : "NULL",
             i0.dli_sname ? i0.dli_sname : "(null)");
    line(b);
    struct link_map *m = (void *)1;
    r = dladdr1(a, &i0, (void **)&m, RTLD_DL_LINKMAP);
    snprintf(b, sizeof b, "dladdr1 LINKMAP s_a ret=%d map==handle %d", r, (void *)m == lib_handle);
    line(b);
    m = (void *)1;
    r = dladdr1((void *)main, &i0, (void **)&m, RTLD_DL_LINKMAP);
    snprintf(b, sizeof b, "dladdr1 LINKMAP main ret=%d map==dlopen(NULL) %d", r, (void *)m == dlopen(NULL, RTLD_NOW));
    line(b);
    for (int fl = 0; fl <= 3; fl += 3) {
        void *x = (void *)1;
        r = dladdr1(a, &i0, &x, fl);
        snprintf(b, sizeof b, "dladdr1 flags=%d ret=%d extra=%s", fl, r, x == (void *)1 ? "untouched" : x ? "non-NULL" : "NULL");
        line(b);
    }
    void *x = (void *)1;
    r = dladdr1((void *)8, &i0, &x, RTLD_DL_LINKMAP);
    snprintf(b, sizeof b, "dladdr1 LINKMAP unmapped ret=%d extra=%s", r, x == (void *)1 ? "untouched" : x ? "non-NULL" : "NULL");
    line(b);
    snprintf(b, sizeof b, "dlerror after dladdr miss=[%s]", dlerror() ? "text" : "NULL");
    line(b);

    line("== dladdr elsewhere");
    show("main (program)", (void *)main);
    show("probe_exported (program)", dlsym(RTLD_DEFAULT, "probe_exported"));
    show("printf (libc)", (void *)printf);
    show("dladdr (libc)", (void *)dladdr);
    show("__tls_get_addr (ld.so)", dlsym(RTLD_DEFAULT, "__tls_get_addr"));
    show("_dl_find_object", dlsym(RTLD_DEFAULT, "_dl_find_object"));
    uintptr_t vd = getauxval(AT_SYSINFO_EHDR);
    if (vd) {
        show("vDSO base", (void *)vd);
        void *vh = dlopen("linux-vdso.so.1", RTLD_NOW | RTLD_NOLOAD);
        void *vc = vh ? dlsym(vh, "__vdso_clock_gettime") : NULL;
        snprintf(b, sizeof b, "vDSO handle %s, __vdso_clock_gettime %s", vh ? "found" : "NULL", vc ? "found" : "NULL");
        line(b);
        if (vc) {
            show("vDSO clock_gettime", vc);
            show("vDSO clock_gettime+1", (char *)vc + 1);
        }
    }
    show("NULL", NULL);
    show("8", (void *)8);
    show("-1", (void *)-1);
    int on_stack;
    show("stack", &on_stack);
    show("heap", malloc(64));

    line("== dlinfo");
    void *prog = dlopen(NULL, RTLD_NOW);
    for (int q = 0; q <= 12; q++)
        if (q != RTLD_DI_SERINFO && q != RTLD_DI_SERINFOSIZE) /* their buffer is the caller's */
            info_req("lib", lib_handle, q);
    for (int q = 1; q <= 11; q += 1)
        if (q != RTLD_DI_SERINFO && q != RTLD_DI_SERINFOSIZE && q != RTLD_DI_ORIGIN)
            info_req("program", prog, q);
    long lmid = 77;
    r = dlinfo(lib_handle, RTLD_DI_LMID, &lmid);
    snprintf(b, sizeof b, "dlinfo LMID ret=%d lmid=%ld", r, lmid);
    line(b);
    struct link_map *lk = NULL;
    r = dlinfo(lib_handle, RTLD_DI_LINKMAP, &lk);
    snprintf(b, sizeof b, "dlinfo LINKMAP ret=%d ==handle %d name=[%s]", r, (void *)lk == lib_handle, lk ? lk->l_name : "-");
    line(b);
    char o[4096];
    strcpy(o, "unset");
    r = dlinfo(lib_handle, RTLD_DI_ORIGIN, o);
    snprintf(b, sizeof b, "dlinfo ORIGIN lib ret=%d [%s]", r, o);
    line(b);
    pid_t op = fork(); /* the program's origin in a child */
    if (op == 0) {
        alarm(10);
        strcpy(o, "unset");
        r = dlinfo(prog, RTLD_DI_ORIGIN, o);
        snprintf(b, sizeof b, "dlinfo ORIGIN program ret=%d [%s]", r, o);
        line(b);
        _exit(0);
    }
    int ost;
    waitpid(op, &ost, 0);
    if (WIFSIGNALED(ost))
        line("dlinfo ORIGIN program: child ended by a signal");
    strcpy(o, "unset");
    r = dlinfo(dlopen("libc.so.6", RTLD_NOW), RTLD_DI_ORIGIN, o);
    snprintf(b, sizeof b, "dlinfo ORIGIN libc ret=%d [%s]", r, o);
    line(b);
    size_t modid = 99;
    r = dlinfo(lib_handle, RTLD_DI_TLS_MODID, &modid);
    snprintf(b, sizeof b, "dlinfo TLS_MODID lib ret=%d modid=%zu", r, modid);
    line(b);
    modid = 99;
    r = dlinfo(prog, RTLD_DI_TLS_MODID, &modid);
    snprintf(b, sizeof b, "dlinfo TLS_MODID program ret=%d modid=%zu", r, modid);
    line(b);
    void *td = (void *)1;
    r = dlinfo(lib_handle, RTLD_DI_TLS_DATA, &td);
    long *(*ta)(void) = (long *(*)(void))sym("lib_tls_addr");
    snprintf(b, sizeof b, "dlinfo TLS_DATA main thread ret=%d data==&s_tls %d data=%s", r, td == (void *)ta(), td ? "non-NULL" : "NULL");
    line(b);
    pthread_t t;
    pthread_create(&t, NULL, tls_thread, NULL);
    pthread_join(t, NULL);
    const ElfW(Phdr) *ph = NULL;
    r = dlinfo(lib_handle, RTLD_DI_PHDR, &ph);
    snprintf(b, sizeof b, "dlinfo PHDR ret=%d phdr==base+64 %d", r, (char *)ph == lib_base + 64);
    line(b);
    serinfo("lib", lib_handle);
    serinfo("program", prog);
    for (int k = 3; k < argc; k++) {
        void *h2 = dlopen(argv[k], RTLD_NOW);
        snprintf(b, sizeof b, "== extra object %s", argv[k]);
        line(b);
        if (!h2) {
            line(dlerror());
            continue;
        }
        info_req("extra", h2, RTLD_DI_TLS_MODID);
        serinfo("extra", h2);
        strcpy(o, "unset");
        r = dlinfo(h2, RTLD_DI_ORIGIN, o);
        snprintf(b, sizeof b, "dlinfo ORIGIN extra ret=%d [%s]", r, o);
        line(b);
        Dl_info di;
        void *f = dlsym(h2, "s_a");
        if (f && dladdr(f, &di)) {
            snprintf(b, sizeof b, "dladdr extra fname=[%s]", di.dli_fname);
            line(b);
        }
    }
    line("== special handles (forked children)");
    void *hs[2] = {RTLD_DEFAULT, RTLD_NEXT};
    for (int k = 0; k < 2; k++) {
        pid_t p = fork();
        if (p == 0) {
            alarm(10);
            info_req(k ? "RTLD_NEXT" : "RTLD_DEFAULT", hs[k], RTLD_DI_LMID);
            _exit(0);
        }
        int st;
        waitpid(p, &st, 0);
        snprintf(b, sizeof b, "  child %s: %s %d", k ? "RTLD_NEXT" : "RTLD_DEFAULT", WIFSIGNALED(st) ? "signal" : "exit",
                 WIFSIGNALED(st) ? WTERMSIG(st) : WEXITSTATUS(st));
        line(b);
    }
    return 0;
}

int probe_exported(void);
int probe_exported(void) { return 3; }
