/* Spec §1.1 contract clause 4: before main runs, the standard streams stdin, stdout and stderr
 * exist on descriptors 0, 1 and 2, and the locale is "C" whatever the environment names (it is
 * set here to a locale that would change the answers). Each stream is used here for the first
 * time after its descriptor was replaced by a pipe, so what it reads or writes shows which
 * descriptor it is on. */
/* RUN-ENV: LC_ALL=de_DE.UTF-8 LANG=de_DE.UTF-8 LC_NUMERIC=de_DE.UTF-8 */
/* EXPECT-STDERR: [stderr ready] */
#include <ctype.h>
#include <locale.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#define CHECK(n, cond) do { if (!(cond)) return n; } while (0)

/* Make descriptor `fd` the read (want_read) or write end of a new pipe; the other end. */
static int pipe_on(int fd, int want_read)
{
    int p[2];
    if (pipe(p) != 0)
        return -1;
    int mine = want_read ? p[0] : p[1], other = want_read ? p[1] : p[0];
    if (dup2(mine, fd) != fd)
        return -1;
    close(mine);
    return other;
}

int main(void)
{
    CHECK(10, stdin != NULL && stdout != NULL && stderr != NULL);
    CHECK(11, fileno(stdin) == 0 && fileno(stdout) == 1 && fileno(stderr) == 2);

    /* The "C" locale: every category, and its answers. */
    const char *all = setlocale(LC_ALL, NULL);
    CHECK(20, all != NULL && strcmp(all, "C") == 0);
    const int cats[] = { LC_CTYPE, LC_NUMERIC, LC_TIME, LC_COLLATE, LC_MONETARY, LC_MESSAGES };
    for (unsigned i = 0; i < sizeof cats / sizeof cats[0]; i++) {
        const char *c = setlocale(cats[i], NULL);
        CHECK(21, c != NULL && strcmp(c, "C") == 0);
    }
    CHECK(22, strcmp(localeconv()->decimal_point, ".") == 0);
    CHECK(23, MB_CUR_MAX == 1 && !isalpha(0xE4));
    char num[16];
    snprintf(num, sizeof num, "%.1f", 1.5);
    CHECK(24, strcmp(num, "1.5") == 0);

    /* stdin reads descriptor 0. */
    int w = pipe_on(0, 1);
    CHECK(30, w >= 0 && write(w, "in", 2) == 2);
    close(w);
    CHECK(31, getc(stdin) == 'i' && getc(stdin) == 'n' && getc(stdin) == EOF);

    /* stdout writes descriptor 1. */
    int r = pipe_on(1, 0);
    CHECK(40, r >= 0);
    CHECK(41, fputs("out", stdout) >= 0 && fflush(stdout) == 0);
    char b[8] = { 0 };
    CHECK(42, read(r, b, sizeof b - 1) == 3 && strcmp(b, "out") == 0);

    /* stderr writes descriptor 2 (the runner's EXPECT-STDERR). */
    CHECK(50, fputs("[stderr ready]\n", stderr) >= 0 && fflush(stderr) == 0);
    return 0;
}
