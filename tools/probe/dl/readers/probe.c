/* probe.c - DL-16c black-box probe of the reference's lock-free readers in a dynamically linked
 * program (plan/ph14/dl-16-dlopen.md section 4): what _dl_find_object reports for a pc in the
 * program, in libc and in a dlopen'd object (the PT_LOAD segment or the whole object; the link
 * map), and what a dl_iterate_phdr callback that calls dlopen sees (does it return; does the
 * running walk visit the new object; dlpi_adds within the walk). Public interfaces only.
 *   probe DIR    (DIR holds libR1.so and libR2.so) */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <link.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

static int in_main(void) { return 1; }
static int main_data = 5;

struct span { unsigned long lo, hi, seglo, seghi; };

/* The whole range and the containing PT_LOAD of the object holding pc, from dl_iterate_phdr. */
static int span_cb(struct dl_phdr_info *i, size_t s, void *d)
{
    (void)s;
    struct span *sp = d;
    unsigned long pc = sp->lo, lo = ~0ul, hi = 0, sl = 0, sh = 0;
    for (int k = 0; k < i->dlpi_phnum; k++) {
        const ElfW(Phdr) *p = &i->dlpi_phdr[k];
        if (p->p_type != PT_LOAD)
            continue;
        unsigned long a = i->dlpi_addr + p->p_vaddr, b = a + p->p_memsz;
        if (a < lo) lo = a;
        if (b > hi) hi = b;
        if (pc >= a && pc < b) { sl = a; sh = b; }
    }
    if (!sl)
        return 0;
    sp->lo = lo; sp->hi = hi; sp->seglo = sl; sp->seghi = sh;
    return 1;
}

static void find(const char *what, void *pc, void *handle)
{
    struct dl_find_object fo;
    memset(&fo, 0, sizeof fo);
    int r = _dl_find_object(pc, &fo);
    struct span sp = { (unsigned long)pc, 0, 0, 0 };
    dl_iterate_phdr(span_cb, &sp);
    const char *range = "other";
    unsigned long s = (unsigned long)fo.dlfo_map_start, e = (unsigned long)fo.dlfo_map_end;
    if (s == sp.seglo && e == sp.seghi)
        range = "segment";
    else if (s == sp.lo && e == sp.hi)
        range = "whole PT_LOAD span";
    else if (s == sp.lo && e >= sp.hi && e - sp.hi < 0x10000)
        range = "whole span rounded up";
    printf("R-1 %s: rc %d, range %s, flags %llu, eh_frame %s, link_map %s\n", what, r, r ? "-" : range,
           (unsigned long long)fo.dlfo_flags, fo.dlfo_eh_frame ? "set" : "0",
           handle ? (fo.dlfo_link_map == handle ? "== handle" : "!= handle") : (fo.dlfo_link_map ? "non-null" : "null"));
}

static const char *dir;
static int walk_n, opened, saw_new;
static unsigned long long first_adds, last_adds;

static int open_cb(struct dl_phdr_info *i, size_t s, void *d)
{
    (void)s; (void)d;
    if (walk_n == 0)
        first_adds = i->dlpi_adds;
    last_adds = i->dlpi_adds;
    if (strstr(i->dlpi_name, "libR2"))
        saw_new = 1;
    if (walk_n++ == 0) {
        char p[4096];
        snprintf(p, sizeof p, "%s/libR2.so", dir);
        opened = dlopen(p, RTLD_NOW) != NULL;
    }
    return 0;
}

static int name_cb(struct dl_phdr_info *i, size_t s, void *d)
{
    (void)s; (void)d;
    const char *n = i->dlpi_name;
    const char *b = strrchr(n, '/');
    printf("R-3 object %s name %s%s adds %llu subs %llu tls_modid %zu\n", b ? b + 1 : (*n ? n : "\"\""),
           *n == '/' ? "absolute" : (*n ? "as given" : "empty"), "", (unsigned long long)i->dlpi_adds,
           (unsigned long long)i->dlpi_subs, i->dlpi_tls_modid);
    return 0;
}

int main(int argc, char **argv)
{
    if (argc < 2)
        return 2;
    dir = argv[1];
    alarm(30);
    char p[4096];
    snprintf(p, sizeof p, "%s/libR1.so", dir);
    void *h1 = dlopen(p, RTLD_NOW);
    if (!h1) { printf("dlopen R1 failed: %s\n", dlerror()); return 1; }
    find("program text", (void *)in_main, dlopen(NULL, RTLD_NOW));
    find("program data", (void *)&main_data, dlopen(NULL, RTLD_NOW));
    find("libc text", (void *)puts, NULL);
    find("dlopen'd text", dlsym(h1, "fn_R1"), h1);
    struct dl_find_object fo;
    printf("R-4 stack pc: rc %d\n", _dl_find_object((void *)&fo, &fo));
    dl_iterate_phdr(name_cb, NULL);
    dl_iterate_phdr(open_cb, NULL);
    printf("R-2 dlopen in a callback: returned %d, the running walk visits it %d, adds first %llu last %llu, objects walked %d\n",
           opened, saw_new, first_adds, last_adds, walk_n);
    walk_n = 0; saw_new = 0;
    dl_iterate_phdr(open_cb, NULL);
    printf("R-2 next walk: visits it %d, objects walked %d, adds %llu\n", saw_new, walk_n, last_adds);
    return 0;
}
