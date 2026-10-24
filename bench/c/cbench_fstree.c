/* File system, high level (phase 12): directory streams, getcwd, realpath, scandir, the nftw
 * and fts walks, mkstemp, and the mount-table reader (§13.1-13.3, §13.8-13.9, §13.13). The same
 * file builds against the products (`cargo xtask bench`) and against the host C library
 * (`cc -O2 bench/c/cbench_fstree.c`), so benchmarks/progress.md compares identical code. Prints
 * `name<TAB>ns/op` lines, the best of several rounds; the lines are formatted by hand so that the
 * measured functions do not format their own results.
 *
 * Everything happens in a scratch directory the program creates under /tmp and removes at the
 * end (by the names it created, without the functions measured); nothing outside it is touched.
 * Its contents: `flat` (1,000 empty files), `tree` (10 directories of 99 files each: 1,000
 * entries below the root), `p/q/r` (with the scratch directory's own two components, a
 * 5-component path for realpath) and `mnt` (a 50-line mount table). Every call is checked once
 * before it is timed; a call that fails ends the program, so a row never times an error path by
 * accident. */
#define _GNU_SOURCE
#include <dirent.h>
#include <fcntl.h>
#include <fts.h>
#include <ftw.h>
#include <limits.h>
#include <mntent.h>
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

static void teardown(void);

/* Report, remove the scratch directory, exit. */
static void fail(const char *what)
{
    (void)!write(2, "cbench_fstree: unexpected result: ", 34);
    (void)!write(2, what, strlen(what));
    (void)!write(2, "\n", 1);
    teardown();
    exit(1);
}

/* The best of seven rounds of `iters` evaluations, per `per` units of each evaluation. */
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
#define MEASURE(name, iters, expr) MEASURE_PER(name, iters, 1.0, expr)

#define FILES 1000
#define DIRS 10
#define PER_DIR 99
#define MNT_LINES 50

static char top[PATH_MAX], base[64];
static int levels; /* CBENCH_STRESS_DEPTH: directories between base and top */
static int made; /* top exists */

/* `top`/`rel` in a static buffer. */
static const char *at(const char *rel)
{
    static char p[PATH_MAX];
    size_t n = strlen(top);
    memcpy(p, top, n);
    p[n] = '/';
    strcpy(p + n + 1, rel);
    return p;
}

static void touch(const char *p)
{
    int fd = open(p, O_WRONLY | O_CREAT | O_EXCL, 0600);
    if (fd < 0)
        fail(p);
    close(fd);
}

static void name3(char *out, const char *prefix, int i)
{
    strcpy(out, prefix);
    size_t n = strlen(out);
    out[n] = (char)('0' + i / 100 % 10);
    out[n + 1] = (char)('0' + i / 10 % 10);
    out[n + 2] = (char)('0' + i % 10);
    out[n + 3] = 0;
}

static void build(void)
{
    char rel[64];
    if (mkdir(at("flat"), 0700) != 0 || mkdir(at("tree"), 0700) != 0)
        fail("mkdir");
    for (int i = 0; i < FILES; i++) {
        name3(rel, "flat/f", i);
        touch(at(rel));
    }
    for (int d = 0; d < DIRS; d++) {
        name3(rel, "tree/d", d);
        if (mkdir(at(rel), 0700) != 0)
            fail("mkdir tree");
        for (int i = 0; i < PER_DIR; i++) {
            char f[64];
            strcpy(f, rel);
            name3(f + strlen(f), "/f", i);
            touch(at(f));
        }
    }
    if (mkdir(at("p"), 0700) != 0 || mkdir(at("p/q"), 0700) != 0 || mkdir(at("p/q/r"), 0700) != 0)
        fail("mkdir p/q/r");
    int fd = open(at("mnt"), O_WRONLY | O_CREAT | O_EXCL, 0600);
    if (fd < 0)
        fail("mnt");
    for (int i = 0; i < MNT_LINES; i++) {
        char line[96];
        strcpy(line, "/dev/sd");
        name3(line + strlen(line), "", i);
        strcat(line, " /mnt/");
        name3(line + strlen(line), "v", i);
        strcat(line, " ext4 rw,relatime,errors=remount-ro 0 2\n");
        (void)!write(fd, line, strlen(line));
    }
    close(fd);
}

static void teardown(void)
{
    if (!made)
        return;
    made = 0;
    char rel[64];
    for (int i = 0; i < FILES; i++) {
        name3(rel, "flat/f", i);
        unlink(at(rel));
    }
    for (int d = 0; d < DIRS; d++) {
        name3(rel, "tree/d", d);
        for (int i = 0; i < PER_DIR; i++) {
            char f[64];
            strcpy(f, rel);
            name3(f + strlen(f), "/f", i);
            unlink(at(f));
        }
        rmdir(at(rel));
    }
    rmdir(at("flat"));
    rmdir(at("tree"));
    rmdir(at("p/q/r"));
    rmdir(at("p/q"));
    rmdir(at("p"));
    unlink(at("mnt"));
    rmdir(top);
    /* The stress levels, deepest first, then the mkdtemp directory itself. */
    for (; levels > 0; levels--) {
        char *s = strrchr(top, '/');
        *s = 0;
        rmdir(top);
    }
}

static char flat[PATH_MAX], tree[PATH_MAX], deep[PATH_MAX], mnt[PATH_MAX], tmpl[PATH_MAX];

/* opendir, readdir to the end, closedir: the entries seen (with . and ..). */
static long scan(void)
{
    DIR *d = opendir(flat);
    if (!d)
        return -1;
    long n = 0;
    while (readdir(d))
        n++;
    closedir(d);
    return n;
}

/* readdir over an open stream, from the start. */
static long rescan(DIR *d)
{
    rewinddir(d);
    long n = 0;
    while (readdir(d))
        n++;
    return n;
}

/* What a tree walker (gnulib fts, as in du and ls -R) does per directory: openat with
   O_DIRECTORY, fdopendir, readdir to the end, closedir; here on a 99-file directory. */
static char small[PATH_MAX];
static long scan_fd(void)
{
    int fd = openat(AT_FDCWD, small, O_RDONLY | O_NOCTTY | O_NONBLOCK | O_NOFOLLOW | O_CLOEXEC | O_DIRECTORY);
    if (fd < 0)
        return -1;
    DIR *d = fdopendir(fd);
    if (!d) {
        close(fd);
        return -1;
    }
    long n = 0;
    while (readdir(d))
        n++;
    closedir(d);
    return n;
}

static long cwd(void)
{
    char buf[PATH_MAX];
    return getcwd(buf, sizeof buf) ? (long)buf[1] : -1;
}

static long real(void)
{
    char buf[PATH_MAX];
    return realpath(deep, buf) ? (long)strlen(buf) : -1;
}

static long scan_sorted(void)
{
    struct dirent **list;
    int n = scandir(flat, &list, NULL, alphasort);
    if (n < 0)
        return -1;
    for (int i = 0; i < n; i++)
        free(list[i]);
    free(list);
    return n;
}

static long visited;
static int visit(const char *path, const struct stat *st, int type, struct FTW *ftw)
{
    (void)path; (void)st; (void)type; (void)ftw;
    visited++;
    return 0;
}

static long walk_nftw(void)
{
    visited = 0;
    return nftw(tree, visit, 16, FTW_PHYS) == 0 ? visited : -1;
}

static long walk_fts_flat(void)
{
    char *argv[] = { flat, NULL };
    FTS *f = fts_open(argv, FTS_PHYSICAL | FTS_NOCHDIR, NULL);
    if (!f)
        return -1;
    long n = 0;
    FTSENT *e;
    while ((e = fts_read(f)))
        n += e->fts_info != FTS_DP;
    return fts_close(f) == 0 ? n : -1;
}

static long walk_fts(void)
{
    char *argv[] = { tree, NULL };
    FTS *f = fts_open(argv, FTS_PHYSICAL, NULL);
    if (!f)
        return -1;
    long n = 0;
    FTSENT *e;
    while ((e = fts_read(f)))
        n += e->fts_info != FTS_DP;
    return fts_close(f) == 0 ? n : -1;
}

static long temp(void)
{
    char t[PATH_MAX];
    strcpy(t, tmpl);
    int fd = mkstemp(t);
    if (fd < 0)
        return -1;
    long r = unlink(t);
    return close(fd) == 0 && r == 0 ? 1 : -1;
}

static long table(void)
{
    FILE *f = setmntent(mnt, "r");
    if (!f)
        return -1;
    long n = 0;
    while (getmntent(f))
        n++;
    endmntent(f);
    return n;
}

int main(void)
{
    strcpy(base, "/tmp/cbench_fstree.XXXXXX");
    if (!mkdtemp(base))
        fail("mkdtemp");
    strcpy(top, base);
    made = 1;
    /* STRESS-ENV-AUDIT-2 `deep` profile: the tree sits CBENCH_STRESS_DEPTH directories (32-byte names) below
     * the scratch directory, so every absolute path the rows use is that much longer and deeper. */
    const char *depth = getenv("CBENCH_STRESS_DEPTH");
    for (int k = 0, n = depth ? atoi(depth) : 0; k < n; k++) {
        size_t l = strlen(top);
        if (l + 40 > sizeof top - 512)
            fail("CBENCH_STRESS_DEPTH too deep");
        snprintf(top + l, sizeof top - l, "/stress-deep-level-%03d-abcdefghij", k);
        if (mkdir(top, 0700) != 0)
            fail("mkdir stress level");
        levels++;
    }
    build();
    strcpy(flat, at("flat"));
    strcpy(tree, at("tree"));
    strcpy(deep, at("p/q/r"));
    strcpy(mnt, at("mnt"));
    strcpy(tmpl, at("tmpXXXXXX"));
    if (chdir(top) != 0)
        fail("chdir");

    if (scan() != FILES + 2)
        fail("opendir/readdir");
    MEASURE("opendir_readdir_closedir_1000", 200, scan());

    DIR *d = opendir(flat);
    if (!d || rescan(d) != FILES + 2)
        fail("readdir");
    MEASURE_PER("readdir_entry", 200, FILES + 2, rescan(d));
    closedir(d);

    strcpy(small, at("tree/d000"));
    if (scan_fd() != PER_DIR + 2)
        fail("fdopendir");
    MEASURE("fdopendir_readdir_closedir_99", 2000, scan_fd());

    if (cwd() != top[1])
        fail("getcwd");
    MEASURE("getcwd", 20000, cwd());

    if (real() != (long)strlen(deep))
        fail("realpath");
    MEASURE("realpath_5", 5000, real());

    if (scan_sorted() != FILES + 2)
        fail("scandir");
    MEASURE("scandir_alphasort_1000", 100, scan_sorted());

    if (walk_nftw() != 1 + DIRS + DIRS * PER_DIR)
        fail("nftw");
    MEASURE("nftw_phys_1000", 50, walk_nftw());

    if (walk_fts() != 1 + DIRS + DIRS * PER_DIR)
        fail("fts");
    MEASURE("fts_physical_1000", 50, walk_fts());

    if (walk_fts_flat() != FILES + 1)
        fail("fts_flat");
    MEASURE("fts_nochdir_flat_1000", 50, walk_fts_flat());

    if (temp() != 1)
        fail("mkstemp");
    MEASURE("mkstemp_unlink_close", 2000, temp());

    if (table() != MNT_LINES)
        fail("getmntent");
    MEASURE("getmntent_50_lines", 2000, table());

    if (chdir("/") != 0)
        fail("chdir /");
    teardown();
    return 0;
}
