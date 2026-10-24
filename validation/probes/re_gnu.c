/* Milestone-6 probe (T28): re_compile_pattern of interval edge cases under GNU syntaxes,
 * printed under glibc and Liella. */
#define _GNU_SOURCE
#include <regex.h>
#include <stdio.h>
#include <string.h>
int main(void)
{
    setvbuf(stdout, 0, _IONBF, 0);
    const char *pats[] = { "a{}", "a{1,*}", "a{,2}", "a{,}", "a{1", "a{x}", "a{2,1}", "{1}a", "a|{1}b" };
    struct { const char *name; reg_syntax_t s; } syn[] = {
        { "POSIX_EGREP", RE_SYNTAX_POSIX_EGREP }, { "EGREP", RE_SYNTAX_EGREP },
        { "POSIX_EXTENDED", RE_SYNTAX_POSIX_EXTENDED },
        { "EGREP|INVALID_INTERVAL_ORD", RE_SYNTAX_EGREP | RE_INVALID_INTERVAL_ORD },
        { "POSIX_EGREP|NO_EMPTY_RANGES", RE_SYNTAX_POSIX_EGREP | RE_NO_EMPTY_RANGES },
    };
    for (size_t i = 0; i < sizeof syn / sizeof syn[0]; i++)
        for (size_t j = 0; j < sizeof pats / sizeof pats[0]; j++) {
            struct re_pattern_buffer b;
            memset(&b, 0, sizeof b);
            re_set_syntax(syn[i].s);
            const char *e = re_compile_pattern(pats[j], strlen(pats[j]), &b);
            printf("%s [%s]: %s\n", syn[i].name, pats[j], e ? e : "ok");
            if (!e) regfree(&b);
        }
    return 0;
}
