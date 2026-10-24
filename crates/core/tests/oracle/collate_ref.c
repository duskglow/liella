/* The reference's collation for collate_differential.rs. argv[1] = locale name (found through
 * LOCPATH). Input: one hex-encoded string per line. Output: for every string its strxfrm key in
 * hex (one line each), then one line of strcoll signs for every ordered pair (i, j) as '<', '='
 * or '>', then one line of wcscoll signs (after mbstowcs; '!' where a string does not convert). */
#include <locale.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <wchar.h>

#define MAX 512
static char *str[MAX];
static wchar_t *wide[MAX];

static char sign(int r) { return r < 0 ? '<' : r > 0 ? '>' : '='; }

int main(int argc, char **argv) {
    static char line[16384];
    int n = 0;
    if (argc < 2 || !setlocale(LC_ALL, argv[1])) return 4;
    while (n < MAX && fgets(line, sizeof line, stdin)) {
        size_t h = strcspn(line, "\r\n");
        line[h] = 0;
        if (h == 1 && line[0] == '-') h = 0;
        char *s = malloc(h / 2 + 1);
        for (size_t i = 0; i < h / 2; i++) {
            unsigned v;
            if (sscanf(line + 2 * i, "%2x", &v) != 1) return 3;
            s[i] = (char)v;
        }
        s[h / 2] = 0;
        str[n] = s;
        size_t w = mbstowcs(NULL, s, 0);
        if (w != (size_t)-1) {
            wide[n] = calloc(w + 1, sizeof(wchar_t));
            mbstowcs(wide[n], s, w + 1);
        }
        n++;
    }
    for (int i = 0; i < n; i++) {
        size_t k = strxfrm(NULL, str[i], 0);
        unsigned char *b = malloc(k + 1);
        strxfrm((char *)b, str[i], k + 1);
        if (k == 0) printf("-");
        for (size_t x = 0; x < k; x++) printf("%02x", b[x]);
        printf("\n");
        free(b);
    }
    for (int i = 0; i < n; i++)
        for (int j = 0; j < n; j++) putchar(sign(strcoll(str[i], str[j])));
    putchar('\n');
    for (int i = 0; i < n; i++)
        for (int j = 0; j < n; j++) putchar(wide[i] && wide[j] ? sign(wcscoll(wide[i], wide[j])) : '!');
    putchar('\n');
    return 0;
}
