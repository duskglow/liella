/* STRSTR-GETLINE: per-call counts of every strstr and getline row of the benches (cbench_memref
 * find_rows, cbench_hot getline_80 / getline_long), plus ablation cases that split each row:
 *   strstr: ss_hit_{64,1k,4k} (same text, hit 5 bytes before the end: slope = candidate scan per
 *   byte, intercept = setup + verify); ss_pairmiss_4k (needle "ZEBRX": its first pair Z...X never
 *   occurs, so the scan runs to the terminator with no verify: hit - pairmiss = verify + return);
 *   strlen_4k (a plain terminator scan of the same text, the floor of any one-pass search).
 *   getline: gl_long (the bench row: 10000-byte lines, default buffer, refills), gl_long_win
 *   (the same file in a 1 MiB buffer: no refill inside a line), memchr_10k / memcpy_10k (the
 *   delimiter search and the copy of one line, alone). */
#include "percall.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define HIDE(x) __asm__ volatile("" : "+r"(x))

static char *ss_text(size_t n) {
    static const char unit[] = "the quick brown fox jumps over the lazy dog and runs away ";
    size_t ul = sizeof unit - 1;
    char *h = malloc(n + 16);
    for (size_t i = 0; i < n; i++) h[i] = unit[i % ul];
    memcpy(h + n - 5, "ZEBRA", 5);
    h[n] = 0;
    return h;
}

#define SS_CASE(nm, len, ndl)                                                          \
    CASE(nm, 100000, 1) {                                                              \
        char *h = ss_text(len);                                                        \
        char nd[] = ndl;                                                               \
        (void)dir;                                                                     \
        for (long i = 0; i < n; i++) { char *x = h, *y = nd; HIDE(x); HIDE(y); pc_sink += (long)strstr(x, y); } \
        PC_CHECKING { char *r = strstr(h, nd); pc_check("%ld", r ? (long)(r - h) : -1L); } \
    }

SS_CASE(ss_hit_4k, 4096, "ZEBRA")
SS_CASE(ss_hit_1k, 1024, "ZEBRA")
SS_CASE(ss_hit_64, 64, "ZEBRA")
SS_CASE(ss_miss_4k, 4096, "lazy cat")
SS_CASE(ss_miss_64, 64, "lazy cat")
SS_CASE(ss_pairmiss_4k, 4096, "ZEBRX")

CASE(strlen_4k, 100000, 1) {
    char *h = ss_text(4096);
    (void)dir;
    for (long i = 0; i < n; i++) { char *x = h; HIDE(x); pc_sink += (long)strlen(x); }
    PC_CHECKING { pc_check("%zu", strlen(h)); }
}

static FILE *gl_file(const char *dir, const char *name, size_t len, int lines, char *vbuf) {
    static char line[10001];
    char p[512];
    snprintf(p, sizeof p, "%s/%s", dir, name);
    FILE *w = fopen(p, "w");
    memset(line, 'a', sizeof line);
    line[len - 1] = '\n';
    for (int i = 0; i < lines; i++) fwrite(line, 1, len, w);
    fclose(w);
    FILE *f = fopen(p, "r");
    if (vbuf) setvbuf(f, vbuf, _IOFBF, 1 << 20);
    return f;
}

static char gl_vbuf[1 << 20];

#define GL_LOOP(f)                                                                     \
    char *lp = 0; size_t cap = 0;                                                      \
    for (long i = 0; i < n; i++) { ssize_t r = getline(&lp, &cap, f); if (r < 0) rewind(f); pc_sink += r; } \
    PC_CHECKING { long k = 0, l = 0; ssize_t r; rewind(f); while ((r = getline(&lp, &cap, f)) > 0) { k++; l += r; } pc_check("%ld %ld", k, l); }

CASE(gl_80, 200000, 1) { FILE *f = gl_file(dir, "sg80", 80, 800, 0); GL_LOOP(f) }
CASE(gl_long, 20000, 1) { FILE *f = gl_file(dir, "sglong", 10000, 20, 0); GL_LOOP(f) }
CASE(gl_long_win, 20000, 1) { FILE *f = gl_file(dir, "sgwin", 10000, 20, gl_vbuf); GL_LOOP(f) }

CASE(memchr_10k, 20000, 1) {
    char *b = malloc(10016);
    (void)dir;
    memset(b, 'a', 10000); b[9999] = '\n';
    for (long i = 0; i < n; i++) { char *x = b; HIDE(x); pc_sink += (long)memchr(x, '\n', 10000); }
    PC_CHECKING { pc_check("%ld", (long)((char *)memchr(b, '\n', 10000) - b)); }
}

CASE(memcpy_10k, 20000, 1) {
    char *b = malloc(10016), *d = malloc(10016);
    (void)dir;
    memset(b, 'a', 10000);
    for (long i = 0; i < n; i++) { char *x = d, *y = b; HIDE(x); HIDE(y); memcpy(x, y, 10000); pc_sink += x[5]; }
    PC_CHECKING { memcpy(d, b, 10000); pc_check("%d", d[9999]); }
}
