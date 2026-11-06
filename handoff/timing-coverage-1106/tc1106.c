/* wl_percall spec (TIMING-COVERAGE-1106): Liella instruction profile of the uncovered sweep rows.
 * Shapes follow bench/c/cbench_memref.c, cbench_hot.c, cbench_locale.c and cbench_malloc.c rows of the same name. */
#include "percall.h"
#include <strings.h>

typedef int (*cmp_fn)(const void *, const void *, size_t);
extern int __memcmpeq(const void *, const void *, size_t);
static cmp_fn volatile f_memcmp = memcmp, f_bcmp = bcmp, f_meq = __memcmpeq;
static void *(*volatile f_memchr)(const void *, int, size_t) = memchr;
static void *(*volatile f_memrchr)(const void *, int, size_t) = memrchr;
static char *(*volatile f_strstr)(const char *, const char *) = strstr;
static int (*volatile f_casecmp)(const char *, const char *) = strcasecmp;
static int (*volatile f_ncasecmp)(const char *, const char *, size_t) = strncasecmp;
static void *(*volatile f_realloc)(void *, size_t) = realloc;
static unsigned char A[4160] __attribute__((aligned(64))), B[4160] __attribute__((aligned(64)));
static char H[8192] __attribute__((aligned(4096)));

/* kind: 0 equal, 1 first byte differs, 2 last byte differs, 3 byte 8 differs */
static void cmp_run(cmp_fn volatile *fp, size_t len, int kind, long n) {
    memset(A, 0x5A, sizeof A); memset(B, 0x5A, sizeof B);
    if (kind == 1) B[0] = 0x5B;
    if (kind == 2) B[len - 1] = 0x5B;
    if (kind == 3) B[8] = 0x5B;
    cmp_fn f = *fp;
    const unsigned char *volatile ap = A, *volatile bp = B;
    for (long i = 0; i < n; i++) pc_sink += f(ap, bp, len);
    PC_CHECKING { int r = f(ap, bp, len); pc_check("%d", PC_SIGN(r)); }
}
CASE(memcmp_diff8_96, 0, 1) { (void)dir; cmp_run(&f_memcmp, 96, 3, n); }
CASE(memcmp_diff8_64, 0, 1) { (void)dir; cmp_run(&f_memcmp, 64, 3, n); }
CASE(memcmp_256, 0, 1) { (void)dir; cmp_run(&f_memcmp, 256, 0, n); }
CASE(bcmp_head_128, 0, 1) { (void)dir; cmp_run(&f_bcmp, 128, 1, n); }
CASE(bcmp_eq_64, 0, 1) { (void)dir; cmp_run(&f_bcmp, 64, 0, n); }
CASE(bcmp_eq_1k, 0, 1) { (void)dir; cmp_run(&f_bcmp, 1024, 0, n); }
CASE(meq_head_1k, 0, 1) { (void)dir; cmp_run(&f_meq, 1024, 1, n); }
CASE(meq_tail_64, 0, 1) { (void)dir; cmp_run(&f_meq, 64, 2, n); }

CASE(memchr_16, 0, 1) {
    (void)dir; memset(A, 'x', 64); A[16] = 0;
    const unsigned char *volatile s = A;
    for (long i = 0; i < n; i++) pc_sink += (long)f_memchr(s, 0, 16);
    PC_CHECKING pc_check("%d", f_memchr(s, 0, 16) != 0);
}
CASE(memrchr_64_hit, 0, 1) {
    (void)dir; memset(A, 'x', 4160); A[5] = 'q';
    const unsigned char *volatile s = A;
    for (long i = 0; i < n; i++) pc_sink += (long)f_memrchr(s, 'q', 6);
    PC_CHECKING pc_check("%d", f_memrchr(s, 'q', 6) != 0);
}

static void strstr_run(size_t len, const char *nd, long n) {
    static const char unit[] = "the quick brown fox jumps over the lazy dog and runs away ";
    char *h = H + 16;
    for (size_t i = 0; i < len; i++) h[i] = unit[i % (sizeof unit - 1)];
    memcpy(h + len - 5, "ZEBRA", 5); h[len] = 0;
    char *volatile hp = h; const char *volatile np = nd;
    for (long i = 0; i < n; i++) pc_sink += (long)f_strstr(hp, np);
    PC_CHECKING { char *r = f_strstr(hp, np); pc_check("%ld", r ? (long)(r - h) : -1L); }
}
CASE(strstr_hit_64, 0, 1) { (void)dir; strstr_run(64, "ZEBRA", n); }
CASE(strstr_miss_64, 0, 1) { (void)dir; strstr_run(64, "lazy cat", n); }
CASE(strstr_hit_4k, 0, 1) { (void)dir; strstr_run(4096, "ZEBRA", n); }

CASE_LOCALE(strcasecmp_c_16, 0, 1, "C") {
    (void)dir; const char *volatile a = "Hello World 1234", *volatile b = "hELLO wORLD 1234";
    for (long i = 0; i < n; i++) pc_sink += f_casecmp(a, b);
    PC_CHECKING pc_check("%d", PC_SIGN(f_casecmp(a, b)));
}
CASE_LOCALE(strncasecmp_c_64, 0, 1, "C") {
    (void)dir; static char ca[257], cb[257];
    for (int i = 0; i < 256; i++) {
        ca[i] = "abcdefghijklmnopqrstuvwxyz0123456789 .-_"[i % 40];
        cb[i] = (i & 1) && ca[i] >= 'a' && ca[i] <= 'z' ? (char)(ca[i] - 32) : ca[i];
    }
    const char *volatile a = ca, *volatile b = cb;
    for (long i = 0; i < n; i++) pc_sink += f_ncasecmp(a, b, 64);
    PC_CHECKING pc_check("%d", PC_SIGN(f_ncasecmp(a, b, 64)));
}

CASE(realloc_grow_x2, 0, 8) {
    (void)dir;
    for (long i = 0; i < n; i++) {
        char *p = malloc(16);
        for (size_t m = 32; m <= 4096; m *= 2) { p[0] = 1; p = f_realloc(p, m); }
        pc_sink += p[0]; free(p);
    }
    PC_CHECKING pc_check("ok");
}
