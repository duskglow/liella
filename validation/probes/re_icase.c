/* Milestone-6 probe (grep case-fold-titlecase, ere #38, stack-overflow): regcomp/regexec
 * results printed for the same calls under glibc and Liella. */
#include <locale.h>
#include <regex.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
static void t(const char *pat, int flags, const char *s)
{
    regex_t re;
    int rc = regcomp(&re, pat, flags);
    if (rc) { printf("[%s] flags=%d comp=%d\n", pat, flags, rc); return; }
    printf("[%s] flags=%d on [%s]: %s\n", pat, flags, s, regexec(&re, s, 0, 0, 0) ? "nomatch" : "match");
    regfree(&re);
}
int main(void)
{
    setvbuf(stdout, 0, _IONBF, 0);
    if (!setlocale(LC_ALL, "en_US.UTF-8")) puts("no locale");
    const char *cases[][3] = { {"\xc2\xb5", "\xce\x9c", "\xce\xbc"}, {"I", "i", "\xc4\xb0"},
                               {"I", "i", "\xc4\xb1"}, {"S", "s", "\xc5\xbf"} };
    for (int c = 0; c < 4; c++)
        for (int i = 0; i < 3; i++)
            for (int j = 0; j < 3; j++)
                t(cases[c][i], REG_ICASE | REG_NOSUB, cases[c][j]);
    t("a{}", REG_EXTENDED, "a");
    t("a{,2}", REG_EXTENDED, "a{,2}");
    t("a{,}", REG_EXTENDED, "a{,}");
    t("a{1,*}", REG_EXTENDED, "a{1,,,}");
    for (int n = 10; n <= 100000; n *= 10) {
        char *p = malloc(n + 1);
        memset(p, '(', n); p[n] = 0;
        regex_t re;
        int rc = regcomp(&re, p, REG_EXTENDED);
        printf("%d x '(': comp=%d\n", n, rc);
        if (!rc) regfree(&re);
        free(p);
    }
    return 0;
}
