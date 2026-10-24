// dl_iterate_phdr in a static program (D-27.dlphdr): the main program, then the vDSO; fields,
// early stop. (stime@GLIBC_2.2.5 is not here: a static program cannot bind a compat version,
// as with glibc's libc.a (R15-2); its logic is tested in-process in s23_time_symbols.)
#define _GNU_SOURCE
#include <errno.h>
#include <link.h>
#include <stdio.h>
#include <string.h>
#include <sys/auxv.h>
#include <time.h>

static __thread int tl = 5;

struct seen {
    int n;
    int bad;
    char names[4][32];
    unsigned long long adds[4], subs[4];
    size_t sizes[4];
};

#define CHECK(c) do { if (!(c)) { printf("FAIL line %d: %s\n", __LINE__, #c); return 1; } } while (0)

static int cb(struct dl_phdr_info *i, size_t sz, void *d) {
    struct seen *s = d;
    if (s->n < 4) {
        strncpy(s->names[s->n], i->dlpi_name ? i->dlpi_name : "(null)", 31);
        s->adds[s->n] = i->dlpi_adds;
        s->subs[s->n] = i->dlpi_subs;
        s->sizes[s->n] = sz;
        if (s->n == 0) {
            unsigned long phdr = getauxval(AT_PHDR);
            int tls = -1;
            if ((unsigned long)i->dlpi_phdr != phdr || i->dlpi_phnum != getauxval(AT_PHNUM)) s->bad |= 1;
            for (int k = 0; k < i->dlpi_phnum; k++)
                if (i->dlpi_phdr[k].p_type == PT_TLS) tls = k;
            if (tls < 0 || i->dlpi_tls_modid != 1 || i->dlpi_tls_data == 0) s->bad |= 2;
            else {
                char *lo = i->dlpi_tls_data, *p = (char *)&tl;
                if (p < lo || p >= lo + i->dlpi_phdr[tls].p_memsz) s->bad |= 4;
            }
            if (i->dlpi_addr != 0) s->bad |= 8; // non-PIE static: no load bias
        } else if (i->dlpi_tls_modid != 0) {
            s->bad |= 16;
        }
    }
    s->n++;
    return 0;
}

static int stop_first(struct dl_phdr_info *i, size_t sz, void *d) {
    (void)i; (void)sz;
    int *n = d;
    (*n)++;
    return 42;
}

static int stop_second(struct dl_phdr_info *i, size_t sz, void *d) {
    (void)i; (void)sz;
    int *n = d;
    (*n)++;
    return *n == 2 ? -7 : 0;
}

int main(void) {
    struct seen s;
    memset(&s, 0, sizeof s);
    tl++;
    int expect = getauxval(AT_SYSINFO_EHDR) ? 2 : 1;
    int r = dl_iterate_phdr(cb, &s);
    CHECK(r == 0);
    CHECK(s.n == expect);
    CHECK(s.bad == 0);
    CHECK(s.names[0][0] == '\0');
    if (expect == 2) CHECK(strcmp(s.names[1], "linux-vdso.so.1") == 0);
    for (int k = 0; k < s.n; k++) {
        CHECK(s.sizes[k] == sizeof(struct dl_phdr_info));
        CHECK(s.adds[k] == (unsigned long long)expect);
        CHECK(s.subs[k] == 0);
    }
    int n = 0;
    CHECK(dl_iterate_phdr(stop_first, &n) == 42);
    CHECK(n == 1);
    n = 0;
    int want = expect == 2 ? -7 : 0;
    CHECK(dl_iterate_phdr(stop_second, &n) == want);
    CHECK(n == expect);
    puts("ok");
    return 0;
}
