/* Spec §3.3 mtrace/muntrace (D-3.5 as decided in phase 13): with MALLOC_TRACE naming a file,
 * mtrace creates it (0666 less the umask) and logs every allocation and release of the malloc
 * family until muntrace; without MALLOC_TRACE it does nothing. errno is left alone. */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <mcheck.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

#define CHECK(clause, cond) do { if (!(cond)) { unlink(path); return __LINE__; } } while (0)

static char path[512];
static char text[8192];

static int count(const char *needle)
{
    int n = 0;
    for (const char *s = text; (s = strstr(s, needle)) != NULL; s++)
        n++;
    return n;
}

int main(void)
{
    const char *dir = getenv("TMPDIR");
    snprintf(path, sizeof path, "%s/liella-mtrace-%d", dir && *dir ? dir : "/tmp", (int)getpid());
    unlink(path);

    /* Without MALLOC_TRACE: nothing. */
    unsetenv("MALLOC_TRACE");
    errno = EINTR;
    mtrace();
    free(malloc(5));
    muntrace();
    CHECK("s3_3_c_3", errno == EINTR);

    setenv("MALLOC_TRACE", path, 1);
    mode_t um = umask(022);
    errno = EINTR;
    mtrace();
    CHECK("s3_3_c_3", errno == EINTR);
    void *volatile a = malloc(10);
    void *volatile b = calloc(4, 8);
    a = realloc(a, 100);
    free(b);
    free(NULL);
    muntrace();
    void *volatile after = malloc(3);
    free(after);
    free(a);
    umask(um);

    struct stat st;
    CHECK("s3_3_c_3", stat(path, &st) == 0 && (st.st_mode & 0777) == 0644);
    int fd = open(path, O_RDONLY);
    CHECK("s3_3_c_3", fd >= 0);
    ssize_t n = read(fd, text, sizeof text - 1);
    close(fd);
    CHECK("s3_3_c_3", n > 0);
    text[n] = 0;
    CHECK("s3_3_c_3", strncmp(text, "= Start\n", 8) == 0);
    CHECK("s3_3_c_3", n >= 6 && strcmp(text + n - 6, "= End\n") == 0);
    /* malloc(10) and calloc(4, 8); the realloc as its old and new block; the calloc block's
     * release; nothing for free(NULL), nothing after muntrace. */
    CHECK("s3_3_c_3", count(" + 0x") == 2 && count(" 0xa\n") == 1 && count(" 0x20\n") == 1);
    CHECK("s3_3_c_3", count(" < 0x") == 1 && count(" > 0x") == 1 && count(" 0x64\n") == 1);
    CHECK("s3_3_c_3", count(" - 0x") == 1 && count("\n@ ") == 5);
    unlink(path);
    return 0;
}
