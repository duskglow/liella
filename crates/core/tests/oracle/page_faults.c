/* Page faults of a short program (PAGEFAULTS-2): argv[1] = "true" (startup only), "strings" (see main), "locale"
 * (setlocale(LC_ALL, "") + malloc + printf, the shape of most command-line tools) or "tool" (the
 * shape of a coreutils program such as printf: HOTTEXT-FAULT-TEST). Prints the
 * process's own minor-fault count at the end (getrusage RUSAGE_SELF ru_minflt) on stderr. */
#include <inttypes.h>
#include <libintl.h>
#include <locale.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/resource.h>

static size_t (*volatile len_fn)(const char *) = strlen;
static int (*volatile ncmp_fn)(const char *, const char *, size_t) = strncmp;
static void *(*volatile mchr_fn)(const void *, int, size_t) = memchr;
static int (*volatile mcmp_fn)(const void *, const void *, size_t) = memcmp;
static char *(*volatile scpy_fn)(char *, const char *) = strcpy;

static void at_exit_hook(void) {}

static long minflt(void) {
    struct rusage ru;
    return getrusage(RUSAGE_SELF, &ru) == 0 ? ru.ru_minflt : -1000000;
}

int main(int argc, char **argv) {
    if (argc > 1 && strcmp(argv[1], "strings") == 0) {
        /* RC1-FIRST: "strings" = after printf, the first calls of the common string functions;
         * prints only the faults those calls took (their entry points are in the hot group). */
        printf("%s %d\n", "a", 5);
        fflush(stdout);
        char b[64];
        const char *volatile s = "a line of text: compared";
        long f0 = minflt();
        long n = (long)len_fn(s) + ncmp_fn(s, "a line", 6) + (mchr_fn(s, ':', 20) != NULL) + mcmp_fn(s, "a l", 3);
        n += scpy_fn(b, s) == b;
        long f1 = minflt();
        fprintf(stderr, "minflt %ld\n", n == 26 ? f1 - f0 : -1);
        return 0;
    }
    if (argc > 1 && strcmp(argv[1], "locale") == 0) {
        setlocale(LC_ALL, "");
        char *volatile p = malloc(100);
        printf("%s %d\n", p ? "a" : "b", 5);
        fflush(stdout);
        free(p);
    }
    if (argc > 1 && strcmp(argv[1], "tool") == 0) {
        /* HOTTEXT-FAULT-TEST: what a coreutils tool (printf "%s %d %x\n" a 1 1) runs before and
         * after main: setlocale, bindtextdomain/textdomain, atexit, getenv, strtoimax, printf,
         * then a stdout flush. A stale hot-text list (functions of this path outside the hot
         * group) shows as extra file-backed text faults; PRINTF-LOOP-FAULTS: 14 -> 17 pages. */
        setlocale(LC_ALL, "");
        bindtextdomain("coreutils", "/nonexistent/locale");
        textdomain("coreutils");
        atexit(at_exit_hook);
        const char *volatile e = getenv("POSIXLY_CORRECT");
        const char *volatile num = "17";
        char *end;
        intmax_t v = strtoimax(num, &end, 10);
        unsigned long x = strtoumax("1f", &end, 16);
        printf("%s %jd %lx %d\n", e ? "p" : "a", v, x, (int)(end != NULL));
        if (fflush(stdout) != 0)
            return 3;
    }
    struct rusage ru;
    if (getrusage(RUSAGE_SELF, &ru) != 0)
        return 2;
    fprintf(stderr, "minflt %ld\n", ru.ru_minflt);
    return 0;
}
