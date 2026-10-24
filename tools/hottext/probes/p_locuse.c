/* Hot-text probe (HOT-TEXT-LOCALE): a typical coreutils start in en_US.UTF-8 - setlocale(LC_ALL, "") then the
 * first locale uses (message domain, codeset, MB_CUR_MAX, localeconv, ctype tables, collation) and the usual
 * exit path (atexit, __fpending, fclose(stdout)). FAULTS-1: these ran from pages outside the hot group in the
 * printf / md5sum / ls / sort workloads (en_US 78 text+data faults vs C 60). regen.sh traces it in en_US. */
#include <ctype.h>
#include <errno.h>
#include <getopt.h>
#include <langinfo.h>
#include <libintl.h>
#include <locale.h>
#include <stdio.h>
#include <stdio_ext.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static void done(void) { if (__fpending(stdout) >= 0) fclose(stdout); }

int main(int argc, char **argv) {
    static const struct option longs[] = {{"help", no_argument, 0, 'h'}, {0, 0, 0, 0}};
    char *volatile a = argc > 0 ? argv[0] : "x";
    char *volatile num = "42";
    setlocale(LC_ALL, "");
    bindtextdomain("coreutils", "/usr/share/locale");
    textdomain("coreutils");
    atexit(done);
    while (getopt_long(argc, argv, "h", longs, 0) != -1) {}
    const char *cs = nl_langinfo(CODESET);
    struct lconv *lc = localeconv();
    errno = 0;
    unsigned long u = strtoul(num, 0, 10);
    long long l = strtoll(num, 0, 10);
    int (*volatile up_fn)(int) = toupper, (*volatile alpha_fn)(int) = isalpha; /* not the header macros */
    int up = up_fn((unsigned char)a[0]) + alpha_fn((unsigned char)a[0]);
    int c = strcoll(a, num);
    printf("%s %s %d %lu %lld %d %d %d %d\n", cs, lc->decimal_point, (int)MB_CUR_MAX, u, l, up, c, isatty(1),
           errno);
    return 0;
}
