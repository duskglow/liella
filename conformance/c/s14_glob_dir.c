/* Spec 14.8 (glob over a real directory): a directory of 3000 files with 40-byte names (about
 * 150 KiB of getdents64 records, so the directory stream's buffer is refilled and enlarged
 * several times) is globbed whole, and every result is compared with the names created, in
 * order: none missing at a buffer boundary, none twice, sorted. Hidden files stay out unless
 * GLOB_PERIOD; GLOB_MARK / GLOB_ONLYDIR / patterns with several components and a literal tail
 * are checked on a small tree. Each failing CHECK exits with its line number. */
#define _GNU_SOURCE
#include <dirent.h>
#include <fcntl.h>
#include <glob.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

#define CHECK(cond) do { if (!(cond)) return __LINE__; } while (0)
#define FILES 3000

static char dir[64];

static int make(const char *name, int is_dir)
{
    char path[160];
    snprintf(path, sizeof path, "%s/%s", dir, name);
    if (is_dir)
        return mkdir(path, 0700);
    int fd = open(path, O_CREAT | O_WRONLY, 0600);
    if (fd < 0)
        return -1;
    return close(fd);
}

static void cleanup(void)
{
    char name[96], path[200];
    for (int i = 0; i < FILES; i++) {
        snprintf(name, sizeof name, "file_%04d_abcdefghijklmnopqrstuvwxyz01", i);
        snprintf(path, sizeof path, "%s/big/%s", dir, name);
        unlink(path);
    }
    const char *rest[] = {"big/.hidden", "big", "sub/inner/leaf", "sub/inner", "sub/plain", "sub/.dot", "sub", ""};
    for (int i = 0; rest[i][0]; i++) {
        snprintf(path, sizeof path, "%s/%s", dir, rest[i]);
        if (rmdir(path) != 0)
            unlink(path);
    }
    rmdir(dir);
}

static int run(void)
{
    char name[96], pat[200];
    glob_t g;
    CHECK(make("big", 1) == 0);
    CHECK(make("sub", 1) == 0 && make("sub/inner", 1) == 0 && make("sub/inner/leaf", 0) == 0);
    CHECK(make("sub/plain", 0) == 0 && make("sub/.dot", 0) == 0);
    for (int i = 0; i < FILES; i++) {
        snprintf(name, sizeof name, "big/file_%04d_abcdefghijklmnopqrstuvwxyz01", i);
        CHECK(make(name, 0) == 0);
    }
    CHECK(make("big/.hidden", 0) == 0);

    /* Everything in the big directory, sorted and complete; the hidden file is left out. */
    snprintf(pat, sizeof pat, "%s/big/*", dir);
    CHECK(glob(pat, 0, NULL, &g) == 0);
    CHECK(g.gl_pathc == FILES);
    for (int i = 0; i < FILES; i++) {
        snprintf(name, sizeof name, "%s/big/file_%04d_abcdefghijklmnopqrstuvwxyz01", dir, i);
        CHECK(strcmp(g.gl_pathv[i], name) == 0);
    }
    CHECK(g.gl_pathv[FILES] == NULL);
    globfree(&g);

    /* Unsorted: the same set (checked with a count per name), nothing twice. */
    CHECK(glob(pat, GLOB_NOSORT, NULL, &g) == 0);
    CHECK(g.gl_pathc == FILES);
    {
        static unsigned char seen[FILES];
        for (int i = 0; i < FILES; i++) {
            int n = -1;
            const char *b = strrchr(g.gl_pathv[i], '/');
            CHECK(b != NULL && sscanf(b, "/file_%d_", &n) == 1 && n >= 0 && n < FILES);
            CHECK(!seen[n]);
            seen[n] = 1;
        }
    }
    globfree(&g);

    /* GLOB_PERIOD adds ".", ".." and the hidden file (they sort before 'f'); a dot pattern finds
     * the hidden file without the flag. */
    CHECK(glob(pat, GLOB_PERIOD, NULL, &g) == 0);
    CHECK(g.gl_pathc == FILES + 3 && strstr(g.gl_pathv[2], "/big/.hidden") != NULL);
    globfree(&g);
    snprintf(pat, sizeof pat, "%s/big/.h*", dir);
    CHECK(glob(pat, 0, NULL, &g) == 0 && g.gl_pathc == 1);
    globfree(&g);

    /* A pattern that picks a tenth of the names, and one that picks one. */
    snprintf(pat, sizeof pat, "%s/big/file_??9?_*", dir);
    CHECK(glob(pat, 0, NULL, &g) == 0 && g.gl_pathc == 300);
    globfree(&g);
    snprintf(pat, sizeof pat, "%s/big/file_2999_*", dir);
    CHECK(glob(pat, 0, NULL, &g) == 0 && g.gl_pathc == 1);
    globfree(&g);
    snprintf(pat, sizeof pat, "%s/big/*3000*", dir);
    CHECK(glob(pat, 0, NULL, &g) == GLOB_NOMATCH);

    /* Several components, MARK and ONLYDIR, a literal tail after the wildcard. */
    snprintf(pat, sizeof pat, "%s/s*/*", dir);
    CHECK(glob(pat, GLOB_MARK, NULL, &g) == 0 && g.gl_pathc == 2);
    CHECK(strcmp(g.gl_pathv[0] + strlen(dir), "/sub/inner/") == 0);
    CHECK(strcmp(g.gl_pathv[1] + strlen(dir), "/sub/plain") == 0);
    globfree(&g);
    CHECK(glob(pat, GLOB_ONLYDIR, NULL, &g) == 0 && g.gl_pathc == 1);
    CHECK(strcmp(g.gl_pathv[0] + strlen(dir), "/sub/inner") == 0);
    globfree(&g);
    snprintf(pat, sizeof pat, "%s/s*/i*/leaf", dir);
    CHECK(glob(pat, 0, NULL, &g) == 0 && g.gl_pathc == 1);
    globfree(&g);
    snprintf(pat, sizeof pat, "%s/*/plain", dir);
    CHECK(glob(pat, 0, NULL, &g) == 0 && g.gl_pathc == 1);
    globfree(&g);
    return 0;
}

int main(void)
{
    snprintf(dir, sizeof dir, "/tmp/s14g-XXXXXX");
    CHECK(mkdtemp(dir) != NULL);
    int r = run();
    cleanup();
    return r;
}
