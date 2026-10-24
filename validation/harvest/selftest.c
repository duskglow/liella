/* A small dynamic program that makes the calls harvest.c logs (its own check: run with the shim
 * preloaded and HARVEST_TREE covering this program, then read the log). */
#define _GNU_SOURCE
#include <errno.h>
#include <regex.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <wchar.h>

int main(void)
{
    regex_t re;
    regmatch_t m[3];
    if (regcomp(&re, "a\\(b*\\)c", 0) == 0) {
        regexec(&re, "xxabbbc", 3, m, 0);
        regexec(&re, "nope", 3, m, 0);
        regfree(&re);
    }
    struct re_pattern_buffer b;
    memset(&b, 0, sizeof b);
    re_syntax_options = RE_SYNTAX_POSIX_EGREP;
    const char *err = re_compile_pattern("(x+)y", 5, &b);
    struct re_registers regs;
    memset(&regs, 0, sizeof regs);
    if (!err)
        re_search(&b, "aaxxxyz", 7, 0, 7, &regs);
    re_compile_pattern("(unclosed", 9, &b);
    char buf[256];
    int n = 0;
    snprintf(buf, sizeof buf, "%d %5.2f %s %ls %p %Lg %.3s %*d %2$d%n", 42, 3.25, "str", L"wide", (void *)buf, 1.5L, "truncate", 6, 7, &n);
    snprintf(buf, sizeof buf, "%2$s-%1$s %%", "one", "two");
    errno = ENOENT;
    printf("%m %c %lu %hhd %zx %jd %a\n", 'q', 123456789012UL, 300, (size_t)255, (long long)-5, 0.1);
    fprintf(stderr, "%s\n", (char *)NULL);
    return 0;
}
