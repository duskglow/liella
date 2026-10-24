/* Patterns (phase 12): fnmatch, glob, regular expressions and wordexp (§14.7-14.10; D-14.4,
 * D-14.7). The same file builds against the products (`cargo xtask bench`) and against the host C
 * library (`cc -O2 bench/c/cbench_patterns.c`), so benchmarks/progress.md compares identical
 * code. Prints `name<TAB>ns/op` lines, the best of several rounds; the lines are formatted by hand
 * so that the measured functions do not format their own results.
 *
 * Rows: fnmatch with a star, a bracket and FNM_PATHNAME; glob over a directory of 1,000 files,
 * sorted and GLOB_NOSORT (a scratch directory the program creates under /tmp and removes at the
 * end; nothing outside it is touched); for regular expressions, compiling and matching together
 * (what a one-shot caller pays), matching with a compiled pattern over a 1 KiB line, with
 * subexpressions, with a back-reference, and a pattern that is exponential for a backtracking
 * matcher on a 4 KiB subject (linear here, D-14.4); wordexp of a tilde and a variable with
 * WRDE_NOCMD, and of an arithmetic expansion (no command substitution: nothing runs a shell).
 * Every call is checked once before it is timed; a call that fails ends the program. */
#define _GNU_SOURCE
#include <fcntl.h>
#include <fnmatch.h>
#include <glob.h>
#include <regex.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <time.h>
#include <unistd.h>
#include <wordexp.h>
#include "cbench_counters.h"

static volatile long sink;

static double now(void)
{
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (double)ts.tv_sec * 1e9 + (double)ts.tv_nsec;
}

static void put(const char *name, double ns)
{
    char buf[64];
    int n = 0;
    while (name[n] && n < 40) {
        buf[n] = name[n];
        n++;
    }
    buf[n++] = '\t';
    unsigned long v = (unsigned long)(ns * 100.0 + 0.5);
    char digits[24];
    int d = 0;
    do {
        digits[d++] = (char)('0' + v % 10);
        v /= 10;
    } while (v || d < 3);
    while (d > 2)
        buf[n++] = digits[--d];
    buf[n++] = '.';
    buf[n++] = digits[1];
    buf[n++] = digits[0];
    buf[n++] = '\n';
    (void)!write(1, buf, (size_t)n);
}

static void teardown(void);

/* Report, remove the scratch directory, exit. */
static void fail(const char *what)
{
    (void)!write(2, "cbench_patterns: unexpected result: ", 36);
    (void)!write(2, what, strlen(what));
    (void)!write(2, "\n", 1);
    teardown();
    exit(1);
}

#include "cbench_rows.h"

#define MEASURE(name, iters, expr)                         \
    do {                                                   \
        if (cb_skip(name))                                 \
            break;                                         \
        double best = 1e300;                               \
        ctr_begin();                                       \
        for (int round = 0; round < 5; round++) {          \
            double t0 = now();                             \
            long acc = 0;                                  \
            for (long i = 0; i < (iters); i++)             \
                acc += (long)(expr);                       \
            sink = acc;                                    \
            double per = (now() - t0) / (double)(iters);   \
            if (per < best)                                \
                best = per;                                \
        }                                                  \
        ctr_end(name, 5.0 * (double)(iters));              \
        put(name, best);                                   \
    } while (0)

static char line[1025];
static char aaaa[4097];

/* regcomp + regexec + regfree: 0 on a match. */
static long once(const char *pat, int cflags, const char *subj, size_t nmatch)
{
    regex_t re;
    regmatch_t m[4];
    if (regcomp(&re, pat, cflags) != 0)
        return 100;
    int r = regexec(&re, subj, nmatch, m, 0);
    regfree(&re);
    return r;
}

static long exec(const regex_t *re, const char *subj, size_t nmatch)
{
    regmatch_t m[4];
    return regexec(re, subj, nmatch, m, 0);
}

static regex_t compiled(const char *pat, int cflags)
{
    regex_t re;
    if (regcomp(&re, pat, cflags) != 0)
        fail(pat);
    return re;
}

/* The glob directory: /tmp/cbench_patterns.XXXXXX/f000 ... f999. */
#define FILES 1000
static char top[64];
static int made; /* top exists */
static char globpat[96];

static const char *file_at(int i)
{
    static char p[96];
    size_t n = strlen(top);
    memcpy(p, top, n);
    p[n] = '/';
    p[n + 1] = 'f';
    p[n + 2] = (char)('0' + i / 100 % 10);
    p[n + 3] = (char)('0' + i / 10 % 10);
    p[n + 4] = (char)('0' + i % 10);
    p[n + 5] = 0;
    return p;
}

static void build(void)
{
    strcpy(top, "/tmp/cbench_patterns.XXXXXX");
    if (!mkdtemp(top))
        fail("mkdtemp");
    made = 1;
    for (int i = 0; i < FILES; i++) {
        int fd = open(file_at(i), O_WRONLY | O_CREAT | O_EXCL, 0600);
        if (fd < 0)
            fail("create");
        close(fd);
    }
    strcpy(globpat, top);
    strcat(globpat, "/f*");
}

static void teardown(void)
{
    if (!made)
        return;
    made = 0;
    for (int i = 0; i < FILES; i++)
        unlink(file_at(i));
    rmdir(top);
}

/* glob + globfree: the matches. */
static long globbed(int flags)
{
    glob_t g;
    if (glob(globpat, flags, NULL, &g) != 0)
        return -1;
    long n = (long)g.gl_pathc;
    globfree(&g);
    return n;
}

/* wordexp + wordfree: the words; `want` the second word, if given. */
static long words(const char *text, int flags, const char *want)
{
    wordexp_t we;
    if (wordexp(text, &we, flags) != 0)
        return -1;
    long n = (long)we.we_wordc;
    if (want && (n < 2 || strcmp(we.we_wordv[1], want) != 0))
        n = -2;
    wordfree(&we);
    return n;
}

int main(void)
{
    /* A 1 KiB line of words with an address near its end. */
    for (int i = 0; i < 1024; i++)
        line[i] = "lorem ipsum dolor sit amet "[i % 27];
    memcpy(line + 990, "mail bob@example.com now", 24);
    line[1024] = 0;
    memset(aaaa, 'a', 4096);
    aaaa[4096] = 0;

    const char *email = "([a-z]+)@([a-z]+)\\.com";
    if (once(email, REG_EXTENDED, line, 3) != 0)
        fail("email once");
    MEASURE("regcomp_regexec_regfree_email", 2000, once(email, REG_EXTENDED, line, 3));

    regex_t lit = compiled("example", REG_EXTENDED | REG_NOSUB);
    if (exec(&lit, line, 0) != 0)
        fail("literal");
    MEASURE("regexec_literal_1k", 5000, exec(&lit, line, 0));

    regex_t sub = compiled(email, REG_EXTENDED);
    if (exec(&sub, line, 3) != 0)
        fail("email");
    MEASURE("regexec_submatch_1k", 2000, exec(&sub, line, 3));

    regex_t cls = compiled("[[:digit:]]+", REG_EXTENDED);
    if (exec(&cls, line, 1) != REG_NOMATCH)
        fail("class");
    MEASURE("regexec_class_nomatch_1k", 2000, exec(&cls, line, 1));

    regex_t br = compiled("\\(b[a-z]*\\)@\\1", 0);
    if (exec(&br, "xx bob@bob yy", 2) != 0)
        fail("backref");
    MEASURE("regexec_backref", 20000, exec(&br, "xx bob@bob yy", 2));

    regex_t patho = compiled("(a|aa)*c", REG_EXTENDED);
    if (exec(&patho, aaaa, 2) != REG_NOMATCH)
        fail("pathological");
    MEASURE("regexec_pathological_4k", 20, exec(&patho, aaaa, 2));

    regfree(&lit);
    regfree(&sub);
    regfree(&cls);
    regfree(&br);
    regfree(&patho);

    if (fnmatch("*.c", "cbench_patterns.c", 0) != 0)
        fail("fnmatch star");
    MEASURE("fnmatch_star", 100000, fnmatch("*.c", "cbench_patterns.c", 0));
    if (fnmatch("[a-m]*_[!0-9]*.[ch]", "cbench_patterns.c", 0) != 0)
        fail("fnmatch bracket");
    MEASURE("fnmatch_bracket", 100000, fnmatch("[a-m]*_[!0-9]*.[ch]", "cbench_patterns.c", 0));
    if (fnmatch("src/*/x*.c", "src/lib/xdir.c", FNM_PATHNAME) != 0)
        fail("fnmatch pathname");
    MEASURE("fnmatch_pathname", 100000, fnmatch("src/*/x*.c", "src/lib/xdir.c", FNM_PATHNAME));
    if (fnmatch("src/*.c", "src/lib/xdir.c", FNM_PATHNAME) != FNM_NOMATCH)
        fail("fnmatch pathname no match");
    MEASURE("fnmatch_pathname_nomatch", 100000, fnmatch("src/*.c", "src/lib/xdir.c", FNM_PATHNAME));

    build();
    if (globbed(0) != FILES)
        fail("glob");
    MEASURE("glob_1000_sorted", 50, globbed(0));
    if (globbed(GLOB_NOSORT) != FILES)
        fail("glob nosort");
    MEASURE("glob_1000_nosort", 50, globbed(GLOB_NOSORT));
    teardown();

    if (!getenv("HOME") || words("~/a $HOME b", WRDE_NOCMD, NULL) != 3)
        fail("wordexp");
    MEASURE("wordexp_tilde_var", 20000, words("~/a $HOME b", WRDE_NOCMD, NULL));
    if (words("x $((3*(4+5)-7)) y", WRDE_NOCMD, "20") != 3)
        fail("wordexp arithmetic");
    MEASURE("wordexp_arith", 20000, words("x $((3*(4+5)-7)) y", WRDE_NOCMD, "20"));
    return 0;
}
