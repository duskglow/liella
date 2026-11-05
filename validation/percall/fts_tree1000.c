/* PERF-NFTW-ENTRY-8: one fts_open/fts_read.../fts_close pass (FTS_PHYSICAL, chdir mode) over the shape of the
   cbench_fstree row fts_physical_1000: a root with 10 subdirectories of 99 files each (1011 entries, pre-order and
   post-order visits both counted by fts_read). The tree is built under <dir>/t1000 in setup (same in n = 0 and
   n = N, so it cancels). per_iteration = 1011 entries (fts_read calls are 1 + 10*2 + 990 + 1 = 1012 incl. root DP). */
#include "percall.h"
#include <sys/stat.h>
#include <fts.h>
static long fts_t_seen;
static void fts_t_build(const char *dir, char *root, unsigned cap) {
    snprintf(root, cap, "%s/t1000", dir);
    mkdir(root, 0700);
    for (int d = 0; d < 10; d++) {
        char p[4200];
        snprintf(p, sizeof p, "%s/d%d", root, d);
        mkdir(p, 0700);
        for (int f = 0; f < 99; f++) {
            char q[4300];
            snprintf(q, sizeof q, "%s/f%03d.dat", p, f);
            int fd = open(q, O_WRONLY | O_CREAT, 0600);
            if (fd >= 0) close(fd);
        }
    }
}
CASE(fts_tree_1000, 50, 1011) {
    char root[4096];
    fts_t_build(dir, root, sizeof root);
    for (long i = 0; i < n; i++) {
        char *const argv[2] = { root, 0 };
        FTS *f = fts_open(argv, FTS_PHYSICAL, 0);
        FTSENT *e;
        while (f && (e = fts_read(f)) != 0) fts_t_seen += (e->fts_info != FTS_DP) + e->fts_level + e->fts_name[0];
        if (f) fts_close(f);
    }
    pc_sink += fts_t_seen;
    PC_CHECKING {
        char *volatile vroot = root;
        char *const argv[2] = { vroot, 0 };
        FTS *f = fts_open(argv, FTS_PHYSICAL, 0);
        FTSENT *e; int k = 0, pre = 0;
        while (f && (e = fts_read(f)) != 0) { k++; pre += e->fts_info != FTS_DP; }
        if (f) fts_close(f);
        pc_check("%d %d", k, pre);
    }
}
