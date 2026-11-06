/* PERF-NFTW-ENTRY-7: one fts_open/fts_read.../fts_close pass (FTS_PHYSICAL, chdir mode, the bench shape) over the
   38-file directory (+ "a" and "text"); per_iteration = entries. */
#include "percall.h"
#include <sys/stat.h>
#include <fts.h>
static long fts_seen;
CASE(fts_phys_38, 20000, 41) {
    for (long i = 0; i < n; i++) {
        char *const argv[2] = { (char *)dir, 0 };
        FTS *f = fts_open(argv, FTS_PHYSICAL, 0);
        FTSENT *e;
        while (f && (e = fts_read(f)) != 0) fts_seen += e->fts_info + e->fts_level + (long)e->fts_statp->st_size + e->fts_name[0];
        if (f) fts_close(f);
    }
    pc_sink += fts_seen;
    PC_CHECKING {
        fts_seen = 0;
        char *const argv[2] = { (char *)dir, 0 };
        FTS *f = fts_open(argv, FTS_PHYSICAL, 0);
        FTSENT *e; int k = 0;
        while (f && (e = fts_read(f)) != 0) { k++; fts_seen += e->fts_info; }
        if (f) fts_close(f);
        pc_check("%d %d", k, fts_seen != 0);
    }
}
