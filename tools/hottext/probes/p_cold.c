/* Hot-text probe (HOT-ORDER): the first calls of cbench_opt's cold sequence after setlocale -
 * strcoll, iconv, localtime_r/strftime/mktime, regcomp/regexec, fnmatch, wordexp, bindtextdomain +
 * dgettext - so their first-call text sits in the hot group (after the startup, stdio, string,
 * setlocale and printf probes in regen.sh's order) instead of wherever object order leaves it. */
#include <fnmatch.h>
#include <iconv.h>
#include <libintl.h>
#include <locale.h>
#include <regex.h>
#include <stdio.h>
#include <string.h>
#include <time.h>
#include <wordexp.h>

/* Through a volatile pointer, as cold.c and cbench call it: a direct call with constant arguments
 * left no strcoll call in the probe at -O2 (HOT-ORDER-2), so strcoll never reached the list. */
static int (*volatile coll_fn)(const char *, const char *) = strcoll;

int main(void)
{
    int ok = 0;
    char buf[128];
    setlocale(LC_ALL, "");
    ok += coll_fn("README.md", "Makefile") > 0;
    iconv_t cd = iconv_open("UTF-16LE", "UTF-8");
    if (cd != (iconv_t)-1) {
        char in[] = "caf\xc3\xa9", out[32];
        char *ip = in, *op = out;
        size_t il = sizeof in - 1, ol = sizeof out;
        ok += iconv(cd, &ip, &il, &op, &ol) != (size_t)-1;
        iconv_close(cd);
    }
    time_t when = 1759579200;
    struct tm r;
    if (localtime_r(&when, &r) != NULL) {
        ok += strftime(buf, sizeof buf, "%c", &r) > 0;
        ok += mktime(&r) != (time_t)-1;
    }
    regex_t re;
    regmatch_t m[3];
    if (regcomp(&re, "^\\([a-z0-9_]*\\) = \\(.*\\)$", 0) == 0) {
        ok += regexec(&re, "option_1 = value_13", 3, m, 0) == 0;
        regfree(&re);
    }
    ok += fnmatch("*.c", "main.c", 0) == 0;
    wordexp_t we;
    if (wordexp("~/a b", &we, WRDE_NOCMD) == 0) {
        ok += we.we_wordc == 2;
        wordfree(&we);
    }
    if (bindtextdomain("hottext", "/nonexistent") != NULL)
        ok += dgettext("hottext", "hello") != NULL;
    return ok == 9 ? 0 : 1;
}
