/* Spec §12.8 (clauses s12_8_c_2, s12_8_c_3): ftell, ftello and fgetpos in a process with one
 * thread (the path that takes no stream lock and answers from the read window, FTELL-BOUNDARY)
 * report the logical position: after reads through fgetc and the header-inline getc_unlocked
 * (which move the window pointer themselves), across buffer refills, after in-window fseek and
 * rewind, after ungetc of the same and of a different byte, and on a wide stream after ungetwc
 * of a two-byte character. Each failing CHECK exits with its line number. Public interfaces only. */
#define _GNU_SOURCE
#include <locale.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
#include <wchar.h>

#define CHECK(cond) do { if (!(cond)) { rc = __LINE__; goto out; } } while (0)
#define N 20000

static char path[] = "/tmp/s12fa-XXXXXX";

static int pos_ok(FILE *f, long want)
{
    fpos_t p;
    if (ftell(f) != want || ftello(f) != (off_t)want || fgetpos(f, &p) != 0)
        return 0;
    return fseek(f, 0, SEEK_CUR) == 0 ? ftell(f) == want && fsetpos(f, &p) == 0 && ftell(f) == want : 0;
}

int main(void)
{
    int rc = 0;
    int fd = mkstemp(path);
    if (fd < 0)
        return 1;
    FILE *w = fdopen(fd, "w");
    if (w == NULL)
        return 2;
    for (int i = 0; i < N; i++)
        fputc('a' + i % 26, w);
    if (fclose(w) != 0)
        return 3;

    FILE *f = fopen(path, "r");
    CHECK(f != NULL);
    CHECK(fseek(f, 0, SEEK_SET) == 0); /* the offset becomes known */
    for (long i = 0; i < N; i++) {
        int c = (i & 1) ? getc_unlocked(f) : fgetc(f);
        CHECK(c == 'a' + i % 26);
        if (i % 97 == 0)
            CHECK(pos_ok(f, i + 1));
        if (i % 389 == 0) {
            CHECK(ungetc(c, f) == c && ftell(f) == i);
            CHECK(fgetc(f) == c && ftell(f) == i + 1);
            CHECK(ungetc('#', f) == '#' && ftell(f) == i);
            CHECK(fgetc(f) == '#' && ftell(f) == i + 1);
        }
    }
    CHECK(pos_ok(f, N));
    CHECK(fgetc(f) == EOF && pos_ok(f, N));
    CHECK(fseek(f, 4100, SEEK_SET) == 0 && fgetc(f) == 'a' + 4100 % 26 && pos_ok(f, 4101));
    CHECK(fseek(f, 4096, SEEK_SET) == 0 && pos_ok(f, 4096));
    CHECK(fseek(f, -10, SEEK_CUR) == 0 && pos_ok(f, 4086) && fgetc(f) == 'a' + 4086 % 26);
    rewind(f);
    CHECK(pos_ok(f, 0) && fgetc(f) == 'a' && pos_ok(f, 1));
    fclose(f);
    f = NULL;

    /* A wide stream: a character given back counts as its encoded length. */
    CHECK(setlocale(LC_ALL, "C.UTF-8") != NULL);
    w = fopen(path, "w");
    CHECK(w != NULL && fputs("x\xc3\xa9y", w) >= 0 && fclose(w) == 0);
    f = fopen(path, "r");
    CHECK(f != NULL && fseek(f, 0, SEEK_SET) == 0);
    CHECK(fgetwc(f) == L'x' && ftell(f) == 1);
    CHECK(fgetwc(f) == 0xe9 && ftell(f) == 3);
    CHECK(ungetwc(0xe9, f) == 0xe9 && ftell(f) == 1);
    fpos_t p;
    CHECK(fgetpos(f, &p) == 0);
    CHECK(fgetwc(f) == 0xe9 && fgetwc(f) == L'y' && ftell(f) == 4);
    CHECK(fsetpos(f, &p) == 0 && fgetwc(f) == 0xe9 && ftell(f) == 3);
    /* A seek inside the window drops a character given back. */
    CHECK(ungetwc(L'Z', f) == L'Z' && fseek(f, 0, SEEK_SET) == 0 && fgetwc(f) == L'x' && ftell(f) == 1);
out:
    if (f != NULL)
        fclose(f);
    unlink(path);
    return rc;
}
