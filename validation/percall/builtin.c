/* Built-in wl_percall set: the per-call libc work of the common coreutils paths. Each case ends with a
 * self-check (PC_CHECKING, see percall.h): the values glibc and Liella must agree on. */
#include "percall.h"
#include <time.h>

CASE(fstatat, 400000, 1) {
    int dfd = open(dir, O_RDONLY | O_DIRECTORY); struct stat st;
    for (long i = 0; i < n; i++) pc_sink += fstatat(dfd, "a", &st, AT_SYMLINK_NOFOLLOW);
    PC_CHECKING { int r = fstatat(dfd, "a", &st, AT_SYMLINK_NOFOLLOW); pc_check("%d %lld %o %ld", r, (long long)st.st_size, (unsigned)st.st_mode, (long)st.st_nlink); }
}
CASE(fstat, 100000, 1) {
    int dfd = open(dir, O_RDONLY | O_DIRECTORY); struct stat st;
    for (long i = 0; i < n; i++) pc_sink += fstat(dfd, &st);
    PC_CHECKING { int r = fstat(dfd, &st); pc_check("%d %o %ld", r, (unsigned)st.st_mode, (long)st.st_nlink); }
}
CASE(stat, 100000, 1) {
    char p[512]; struct stat st; snprintf(p, sizeof p, "%s/a", dir);
    for (long i = 0; i < n; i++) pc_sink += stat(p, &st);
    PC_CHECKING { int r = stat(p, &st); pc_check("%d %lld %o", r, (long long)st.st_size, (unsigned)st.st_mode); }
}
CASE(readdir, 400000, 39) { /* one opendir..closedir pass over 39 entries + . and .. = 41 readdir returns */
    for (long i = 0; i < n; i++) {
        DIR *d = opendir(dir); struct dirent *e;
        while ((e = readdir(d))) pc_sink += e->d_type;
        closedir(d);
    }
    PC_CHECKING {
        DIR *d = opendir(dir); struct dirent *e; long k = 0, t = 0, l = 0;
        while ((e = readdir(d))) { k++; t += e->d_type; l += (long)strlen(e->d_name); }
        closedir(d); pc_check("%ld %ld %ld", k, t, l);
    }
}
CASE(opendir_closedir, 10000, 1) {
    for (long i = 0; i < n; i++) { DIR *d = opendir(dir); pc_sink += (long)d; closedir(d); }
    PC_CHECKING { DIR *d = opendir(dir); pc_check("%d", d != NULL); if (d) closedir(d); }
}
CASE(malloc_free, 1000000, 1) {
    for (long i = 0; i < n; i++) { void *volatile p = malloc(24 + (i & 127)); free(p); }
    PC_CHECKING { void *volatile p = malloc(24 + 5); pc_check("%d %d", p != NULL, (int)((unsigned long)p & 15)); free(p); }
}
CASE(realloc_chain, 1000, 9) { /* tar's directory buffer: 2187 grown by half to 56040, then freed; 8 reallocs + malloc per iteration */
    static const size_t st[] = {3280, 4920, 7380, 11070, 16605, 24907, 37360, 56040};
    for (long i = 0; i < n; i++) {
        char *volatile p = malloc(2187); p[0] = 1;
        for (int k = 0; k < 8; k++) { p = realloc(p, st[k]); p[st[k] - 1] = 1; }
        free(p);
    }
    PC_CHECKING { char *volatile p = malloc(2187); p[0] = 7; p = realloc(p, 56040); pc_check("%d", p != NULL && p[0] == 7); free(p); }
}
CASE(fopen_fclose, 10000, 1) {
    char p[512]; snprintf(p, sizeof p, "%s/text", dir);
    for (long i = 0; i < n; i++) { FILE *f = fopen(p, "r"); pc_sink += (long)f; fclose(f); }
    PC_CHECKING { FILE *f = fopen(p, "r"); pc_check("%d %d", f != NULL, f ? fgetc(f) : -2); if (f) fclose(f); }
}
CASE(fgets_line, 1000000, 200) { /* a 200-line file read through to EOF per iteration, divided by 200 */
    char p[512], b[256]; snprintf(p, sizeof p, "%s/text", dir);
    FILE *f = fopen(p, "r");
    for (long i = 0; i < n; i++) { rewind(f); while (fgets(b, sizeof b, f)) pc_sink += b[0]; }
    PC_CHECKING { long k = 0, l = 0; rewind(f); while (fgets(b, sizeof b, f)) { k++; l += (long)strlen(b); } pc_check("%ld %ld", k, l); }
}
CASE(fwrite_short, 1000000, 1) {
    FILE *f = fopen("/dev/null", "w");
    for (long i = 0; i < n; i++) pc_sink += fwrite("hello world\n", 1, 12, f);
    PC_CHECKING pc_check("%zu", fwrite("hello world\n", 1, 12, f));
}
CASE(printf_short, 1000000, 1) {
    FILE *f = fopen("/dev/null", "w");
    for (long i = 0; i < n; i++) pc_sink += fprintf(f, "%d %s\n", (int)i, "name");
    PC_CHECKING {
        char b[64]; int volatile v = 12345;
        int r = fprintf(f, "%d %s\n", v, "name"); snprintf(b, sizeof b, "%d %s", v, "name"); pc_check("%d %s", r, b);
    }
}
CASE(setlocale_query, 10000, 1) {
    for (long i = 0; i < n; i++) pc_sink += (long)setlocale(LC_ALL, NULL);
    PC_CHECKING pc_check("%s", setlocale(LC_ALL, NULL));
}
CASE(strcoll_short, 1000000, 1) { /* in the environment's locale (wl_percall runs it in en_US.UTF-8) */
    const char *l = setlocale(LC_ALL, "");
    char *volatile a = "some_file_name.h", *volatile b = "some_file_name.c";
    for (long i = 0; i < n; i++) pc_sink += strcoll(a, b);
    PC_CHECKING { /* the probe pair, then a case-folding pair that a byte order (strcmp) gets wrong */
        char *volatile x = "a", *volatile y = "B";
        int r1 = strcoll(a, b), r2 = strcoll(x, y);
        pc_check("%s %d %d", l ? l : "(null)", PC_SIGN(r1), PC_SIGN(r2));
    }
}
CASE(getenv, 100000, 1) {
    setenv("PC_PROBE", "1", 1);
    for (long i = 0; i < n; i++) pc_sink += (long)getenv("PC_PROBE");
    PC_CHECKING { const char *v = getenv("PC_PROBE"); pc_check("%s", v ? v : "(null)"); }
}
