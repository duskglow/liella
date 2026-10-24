/* strverscmp end to end (validation T18): built against the host C library and against
 * Liella's products, the outputs must be identical (versions_products.rs). Every pair of
 * strings of 0 to 4 characters over "012a." (781 strings): one line per left string, one sign
 * character per right string. */
#define _GNU_SOURCE
#include <stdio.h>
#include <string.h>

static char s[781][5];
static int n;

static void gen(char *b, int len)
{
    b[len] = 0;
    memcpy(s[n++], b, (size_t)len + 1);
    if (len == 4)
        return;
    static const char al[] = "012a.";
    for (int i = 0; i < 5; i++) {
        b[len] = al[i];
        gen(b, len + 1);
    }
}

int main(void)
{
    char b[5];
    gen(b, 0);
    for (int i = 0; i < n; i++) {
        for (int j = 0; j < n; j++) {
            int c = strverscmp(s[i], s[j]);
            putchar(c < 0 ? '<' : c > 0 ? '>' : '=');
        }
        printf(" [%s]\n", s[i]);
    }
    return 0;
}
