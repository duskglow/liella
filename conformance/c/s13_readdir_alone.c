/* Spec §13.2 (clauses s13_2_c_2, s13_2_c_4): readdir in a process with one thread (the path
 * that takes no stream lock, DU-3) returns every entry exactly once, as whole records, across
 * several buffer refills; telldir/seekdir resume at the entry after the position taken and
 * rewinddir starts over. Each failing CHECK exits with its line number. Public interfaces only. */
#define _GNU_SOURCE
#include <dirent.h>
#include <fcntl.h>
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#define CHECK(cond) do { if (!(cond)) { rc = __LINE__; goto out; } } while (0)
#define FILES 1500

static char dir[] = "/tmp/s13rd-XXXXXX";
static unsigned char seen[FILES];

static void cleanup(void)
{
    char p[64];
    for (int i = 0; i < FILES; i++) {
        snprintf(p, sizeof p, "%s/f%04d", dir, i);
        unlink(p);
    }
    rmdir(dir);
}

int main(void)
{
    int rc = 0, dots = 0, n = 0;
    long pos = -1;
    char after[256] = "", p[64];
    DIR *d = NULL;
    if (mkdtemp(dir) == NULL)
        return __LINE__;
    for (int i = 0; i < FILES; i++) {
        snprintf(p, sizeof p, "%s/f%04d", dir, i);
        int fd = open(p, O_CREAT | O_WRONLY | O_CLOEXEC, 0600);
        CHECK(fd >= 0);
        close(fd);
    }
    d = opendir(dir);
    CHECK(d != NULL);
    for (struct dirent *e; (e = readdir(d)) != NULL; n++) {
        size_t len = strlen(e->d_name);
        CHECK(e->d_reclen % 8 == 0 && e->d_reclen >= offsetof(struct dirent, d_name) + len + 1);
        if (pos != -1 && after[0] == 0)
            memcpy(after, e->d_name, len + 1);
        if (n == FILES / 2)
            pos = telldir(d);
        if (strcmp(e->d_name, ".") == 0 || strcmp(e->d_name, "..") == 0) {
            dots++;
            continue;
        }
        int i = -1;
        CHECK(sscanf(e->d_name, "f%4d", &i) == 1 && i >= 0 && i < FILES && len == 5);
        CHECK(seen[i]++ == 0);
    }
    CHECK(n == FILES + 2 && dots == 2 && pos != -1 && after[0] != 0);
    seekdir(d, pos);
    struct dirent *e = readdir(d);
    CHECK(e != NULL && strcmp(e->d_name, after) == 0);
    rewinddir(d);
    for (n = 0; readdir(d) != NULL; n++)
        ;
    CHECK(n == FILES + 2);
out:
    if (d)
        closedir(d);
    cleanup();
    return rc;
}
