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
#define LEVELS 10
#define LEVELLEN 200

static char top[PATH_MAX];
static int made;

static void deep_files(const char *what, int create);
static char deep[PATH_MAX], deepflat[PATH_MAX + 8];
static int deep_levels;

static void teardown(void)
{
    char p[PATH_MAX + 32];
    if (!made)
        return;
    if (deep_levels) {
        if (deepflat[0]) {
            deep_files("", 0);
            rmdir(deepflat);
        }
        for (int l = deep_levels; l > 0; l--) {
            size_t n = strlen(top) + (size_t)l * (LEVELLEN + 1);
            deep[n] = 0;
            rmdir(deep);
        }
    }
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

/* Element sizes 4, 8, 16, 24 and 64 bytes: a 4-byte key first, the rest padding that is filled
 * deterministically and never compared. The same plain comparator work on both sides; the comparisons per
 * sort are counted once with a separate counting comparator on the library under test. */
typedef struct { int k; } e4_t;
typedef struct { int k; char pad[4]; } e8_t;
typedef struct { int k; char pad[12]; } e16_t;
typedef struct { int k; char pad[20]; } e24_t;
typedef struct { int k; char pad[60]; } e64_t;

#define QS_SIZE(T, SZ)                                                         \
    static T master_##SZ[N], work_##SZ[N];                                     \
    static int cmp_##SZ(const void *a, const void *b)                          \
    {                                                                          \
        int x = ((const T *)a)->k, y = ((const T *)b)->k;                      \
        return (x > y) - (x < y);                                              \
    }                                                                          \
    static int cmpc_##SZ(const void *a, const void *b)                         \
    {                                                                          \
        ncmp++;                                                                \
        return cmp_##SZ(a, b);                                                 \
    }                                                                          \
    static long prep_##SZ(void)                                                \
    {                                                                          \
        unsigned q = 777u + SZ;                                                \
        for (int i = 0; i < N; i++) {                                          \
            q = q * 1103515245u + 12345u;                                      \
            memset(&master_##SZ[i], i & 0xff, sizeof(T));                      \
            master_##SZ[i].k = (int)(q >> 8);                                  \
        }                                                                      \
        memcpy(work_##SZ, master_##SZ, sizeof work_##SZ);                      \
        ncmp = 0;                                                              \
        qsort(work_##SZ, N, sizeof(T), cmpc_##SZ);                             \
        for (int i = 1; i < N; i++)                                            \
            if (work_##SZ[i - 1].k > work_##SZ[i].k)                           \
                fail("qsort order " #SZ);                                      \
        printf("comparisons_per_sort_e%d\t%ld.00\n", SZ, ncmp);                \
        return ncmp;                                                           \
    }

/* The sort bodies are plain functions (not generated by QS_SIZE) so the row map, which reads the source,
 * follows each MEASURE site through them to qsort. */
static long sort_with(void *work, const void *master, size_t total, size_t esz, int (*cmp)(const void *, const void *))
{
    memcpy(work, master, total);
    qsort(work, total / esz, esz, cmp);
    return ((const int *)work)[0];
}

QS_SIZE(e4_t, 4)
QS_SIZE(e8_t, 8)
QS_SIZE(e16_t, 16)
QS_SIZE(e24_t, 24)
QS_SIZE(e64_t, 64)

static long sort_4(void) { return sort_with(work_4, master_4, sizeof work_4, 4, cmp_4); }
static long sort_8(void) { return sort_with(work_8, master_8, sizeof work_8, 8, cmp_8); }
static long sort_16(void) { return sort_with(work_16, master_16, sizeof work_16, 16, cmp_16); }
static long sort_24(void) { return sort_with(work_24, master_24, sizeof work_24, 24, cmp_24); }
static long sort_64(void) { return sort_with(work_64, master_64, sizeof work_64, 64, cmp_64); }

static void deep_files(const char *what, int create)
{
    char p[PATH_MAX + 32];
    for (int i = 0; i < FILES; i++) {
        snprintf(p, sizeof p, "%s/f%04d", deepflat, i);
        if (create) {
            int fd = open(p, O_WRONLY | O_CREAT | O_EXCL, 0600);
            if (fd < 0)
                fail(what);
            close(fd);
        } else
            unlink(p);
    }
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

static long walk_fts_at(const char *flat)
{
    char *argv[] = {(char *)flat, NULL};
    FTS *f = fts_open(argv, FTS_PHYSICAL | FTS_NOCHDIR, NULL);
    if (!f)
        return -1;
    long n = 0;
    FTSENT *e;
    while ((e = fts_read(f)))
        n++;
    return fts_close(f) == 0 ? n : -1;
}

static long walk_fts(void)
{
    char flat[PATH_MAX + 8];
    snprintf(flat, sizeof flat, "%s/flat", top);
    return walk_fts_at(flat);
}

static long walk_fts_deep(void)
{
    return walk_fts_at(deepflat);
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
    long c4 = prep_4(), c8 = prep_8(), c16 = prep_16(), c24 = prep_24(), c64 = prep_64();
    MEASURE_PER("qsort_c_cmp_1024_e4", 100, (double)c4, sort_4());
    MEASURE_PER("qsort_c_cmp_1024_e8", 100, (double)c8, sort_8());
    MEASURE_PER("qsort_c_cmp_1024_e16", 100, (double)c16, sort_16());
    MEASURE_PER("qsort_c_cmp_1024_e24", 100, (double)c24, sort_24());
    MEASURE_PER("qsort_c_cmp_1024_e64", 100, (double)c64, sort_64());

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

    /* A base path of about 2 KiB: LEVELS nested directories of LEVELLEN characters under the scratch
     * directory, then `flat` with its own 1,000 files. */
    snprintf(deep, sizeof deep, "%s", top);
    for (int l = 1; l <= LEVELS; l++) {
        size_t n = strlen(deep);
        deep[n] = '/';
        memset(deep + n + 1, 'a' + (l % 26), LEVELLEN);
        deep[n + 1 + LEVELLEN] = 0;
        if (mkdir(deep, 0700))
            fail("mkdir deep");
        deep_levels = l;
    }
    snprintf(deepflat, sizeof deepflat, "%s/flat", deep);
    if (mkdir(deepflat, 0700))
        fail("mkdir deep flat");
    deep_files("create deep", 1);
    if (strlen(deepflat) < 2000 || strlen(deepflat) > 2100)
        fail("deep path length");
    if (walk_fts_deep() != FILES + 2)
        fail("fts deep count");
    MEASURE_PER("fts_entry_deep2k", 50, FILES + 2, walk_fts_deep());

    teardown();
    return 0;
}
