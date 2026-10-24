/* Spec §12.5 and §12.13, D-12.2/D-12.11: the buffer window of FILE. Compiled with optimisation,
 * <stdio.h> makes getc_unlocked, putc_unlocked, feof_unlocked and ferror_unlocked header-inline
 * code: they move the window pointers of the FILE object themselves and call __uflow or
 * __overflow only when the window is empty or full. A file larger than any buffer is written
 * and read back through them alone, so both entry points are reached many times. */
#define _DEFAULT_SOURCE
#include <stdio.h>
#include <unistd.h>

#define CHECK(n, cond) do { if (!(cond)) return n; } while (0)

enum { N = 100000 };

static int run(FILE *f, int fd)
{
    for (int i = 0; i < N; i++)
        CHECK(3, putc_unlocked('a' + i % 26, f) == 'a' + i % 26);
    CHECK(4, putc_unlocked(0x1ff, f) == 0xff);                   /* as unsigned char */
    CHECK(5, fflush(f) == 0);
    CHECK(6, lseek(fd, 0, SEEK_END) == N + 1);                   /* every byte reached the file */
    rewind(f);
    for (int i = 0; i < N; i++)
        CHECK(7, getc_unlocked(f) == 'a' + i % 26);
    CHECK(8, getc_unlocked(f) == 0xff && !feof_unlocked(f));
    CHECK(9, getc_unlocked(f) == EOF && feof_unlocked(f) && !ferror_unlocked(f));
    /* A byte given back is in the window the inline getc reads (D-12.1). */
    rewind(f);
    CHECK(10, getc_unlocked(f) == 'a' && ungetc('Z', f) == 'Z');
    CHECK(11, getc_unlocked(f) == 'Z' && getc_unlocked(f) == 'b');
    /* The entry points called directly: __uflow takes the next byte. */
    CHECK(12, __uflow(f) == 'c' && getc_unlocked(f) == 'd');
    return 0;
}

int main(void)
{
    FILE *f = tmpfile();
    CHECK(1, f != NULL);
    int fd = fileno(f);
    CHECK(2, fd >= 0);
    int r = run(f, fd);
    fclose(f);
    return r;
}
