/* Spec §27.1 (s27_1_c_6 dladdr, dladdr1): an address mapped to its object and symbol, as the
 * reference answers (the dladdr probe, tools/probe/dl/dladdr/; spec/behaviour/dynlink.md "dladdr
 * and dlinfo"). Against the helper dla_syms (a fixed symbol layout, opened by dlopen):
 *   - the object: dli_fname is the path it was opened by, dli_fbase its load bias (the address
 *     of s_a less its st_value in the file; the program's from dl_iterate_phdr), the link map's
 *     l_addr;
 *   - the symbol: only a dynamic symbol that HOLDS the address (st_value <= a < st_value +
 *     st_size, or a == st_value for size 0); none (a local-code gap, past a size-0 symbol, the ELF
 *     header): ret 1 with dli_sname and dli_saddr NULL; overlapping holders: the highest start;
 *     equal starts: the lowest .dynsym index (read from the file); an IFUNC by its resolver;
 *   - dladdr1: RTLD_DL_SYMENT the Elf64_Sym (NULL with no holder), RTLD_DL_LINKMAP the handle
 *     (the program's: dlopen(NULL)); other flags leave *extra alone;
 *   - an address no object holds (NULL, -1, the stack, the heap): 0, *info and *extra untouched;
 *   - the program (-rdynamic): dli_fname is argv[0], main found; libc found;
 *   - a backtrace: return addresses through the helper's dla_call map to dla_call and to main.
 * Each failed check prints its line; the exit status is the number of failed checks. */
/* DLOPEN-HELPERS: dla_syms */
/* LINK-EXPORT-DYNAMIC: yes */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <elf.h>
#include <link.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static int fails;
#define CHECK(c) do { if (!(c)) { fprintf(stderr, "s27_dladdr: line %d: %s\n", __LINE__, #c); fails++; } } while (0)

/* The page of the lowest PT_LOAD address of the object (dl_iterate_phdr) holding the address. */
struct lb { uintptr_t a, base; };
static int lb_cb(struct dl_phdr_info *in, size_t sz, void *p)
{
    (void)sz;
    struct lb *q = p;
    uintptr_t lo = UINTPTR_MAX, hi = 0;
    for (int k = 0; k < in->dlpi_phnum; k++) {
        const ElfW(Phdr) *ph = &in->dlpi_phdr[k];
        if (ph->p_type != PT_LOAD)
            continue;
        uintptr_t s = in->dlpi_addr + ph->p_vaddr, e = s + ph->p_memsz;
        if (s < lo) lo = s;
        if (e > hi) hi = e;
    }
    if (lo <= q->a && q->a < hi) {
        q->base = lo & ~(uintptr_t)(sysconf(_SC_PAGESIZE) - 1);
        return 1;
    }
    return 0;
}
static uintptr_t load_base(const void *a)
{
    struct lb q = {(uintptr_t)a, 0};
    dl_iterate_phdr(lb_cb, &q);
    return q.base;
}

/* The .dynsym index of NAME in FILE (section headers), -1 if absent; its st_value in *VALUE. */
static long dynsym_index(const char *file, const char *name, uint64_t *value)
{
    FILE *f = fopen(file, "rb");
    long r = -1;
    if (f == NULL)
        return -1;
    Elf64_Ehdr eh;
    if (fread(&eh, sizeof eh, 1, f) == 1) {
        Elf64_Shdr sh[64];
        int n = eh.e_shnum < 64 ? eh.e_shnum : 64;
        if (fseek(f, (long)eh.e_shoff, SEEK_SET) == 0 && fread(sh, sizeof sh[0], (size_t)n, f) == (size_t)n) {
            for (int s = 0; s < n && r < 0; s++) {
                if (sh[s].sh_type != SHT_DYNSYM || sh[s].sh_link >= (unsigned)n)
                    continue;
                const Elf64_Shdr *st = &sh[sh[s].sh_link];
                char *str = malloc(st->sh_size);
                Elf64_Sym *sy = malloc(sh[s].sh_size);
                if (str && sy && fseek(f, (long)st->sh_offset, SEEK_SET) == 0 && fread(str, 1, st->sh_size, f) == st->sh_size &&
                    fseek(f, (long)sh[s].sh_offset, SEEK_SET) == 0 && fread(sy, 1, sh[s].sh_size, f) == sh[s].sh_size) {
                    for (size_t k = 0; k < sh[s].sh_size / sizeof *sy; k++)
                        if (sy[k].st_name < st->sh_size && strcmp(str + sy[k].st_name, name) == 0) {
                            r = (long)k;
                            *value = sy[k].st_value;
                            break;
                        }
                }
                free(str);
                free(sy);
            }
        }
    }
    fclose(f);
    return r;
}

/* Of the names (all at one address), the one with the lowest .dynsym index. */
static const char *lowest(const char *file, const char *const *names, int n)
{
    const char *best = NULL;
    long bi = -1;
    for (int k = 0; k < n; k++) {
        uint64_t v;
        long i = dynsym_index(file, names[k], &v);
        if (i >= 0 && (bi < 0 || i < bi)) {
            bi = i;
            best = names[k];
        }
    }
    return best;
}

static const char *path;  /* the helper's path */
static void *h;           /* its handle */
static uintptr_t base;    /* its load base */

/* dladdr(A) finds the helper, symbol WANT (NULL: none) starting at SADDR. */
static int at(const char *what, const void *a, const char *want, const void *saddr)
{
    Dl_info d;
    memset(&d, 0x5a, sizeof d);
    int r = dladdr(a, &d);
    int ok = r == 1 && d.dli_fname != NULL && strcmp(d.dli_fname, path) == 0 && (uintptr_t)d.dli_fbase == base &&
             (want == NULL ? d.dli_sname == NULL && d.dli_saddr == NULL
                           : d.dli_sname != NULL && strcmp(d.dli_sname, want) == 0 && d.dli_saddr == saddr);
    if (!ok)
        fprintf(stderr, "s27_dladdr: %s: ret %d fname [%s] fbase %s sname [%s] saddr %s, want [%s]\n", what, r,
                r && d.dli_fname ? d.dli_fname : "-", (uintptr_t)d.dli_fbase == base ? "ok" : "wrong",
                r && d.dli_sname ? d.dli_sname : "(null)", d.dli_saddr == saddr ? "ok" : "wrong", want ? want : "(null)");
    return ok;
}

/* No object holds A: 0, with *info (and *extra) untouched. */
static int miss(const void *a)
{
    Dl_info d, z;
    void *extra = (void *)0x5a5a;
    memset(&d, 0x5a, sizeof d);
    memcpy(&z, &d, sizeof d);
    int r = dladdr(a, &d);
    int r1 = dladdr1(a, &d, &extra, RTLD_DL_LINKMAP);
    return r == 0 && r1 == 0 && memcmp(&d, &z, sizeof d) == 0 && extra == (void *)0x5a5a;
}

__attribute__((noinline)) static void *callback(void) { return __builtin_return_address(0); }
extern int main(int, char **);

int main(int argc, char **argv)
{
    const char *dir = getenv("CONF_HELPERS");
    if (argc < 1 || dir == NULL)
        return 100;
    char p[4096];
    snprintf(p, sizeof p, "%s/libdla_syms.so", dir);
    path = p;
    h = dlopen(p, RTLD_NOW);
    if (h == NULL) {
        fprintf(stderr, "s27_dladdr: dlopen %s: %s\n", p, dlerror());
        return 100;
    }
#define S(n) ((char *)dlsym(h, n))
    char *s_a = S("s_a"), *s_zero = S("s_zero"), *s_b = S("s_b"), *s_w1 = S("s_w1"), *s_nt = S("s_nt");
    char *s_outer = S("s_outer"), *s_impl = S("s_impl"), *s_tail = S("s_tail"), *s_obj = S("s_obj");
    CHECK(s_a && s_zero && s_b && s_w1 && s_nt && s_outer && s_impl && s_tail && s_obj);
    if (fails)
        return fails;
    uint64_t v_a = 0;
    CHECK(dynsym_index(p, "s_a", &v_a) > 0 && v_a != 0);
    base = (uintptr_t)s_a - v_a; /* the load bias; the first PT_LOAD maps offset 0 at address 0 */
    CHECK(memcmp((void *)base, ELFMAG, SELFMAG) == 0);
    struct link_map *lm = NULL;
    CHECK(dlinfo(h, RTLD_DI_LINKMAP, &lm) == 0 && lm != NULL && lm->l_addr == base);

    /* The object without a symbol: its ELF header. */
    CHECK(at("base", (void *)base, NULL, NULL));
    CHECK(at("base+64", (void *)(base + 64), NULL, NULL));
    /* Symbols that hold the address, and the gaps between them. */
    CHECK(at("s_a", s_a, "s_a", s_a));
    CHECK(at("s_a+15", s_a + 15, "s_a", s_a));
    CHECK(at("s_a+16 (local code)", s_a + 16, NULL, NULL));
    CHECK(at("s_a+47 (local code)", s_a + 47, NULL, NULL));
    CHECK(at("s_zero", s_zero, "s_zero", s_zero));
    CHECK(at("s_zero+8 (size 0)", s_zero + 8, NULL, NULL));
    static const char *const b3[] = {"s_b", "s_b_alias", "s_b_weak"}, *const w2[] = {"s_w1", "s_w2"};
    const char *bwant = lowest(p, b3, 3), *wwant = lowest(p, w2, 2);
    CHECK(bwant != NULL && wwant != NULL);
    CHECK(at("s_b (equal starts)", s_b, bwant, s_b));
    CHECK(at("s_b+8 (equal starts)", s_b + 8, bwant, s_b));
    CHECK(at("s_w1/s_w2 (equal starts, weak and global)", s_w1 + 3, wwant, s_w1));
    CHECK(at("s_nt (no type)", s_nt, "s_nt", s_nt));
    CHECK(s_outer == s_nt + 4);
    CHECK(at("s_nt+8 (s_nt and s_outer hold it)", s_nt + 8, "s_outer", s_outer));
    CHECK(at("ifres (the IFUNC's resolver)", s_impl - 16, "s_if", s_impl - 16));
    CHECK(at("ifres+12", s_impl - 4, "s_if", s_impl - 16));
    CHECK((char *)dlsym(h, "s_if") == s_impl);
    CHECK(at("s_impl", s_impl, "s_impl", s_impl));
    CHECK(at("s_impl+4 (s_outer and s_impl hold it)", s_impl + 4, "s_impl", s_impl));
    CHECK(at("s_tail+15", s_tail + 15, "s_tail", s_tail));
    CHECK(at("s_obj", s_obj, "s_obj", s_obj));
    CHECK(at("s_obj+4", s_obj + 4, "s_obj", s_obj));

    /* dladdr1. */
    Dl_info d;
    const ElfW(Sym) *sym = (const void *)1;
    CHECK(dladdr1(s_a + 4, &d, (void **)&sym, RTLD_DL_SYMENT) == 1 && sym != NULL && sym != (const void *)1 &&
          sym->st_size == 16 && ELF64_ST_TYPE(sym->st_info) == STT_FUNC && (char *)base + sym->st_value == s_a &&
          d.dli_saddr == s_a);
    CHECK(dladdr1(s_impl - 16, &d, (void **)&sym, RTLD_DL_SYMENT) == 1 && sym != NULL &&
          ELF64_ST_TYPE(sym->st_info) == STT_GNU_IFUNC && (char *)base + sym->st_value == s_impl - 16);
    sym = (const void *)1;
    CHECK(dladdr1((void *)(base + 1), &d, (void **)&sym, RTLD_DL_SYMENT) == 1 && sym == NULL && d.dli_sname == NULL);
    void *map = NULL;
    CHECK(dladdr1(s_tail, &d, &map, RTLD_DL_LINKMAP) == 1 && map == h && map == (void *)lm);
    void *g = dlopen(NULL, RTLD_LAZY);
    map = NULL;
    CHECK(dladdr1((void *)main, &d, &map, RTLD_DL_LINKMAP) == 1 && map == g);
    void *extra = (void *)0x5a5a;
    CHECK(dladdr1(s_a, &d, &extra, 0) == 1 && extra == (void *)0x5a5a && strcmp(d.dli_sname, "s_a") == 0);
    CHECK(dladdr1(s_a, &d, &extra, 3) == 1 && extra == (void *)0x5a5a);

    /* Addresses no object holds. */
    int local = 0;
    void *heap = malloc(64);
    CHECK(miss(NULL));
    CHECK(miss((void *)8));
    CHECK(miss((void *)-1));
    CHECK(miss(&local));
    CHECK(miss(heap));
    free(heap);
    CHECK(dlerror() == NULL);

    /* The program and the C library. */
    CHECK(dladdr((void *)main, &d) == 1 && d.dli_fname && strcmp(d.dli_fname, argv[0]) == 0 &&
          (uintptr_t)d.dli_fbase == load_base((void *)main) && d.dli_sname && strcmp(d.dli_sname, "main") == 0 &&
          d.dli_saddr == (void *)main);
    void *pf = dlsym(RTLD_DEFAULT, "printf");
    CHECK(pf != NULL && dladdr(pf, &d) == 1 && d.dli_fname && strstr(d.dli_fname, "libc") != NULL &&
          memcmp(d.dli_fbase, ELFMAG, SELFMAG) == 0 && (char *)d.dli_fbase < (char *)pf && d.dli_saddr == pf && d.dli_sname != NULL &&
          dlsym(RTLD_DEFAULT, d.dli_sname) == pf);

    /* A backtrace through the helper: callback's return address is in dla_call, dla_call's in main. */
    void *(*call)(void *(*)(void)) = (void *(*)(void *(*)(void)))dlsym(h, "dla_call");
    void **last = dlsym(h, "dla_last_return");
    CHECK(call != NULL && last != NULL);
    if (call && last) {
        void *r = call(callback);
        CHECK(at("callback's return address", r, "dla_call", (void *)call));
        CHECK((char *)r > (char *)call);
        CHECK(dladdr(*last, &d) == 1 && d.dli_fname && strcmp(d.dli_fname, argv[0]) == 0 && d.dli_sname &&
              strcmp(d.dli_sname, "main") == 0 && d.dli_saddr == (void *)main);
    }
    return fails;
}
