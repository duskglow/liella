/* PARITY-ROWS-1105: per-call profile of the non-libm rows still above 1.05x instructions:u */
#define _XOPEN_SOURCE 700
#include "percall.h"
#include <fnmatch.h>
#include <ftw.h>
#include <glob.h>

static char *volatile p1105;
static char m1105[4096];

CASE(realloc_within_usable, 10000000, 1) {
    for (long i = 0; i < n; i++) { char *p = malloc(17); p = realloc(p, 24); p[23] = 1; free(p); }
    PC_CHECKING { char *volatile p = malloc(17); p = realloc(p, 24); pc_check("%d", p != NULL); free(p); }
}
CASE(malloc_free_17, 10000000, 1) {
    for (long i = 0; i < n; i++) { p1105 = malloc(17); free(p1105); }
    PC_CHECKING { char *volatile p = malloc(17); pc_check("%d", p != NULL); free(p); }
}
CASE_LOCALE(fnmatch_bracket, 100000, 1, "C") {
    for (long i = 0; i < n; i++) pc_sink += fnmatch("[a-m]*_[!0-9]*.[ch]", "cbench_patterns.c", 0);
    PC_CHECKING { const char *volatile s = "cbench_patterns.c"; pc_check("%d", fnmatch("[a-m]*_[!0-9]*.[ch]", s, 0)); }
}
static void *(*volatile ms1105)(void *, int, size_t) = memset;
CASE(memset_4k, 1000000, 1) {
    for (long i = 0; i < n; i++) { ms1105(m1105, (int)(i & 7), sizeof m1105); pc_sink += m1105[100]; }
    PC_CHECKING { memset(m1105, 7, sizeof m1105); pc_check("%d %d", m1105[0], m1105[4095]); }
}
CASE_LOCALE(glob_sorted_38, 50, 1, "C") {
    char pat[600]; snprintf(pat, sizeof pat, "%s/*", dir);
    for (long i = 0; i < n; i++) { glob_t g; glob(pat, 0, NULL, &g); pc_sink += g.gl_pathc; globfree(&g); }
    PC_CHECKING { glob_t g; int r = glob(pat, 0, NULL, &g); pc_check("%d %zu", r, g.gl_pathc); globfree(&g); }
}
static long nf1105;
static int cb1105(const char *p, const struct stat *sb, int t, struct FTW *f) { (void)p; (void)f; nf1105 += t + (sb->st_size & 1); return 0; }
CASE_LOCALE(nftw_phys_38, 1000, 1, "C") {
    for (long i = 0; i < n; i++) nftw(dir, cb1105, 16, FTW_PHYS);
    PC_CHECKING { nf1105 = 0; int r = nftw(dir, cb1105, 16, FTW_PHYS); pc_check("%d %ld", r, nf1105); }
}
