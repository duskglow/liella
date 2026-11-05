/* PERF-NFTW-ENTRY: one nftw(FTW_PHYS) pass over the 38-file directory (+ "a" and "text"); per_iteration = entries. */
#define _XOPEN_SOURCE 700
#include "percall.h"
#include <sys/stat.h>
#include <ftw.h>
static long nftw_seen;
static int nftw_cb(const char *p, const struct stat *s, int t, struct FTW *w) { nftw_seen += t + w->level + (long)s->st_size + p[0]; return 0; }
CASE(nftw_phys_38, 20000, 41) {
    for (long i = 0; i < n; i++) { nftw(dir, nftw_cb, 16, FTW_PHYS); }
    pc_sink += nftw_seen;
    PC_CHECKING { nftw_seen = 0; int r = nftw(dir, nftw_cb, 16, FTW_PHYS); pc_check("%d %d", r, nftw_seen != 0); }
}
