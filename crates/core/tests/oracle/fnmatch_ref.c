/* The reference's fnmatch for fnmatch_differential.rs: one case per input line,
 * "<locale 0|1> <flags> <hex pattern> <hex string>", one result per output line: 0, 1 or -1
 * (any other return). Locale 0 = "C", 1 = "C.UTF-8". */
#define _GNU_SOURCE
#include <fnmatch.h>
#include <locale.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int unhex(const char *h, char *out, size_t cap) {
    size_t n = strlen(h);
    if (n % 2 || n / 2 >= cap) return -1;
    for (size_t i = 0; i < n / 2; i++) {
        unsigned v;
        if (sscanf(h + 2 * i, "%2x", &v) != 1) return -1;
        out[i] = (char)v;
    }
    out[n / 2] = 0;
    return 0;
}

int main(void) {
    static char line[65536], ph[32768], sh[32768], p[16384], s[16384];
    int cur = -1;
    while (fgets(line, sizeof line, stdin)) {
        int loc, flags;
        ph[0] = sh[0] = 0;
        if (sscanf(line, "%d %d %32767s %32767s", &loc, &flags, ph, sh) < 2) return 2;
        if (!strcmp(ph, "-")) ph[0] = 0;
        if (!strcmp(sh, "-")) sh[0] = 0;
        if (unhex(ph, p, sizeof p) || unhex(sh, s, sizeof s)) return 3;
        if (loc != cur) {
            if (!setlocale(LC_ALL, loc ? "C.UTF-8" : "C")) return 4;
            cur = loc;
        }
        int r = fnmatch(p, s, flags);
        printf("%d\n", r == 0 ? 0 : r == FNM_NOMATCH ? 1 : -1);
    }
    return 0;
}
