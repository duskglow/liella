/* Three rows the saturation register listed as unmeasured (BENCH-ROWS-UNMEASURED): `qsort` with a C
 * comparator (per comparison), `readdir` per entry, and `fts` per entry. The same file builds against the
 * products (`cargo xtask bench`) and against the host C library (`cc -O2 bench/c/cbench_unmeasured.c`). Prints
 * `name<TAB>ns/op` lines; the instruction numbers come from `--counters-only` (CBENCH_COUNTERS=1).
 *
 * The scratch directory (1,000 empty files, `flat`) is created under /tmp and removed by the names the program
 * created. Nothing here depends on directory order, inode numbers or d_type: the rows only count entries.
 * The qsort row sorts the same 1,024 pseudo-random ints each iteration (copied from a master array, so the copy
 * is part of the count: 4 KiB, about 130 instructions, against about 10,000 comparisons); the comparisons per
 * sort are counted once before the timing with a separate counting comparator, so the measured comparator is
 * the plain one a C program has (6 instructions of its own). */
#define _GNU_SOURCE
#include <dirent.h>
#include <fcntl.h>
#include <fts.h>
#include <limits.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <time.h>
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
    printf("%s\t%.2f\n", name, ns);
}

#include "cbench_rows.h"

#define MEASURE_PER(name, iters, per, expr)                        \
    do {                                                           \
        if (cb_skip(name))                                         \
            break;                                                 \
        double best = 1e300;                                       \
        ctr_begin();                                               \
        for (int round = 0; round < 7; round++) {                  \
            double t0 = now();                                     \
            long acc = 0;                                          \
            for (long i = 0; i < (iters); i++)                     \
                acc += (long)(expr);                               \
            sink = acc;                                            \
            double t = (now() - t0) / ((double)(iters) * (per));   \
            if (t < best)                                          \
                best = t;                                          \
        }                                                          \
        ctr_end(name, 7.0 * (double)(iters) * (per));              \
        put(name, best);                                           \
    } while (0)

#define FILES 1000
#define N 1024

static char top[PATH_MAX];
static int made;

static void teardown(void)
{
    char p[PATH_MAX + 32];
    if (!made)
        return;
    for (int i = 0; i < FILES; i++) {
        snprintf(p, sizeof p, "%s/flat/f%04d", top, i);
        unlink(p);
    }
    snprintf(p, sizeof p, "%s/flat", top);
    rmdir(p);
    rmdir(top);
}

static void fail(const char *what)
{
    fprintf(stderr, "cbench_unmeasured: unexpected result: %s\n", what);
    teardown();
    exit(1);
}

static int master[N], work[N];
static long ncmp;

static int cmp_plain(const void *a, const void *b)
{
    int x = *(const int *)a, y = *(const int *)b;
    return (x > y) - (x < y);
}

static int cmp_counting(const void *a, const void *b)
{
    ncmp++;
    return cmp_plain(a, b);
}

static long sort_once(void)
{
    memcpy(work, master, sizeof work);
    qsort(work, N, sizeof work[0], cmp_plain);
    return work[0] + work[N - 1];
}

static DIR *dirp;

static long rescan(void)
{
    long n = 0;
    rewinddir(dirp);
    while (readdir(dirp))
        n++;
    return n;
}

static long walk_fts(void)
{
    char flat[PATH_MAX + 8];
    snprintf(flat, sizeof flat, "%s/flat", top);
    char *argv[] = {flat, NULL};
    FTS *f = fts_open(argv, FTS_PHYSICAL | FTS_NOCHDIR, NULL);
    if (!f)
        return -1;
    long n = 0;
    FTSENT *e;
    while ((e = fts_read(f)))
        n++;
    return fts_close(f) == 0 ? n : -1;
}

int main(void)
{
    unsigned s = 12345;
    for (int i = 0; i < N; i++) {
        s = s * 1103515245u + 12345u;
        master[i] = (int)(s >> 8);
    }
    memcpy(work, master, sizeof work);
    qsort(work, N, sizeof work[0], cmp_counting);
    for (int i = 1; i < N; i++)
        if (work[i - 1] > work[i])
            fail("qsort order");
    long per_sort = ncmp;
    printf("comparisons_per_sort\t%ld.00\n", per_sort);

    snprintf(top, sizeof top, "/tmp/cbench_unmeasured.XXXXXX");
    if (!mkdtemp(top))
        fail("mkdtemp");
    made = 1;
    char p[PATH_MAX + 32];
    snprintf(p, sizeof p, "%s/flat", top);
    if (mkdir(p, 0700))
        fail("mkdir");
    for (int i = 0; i < FILES; i++) {
        snprintf(p, sizeof p, "%s/flat/f%04d", top, i);
        int fd = open(p, O_WRONLY | O_CREAT | O_EXCL, 0600);
        if (fd < 0)
            fail("create");
        close(fd);
    }

    MEASURE_PER("qsort_c_cmp_1024", 100, (double)per_sort, sort_once());

    snprintf(p, sizeof p, "%s/flat", top);
    dirp = opendir(p);
    if (!dirp)
        fail("opendir");
    if (rescan() != FILES + 2)
        fail("readdir count");
    MEASURE_PER("readdir_entry_flat", 200, FILES + 2, rescan());
    closedir(dirp);

    if (walk_fts() != FILES + 2)
        fail("fts count");
    MEASURE_PER("fts_entry_flat", 50, FILES + 2, walk_fts());

    teardown();
    return 0;
}
