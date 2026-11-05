/* wl_percall spec (PARITY-ROWS-AVX2): instructions per call of the string/memory families whose bench rows are
 * above 1.05 on the AVX2 counter class, at a ladder of sizes, Liella vs glibc. Where the glibc count steps shows
 * which vector width / loop shape it uses at that size. All buffers 64-byte aligned; calls go through volatile
 * function pointers so the compiler cannot fold them.
 *   bash validation/wl_percall.sh --spec validation/percall/avx2_families.c <dir|rev> */
#include "percall.h"

static size_t (*volatile pc_strlen)(const char *) = strlen;
static size_t (*volatile pc_strnlen)(const char *, size_t) = strnlen;
static void *(*volatile pc_memchr)(const void *, int, size_t) = memchr;
static void *(*volatile pc_rawmemchr)(const void *, int) = rawmemchr;
static int (*volatile pc_memcmp)(const void *, const void *, size_t) = memcmp;
static int (*volatile pc_strcmp)(const char *, const char *) = strcmp;
static void *(*volatile pc_memset)(void *, int, size_t) = memset;
static void *(*volatile pc_memmove)(void *, const void *, size_t) = memmove;
static void *(*volatile pc_memcpy)(void *, const void *, size_t) = memcpy;

static char pc_a[65536 + 256] __attribute__((aligned(64)));
static char pc_b[65536 + 256] __attribute__((aligned(64)));

static void pc_prep(size_t len) {
    for (size_t i = 0; i < len; i++) { pc_a[i] = (char)('a' + (i % 23)); pc_b[i] = pc_a[i]; }
    pc_a[len] = 0; pc_b[len] = 0;
}

#define FAM_STR(len)                                                                           \
    CASE(strlen_##len, 0, 1) { (void)dir; pc_prep(len); const char *volatile p = pc_a; const char *s = p;       \
        for (long i = 0; i < n; i++) pc_sink += pc_strlen(s);                                   \
        PC_CHECKING { pc_check("%zu", pc_strlen(p)); } }                                        \
    CASE(strnlen_##len, 0, 1) { (void)dir; pc_prep(len); const char *volatile p = pc_a; const char *s = p;      \
        for (long i = 0; i < n; i++) pc_sink += pc_strnlen(s, 1 << 20);                         \
        PC_CHECKING { pc_check("%zu", pc_strnlen(p, 1 << 20)); } }                              \
    CASE(memchr_##len, 0, 1) { (void)dir; pc_prep(len); const char *volatile p = pc_a; const char *s = p;       \
        for (long i = 0; i < n; i++) pc_sink += (long)(pc_memchr(s, 0, len + 1) != 0);          \
        PC_CHECKING { pc_check("%ld", (long)((char *)pc_memchr(p, 0, len + 1) - p)); } }        \
    CASE(rawmemchr_##len, 0, 1) { (void)dir; pc_prep(len); const char *volatile p = pc_a; const char *s = p;    \
        for (long i = 0; i < n; i++) pc_sink += (long)(pc_rawmemchr(s, 0) != 0);                \
        PC_CHECKING { pc_check("%ld", (long)((char *)pc_rawmemchr(p, 0) - p)); } }              \
    CASE(strcmp_eq_##len, 0, 1) { (void)dir; pc_prep(len); const char *volatile p = pc_a; const char *s = p;    \
        const char *volatile q = pc_b; const char *t = q;                                       \
        for (long i = 0; i < n; i++) pc_sink += pc_strcmp(s, t);                                \
        PC_CHECKING { pc_check("%d", PC_SIGN(pc_strcmp(p, q))); } }                             \
    CASE(memcmp_eq_##len, 0, 1) { (void)dir; pc_prep(len); const char *volatile p = pc_a; const char *s = p;    \
        const char *volatile q = pc_b; const char *t = q;                                       \
        for (long i = 0; i < n; i++) pc_sink += pc_memcmp(s, t, len);                           \
        PC_CHECKING { pc_check("%d", PC_SIGN(pc_memcmp(p, q, len))); } }                        \
    CASE(memset_##len, 0, 1) { (void)dir; char *volatile p = pc_a; char *d = p;                 \
        for (long i = 0; i < n; i++) { pc_memset(d, (int)(i & 7), len); pc_sink += d[len / 2]; } \
        PC_CHECKING { pc_memset(p, 0x5A, len); pc_check("%d %d", p[0], p[len - 1]); } }         \
    CASE(memmove_##len, 0, 1) { (void)dir; pc_prep(len); char *volatile p = pc_a; char *d = p;  \
        const char *volatile q = pc_b; const char *t = q;                                       \
        for (long i = 0; i < n; i++) { pc_memmove(d, t, len); pc_sink += d[len / 2]; }          \
        PC_CHECKING { pc_memmove(p, q, len); pc_check("%d %d", p[0], p[len - 1]); } }           \
    CASE(memcpy_##len, 0, 1) { (void)dir; pc_prep(len); char *volatile p = pc_a; char *d = p;   \
        const char *volatile q = pc_b; const char *t = q;                                       \
        for (long i = 0; i < n; i++) { pc_memcpy(d, t, len); pc_sink += d[len / 2]; }           \
        PC_CHECKING { pc_memcpy(p, q, len); pc_check("%d %d", p[0], p[len - 1]); } }

FAM_STR(16) FAM_STR(31) FAM_STR(32) FAM_STR(33) FAM_STR(64) FAM_STR(65) FAM_STR(128) FAM_STR(129) FAM_STR(256)
FAM_STR(1024) FAM_STR(4096) FAM_STR(16384) FAM_STR(65536)
