/* glibc references for the bench_mem rows (TOOL-MEMREF, owner 2026-10-08: parity is the upper bound).
 * The same file builds against the products (`cargo xtask bench`) and against the host C library
 * (`cc -O2 bench/c/cbench_memref.c`); the `ratio_host` rows of bench/budgets.tsv give the median
 * Liella/glibc ratio per row. Row names, sizes, buffers and the timing method are those of
 * crates/boundary/examples/bench_mem.rs: 2000 warm-up calls, then the fastest of 41 batches of 2000
 * calls (per-call ns). Method differences from bench_mem (assumption A8), which measures the Rust
 * functions directly: here both libraries are reached through their C ABI symbols (a static call
 * for Liella, the PLT and the resolved IFUNC slot for glibc), as a C caller reaches them. Compiled
 * with -fno-builtin, so every call is a real library call. Prints `name<TAB>ns/op`. */
#define _GNU_SOURCE
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <wchar.h>
#include <unistd.h>
#include "cbench_counters.h"

static volatile long sink;

static double now(void)
{
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (double)ts.tv_sec * 1e9 + (double)ts.tv_nsec;
}

static void put(const char *name, double ns)
{
    char buf[64];
    int n = 0;
    while (name[n] && n < 40) {
        buf[n] = name[n];
        n++;
    }
    buf[n++] = '\t';
    unsigned long v = (unsigned long)(ns * 100.0 + 0.5);
    char digits[24];
    int d = 0;
    do {
        digits[d++] = (char)('0' + v % 10);
        v /= 10;
    } while (v || d < 3);
    while (d > 2)
        buf[n++] = digits[--d];
    buf[n++] = '.';
    buf[n++] = digits[1];
    buf[n++] = digits[0];
    buf[n++] = '\n';
    (void)!write(1, buf, (size_t)n);
}

/* Hides a pointer's origin from the optimizer (the C counterpart of bench_mem's black_box). */
#define HIDE(p) __asm__ volatile("" : "+r"(p))

#include "cbench_rows.h"

#define MEASURE(name, ...)                                   \
    do {                                                     \
        if (cb_skip(name))                                   \
            break;                                           \
        double best = 1e300;                                 \
        long acc = 0;                                        \
        for (int i = 0; i < 2000; i++) {                     \
            __VA_ARGS__;                                     \
        }                                                    \
        ctr_begin();                                         \
        for (int round = 0; round < 41; round++) {           \
            double t0 = now();                               \
            for (int i = 0; i < 2000; i++) {                 \
                __VA_ARGS__;                                 \
            }                                                \
            double per = (now() - t0) / 2000.0;              \
            if (per < best)                                  \
                best = per;                                  \
        }                                                    \
        ctr_end(name, 41.0 * 2000.0);                        \
        sink = acc;                                          \
        put(name, best);                                     \
    } while (0)

/* all = 0: only the copy, fill, move and compare rows (the sizes between the kernel classes). */
static void size_rows(size_t n, const char *tag, int all)
{
    unsigned char *src = malloc(n + 1), *other = malloc(n + 1), *dst = malloc(n + 16);
    if (!src || !other || !dst)
        abort();
    memset(src, 0x5A, n);
    memset(other, 0x5A, n);
    memset(dst, 0, n);
    unsigned char *text = malloc(n + 1);
    if (!text)
        abort();
    memset(text, 'x', n);
    text[n] = 0;
    char name[32];
#define ROW(fn, ...)                                        \
    do {                                                    \
        strcpy(name, fn "_");                               \
        strcat(name, tag);                                  \
        MEASURE(name, __VA_ARGS__);                                \
    } while (0)
    ROW("memcpy", { unsigned char *d = dst, *s = src; HIDE(d); HIDE(s); memcpy(d, s, n); });
    ROW("memset", { unsigned char *d = dst; HIDE(d); memset(d, 7, n); });
    /* MEM-K1: an overlapping move 8 bytes up (the destination buffer has 16 spare bytes). */
    ROW("memmove", { unsigned char *d = dst; HIDE(d); memmove(d + 8, d, n); });
    ROW("memcmp", { unsigned char *a = src, *b = other; HIDE(a); HIDE(b); acc += memcmp(a, b, n); });
    if (all) {
        ROW("memchr", { unsigned char *s = src; HIDE(s); acc += (long)memchr(s, 0, n); });
        ROW("strlen", { unsigned char *t = text; HIDE(t); acc += (long)strlen((char *)t); });
        ROW("strnlen", { unsigned char *t = text; HIDE(t); acc += (long)strnlen((char *)t, n + 1); });
        ROW("rawmemchr", { unsigned char *t = text; HIDE(t); acc += (long)rawmemchr(t, 0); });
    }
    free(src);
    free(other);
    free(dst);
    free(text);
}

/* STRSTR-FIND: substring searches over English-like text (4 KiB and 64 bytes). "miss" = the needle
 * shares a long prefix with the text but never matches; "hit" = the only match is at the end. */
static void find_rows(void)
{
    static const char unit[] = "the quick brown fox jumps over the lazy dog and runs away ";
    size_t ul = sizeof unit - 1;
    for (int pass = 0; pass < 2; pass++) {
        size_t n = pass ? 64 : 4096;
        const char *tag = pass ? "64" : "4k";
        char *h = malloc(n + 16);
        wchar_t *w = malloc((n + 16) * sizeof(wchar_t));
        if (!h || !w)
            abort();
        for (size_t i = 0; i < n; i++)
            h[i] = unit[i % ul];
        memcpy(h + n - 5, "ZEBRA", 5);
        h[n] = 0;
        for (size_t i = 0; i <= n; i++) {
            w[i] = (wchar_t)(unsigned char)h[i];
        }
        char nm_miss[] = "lazy cat";
        char nm_hit[] = "ZEBRA";
        char nm_hit_ci[] = "zebra";
        char nm_miss_ci[] = "LAZY CAT";
        wchar_t wmiss[] = L"lazy cat", whit[] = L"ZEBRA";
        char name[48];
#define FROW(fn, ...)                            do {                                         strcpy(name, fn "_");                    strcat(name, tag);                       MEASURE(name, __VA_ARGS__);          } while (0)
        FROW("strstr_miss", { char *x = h, *y = nm_miss; HIDE(x); HIDE(y); acc += (long)strstr(x, y); });
        FROW("strstr_hit", { char *x = h, *y = nm_hit; HIDE(x); HIDE(y); acc += (long)strstr(x, y); });
        FROW("strcasestr_miss", { char *x = h, *y = nm_miss_ci; HIDE(x); HIDE(y); acc += (long)strcasestr(x, y); });
        FROW("strcasestr_hit", { char *x = h, *y = nm_hit_ci; HIDE(x); HIDE(y); acc += (long)strcasestr(x, y); });
        FROW("memmem_miss", { char *x = h, *y = nm_miss; HIDE(x); HIDE(y); acc += (long)memmem(x, n, y, 8); });
        FROW("wcsstr_miss", { wchar_t *x = w, *y = wmiss; HIDE(x); HIDE(y); acc += (long)wcsstr(x, y); });
        FROW("wcsstr_hit", { wchar_t *x = w, *y = whit; HIDE(x); HIDE(y); acc += (long)wcsstr(x, y); });
        free(h);
        free(w);
    }
}

/* BENCH-ALIGN: the short HOT rows again, but the operands' start addresses cycle through a fixed
 * table of offsets weighted like real calls (ARG-HIST: ~66% not 16-aligned, ~34% 16-aligned, ~47%
 * 8-aligned, ~9% 64-aligned), instead of one malloc alignment. SHORT-2 found that a head change which
 * helps some (address & 63) values and hurts others never shows in the single-alignment rows above.
 * Every operand lives in a "slab" at (pool + o * 256 + o), congruent to o mod 64, so the offset o is
 * the alignment itself; the contents (n bytes, then a NUL for the string rows) are the same for every
 * o. The table is generated by an integer LCG from constants in this file, so the Liella build and the
 * host-C-library twin (this same file) cycle the SAME offsets; main() checks the table's checksum
 * against the expected value and aborts on a mismatch. Rows: <fn>_<n>_mix. */
#define MIX_N 1024
#define MIX_SLAB 256
#define MIX_SUM 0x4983ccbffe7e19a2UL /* BENCH-ALIGN-SUM */
static unsigned char mix_tbl[MIX_N];
static unsigned long mix_rng;

static unsigned mix_draw(void)
{
    mix_rng = mix_rng * 6364136223846793005UL + 1442695040888963407UL;
    return (unsigned)(mix_rng >> 33);
}

static unsigned long mix_build(void)
{
    static const unsigned char a16[3] = {16, 32, 48};
    static const unsigned char a8[4] = {8, 24, 40, 56};
    mix_rng = 0x9E3779B97F4A7C15UL;
    unsigned long sum = 1469598103934665603UL;
    for (int i = 0; i < MIX_N; i++) {
        unsigned r = mix_draw() & 63, o;
        if (r < 6)
            o = 0;
        else if (r < 22)
            o = a16[mix_draw() % 3];
        else if (r < 30)
            o = a8[mix_draw() % 4];
        else {
            o = 1 + mix_draw() % 63;
            if ((o & 7) == 0)
                o++;
        }
        mix_tbl[i] = (unsigned char)o;
        sum = (sum ^ o) * 1099511628211UL;
    }
    return sum;
}

/* 64 slabs, the pool itself 64-aligned (leaked: the process ends after the rows). */
static unsigned char *mix_pool(unsigned char fill, size_t n, int nul)
{
    unsigned char *raw = malloc(64 * MIX_SLAB + 64);
    if (!raw)
        abort();
    unsigned char *pool = (unsigned char *)(((unsigned long)raw + 63) & ~63UL);
    memset(pool, 0, 64 * MIX_SLAB);
    for (int o = 0; o < 64; o++) {
        unsigned char *p = pool + o * MIX_SLAB + o;
        memset(p, fill, n);
        if (nul)
            p[n] = 0;
    }
    return pool;
}

#define SLAB(pool, o) ((pool) + (size_t)(o) * MIX_SLAB + (o))

static void mix_rows(size_t n, const char *tag)
{
    unsigned char *pa = mix_pool(0x5A, n, 0), *pb = mix_pool(0x5A, n, 0), *pc = mix_pool(0, n, 0);
    unsigned char *pt = mix_pool('x', n, 1), *pu = mix_pool('x', n, 1);
    char name[32];
#define MROW(fn, ...)                                       \
    do {                                                    \
        strcpy(name, fn "_");                               \
        strcat(name, tag);                                  \
        strcat(name, "_mix");                               \
        MEASURE(name, __VA_ARGS__);                         \
    } while (0)
#define OA (mix_tbl[i & (MIX_N - 1)])
#define OB (mix_tbl[(i * 7 + 300) & (MIX_N - 1)])
    MROW("memcpy", { unsigned char *d = SLAB(pc, OB), *s = SLAB(pa, OA); HIDE(d); HIDE(s); memcpy(d, s, n); });
    MROW("memset", { unsigned char *d = SLAB(pc, OA); HIDE(d); memset(d, 7, n); });
    MROW("memcmp", { unsigned char *a = SLAB(pa, OA), *b = SLAB(pb, OB); HIDE(a); HIDE(b); acc += memcmp(a, b, n); });
    MROW("memchr", { unsigned char *s = SLAB(pa, OA); HIDE(s); acc += (long)memchr(s, 0, n); });
    MROW("strlen", { unsigned char *t = SLAB(pt, OA); HIDE(t); acc += (long)strlen((char *)t); });
    MROW("strnlen", { unsigned char *t = SLAB(pt, OA); HIDE(t); acc += (long)strnlen((char *)t, n + 1); });
    MROW("rawmemchr", { unsigned char *t = SLAB(pt, OA); HIDE(t); acc += (long)rawmemchr(t, 0); });
    MROW("strcmp", { unsigned char *a = SLAB(pt, OA), *b = SLAB(pu, OB); HIDE(a); HIDE(b); acc += strcmp((char *)a, (char *)b); });
}

/* MEMCMP-EQ-2: equality calls (__memcmpeq, bcmp) on equal inputs, a difference in the first byte ("head") and in
 * the last byte ("tail"), and the ordered memcmp with the first difference at byte 8 (the shape of sort -C:
 * ARG-HIST, n 64-127). Row names <fn>_<shape>_<n>. */
int __memcmpeq(const void *, const void *, size_t);

static void eq_rows(size_t n, const char *tag)
{
    unsigned char *a = malloc(n + 1), *same = malloc(n + 1), *head = malloc(n + 1), *tail = malloc(n + 1);
    if (!a || !same || !head || !tail)
        abort();
    memset(a, 0x5A, n);
    memset(same, 0x5A, n);
    memset(head, 0x5A, n);
    memset(tail, 0x5A, n);
    head[0] = 0x5B;
    tail[n - 1] = 0x5B;
    char name[40];
#define EROW(fn, shape, ...)                                \
    do {                                                    \
        strcpy(name, fn "_" shape "_");                     \
        strcat(name, tag);                                  \
        MEASURE(name, __VA_ARGS__);                         \
    } while (0)
    EROW("__memcmpeq", "eq", { unsigned char *x = a, *y = same; HIDE(x); HIDE(y); acc += __memcmpeq(x, y, n); });
    EROW("__memcmpeq", "head", { unsigned char *x = a, *y = head; HIDE(x); HIDE(y); acc += __memcmpeq(x, y, n); });
    EROW("__memcmpeq", "tail", { unsigned char *x = a, *y = tail; HIDE(x); HIDE(y); acc += __memcmpeq(x, y, n); });
    EROW("bcmp", "eq", { unsigned char *x = a, *y = same; HIDE(x); HIDE(y); acc += bcmp(x, y, n); });
    EROW("bcmp", "head", { unsigned char *x = a, *y = head; HIDE(x); HIDE(y); acc += bcmp(x, y, n); });
    EROW("bcmp", "tail", { unsigned char *x = a, *y = tail; HIDE(x); HIDE(y); acc += bcmp(x, y, n); });
    free(a);
    free(same);
    free(head);
    free(tail);
}

/* memcmp with the first difference at byte 8 (sort -C: n 64-127, difference within bytes 8-31). */
static void diff8_row(size_t n, const char *tag)
{
    unsigned char *a = malloc(n + 1), *b = malloc(n + 1);
    if (!a || !b)
        abort();
    memset(a, 0x5A, n);
    memset(b, 0x5A, n);
    b[8] = 0x5B;
    char name[40];
    strcpy(name, "memcmp_diff8_");
    strcat(name, tag);
    MEASURE(name, { unsigned char *x = a, *y = b; HIDE(x); HIDE(y); acc += memcmp(x, y, n); });
    free(a);
    free(b);
}

int main(void)
{
    unsigned long mixsum = mix_build();
    if (mixsum != MIX_SUM) {
        char msg[64] = "mix table checksum mismatch 0x";
        int n = 30;
        for (int sh = 60; sh >= 0; sh -= 4)
            msg[n++] = "0123456789abcdef"[(mixsum >> sh) & 15];
        msg[n++] = '\n';
        (void)!write(2, msg, (size_t)n);
        abort();
    }
    find_rows();
    size_rows(16, "16", 1); /* MEM-K1: the small-copy kernel's smallest block class */
    size_rows(64, "64", 1);
    /* MEM-K5: the 65-128 byte classes (copy, fill, move, compare) at their largest size. */
    size_rows(128, "128", 0);
    /* COPY-256: the 129-256 copy class (D-4.25): the fstatat (144 B) and statx (256 B) struct sizes. */
    size_rows(144, "144", 0);
    size_rows(256, "256", 0);
    size_rows(4096, "4k", 1);
    size_rows(65536, "64k", 1);
    {
        static const size_t eqn[] = {8, 16, 32, 64, 128, 1024};
        static const char *const eqt[] = {"8", "16", "32", "64", "128", "1k"};
        for (int k = 0; k < 6; k++)
            eq_rows(eqn[k], eqt[k]);
        diff8_row(64, "64");
        diff8_row(96, "96");
        diff8_row(127, "127");
    }
    mix_rows(16, "16");
    mix_rows(64, "64");
    /* Source and destination 64 bytes off a 4 KiB distance, as bench_mem's *_4k_off rows. */
    size_t n = 4096;
    unsigned char *src = malloc(n + 128), *dst = malloc(n + 128);
    if (!src || !dst)
        abort();
    memset(src, 0x5A, n + 128);
    memset(dst, 0, n + 128);
    MEASURE("memcpy_4k_off", { unsigned char *d = dst + 64, *s = src + 64; HIDE(d); HIDE(s); memcpy(d, s, n); });
    MEASURE("memset_4k_off", { unsigned char *d = dst + 64; HIDE(d); memset(d, 7, n); });
    free(src);
    free(dst);
    return 0;
}
