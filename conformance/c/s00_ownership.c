/* Spec C0.7 (clause s0_6_c0_7_0): pointers the caller passes are borrowed for the call only;
 * a returned pointer is (a) caller-owned heap memory released with free, (b) library storage
 * (C0.2), or (c) a view into the caller's storage, as each entry states:
 *  - borrowed: setenv, fopen's mode, regcomp's pattern, strdup's source and getaddrinfo-style
 *    inputs are copied, so changing the caller's buffer afterwards changes nothing; putenv
 *    states otherwise (the string becomes part of the environment);
 *  - (a) strdup, realpath(path, NULL), getcwd(NULL, 0), asprintf, open_memstream's buffer,
 *    scandir's list: blocks of the malloc family (malloc_usable_size covers them; free works);
 *  - (b) the C0.2 functions (s00_threads.c);
 *  - (c) strchr, strstr, strtok_r, realpath(path, buf), getcwd(buf, n), strerror_r's GNU form
 *    when it uses the buffer: addresses inside the caller's object.
 * Each failing CHECK exits with its line number. */
#define _GNU_SOURCE
#include <dirent.h>
#include <limits.h>
#include <malloc.h>
#include <regex.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#define CHECK(cond) do { if (!(cond)) return __LINE__; } while (0)
#define INSIDE(p, obj) ((const char *)(p) >= (const char *)(obj) && (const char *)(p) < (const char *)(obj) + sizeof(obj))

static int only_dirs(const struct dirent *d) { return d->d_name[0] != '.'; }

int main(void)
{
    /* Borrowed for the call: copies are taken. */
    char value[] = "first";
    CHECK(setenv("C07_VAL", value, 1) == 0);
    value[0] = 'X';
    CHECK(strcmp(getenv("C07_VAL"), "first") == 0);
    char pattern[] = "^ab+$";
    regex_t re;
    CHECK(regcomp(&re, pattern, REG_EXTENDED | REG_NOSUB) == 0);
    memset(pattern, 0, sizeof pattern);
    CHECK(regexec(&re, "abbb", 0, NULL, 0) == 0);
    regfree(&re);
    char mode[] = "r";
    FILE *f = fopen("/dev/null", mode);
    mode[0] = 'w';
    CHECK(f != NULL && fputc('x', f) == EOF);
    fclose(f);
    /* putenv says otherwise: the caller's string is the environment entry. */
    static char entry[] = "C07_PUT=one";
    CHECK(putenv(entry) == 0);
    entry[8] = 'X';
    CHECK(strcmp(getenv("C07_PUT"), "Xne") == 0);

    /* (a) caller-owned heap blocks. */
    char *d = strdup("owned");
    CHECK(d && malloc_usable_size(d) >= 6);
    free(d);
    char *rp = realpath("/", NULL);
    CHECK(rp && strcmp(rp, "/") == 0 && malloc_usable_size(rp) >= 2);
    free(rp);
    char *cwd = getcwd(NULL, 0);
    CHECK(cwd && malloc_usable_size(cwd) > strlen(cwd));
    free(cwd);
    char *as;
    CHECK(asprintf(&as, "%d-%s", 7, "x") == 3 && malloc_usable_size(as) >= 4);
    free(as);
    char *mb;
    size_t ms;
    FILE *m = open_memstream(&mb, &ms);
    CHECK(m && fputs("memory", m) >= 0 && fclose(m) == 0);
    CHECK(ms == 6 && malloc_usable_size(mb) >= 7);
    free(mb);
    struct dirent **list;
    int n = scandir("/", &list, only_dirs, alphasort);
    CHECK(n > 0 && malloc_usable_size(list) >= (size_t)n * sizeof *list);
    for (int i = 0; i < n; i++) {
        CHECK(malloc_usable_size(list[i]) > 0);
        free(list[i]);
    }
    free(list);

    /* (c) views into the caller's storage. */
    char text[] = "alpha,beta,gamma";
    CHECK(INSIDE(strchr(text, 'b'), text) && INSIDE(strstr(text, "gam"), text));
    char *save;
    char *tok = strtok_r(text, ",", &save);
    CHECK(tok == text && INSIDE(save, text));
    char buf[PATH_MAX];
    CHECK(realpath("/", buf) == buf);
    char cb[PATH_MAX];
    CHECK(getcwd(cb, sizeof cb) == cb);
    char eb[256];
    char *em = strerror_r(12345, eb, sizeof eb);
    CHECK(em != NULL);
    return 0;
}
