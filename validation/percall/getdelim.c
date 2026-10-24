/* GETDELIM-3: getline per call, in-window only (1 MiB buffer) vs default buffer (refills) */
#include "percall.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static FILE *gd_file(const char *dir, const char *name, int big, char *vbuf) {
    char p[512], line[80];
    snprintf(p, sizeof p, "%s/%s", dir, name);
    FILE *w = fopen(p, "w");
    memset(line, 'a', sizeof line);
    line[79] = '\n';
    for (int i = 0; i < 800; i++) fwrite(line, 1, 80, w);
    fclose(w);
    FILE *f = fopen(p, "r");
    if (big) setvbuf(f, vbuf, _IOFBF, 1 << 20);
    return f;
}

static char gd_vbuf[1 << 20];

CASE(gl_big, 2000, 800) {
    FILE *f = gd_file(dir, "gdbig", 1, gd_vbuf);
    char *lp = malloc(256); size_t cap = 256;
    for (long i = 0; i < n; i++) { rewind(f); while (getline(&lp, &cap, f) > 0) pc_sink += lp[0]; }
    PC_CHECKING { long k = 0, l = 0; ssize_t r; rewind(f); while ((r = getline(&lp, &cap, f)) > 0) { k++; l += r; } pc_check("%ld %ld", k, l); }
}

CASE(gl_4k, 2000, 800) {
    FILE *f = gd_file(dir, "gd4k", 0, 0);
    char *lp = malloc(256); size_t cap = 256;
    for (long i = 0; i < n; i++) { rewind(f); while (getline(&lp, &cap, f) > 0) pc_sink += lp[0]; }
    PC_CHECKING { long k = 0, l = 0; ssize_t r; rewind(f); while ((r = getline(&lp, &cap, f)) > 0) { k++; l += r; } pc_check("%ld %ld", k, l); }
}
