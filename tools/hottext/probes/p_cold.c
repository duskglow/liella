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
#include <fcntl.h>
#include <stdint.h>
#include <stdlib.h>
#include <sys/stat.h>
#include <unistd.h>

static void put32(unsigned char *p, uint32_t v)
{
    for (int i = 0; i < 4; i++)
        p[i] = (unsigned char)(v >> (8 * i));
}

/* A two-entry catalog (header, "hello" -> "hallo") in the .mo layout the GNU gettext manual
 * documents: magic, revision, count, the two string-table offsets, an empty hash table. */
static void write_mo(const char *path)
{
    static const char header[] = "Content-Type: text/plain; charset=UTF-8\n";
    const char *orig[2] = {"", "hello"}, *trans[2] = {header, "hallo"};
    unsigned char buf[256] = {0};
    uint32_t n = 2, o = 28, t = o + 8 * n, at = t + 8 * n;
    put32(buf, 0x950412de);
    put32(buf + 8, n);
    put32(buf + 12, o);
    put32(buf + 16, t);
    put32(buf + 24, at);
    for (uint32_t i = 0; i < 2 * n; i++) {
        const char *s = i < n ? orig[i] : trans[i - n];
        uint32_t len = (uint32_t)strlen(s), e = i < n ? o + 8 * i : t + 8 * (i - n);
        put32(buf + e, len);
        put32(buf + e + 4, at);
        memcpy(buf + at, s, len);
        at += len + 1;
    }
    int f = open(path, O_WRONLY | O_CREAT | O_TRUNC | O_CLOEXEC, 0600);
    if (f >= 0) {
        (void)!write(f, buf, at);
        close(f);
    }
}

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
    /* A real catalog, looked up twice with a same-directory rebind between (cbench's second pass,
     * COLDSTART-BINDTEXTDOMAIN-REBIND-2): the repeat lookup's code (the resolution cache's text
     * compare, the hinted catalog search) is otherwise outside the group, and the second
     * bindtextdomain + dgettext of a fresh process took two file-backed text faults. */
    char dir[] = "/tmp/hottext.XXXXXX", sub[64], mo[96];
    if (mkdtemp(dir) == NULL)
        return 1;
    snprintf(sub, sizeof sub, "%s/en_US", dir);
    mkdir(sub, 0700);
    snprintf(sub, sizeof sub, "%s/en_US/LC_MESSAGES", dir);
    mkdir(sub, 0700);
    snprintf(mo, sizeof mo, "%s/hottext.mo", sub);
    write_mo(mo);
    for (int k = 0; k < 2; k++) {
        if (bindtextdomain("hottext", dir) != NULL)
            ok += strcmp(dgettext("hottext", "hello"), "hallo") == 0;
        ok += strcmp(dgettext("hottext-absent", "no catalog"), "no catalog") == 0;
    }
    unlink(mo);
    rmdir(sub);
    snprintf(sub, sizeof sub, "%s/en_US", dir);
    rmdir(sub);
    rmdir(dir);
    return ok == 12 ? 0 : 1;
}
