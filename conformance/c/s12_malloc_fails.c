/* T22 (spec §12, the standard streams; milestone 5): with the program's malloc failing from
 * the start of main, the standard streams and dprintf still work: stdout (line-buffered or fully
 * buffered) and stderr get their first use only now, so their buffers cannot be allocated; they
 * fall back to each stream's spare buffer, and dprintf formats on the stack. Each failing CHECK
 * exits with its line number. */
/* EXPECT-STDERR: stderr works: 7 */
/* EXPECT-STDERR: perror works: Cannot allocate memory */
#define _GNU_SOURCE
#include <errno.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

#define CHECK(cond) do { if (!(cond)) return __LINE__; } while (0)

/* The program's allocator (D-X.14): it refuses everything once main starts. */
static int refusing;
static char arena[1 << 20];
static size_t used;

void *malloc(size_t n)
{
    if (refusing || used + n + 16 > sizeof arena) {
        errno = ENOMEM;
        return NULL;
    }
    void *p = arena + used;
    used += (n + 15) & ~(size_t)15;
    return p;
}
void free(void *p) { (void)p; }
void *calloc(size_t n, size_t s)
{
    void *p = malloc(n * s);
    if (p)
        memset(p, 0, n * s);
    return p;
}
void *realloc(void *p, size_t n)
{
    void *q = malloc(n);
    if (q && p)
        memcpy(q, p, n);
    return q;
}

int main(void)
{
    refusing = 1;
    CHECK(malloc(1) == NULL);
    int p[2];
    CHECK(pipe(p) == 0);
    int saved = dup(1);
    CHECK(saved >= 0 && dup2(p[1], 1) == 1);
    CHECK(printf("stdout %s %d\n", "works", 42) == 16);
    CHECK(fputs("and more text than a spare buffer holds at once: ", stdout) >= 0);
    CHECK(printf("%0100d\n", 5) == 101);
    CHECK(fflush(stdout) == 0);
    CHECK(dup2(saved, 1) == 1);
    char got[512] = {0};
    ssize_t n = 0;
    while (n < 16 + 49 + 101) {
        ssize_t k = read(p[0], got + n, sizeof got - 1 - n);
        CHECK(k > 0);
        n += k;
    }
    CHECK(n == 16 + 49 + 101);
    CHECK(strncmp(got, "stdout works 42\nand more text", 29) == 0 && got[n - 1] == '\n' && got[n - 2] == '5');
    CHECK(fprintf(stderr, "stderr works: %d\n", 7) == 16);
    errno = ENOMEM;
    perror("perror works");
    CHECK(dprintf(p[1], "dprintf %s %d\n", "works", 3) == 16);
    n = read(p[0], got, sizeof got - 1);
    CHECK(n == 16 && strncmp(got, "dprintf works 3\n", 16) == 0);
    return 0;
}
