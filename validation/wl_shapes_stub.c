/* WL-STAKES: LD_PRELOAD observer of the ARGUMENT SHAPES of the calls a PROGRAM (and its other shared libraries)
 * makes into the mem, str and malloc family of the C library. Companion of wl_entries_stub.c (which counts every
 * entry point with signature-agnostic trampolines); this one needs the arguments, so it has one typed wrapper per
 * function. Nothing inside the C library is observed: only calls that bind to these exported names from outside
 * it. The wrapper measures the shape with its own byte loops (never through a wrapped function), then calls the
 * real function (dlsym(RTLD_NEXT)) with the original arguments and returns its result unchanged.
 *
 * Shape key per call: symbol, size bucket, aux bucket, flag (bucket = exact 0..64, then powers of two):
 *   memcpy mempcpy memmove memset (+ __*_chk)   size = n
 *   memcmp bcmp __memcmpeq                       size = n, aux = first-difference index, flag 1 = equal (aux = n)
 *   strcmp                                       size = bytes examined (decision index + 1), flag 1 = equal
 *   strncmp                                      size = n, aux = decision index (difference or NUL), flag 1 = equal
 *   strlen                                       size = result
 *   strchr strrchr                               size = bytes scanned (strlen + 1 for strrchr / not found), flag 1 = found
 *   memchr                                       size = n, aux = match index, flag 1 = found
 *   strcpy stpcpy (+ __*_chk), strdup            size = strlen(src)
 *   strncpy (+ __strncpy_chk)                    size = n, aux = strnlen(src, n)
 *   malloc                                       size = n
 *   calloc                                       size = nmemb * size, aux = nmemb
 *   realloc                                      size = new n, aux = malloc_usable_size(old) (0: NULL), flag 1 = moved
 *   free                                         size = malloc_usable_size(p), flag 1 = NULL
 * Per key: calls, sum of sizes, sum of aux (the generator picks a representative size inside a bucket from them).
 *
 * Dump at ELF fini to $WLS_DIR/<program_invocation_short_name>.<pid>, lines
 *   sym<TAB>size_lo<TAB>aux_lo<TAB>flag<TAB>calls<TAB>sum_size<TAB>sum_aux
 * A fork child starts from zero (pthread_atfork); a process ending with _exit or a fatal signal writes nothing
 * (same limits as wl_entries). Calls made before the real functions are resolved (dlsym's own allocations) are
 * served from a static arena and not counted. The table is a fixed open-addressing hash; keys that do not fit
 * are counted in the "#overflow" line. Build: gcc -O2 -shared -fPIC -fno-builtin -fno-tree-loop-distribute-patterns. */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <fcntl.h>
#include <malloc.h>
#include <pthread.h>
#include <stddef.h>
#include <stdint.h>
#include <stdlib.h>
#include <unistd.h>

#define WLS_SYMS                                                                                              \
    X(0, memcpy) X(1, mempcpy) X(2, memmove) X(3, memset) X(4, memcmp) X(5, bcmp) X(6, __memcmpeq)              \
    X(7, strlen) X(8, strchr) X(9, strrchr) X(10, memchr) X(11, strcmp) X(12, strncmp) X(13, strcpy)            \
    X(14, stpcpy) X(15, strncpy) X(16, strdup) X(17, malloc) X(18, free) X(19, realloc) X(20, calloc)           \
    X(21, __memcpy_chk) X(22, __mempcpy_chk) X(23, __memmove_chk) X(24, __memset_chk) X(25, __strcpy_chk)       \
    X(26, __stpcpy_chk) X(27, __strncpy_chk)
#define WLS_N 28
/* The _chk variants are reported under the plain name (same work plus a bounds check). */
static const char *const wls_name[WLS_N] = {
    "memcpy", "mempcpy", "memmove", "memset", "memcmp", "bcmp", "__memcmpeq", "strlen", "strchr", "strrchr",
    "memchr", "strcmp", "strncmp", "strcpy", "stpcpy", "strncpy", "strdup", "malloc", "free", "realloc",
    "calloc", "memcpy", "mempcpy", "memmove", "memset", "strcpy", "stpcpy", "strncpy"};
static const char *const wls_sym[WLS_N] = {
#define X(i, n) [i] = #n,
    WLS_SYMS
#undef X
};

static void *wls_real[WLS_N];
static volatile int wls_state; /* 0 unresolved, 1 resolving, 2 resolved */

/* ---- static arena for allocations made while resolving (dlsym may allocate) ---- */
static _Alignas(16) unsigned char wls_arena[65536];
static size_t wls_arena_used;
static int in_arena(const void *p) {
    return (const unsigned char *)p >= wls_arena && (const unsigned char *)p < wls_arena + sizeof wls_arena;
}
static void *arena_alloc(size_t n) {
    size_t a = (n + 15) & ~(size_t)15;
    size_t o = __atomic_fetch_add(&wls_arena_used, a, __ATOMIC_RELAXED);
    if (o + a > sizeof wls_arena) return NULL;
    return wls_arena + o; /* static storage: already zero (calloc) */
}

static void wls_resolve(void) {
    int z = 0;
    /* Already resolving: a call from inside dlsym (its allocations). Return at once; the allocator wrappers
     * fall back to the arena. Resolution runs in the constructor, before the program can start threads. */
    if (!__atomic_compare_exchange_n(&wls_state, &z, 1, 0, __ATOMIC_ACQ_REL, __ATOMIC_ACQUIRE)) return;
    for (int i = 0; i < WLS_N; i++) wls_real[i] = dlsym(RTLD_NEXT, wls_sym[i]);
    __atomic_store_n(&wls_state, 2, __ATOMIC_RELEASE);
}
#define REAL(i, T) ((T)(wls_state == 2 ? wls_real[i] : (wls_resolve(), wls_real[i])))

/* ---- shape table ---- */
#define TB 8192
struct ent { unsigned long key, calls, ssize, saux; };
static struct ent wls_tab[TB];
static unsigned long wls_overflow;

static unsigned bucket(size_t n) {
    if (n <= 64) return (unsigned)n;
    return 58u + (unsigned)(64 - __builtin_clzl((unsigned long)(n - 1))); /* 65..128 -> 65, 129..256 -> 66 ... */
}
static unsigned long bucket_lo(unsigned b) { return b <= 64 ? b : (1UL << (b - 58 - 1)) + 1; }

static void rec(int s, size_t size, size_t aux, int flag) {
    if (wls_state != 2) return;
    unsigned long key = 1 + (((unsigned long)s << 24) | ((unsigned long)bucket(size) << 16) |
                             ((unsigned long)bucket(aux) << 1) | (unsigned long)(flag & 1));
    unsigned long h = (key * 0x9E3779B97F4A7C15UL) >> 51;
    for (int probe = 0; probe < 64; probe++) {
        struct ent *e = &wls_tab[(h + probe) & (TB - 1)];
        unsigned long k = __atomic_load_n(&e->key, __ATOMIC_ACQUIRE);
        if (k == 0) {
            unsigned long z = 0;
            if (__atomic_compare_exchange_n(&e->key, &z, key, 0, __ATOMIC_ACQ_REL, __ATOMIC_ACQUIRE)) k = key;
            else k = z;
        }
        if (k == key) {
            __atomic_fetch_add(&e->calls, 1, __ATOMIC_RELAXED);
            __atomic_fetch_add(&e->ssize, size, __ATOMIC_RELAXED);
            __atomic_fetch_add(&e->saux, aux, __ATOMIC_RELAXED);
            return;
        }
    }
    __atomic_fetch_add(&wls_overflow, 1, __ATOMIC_RELAXED);
}

static size_t my_strlen(const char *s) { const volatile char *p = s; size_t n = 0; while (p[n]) n++; return n; }
static size_t my_strnlen(const char *s, size_t m) { const volatile char *p = s; size_t n = 0; while (n < m && p[n]) n++; return n; }
static size_t first_diff(const void *a, const void *b, size_t n) {
    const volatile unsigned char *x = a, *y = b;
    size_t i = 0;
    while (i < n && x[i] == y[i]) i++;
    return i;
}
static size_t usable(void *p) { return (p == NULL || in_arena(p)) ? 0 : malloc_usable_size(p); }

/* ---- wrappers ---- */
typedef void *(*set_t)(void *, int, size_t);
typedef void *(*setchk_t)(void *, int, size_t, size_t);
typedef size_t (*len_t)(const char *);
typedef char *(*chr_t)(const char *, int);
typedef void *(*mchr_t)(const void *, int, size_t);
typedef int (*scmp_t)(const char *, const char *);
typedef int (*sncmp_t)(const char *, const char *, size_t);
typedef char *(*scpy_t)(char *, const char *);
typedef char *(*sncpy_t)(char *, const char *, size_t);
typedef char *(*sncpychk_t)(char *, const char *, size_t, size_t);
typedef char *(*sdup_t)(const char *);
typedef void *(*cpy_t)(void *, const void *, size_t);
typedef void *(*cpychk_t)(void *, const void *, size_t, size_t);
void *memcpy(void *d, const void *s, size_t n) { rec(0, n, 0, 0); return REAL(0, cpy_t)(d, s, n); }
void *mempcpy(void *d, const void *s, size_t n) { rec(1, n, 0, 0); return REAL(1, cpy_t)(d, s, n); }
void *memmove(void *d, const void *s, size_t n) { rec(2, n, 0, 0); return REAL(2, cpy_t)(d, s, n); }
void *memset(void *d, int c, size_t n) { rec(3, n, 0, 0); return REAL(3, set_t)(d, c, n); }
void *__memcpy_chk(void *d, const void *s, size_t n, size_t l) { rec(0, n, 0, 0); return REAL(21, cpychk_t)(d, s, n, l); }
void *__mempcpy_chk(void *d, const void *s, size_t n, size_t l) { rec(1, n, 0, 0); return REAL(22, cpychk_t)(d, s, n, l); }
void *__memmove_chk(void *d, const void *s, size_t n, size_t l) { rec(2, n, 0, 0); return REAL(23, cpychk_t)(d, s, n, l); }
void *__memset_chk(void *d, int c, size_t n, size_t l) {
    rec(3, n, 0, 0);
    return REAL(24, setchk_t)(d, c, n, l);
}

typedef int (*cmp_t)(const void *, const void *, size_t);
static int do_cmp(int s, const void *a, const void *b, size_t n) {
    if (wls_state == 2) { size_t i = first_diff(a, b, n); rec(s, n, i, i == n); }
    return REAL(s, cmp_t)(a, b, n);
}
int memcmp(const void *a, const void *b, size_t n) { return do_cmp(4, a, b, n); }
int bcmp(const void *a, const void *b, size_t n) { return do_cmp(5, a, b, n); }
int __memcmpeq(const void *a, const void *b, size_t n) { return do_cmp(6, a, b, n); }

size_t strlen(const char *s) { size_t r = REAL(7, len_t)(s); rec(7, r, 0, 0); return r; }
char *strchr(const char *s, int c) {
    char *r = REAL(8, chr_t)(s, c);
    if (wls_state == 2) rec(8, r ? (size_t)(r - s) + 1 : my_strlen(s) + 1, 0, r != NULL);
    return r;
}
char *strrchr(const char *s, int c) {
    char *r = REAL(9, chr_t)(s, c);
    if (wls_state == 2) rec(9, my_strlen(s) + 1, 0, r != NULL);
    return r;
}
void *memchr(const void *s, int c, size_t n) {
    void *r = REAL(10, mchr_t)(s, c, n);
    rec(10, n, r ? (size_t)((const char *)r - (const char *)s) : n, r != NULL);
    return r;
}
int strcmp(const char *a, const char *b) {
    if (wls_state == 2) {
        const volatile unsigned char *x = (const unsigned char *)a, *y = (const unsigned char *)b;
        size_t i = 0;
        while (x[i] == y[i] && x[i]) i++;
        rec(11, i + 1, 0, x[i] == y[i]);
    }
    return REAL(11, scmp_t)(a, b);
}
int strncmp(const char *a, const char *b, size_t n) {
    if (wls_state == 2) {
        const volatile unsigned char *x = (const unsigned char *)a, *y = (const unsigned char *)b;
        size_t i = 0;
        while (i < n && x[i] == y[i] && x[i]) i++;
        rec(12, n, i, i == n || x[i] == y[i]);
    }
    return REAL(12, sncmp_t)(a, b, n);
}
char *strcpy(char *d, const char *s) { if (wls_state == 2) rec(13, my_strlen(s), 0, 0); return REAL(13, scpy_t)(d, s); }
char *stpcpy(char *d, const char *s) { if (wls_state == 2) rec(14, my_strlen(s), 0, 0); return REAL(14, scpy_t)(d, s); }
char *strncpy(char *d, const char *s, size_t n) {
    if (wls_state == 2) rec(15, n, my_strnlen(s, n), 0);
    return REAL(15, sncpy_t)(d, s, n);
}
char *__strcpy_chk(char *d, const char *s, size_t l) {
    if (wls_state == 2) rec(13, my_strlen(s), 0, 0);
    return REAL(25, sncpy_t)(d, s, l);
}
char *__stpcpy_chk(char *d, const char *s, size_t l) {
    if (wls_state == 2) rec(14, my_strlen(s), 0, 0);
    return REAL(26, sncpy_t)(d, s, l);
}
char *__strncpy_chk(char *d, const char *s, size_t n, size_t l) {
    if (wls_state == 2) rec(15, n, my_strnlen(s, n), 0);
    return REAL(27, sncpychk_t)(d, s, n, l);
}
char *strdup(const char *s) { if (wls_state == 2) rec(16, my_strlen(s), 0, 0); return REAL(16, sdup_t)(s); }

void *malloc(size_t n) {
    if (wls_state != 2) { wls_resolve(); if (wls_state != 2) return arena_alloc(n); }
    rec(17, n, 0, 0);
    return ((void *(*)(size_t))wls_real[17])(n);
}
void *calloc(size_t m, size_t n) {
    if (wls_state != 2) { wls_resolve(); if (wls_state != 2) return arena_alloc(m * n); }
    rec(20, m * n, m, 0);
    return ((void *(*)(size_t, size_t))wls_real[20])(m, n);
}
void free(void *p) {
    if (p != NULL && in_arena(p)) return;
    if (wls_state != 2) { wls_resolve(); if (wls_state != 2) return; }
    rec(18, usable(p), 0, p == NULL);
    ((void (*)(void *))wls_real[18])(p);
}
void *realloc(void *p, size_t n) {
    if (wls_state != 2) { wls_resolve(); if (wls_state != 2) return arena_alloc(n); }
    if (p != NULL && in_arena(p)) { /* copy out of the arena; old size bounded by the arena's end */
        unsigned char *q = ((void *(*)(size_t))wls_real[17])(n);
        size_t m = (size_t)(wls_arena + sizeof wls_arena - (unsigned char *)p);
        if (q) for (size_t i = 0; i < n && i < m; i++) q[i] = ((unsigned char *)p)[i];
        return q;
    }
    size_t old = usable(p);
    void *r = ((void *(*)(void *, size_t))wls_real[19])(p, n);
    rec(19, n, old, r != p);
    return r;
}

/* ---- fork reset and dump ---- */
static void wls_child(void) {
    for (int i = 0; i < TB; i++)
        if (wls_tab[i].key) { wls_tab[i].key = 0; wls_tab[i].calls = wls_tab[i].ssize = wls_tab[i].saux = 0; }
    wls_overflow = 0;
}
__attribute__((constructor)) static void wls_init(void) {
    wls_resolve();
    pthread_atfork(NULL, NULL, wls_child);
}

static char *put(char *p, const char *s) { while (*s) *p++ = *s++; return p; }
static char *putu(char *p, unsigned long v) {
    char t[24]; int k = 0;
    do { t[k++] = (char)('0' + v % 10); v /= 10; } while (v);
    while (k) *p++ = t[--k];
    return p;
}
__attribute__((destructor)) static void wls_dump(void) {
    const char *dir = getenv("WLS_DIR");
    if (!dir) return;
    char path[4096], *p = path;
    if (my_strlen(dir) + 300 > sizeof path) return;
    p = put(p, dir); p = put(p, "/"); p = put(p, program_invocation_short_name); p = put(p, ".");
    p = putu(p, (unsigned long)getpid()); *p = 0;
    int fd = open(path, O_WRONLY | O_CREAT | O_TRUNC | O_CLOEXEC, 0644);
    if (fd < 0) return;
    char buf[256];
    for (int i = 0; i < TB; i++) {
        struct ent *e = &wls_tab[i];
        if (!e->key || !e->calls) continue;
        unsigned long k = e->key - 1;
        p = buf;
        p = put(p, wls_name[k >> 24]); *p++ = '\t';
        p = putu(p, bucket_lo((unsigned)((k >> 16) & 0xff))); *p++ = '\t';
        p = putu(p, bucket_lo((unsigned)((k >> 1) & 0x7fff))); *p++ = '\t';
        p = putu(p, k & 1); *p++ = '\t';
        p = putu(p, e->calls); *p++ = '\t';
        p = putu(p, e->ssize); *p++ = '\t';
        p = putu(p, e->saux); *p++ = '\n';
        if (write(fd, buf, (size_t)(p - buf)) < 0) break;
    }
    if (wls_overflow) {
        p = put(buf, "#overflow\t0\t0\t0\t"); p = putu(p, wls_overflow); p = put(p, "\t0\t0\n");
        if (write(fd, buf, (size_t)(p - buf)) < 0) { }
    }
    close(fd);
}
