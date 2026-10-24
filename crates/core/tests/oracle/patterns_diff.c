/* Patterns end to end (spec §14.7-14.10; D-14.4, D-14.5): fnmatch, glob/globfree/glob_pattern_p,
 * the POSIX and GNU regex interfaces, wordexp/wordfree. Built against the host C library and
 * against Liella's products, the outputs must match line for line. Black-box: every line is a
 * result of a public call. Run by patterns_run.sh PROGRAM SCRATCH, which makes the scratch
 * directory and the locales; the program writes its fixture there.
 *
 * Output conventions: strings are printed in brackets with bytes outside 0x20-0x7e and the
 * backslash written as \xHH; the scratch directory is printed as <dir>, the caller's PID as
 * <pid>, the passwd home of the caller as <home>. Each section runs in a forked child with an
 * alarm and a parent-death signal, so a hang or crash shows as one status line and leaves
 * nothing behind.
 *
 * Usage: patterns_diff SCRATCH [SECTION]       (SECTION runs one section only)
 *        built with -DPATTERNS_COMPAT_GLOB:  patterns_diff SCRATCH compat
 *        (glob@GLIBC_2.2.5 against the current glob; host builds only, see patterns.md). */
#define _GNU_SOURCE
#define _REGEX_RE_COMP
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <fnmatch.h>
#include <glob.h>
#include <locale.h>
#include <pwd.h>
#include <regex.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/prctl.h>
#include <sys/stat.h>
#include <sys/statfs.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <unistd.h>
#include <wordexp.h>
#include "tscale.h"

static char g_dir[4096];  /* the scratch directory (absolute) */
static size_t g_dirlen;
static char g_home[4096]; /* passwd home of the caller */
static char g_pid[32];
static const char *g_argv0; /* the program's argv[0] */

/* ------------------------------------------------------------------ output */

static void rawesc(const char *s, size_t n)
{
    for (size_t i = 0; i < n; i++) {
        unsigned char c = (unsigned char)s[i];
        if (c == '\\' || c < 0x20 || c >= 0x7f)
            printf("\\x%02x", c);
        else
            putchar(c);
    }
}

/* A string with the scratch directory, home and PID replaced. */
static void sub(const char *s)
{
    if (!s) {
        fputs("(null)", stdout);
        return;
    }
    while (*s) {
        if (g_dirlen && strncmp(s, g_dir, g_dirlen) == 0) {
            fputs("<dir>", stdout);
            s += g_dirlen;
            continue;
        }
        size_t hl = strlen(g_home);
        if (hl > 1 && strncmp(s, g_home, hl) == 0 && (s[hl] == 0 || s[hl] == '/')) {
            fputs("<home>", stdout);
            s += hl;
            continue;
        }
        rawesc(s, 1);
        s++;
    }
}

static void q(const char *s)
{
    if (!s) {
        fputs("(null)", stdout);
        return;
    }
    if (g_pid[0] && strcmp(s, g_pid) == 0) {
        fputs("[<pid>]", stdout);
        return;
    }
    if (g_argv0 && strcmp(s, g_argv0) == 0) {
        fputs("[<argv0>]", stdout);
        return;
    }
    putchar('[');
    sub(s);
    putchar(']');
}

static void nl(void)
{
    putchar('\n');
    fflush(stdout);
}

static int use_locale(const char *name)
{
    const char *r = setlocale(LC_ALL, name);
    printf("-- locale %s: %s\n", name, r ? "ok" : "unavailable");
    fflush(stdout);
    return r != NULL;
}

static const char *locales[] = {"C", "C.UTF-8", "en_US.UTF-8"};
#define NLOC (sizeof locales / sizeof *locales)

/* Run CODE in a forked child (its output goes through the same stdout) and report how the
 * child ended when that was not a normal exit 0. For calls that may abort or crash. */
static void child_status(const char *label, int st)
{
    if (WIFEXITED(st) && WEXITSTATUS(st) == 0)
        return;
    if (WIFSIGNALED(st))
        printf("%s: child killed by signal %d\n", label, WTERMSIG(st));
    else
        printf("%s: child exit %d\n", label, WEXITSTATUS(st));
    fflush(stdout);
}
#define IN_CHILD(label, ...)                        \
    do {                                            \
        fflush(stdout);                             \
        pid_t cp_ = fork();                         \
        if (cp_ == 0) {                             \
            tw_alarm(30);                              \
            signal(SIGABRT, SIG_DFL);               \
            prctl(PR_SET_DUMPABLE, 0);              \
            __VA_ARGS__;                            \
            fflush(stdout);                         \
            _exit(0);                               \
        }                                           \
        int cst_ = 0;                               \
        while (waitpid(cp_, &cst_, 0) < 0 && errno == EINTR) { \
        }                                           \
        child_status(label, cst_);                  \
    } while (0)

/* ------------------------------------------------------------------ fnmatch */

#define P FNM_PATHNAME
#define PER FNM_PERIOD
#define NE FNM_NOESCAPE
#define LD FNM_LEADING_DIR
#define CF FNM_CASEFOLD
#define X FNM_EXTMATCH

static void fnm_flags(int f)
{
    if (!f) {
        fputs("0", stdout);
        return;
    }
    const char *sep = "";
    static const struct {
        int f;
        const char *n;
    } t[] = {{FNM_NOESCAPE, "NOESCAPE"}, {FNM_PATHNAME, "PATHNAME"}, {FNM_PERIOD, "PERIOD"}, {FNM_LEADING_DIR, "LEADING_DIR"}, {FNM_CASEFOLD, "CASEFOLD"}, {FNM_EXTMATCH, "EXTMATCH"}};
    for (size_t i = 0; i < sizeof t / sizeof *t; i++)
        if (f & t[i].f) {
            printf("%s%s", sep, t[i].n);
            sep = "|";
            f &= ~t[i].f;
        }
    if (f)
        printf("%s0x%x", sep, f);
}

struct fc {
    const char *p, *s;
    int f;
};

static void fnm(const char *p, const char *s, int f)
{
    errno = 0;
    int r = fnmatch(p, s, f);
    fputs("fnmatch ", stdout);
    q(p);
    putchar(' ');
    q(s);
    putchar(' ');
    fnm_flags(f);
    printf(" = %d", r);
    nl();
}

static const struct fc fcases[] = {
    /* basics */
    {"abc", "abc", 0}, {"abc", "abd", 0}, {"abc", "ab", 0}, {"a?c", "abc", 0}, {"a?c", "ac", 0}, {"a*c", "ac", 0},
    {"a*c", "abbbc", 0}, {"a*c", "abcd", 0}, {"*", "", 0}, {"?", "", 0}, {"", "", 0}, {"", "a", 0}, {"a", "", 0},
    {"**", "abc", 0}, {"*a*", "bab", 0}, {"*.c", "x.c", 0}, {"*?*?", "ab", 0}, {"*?*?", "a", 0}, {"a*b*c", "aXbYbZc", 0},
    /* bracket expressions */
    {"[abc]", "b", 0}, {"[abc]", "d", 0}, {"[abc]", "", 0}, {"[!abc]", "d", 0}, {"[!abc]", "a", 0}, {"[^abc]", "d", 0},
    {"[^abc]", "a", 0}, {"[]a]", "]", 0}, {"[]a]", "a", 0}, {"[!]a]", "]", 0}, {"[!]a]", "b", 0}, {"[^]a]", "]", 0},
    {"[]-a]", "^", 0}, {"[]-a]", "-", 0}, {"[a-]", "-", 0}, {"[-a]", "-", 0}, {"[!-a]", "-", 0}, {"[a-c]", "b", 0}, {"[a-c]", "d", 0},
    {"[c-a]", "b", 0}, {"[c-a]", "c", 0}, {"[c-a]", "a", 0}, {"[a-a]", "a", 0}, {"[a-c-e]", "d", 0}, {"[a-c-e]", "-", 0},
    {"[%--]", "+", 0}, {"[--/]", ".", 0}, {"[!]", "!", 0}, {"[!]", "]", 0},
    {"[[:alpha:]]", "a", 0}, {"[[:alpha:]]", "1", 0}, {"[[:digit:][:upper:]]", "Q", 0}, {"[![:space:]]", " ", 0},
    {"[![:space:]]", "x", 0}, {"[[:alnum:]_]", "_", 0}, {"[[:blank:]]", "\t", 0}, {"[[:cntrl:]]", "\x01", 0},
    {"[[:graph:]]", " ", 0}, {"[[:print:]]", " ", 0}, {"[[:punct:]]", "!", 0}, {"[[:xdigit:]]", "f", 0}, {"[[:xdigit:]]", "g", 0},
    {"[[:lower:]]", "a", 0}, {"[[:upper:]]", "a", 0},
    {"[[:foo:]]", "f", 0}, {"[[:foo:]]", "[", 0}, {"[[:foo:]]", ":", 0}, {"[[:alpha:]", "a", 0}, {"[[:alpha:]", "[", 0},
    {"[[:alpha]", "a", 0}, {"[[:alpha]", ":]", 0}, {"[[:alpha:]-z]", "-", 0}, {"[a-[:alpha:]]", "b", 0}, {"[[:ALPHA:]]", "a", 0},
    {"[[:]]", ":", 0}, {"[[::]]", ":", 0}, {"[[:alpha:]x]", "x", 0},
    {"[[=a=]]", "a", 0}, {"[[=a=]]", "b", 0}, {"[[=a=]b]", "b", 0}, {"[[=ab=]]", "a", 0}, {"[[==]]", "=", 0},
    {"[[.a.]]", "a", 0}, {"[[.a.]]", "b", 0}, {"[[.-.]]", "-", 0}, {"[[.hyphen.]]", "-", 0}, {"[[.space.]]", " ", 0},
    {"[[.ab.]]", "a", 0}, {"[[.ab.]]", "ab", 0}, {"[[.a.]-c]", "b", 0}, {"[a-[.c.]]", "b", 0}, {"[[.a.]", "a", 0},
    {"[[.]", "[", 0}, {"[[=]", "[", 0}, {"[[:]", "[", 0}, {"[[.].]]", "]", 0},
    {"[", "[", 0}, {"[a", "[a", 0}, {"[a", "a", 0}, {"[!", "[!", 0}, {"[]", "[]", 0}, {"[]", "]", 0}, {"[!]", "[!]", 0},
    {"a[", "a[", 0}, {"[a-", "[a-", 0}, {"*[", "x[", 0}, {"[*", "[x", 0}, {"[]]", "]", 0}, {"[]]]", "]]", 0},
    /* escapes */
    {"\\*", "*", 0}, {"\\*", "a", 0}, {"\\a", "a", 0}, {"\\", "\\", 0}, {"\\", "", 0}, {"a\\", "a\\", 0}, {"a\\", "a", 0},
    {"[\\]]", "]", 0}, {"[\\]]", "\\", 0}, {"[\\]]", "\\]", 0}, {"[\\!a]", "!", 0}, {"[a\\-c]", "b", 0}, {"[a\\-c]", "-", 0},
    {"[\\a-c]", "b", 0}, {"[\\\\]", "\\", 0}, {"\\[a]", "[a]", 0}, {"\\?", "?", 0}, {"\\?", "a", 0},
    /* FNM_NOESCAPE */
    {"\\*", "*", NE}, {"\\*", "\\*", NE}, {"\\*", "\\x", NE}, {"[\\]]", "\\]", NE}, {"[\\]]", "]", NE}, {"a\\", "a\\", NE},
    {"\\", "\\", NE}, {"[\\\\]", "\\", NE}, {"\\?", "\\a", NE},
    /* FNM_PATHNAME */
    {"*", "a/b", 0}, {"*", "a/b", P}, {"a/*", "a/b", P}, {"a/*", "a/b/c", P}, {"a?b", "a/b", P}, {"a?b", "a/b", 0},
    {"a[/]b", "a/b", P}, {"a[/]b", "a/b", 0}, {"a[!x]b", "a/b", P}, {"a[!x]b", "a/b", 0}, {"a[a/]b", "a/b", P},
    {"*/*", "a/b", P}, {"/*", "/a", P}, {"a/", "a/", P}, {"*", "/", P}, {"*", "", P}, {"a\\/b", "a/b", P}, {"a*/b", "a/x/b", P},
    {"*/b", "/b", P}, {"a//b", "a/b", P}, {"a/b", "a//b", P}, {"*", "a/", P}, {"*/", "a/", P}, {"a/[[:punct:]]", "a//", P},
    /* FNM_PERIOD */
    {"*", ".a", PER}, {"*", ".a", 0}, {"?", ".", PER}, {"[.]", ".", PER}, {"[!a]", ".", PER}, {"[[:punct:]]", ".", PER},
    {".*", ".a", PER}, {"\\.a", ".a", PER}, {"a*", ".a", PER}, {"a/*", "a/.b", PER}, {"a/*", "a/.b", P | PER},
    {"a/?b", "a/.b", P | PER}, {"*/.b", "a/.b", P | PER}, {"a*b", "a.b", PER}, {"*", "a/.b", PER}, {"*", "a/.b", P | PER},
    {"a/.*", "a/.b", P | PER}, {"*/*", "a/.b", P | PER}, {"*", ".", PER}, {"*", "..", PER}, {".", ".", PER},
    {"a*", "a/.b", PER}, {"a*", "a/.b", P | PER}, {"a?.b", "a/.b", PER},
    /* FNM_LEADING_DIR */
    {"a", "a/b", LD}, {"a", "a/b", 0}, {"a*", "ab/c", LD}, {"a*", "ab/c", LD | P}, {"a/b", "a/b/c/d", LD}, {"a", "ab", LD},
    {"a?", "a/", LD}, {"*", "a/b", LD | P}, {"a/", "a/b", LD}, {"a", "a/", LD}, {"a", "a//", LD}, {"a/*", "a/b/c", LD | P},
    {"a*", "a/b", LD | P}, {"", "/a", LD}, {"*b", "a/b/c", LD}, {"*b", "a/b/c", LD | P}, {"[!x]", "a/b", LD},
    /* FNM_CASEFOLD */
    {"abc", "ABC", CF}, {"ABC", "abc", CF}, {"[a-c]", "B", CF}, {"[A-C]", "b", CF}, {"[[:upper:]]", "a", CF},
    {"[[:lower:]]", "A", CF}, {"[!a]", "A", CF}, {"\\A", "a", CF}, {"[Z-a]", "_", 0}, {"[Z-a]", "_", CF}, {"[Z-a]", "z", CF},
    {"[[=a=]]", "A", CF}, {"[[.a.]]", "A", CF}, {"a*", "A", CF}, {"?(A)", "a", CF | X},
    /* FNM_EXTMATCH */
    {"?(a|b)", "", X}, {"?(a|b)", "a", X}, {"?(a|b)", "ab", X}, {"*(ab)", "", X}, {"*(ab)", "abab", X}, {"*(ab)", "aba", X},
    {"+(ab)", "", X}, {"+(ab)", "ab", X}, {"+(ab)", "ababab", X}, {"@(a|b)", "a", X}, {"@(a|b)", "", X}, {"@(a|b)", "ab", X},
    {"!(a)", "a", X}, {"!(a)", "b", X}, {"!(a)", "", X}, {"!(a)", "aa", X}, {"*(a|b)c", "abbac", X},
    {"@(a|ab)@(c|bcd)", "abcd", X}, {"x!(y)z", "xyz", X}, {"x!(y)z", "xaz", X}, {"x!(y)z", "xz", X}, {"x!(y)z", "xyyz", X},
    {"+(a|b)", "", X}, {"?(a", "?(a", X}, {"?(a", "a", X}, {"*(a", "*(a", X}, {"*(a", "x(a", X}, {"@()", "", X}, {"@(|a)", "", X},
    {"@(a|)", "a", X}, {"*(a)", "a", 0}, {"*(a)", "x(a)", 0}, {"?(a)", "b(a)", 0}, {"!(a)", "!(a)", 0}, {"+(a)", "+(a)", 0},
    {"@(a)", "@(a)", 0}, {"!(*.c)", "x.c", X}, {"!(*.c)", "x.h", X}, {"*(a/b)", "a/b", X | P}, {"*(a/b)", "a/b", X},
    {"@(a|b)/c", "a/c", X | P}, {"!(x)", "a/b", X | P}, {"!(x)", "a/b", X}, {"!(x)", ".a", X | PER}, {"*(x)", ".a", X | PER},
    {"+([[:digit:]])", "123", X}, {"+([[:digit:]])", "12a", X}, {"@([)])", ")", X}, {"@(a\\|b)", "a|b", X},
    {"@(a\\|b)", "a", X}, {"@(a\\)b)", "a)b", X}, {"@(a[|]b)", "a|b", X}, {"*(*(a)b)", "abaab", X}, {"!(!(a))", "a", X},
    {"!(!(a))", "b", X}, {"?(a)?(a)", "aa", X}, {"*(a)*(a)", "aaa", X}, {"@(a)", "A", X | CF}, {"!(a*)", "abc", X},
    {"a!(b)c", "ac", X}, {"a!(b)*", "abc", X}, {"*!(.c)", "x.c", X}, {"@(?(a))", "", X}, {"?(a|b|c)d", "cd", X},
    {"*(a|b", "*(a|b", X}, {"@(a))", "a)", X}, {"[@(a)]", "a", X}, {"\\@(a)", "@(a)", X}, {"\\@(a)", "a", X},
    {"@(a)", "@(a)", X | NE}, {"@(\\a)", "\\a", X | NE}, {"!(a)b", "b", X}, {"!(a)", "/", X | P}, {"!()", "", X},
    {"!()", "a", X}, {"+()", "", X},
};

/* Run in each locale: bracket expressions, ranges, classes, invalid multibyte text. */
static const struct fc floc[] = {
    {"[a-z]", "B", 0}, {"[A-Z]", "b", 0}, {"[a-z]", "y", 0}, {"[a-z]", "\xc3\xa9", 0}, {"[a-z\xc3\xa9]", "\xc3\xa9", 0},
    {"?", "\xc3\xa9", 0}, {"??", "\xc3\xa9", 0}, {"*", "\xff", 0}, {"?", "\xff", 0}, {"\xff", "\xff", 0}, {"[\xff]", "\xff", 0},
    {"a*", "a\xff", 0}, {"a?", "a\xff", 0}, {"*", "a\xc3", 0}, {"?", "\xc3", 0}, {"[[:alpha:]]", "\xc3\xa9", 0},
    {"[[:upper:]]", "\xc3\x89", 0}, {"[[:lower:]]", "\xc3\x89", 0}, {"[[=e=]]", "\xc3\xa9", 0}, {"[[=e=]]", "e", 0},
    {"[[=\xc3\xa9=]]", "e", 0}, {"[[=\xc3\xa9=]]", "\xc3\xa9", 0}, {"[[=E=]]", "e", 0}, {"[[=e=]]", "E", 0},
    {"[[.\xc3\xa9.]]", "\xc3\xa9", 0}, {"[\xc3\xa0-\xc3\xbf]", "\xc3\xa9", 0}, {"[[:alpha:]]", "\xc3\x9f", 0},
    {"\xc3\xa9", "\xc3\x89", CF}, {"[\xc3\xa9]", "\xc3\x89", CF}, {"[a-c]", "B", CF}, {"[\xc3\xa9]", "\xc3", 0},
    {"\xc3*", "\xc3\xa9", 0}, {"\xc3?", "\xc3\xa9", 0}, {"[[.ch.]]", "ch", 0}, {"[[.ch.]]", "c", 0}, {"[Z-a]", "_", 0},
    {"[a-Z]", "b", 0}, {"[A-z]", "_", 0}, {"[A-z]", "b", 0}, {"[a-A]", "a", 0}, {"[0-9]", "\xd9\xa3", 0},
    {"[[:digit:]]", "\xd9\xa3", 0}, {"[[:space:]]", "\xe3\x80\x80", 0}, {"[!a]", "\xc3\xa9", 0}, {"[!a]", "\xff", 0},
    {"*\xc3\xa9", "x\xc3\xa9", 0}, {"[\xe2\x82\xac]", "\xe2\x82\xac", 0}, {"?", "\xe2\x82", 0}, {"\\\xc3\xa9", "\xc3\xa9", 0},
    {"[b-\xc3\xa9]", "c", 0}, {"[b-\xc3\xa9]", "\xc3\xa0", 0}, {"[b-\xc3\xa9]", "\xc3\xaa", 0}, {"[b-\xc3\xa9]", "B", 0},
    {"[b-\xc3\xa9]", "C", 0}, {"@(\xc3\xa9|a)", "\xc3\xa9", X}, {"*(?)", "\xc3\xa9\xc3\xa9", X}, {"*(?)", "\xff", X},
    {"[[:alpha:]]", "\xff", 0}, {"*", "\xc3\xa9/\xff", P}, {"[\xa9]", "\xa9", 0},
    /* FNM_EXTMATCH | FNM_LEADING_DIR (FNMATCH-EXTMATCH-LEADING-DIR-2): the 16 pattern/subject pairs where the reference
     * answers differently from the documented FNM_LEADING_DIR definition (whole string, or an initial segment
     * followed by '/'). Pinned as known differences in patterns_products.rs; run in every locale. */
    {"!(a)", "a/b", X | LD}, {"!(a)", "a/", X | LD}, {"!(*.c)", "a.c/d", X | LD}, {"+([ab])c", "ab/c", X | LD},
    {"+([ab])c", "a/b/c", X | LD}, {"!(a)", "a/b/c", X | LD}, {"!(b)", "b/c", X | LD}, {"!(a)/*", "a/b/c", X | LD},
    {"+([ab])c", "b/c", X | LD}, {"*([ab])c", "ab/c", X | LD}, {"*([ab])c", "a/b/c", X | LD}, {"*([ab])c", "b/c", X | LD},
    {"+(a|b)c", "ab/c", X | LD}, {"+(a|b)c", "a/b/c", X | LD}, {"+(a|b)c", "b/c", X | LD}, {"+([ab])/c", "a/b/c", X | LD},
};

/* The fnmatch corpus: every pattern of up to 4 tokens against every subject of up to 3
 * characters over {a, /, .}, under six flag sets. One line per (flags, pattern): the results
 * in subject order, '0' match, '1' FNM_NOMATCH, 'E' anything else. */
static const char *ftok1[] = {"a", "*", "?", "[a]", "/", ".", "[!a]"};
static const char **ftok = ftok1;
static size_t nftok = sizeof ftok1 / sizeof *ftok1;
static char fsubj[64][4];
static int nfsubj;

static void fsubjects(void)
{
    const char al[] = "a/.";
    nfsubj = 0;
    for (int len = 0; len <= 3; len++) {
        int total = 1;
        for (int i = 0; i < len; i++)
            total *= 3;
        for (int k = 0; k < total; k++) {
            int v = k;
            for (int i = len - 1; i >= 0; i--) {
                fsubj[nfsubj][i] = al[v % 3];
                v /= 3;
            }
            fsubj[nfsubj][len] = 0;
            nfsubj++;
        }
    }
}

static void fcorpus_pat(char *buf, int depth, int maxdepth, const int *flags, int nflags)
{
    if (depth == maxdepth) {
        for (int f = 0; f < nflags; f++) {
            fputs("fc ", stdout);
            fnm_flags(flags[f]);
            putchar(' ');
            q(buf);
            putchar(' ');
            for (int s = 0; s < nfsubj; s++) {
                int r = fnmatch(buf, fsubj[s], flags[f]);
                putchar(r == 0 ? '0' : r == FNM_NOMATCH ? '1' : 'E');
            }
            nl();
        }
        return;
    }
    size_t l = strlen(buf);
    for (size_t t = 0; t < nftok; t++) {
        strcpy(buf + l, ftok[t]);
        fcorpus_pat(buf, depth + 1, maxdepth, flags, nflags);
        buf[l] = 0;
    }
}

static void sec_fnmatch(void)
{
    puts("== fnmatch");
    printf("constants NOMATCH=%d NOESCAPE=%d PATHNAME=%d PERIOD=%d LEADING_DIR=%d CASEFOLD=%d EXTMATCH=%d\n", FNM_NOMATCH, FNM_NOESCAPE,
           FNM_PATHNAME, FNM_PERIOD, FNM_LEADING_DIR, FNM_CASEFOLD, FNM_EXTMATCH);
    use_locale("C");
    for (size_t i = 0; i < sizeof fcases / sizeof *fcases; i++)
        fnm(fcases[i].p, fcases[i].s, fcases[i].f);
    /* unknown flag bits */
    fnm("a", "a", 1 << 10);
    fnm("a", "b", 1 << 10);
    fnm("a", "a", -1 & ~(FNM_EXTMATCH | FNM_CASEFOLD | FNM_LEADING_DIR | FNM_PERIOD | FNM_PATHNAME | FNM_NOESCAPE));
    for (size_t l = 0; l < NLOC; l++) {
        if (!use_locale(locales[l]))
            continue;
        for (size_t i = 0; i < sizeof floc / sizeof *floc; i++)
            fnm(floc[i].p, floc[i].s, floc[i].f);
    }
    use_locale("C");
    puts("-- corpus: tokens a * ? [a] / . [!a]; subjects over {a,/,.} of length 0-3 in this order:");
    fsubjects();
    fputs("subjects", stdout);
    for (int s = 0; s < nfsubj; s++) {
        putchar(' ');
        q(fsubj[s]);
    }
    nl();
    static const int flags[] = {0, P, PER, P | PER, LD, LD | P, NE | P | PER};
    char buf[64] = "";
    for (int d = 0; d <= 4; d++)
        fcorpus_pat(buf, 0, d, flags, sizeof flags / sizeof *flags);
    /* The extmatch corpus (#1824): tokens a * / !(a) +([a]) up to 3 long, against the same subjects, under
     * EXTMATCH with and without LEADING_DIR and PATHNAME. Same section, same line form ("fc <flags> [pattern] bits"). */
    static const char *ftok2[] = {"a", "*", "/", "!(a)", "+([a])"};
    static const int flags2[] = {X, X | LD, X | P, X | P | LD};
    ftok = ftok2;
    nftok = sizeof ftok2 / sizeof *ftok2;
    buf[0] = 0;
    for (int d = 0; d <= 3; d++)
        fcorpus_pat(buf, 0, d, flags2, sizeof flags2 / sizeof *flags2);
}

/* ------------------------------------------------------------------ glob */

static const char *gflag_names[] = {"ERR", "MARK", "NOSORT", "DOOFFS", "NOCHECK", "APPEND", "NOESCAPE", "PERIOD", "MAGCHAR",
                                    "ALTDIRFUNC", "BRACE", "NOMAGIC", "TILDE", "ONLYDIR", "TILDE_CHECK"};

static void glob_flags(int f)
{
    if (!f) {
        fputs("0", stdout);
        return;
    }
    const char *sep = "";
    for (int i = 0; i < 15; i++)
        if (f & (1 << i)) {
            printf("%s%s", sep, gflag_names[i]);
            sep = "|";
        }
    if (f & ~0x7fff)
        printf("%s0x%x", sep, f & ~0x7fff);
}

static int errfunc_ret;
static int errfunc_calls;
static int errfunc(const char *path, int err)
{
    errfunc_calls++;
    fputs("  errfunc ", stdout);
    q(path);
    printf(" errno=%d -> %d", err, errfunc_ret);
    nl();
    return errfunc_ret;
}

static void glob_show(const char *label, const char *pat, int flags, int r, const glob_t *g)
{
    printf("%s ", label);
    q(pat);
    putchar(' ');
    glob_flags(flags);
    printf(" -> %d pathc=%zu offs=%zu", r, g->gl_pathc, g->gl_offs);
    printf(" magchar=%d", (g->gl_flags & GLOB_MAGCHAR) != 0);
    if (!g->gl_pathv)
        fputs(" pathv=NULL", stdout);
    else {
        size_t n = g->gl_offs + g->gl_pathc;
        if (n > 4096)
            n = 0;
        for (size_t i = 0; i <= n; i++) {
            putchar(' ');
            if (i == n && g->gl_pathv[i] == NULL)
                fputs("$", stdout); /* the terminating NULL is present */
            else
                q(g->gl_pathv[i]);
        }
    }
    nl();
}

/* One glob call on a fresh glob_t (gl_offs = offs). */
static void gl(const char *pat, int flags, size_t offs, int with_errfunc)
{
    glob_t g;
    memset(&g, 0, sizeof g);
    g.gl_offs = offs;
    errfunc_calls = 0;
    int r = glob(pat, flags, with_errfunc ? errfunc : NULL, &g);
    glob_show("glob", pat, flags, r, &g);
    globfree(&g);
}

static void mk(const char *path, const char *content)
{
    int fd = open(path, O_WRONLY | O_CREAT | O_TRUNC, 0644);
    if (fd < 0) {
        printf("fixture: cannot create %s errno=%d\n", path, errno);
        return;
    }
    if (content && write(fd, content, strlen(content)) < 0)
        printf("fixture: write %s errno=%d\n", path, errno);
    close(fd);
}

static const char *sort_names[] = {"a", "A", "b", "B", "_a", "-a", "a-b", "ab", "a b", "a.b", "aB", "Ab", "10", "9", "1",
                                   "\xc3\xa9", "e", "E", "f", "\xc3\xa4", "z", "Z", "e\xcc\x81", "#a", "~a", "\xc3\x89", "a_b", "a1", "a10", "a9", ".x"};

static void fixture(void)
{
    if (chdir(g_dir) != 0) {
        printf("fixture: chdir errno=%d\n", errno);
        exit(1);
    }
    mkdir("sort", 0755);
    for (size_t i = 0; i < sizeof sort_names / sizeof *sort_names; i++) {
        char p[256];
        snprintf(p, sizeof p, "sort/%s", sort_names[i]);
        mk(p, NULL);
    }
    mkdir("tree", 0755);
    mkdir("tree/d1", 0755);
    mkdir("tree/d1/sub", 0755);
    mkdir("tree/d2", 0755);
    mkdir("tree/.hdir", 0755);
    mkdir("tree/noread", 0755);
    mkdir("tree/noexec", 0755);
    mk("tree/d1/f1", NULL);
    mk("tree/d1/f2", NULL);
    mk("tree/d1/.h1", NULL);
    mk("tree/d1/sub/x", NULL);
    mk("tree/.hdir/y", NULL);
    mk("tree/noread/z", NULL);
    mk("tree/noexec/w", NULL);
    mk("tree/file1", NULL);
    mk("tree/file2", NULL);
    mk("tree/.hid", NULL);
    mk("tree/a", NULL);
    mk("tree/b", NULL);
    mk("tree/[ab]", NULL);
    mk("tree/[x", NULL);
    mk("tree/q*", NULL);
    mk("tree/q?", NULL);
    mk("tree/qx", NULL);
    mk("tree/b\\s", NULL);
    mk("tree/bs", NULL);
    mk("tree/{a,b}", NULL);
    mk("tree/~", NULL);
    if (symlink("d1", "tree/link_d1") || symlink("nowhere", "tree/dangling") || symlink("file1", "tree/link_file1") ||
        symlink("loop", "tree/loop"))
        printf("fixture: symlink errno=%d\n", errno);
    chmod("tree/noread", 0300);  /* searchable, not readable */
    chmod("tree/noexec", 0600);  /* readable, not searchable */
    mkdir("tilde", 0755);
    mk("tilde/t1", NULL);
    mkdir("words", 0755);
    mk("words/w1", NULL);
    mk("words/w2", NULL);
    mk("words/.w3", NULL);
    mk("words/w*", NULL);
}

static void fs_line(void)
{
    struct statfs sf;
    if (statfs(g_dir, &sf) != 0) {
        printf("fs: statfs errno=%d\n", errno);
        return;
    }
    const char *n = "unknown";
    switch ((unsigned long)sf.f_type) {
    case 0xEF53: n = "ext2/ext3"; break; /* stat -f -c %T prints this name for ext4 too */
    case 0x01021994: n = "tmpfs"; break;
    case 0x9123683E: n = "btrfs"; break;
    case 0x58465342: n = "xfs"; break;
    case 0x2FC12FC1: n = "zfs"; break;
    case 0x794C7630: n = "overlayfs"; break;
    }
    printf("fs: scratch directory f_type=0x%lx (%s)\n", (unsigned long)sf.f_type, n);
}

/* GLOB_ALTDIRFUNC: a virtual tree. */
struct vent {
    const char *dir, *name;
    unsigned char type;
};
static const struct vent vtree[] = {
    {"v", "x", DT_REG}, {"v", "y", DT_DIR}, {"v", ".p", DT_REG}, {"v", "u", DT_UNKNOWN}, {"v", "ud", DT_UNKNOWN},
    {"v/y", "z", DT_REG}, {"v/ud", "k", DT_REG}, {"v", "locked", DT_DIR}, {".", "v", DT_DIR}, {"v", "dang", DT_LNK},
};
/* v/dang is a dangling symbolic link: gl_lstat finds it, gl_stat does not. */
static int vdang_stat, vdang_lstat;
#define NV (sizeof vtree / sizeof *vtree)
struct vdir {
    char path[256];
    size_t pos;
    struct dirent de;
};
static char vcalls[64][300];
static int nvcalls;
static void vlog(const char *kind, const char *p)
{
    if (nvcalls < 64)
        snprintf(vcalls[nvcalls++], sizeof vcalls[0], "%s:%s", kind, p);
}
static int vkind(const char *p) /* 0 none, 1 file, 2 dir */
{
    if (!strcmp(p, "v") || !strcmp(p, "v/y") || !strcmp(p, "v/ud") || !strcmp(p, "v/locked") || !strcmp(p, ".") || !strcmp(p, "v/"))
        return 2;
    if (!strcmp(p, "v/x") || !strcmp(p, "v/.p") || !strcmp(p, "v/u") || !strcmp(p, "v/y/z") || !strcmp(p, "v/ud/k"))
        return 1;
    return 0;
}
static void *v_opendir(const char *p)
{
    vlog("opendir", p);
    const char *d = p[0] ? p : ".";
    if (!strcmp(d, "v/locked")) {
        errno = EACCES;
        return NULL;
    }
    if (vkind(d) != 2) {
        errno = vkind(d) ? ENOTDIR : ENOENT;
        return NULL;
    }
    struct vdir *v = calloc(1, sizeof *v);
    snprintf(v->path, sizeof v->path, "%s", d);
    size_t l = strlen(v->path);
    if (l > 1 && v->path[l - 1] == '/')
        v->path[l - 1] = 0;
    return v;
}
static struct dirent *v_readdir(void *h)
{
    struct vdir *v = h;
    while (v->pos < NV) {
        const struct vent *e = &vtree[v->pos++];
        if (strcmp(e->dir, v->path))
            continue;
        memset(&v->de, 0, sizeof v->de);
        v->de.d_ino = 1000 + v->pos;
        v->de.d_type = e->type;
        snprintf(v->de.d_name, sizeof v->de.d_name, "%s", e->name);
        return &v->de;
    }
    return NULL;
}
static void v_closedir(void *h) { free(h); }
static int v_stat_common(const char *kind, const char *p, struct stat *st)
{
    vlog(kind, p);
    memset(st, 0, sizeof *st);
    if (!strcmp(p, "v/dang")) {
        if (kind[0] == 'l') {
            vdang_lstat++;
            st->st_mode = S_IFLNK | 0777;
            return 0;
        }
        vdang_stat++;
        errno = ENOENT;
        return -1;
    }
    int k = vkind(p);
    if (!k) {
        errno = ENOENT;
        return -1;
    }
    st->st_mode = k == 2 ? S_IFDIR | 0755 : S_IFREG | 0644;
    return 0;
}
static int v_stat(const char *p, struct stat *st) { return v_stat_common("stat", p, st); }
static int v_lstat(const char *p, struct stat *st) { return v_stat_common("lstat", p, st); }

static int cmpstr(const void *a, const void *b) { return strcmp((const char *)a, (const char *)b); }

static int (*glob_fn)(const char *, int, int (*)(const char *, int), glob_t *) = glob;
static const char *glob_alt_label = "glob-alt";
static int show_dang; /* compat section: whether gl_stat or gl_lstat decided about v/dang */

static void gl_alt(const char *pat, int flags)
{
    glob_t g;
    memset(&g, 0, sizeof g);
    g.gl_opendir = v_opendir;
    g.gl_readdir = v_readdir;
    g.gl_closedir = v_closedir;
    g.gl_stat = v_stat;
    g.gl_lstat = v_lstat;
    nvcalls = 0;
    errfunc_ret = 0;
    vdang_stat = vdang_lstat = 0;
    int r = glob_fn(pat, flags | GLOB_ALTDIRFUNC, errfunc, &g);
    glob_show(glob_alt_label, pat, flags | GLOB_ALTDIRFUNC, r, &g);
    if (show_dang)
        printf("  v/dang: gl_stat %s, gl_lstat %s\n", vdang_stat ? "called" : "not called", vdang_lstat ? "called" : "not called");
    /* which directories were opened (sorted, unique): informative */
    qsort(vcalls, (size_t)nvcalls, sizeof vcalls[0], cmpstr);
    fputs("  opened:", stdout);
    for (int i = 0; i < nvcalls; i++)
        if (!strncmp(vcalls[i], "opendir:", 8) && (i == 0 || strcmp(vcalls[i], vcalls[i - 1]))) {
            putchar(' ');
            q(vcalls[i] + 8);
        }
    nl();
    globfree(&g);
}

static void sec_glob(void)
{
    puts("== glob");
    printf("constants ERR=%d MARK=%d NOSORT=%d DOOFFS=%d NOCHECK=%d APPEND=%d NOESCAPE=%d PERIOD=%d MAGCHAR=%d ALTDIRFUNC=%d BRACE=%d "
           "NOMAGIC=%d TILDE=%d ONLYDIR=%d TILDE_CHECK=%d NOSPACE=%d ABORTED=%d NOMATCH=%d NOSYS=%d\n",
           GLOB_ERR, GLOB_MARK, GLOB_NOSORT, GLOB_DOOFFS, GLOB_NOCHECK, GLOB_APPEND, GLOB_NOESCAPE, GLOB_PERIOD, GLOB_MAGCHAR,
           GLOB_ALTDIRFUNC, GLOB_BRACE, GLOB_NOMAGIC, GLOB_TILDE, GLOB_ONLYDIR, GLOB_TILDE_CHECK, GLOB_NOSPACE, GLOB_ABORTED,
           GLOB_NOMATCH, GLOB_NOSYS);
    fs_line();
    use_locale("C");
    if (chdir(g_dir) != 0)
        return;
    errfunc_ret = 0;
    puts("-- basics");
    gl("tree/*", 0, 0, 0);
    gl("tree/file?", 0, 0, 0);
    gl("tree/file[12]", 0, 0, 0);
    gl("tree/file[!1]", 0, 0, 0);
    gl("tree/*1", 0, 0, 0);
    gl("tree/nomatch*", 0, 0, 0);
    gl("tree/nomatch", 0, 0, 0);
    gl("tree/file1", 0, 0, 0);
    gl("tree/d1", 0, 0, 0);
    gl("tree/d1/", 0, 0, 0);
    gl("tree/file1/", 0, 0, 0);
    gl("tree/*/", 0, 0, 0);
    gl("tree/*/*", 0, 0, 0);
    gl("tree/d*/f*", 0, 0, 0);
    gl("tree/*/sub/*", 0, 0, 0);
    gl("tree//file1", 0, 0, 0);
    gl("tree//file*", 0, 0, 0);
    gl("tree/d1/../file*", 0, 0, 0);
    gl("tree/*/../file1", 0, 0, 0);
    gl("./tree/file*", 0, 0, 0);
    gl("", 0, 0, 0);
    gl("/", 0, 0, 0);
    gl("//", 0, 0, 0);
    gl("*", 0, 0, 0);
    gl("tree", 0, 0, 0);
    gl("tr?e", 0, 0, 0);
    gl("t*/d2", 0, 0, 0);
    gl("tree/d2/*", 0, 0, 0);
    {
        char abs[4200];
        snprintf(abs, sizeof abs, "%s/tree/file*", g_dir);
        gl(abs, 0, 0, 0);
        snprintf(abs, sizeof abs, "%s/tree/d1", g_dir);
        gl(abs, GLOB_MARK, 0, 0);
    }
    puts("-- symbolic links");
    gl("tree/link*", 0, 0, 0);
    gl("tree/dangling", 0, 0, 0);
    gl("tree/dang*", 0, 0, 0);
    gl("tree/loop", 0, 0, 0);
    gl("tree/lo?p", 0, 0, 0);
    gl("tree/link_d1/*", 0, 0, 0);
    gl("tree/dangling/*", 0, 0, 0);
    gl("tree/dangling/", 0, 0, 0);
    gl("tree/l*/", 0, 0, 0);
    gl("tree/[dl]*", GLOB_MARK, 0, 0);
    gl("tree/[dl]*", GLOB_ONLYDIR, 0, 0);
    puts("-- brackets, escapes, GLOB_NOESCAPE");
    gl("tree/[ab]", 0, 0, 0);
    gl("tree/\\[ab\\]", 0, 0, 0);
    gl("tree/\\[ab]", 0, 0, 0);
    gl("tree/\\[ab\\]", GLOB_NOESCAPE, 0, 0);
    gl("tree/[[]ab]", 0, 0, 0);
    gl("tree/[x", 0, 0, 0);
    gl("tree/[x*", 0, 0, 0);
    gl("tree/[", 0, 0, 0);
    gl("tree/[*", 0, 0, 0);
    gl("tree/q*", 0, 0, 0);
    gl("tree/q\\*", 0, 0, 0);
    gl("tree/q\\*", GLOB_NOESCAPE, 0, 0);
    gl("tree/q\\?", 0, 0, 0);
    gl("tree/q[*]", 0, 0, 0);
    gl("tree/b\\s", 0, 0, 0);
    gl("tree/b\\s", GLOB_NOESCAPE, 0, 0);
    gl("tree/b\\\\s", 0, 0, 0);
    gl("tree/b\\\\*", 0, 0, 0);
    gl("tree/b\\\\*", GLOB_NOESCAPE, 0, 0);
    gl("tree/b[\\\\]s", 0, 0, 0);
    gl("tree/b[\\\\]s", GLOB_NOESCAPE, 0, 0);
    gl("tree/fil\\e1", 0, 0, 0);
    gl("tree/fil\\e*", 0, 0, 0);
    gl("tree/file1\\", 0, 0, 0);
    gl("tree/file*\\", 0, 0, 0);
    gl("tree/[[:digit:]]", 0, 0, 0);
    gl("tree/file[[:digit:]]", 0, 0, 0);
    gl("tree/[[:alpha:]]", 0, 0, 0);
    gl("tree/[!a]", 0, 0, 0);
    gl("tree/[^a]", 0, 0, 0);
    gl("tree/*(1)", 0, 0, 0);
    puts("-- GLOB_MARK");
    gl("tree/*", GLOB_MARK, 0, 0);
    gl("tree/d1", GLOB_MARK, 0, 0);
    gl("tree/d1/", GLOB_MARK, 0, 0);
    gl("tree/*/", GLOB_MARK, 0, 0);
    gl("tree/nomatch", GLOB_MARK | GLOB_NOCHECK, 0, 0);
    gl("tree/link_d1", GLOB_MARK, 0, 0);
    gl("tree", GLOB_MARK, 0, 0);
    gl("tree/.", GLOB_MARK, 0, 0);
    puts("-- GLOB_PERIOD");
    gl("tree/*", GLOB_PERIOD, 0, 0);
    gl("tree/.*", 0, 0, 0);
    gl("tree/.*", GLOB_PERIOD, 0, 0);
    gl("tree/?hid", 0, 0, 0);
    gl("tree/?hid", GLOB_PERIOD, 0, 0);
    gl("tree/[.]hid", 0, 0, 0);
    gl("tree/[.]hid", GLOB_PERIOD, 0, 0);
    gl("tree/*/*", GLOB_PERIOD, 0, 0);
    gl("tree/.h*/*", 0, 0, 0);
    gl("tree/d1/.*", 0, 0, 0);
    gl("tree/.?", 0, 0, 0);
    gl("tree/.??", 0, 0, 0);
    gl("tree/\\.hid", 0, 0, 0);
    gl("tree/[!a]*", 0, 0, 0);
    puts("-- GLOB_ONLYDIR");
    gl("tree/*", GLOB_ONLYDIR, 0, 0);
    gl("tree/file1", GLOB_ONLYDIR, 0, 0);
    gl("tree/d*", GLOB_ONLYDIR | GLOB_MARK, 0, 0);
    gl("tree/*/*", GLOB_ONLYDIR, 0, 0);
    gl("tree/nomatch", GLOB_ONLYDIR | GLOB_NOCHECK, 0, 0);
    puts("-- GLOB_NOCHECK, GLOB_NOMAGIC");
    gl("tree/nomatch", GLOB_NOCHECK, 0, 0);
    gl("tree/nomatch*", GLOB_NOCHECK, 0, 0);
    gl("tree/no\\match*", GLOB_NOCHECK, 0, 0);
    gl("tree/no\\match", GLOB_NOCHECK, 0, 0);
    gl("tree/no\\match", GLOB_NOCHECK | GLOB_NOESCAPE, 0, 0);
    gl("tree/no[ma]tch", GLOB_NOCHECK, 0, 0);
    gl("tree/*/nomatch", GLOB_NOCHECK, 0, 0);
    gl("", GLOB_NOCHECK, 0, 0);
    gl("tree/nomatch", GLOB_NOMAGIC, 0, 0);
    gl("tree/nomatch*", GLOB_NOMAGIC, 0, 0);
    gl("tree/no\\match", GLOB_NOMAGIC, 0, 0);
    gl("tree/no[", GLOB_NOMAGIC, 0, 0);
    gl("tree/file1", GLOB_NOMAGIC, 0, 0);
    gl("tree/fi*", GLOB_NOMAGIC, 0, 0);
    gl("", GLOB_NOMAGIC, 0, 0);
    gl("tree/*/no", GLOB_NOMAGIC, 0, 0);
    puts("-- GLOB_DOOFFS, GLOB_APPEND");
    gl("tree/file*", GLOB_DOOFFS, 2, 0);
    gl("tree/nomatch", GLOB_DOOFFS, 2, 0);
    gl("tree/nomatch", GLOB_DOOFFS | GLOB_NOCHECK, 3, 0);
    gl("tree/file*", 0, 5, 0); /* gl_offs without GLOB_DOOFFS */
    {
        glob_t g;
        memset(&g, 0, sizeof g);
        g.gl_offs = 2;
        int r = glob("tree/file*", GLOB_DOOFFS, NULL, &g);
        glob_show("append#1", "tree/file*", GLOB_DOOFFS, r, &g);
        r = glob("tree/d*", GLOB_DOOFFS | GLOB_APPEND, NULL, &g);
        glob_show("append#2", "tree/d*", GLOB_DOOFFS | GLOB_APPEND, r, &g);
        r = glob("tree/nomatch*", GLOB_DOOFFS | GLOB_APPEND, NULL, &g);
        glob_show("append#3", "tree/nomatch*", GLOB_DOOFFS | GLOB_APPEND, r, &g);
        r = glob("tree/nomatch", GLOB_DOOFFS | GLOB_APPEND | GLOB_NOCHECK, NULL, &g);
        glob_show("append#4", "tree/nomatch", GLOB_DOOFFS | GLOB_APPEND | GLOB_NOCHECK, r, &g);
        r = glob("tree/b*", GLOB_APPEND, NULL, &g); /* DOOFFS dropped on a later call */
        glob_show("append#5", "tree/b*", GLOB_APPEND, r, &g);
        globfree(&g);
        memset(&g, 0, sizeof g);
        r = glob("tree/nomatch*", 0, NULL, &g);
        glob_show("append-after-nomatch#1", "tree/nomatch*", 0, r, &g);
        r = glob("tree/file*", GLOB_APPEND, NULL, &g);
        glob_show("append-after-nomatch#2", "tree/file*", GLOB_APPEND, r, &g);
        globfree(&g);
        memset(&g, 0, sizeof g);
        r = glob("tree/file2", GLOB_NOSORT, NULL, &g);
        glob_show("append-sorting#1", "tree/file2", GLOB_NOSORT, r, &g);
        r = glob("tree/[ab]", GLOB_APPEND, NULL, &g);
        glob_show("append-sorting#2", "tree/[ab]", GLOB_APPEND, r, &g);
        r = glob("tree/file1", GLOB_APPEND, NULL, &g);
        glob_show("append-sorting#3", "tree/file1", GLOB_APPEND, r, &g);
        globfree(&g);
        /* globfree twice, and on a zeroed glob_t */
        memset(&g, 0, sizeof g);
        globfree(&g);
        puts("globfree on a zeroed glob_t: ok");
        r = glob("tree/file*", 0, NULL, &g);
        globfree(&g);
        printf("after globfree: pathv=%s\n", g.gl_pathv ? "set" : "NULL");
    }
    puts("-- GLOB_ERR and the error callback");
    errfunc_ret = 0;
    gl("tree/noread/*", 0, 0, 1);
    gl("tree/noread/*", GLOB_ERR, 0, 1);
    gl("tree/noread/z", 0, 0, 1);
    gl("tree/noread/z*", 0, 0, 1);
    gl("tree/*/z", 0, 0, 1);
    gl("tree/*/z", GLOB_ERR, 0, 1);
    gl("tree/*/*", 0, 0, 1);
    gl("tree/*/*", GLOB_ERR, 0, 1);
    gl("tree/noexec/*", 0, 0, 1);
    gl("tree/noexec/w", 0, 0, 1);
    gl("tree/noexec/*", GLOB_MARK, 0, 1);
    gl("tree/noexec/*", GLOB_ERR, 0, 1);
    gl("tree/noread/*", GLOB_NOCHECK, 0, 1);
    gl("tree/noread/*", GLOB_ERR | GLOB_NOCHECK, 0, 1);
    gl("tree/nodir/*", 0, 0, 1);
    gl("tree/nodir/*", GLOB_ERR, 0, 1);
    gl("tree/file1/*", 0, 0, 1);
    gl("tree/file1/*", GLOB_ERR, 0, 1);
    gl("tree/dangling/*", GLOB_ERR, 0, 1);
    gl("tree/loop/*", GLOB_ERR, 0, 1);
    errfunc_ret = 1;
    gl("tree/noread/*", 0, 0, 1);
    gl("tree/*/*", 0, 0, 1);
    gl("tree/*/*", GLOB_NOCHECK, 0, 1);
    gl("tree/nodir/*", 0, 0, 1);
    errfunc_ret = 0;
    {
        glob_t g;
        memset(&g, 0, sizeof g);
        int r = glob("tree/file*", 0, NULL, &g);
        glob_show("abort-append#1", "tree/file*", 0, r, &g);
        errfunc_ret = 1;
        r = glob("tree/*/*", GLOB_APPEND, errfunc, &g);
        glob_show("abort-append#2", "tree/*/*", GLOB_APPEND, r, &g);
        errfunc_ret = 0;
        globfree(&g);
    }
    puts("-- GLOB_BRACE");
    gl("tree/{d1,d2}", GLOB_BRACE, 0, 0);
    gl("tree/{d2,d1}", GLOB_BRACE, 0, 0);
    gl("tree/{file2,file1}", GLOB_BRACE, 0, 0);
    gl("tree/{file*,d*}", GLOB_BRACE, 0, 0);
    gl("tree/{d*,file*}", GLOB_BRACE, 0, 0);
    gl("tree/{d*,file*}", GLOB_BRACE | GLOB_NOSORT, 0, 0);
    gl("tree/{a,{b,file1}}", GLOB_BRACE, 0, 0);
    gl("tree/{}", GLOB_BRACE, 0, 0);
    gl("tree/{,file1}", GLOB_BRACE, 0, 0);
    gl("tree/{file1,}", GLOB_BRACE, 0, 0);
    gl("tree/{file1}", GLOB_BRACE, 0, 0);
    gl("tree/{d1,nomatch}", GLOB_BRACE, 0, 0);
    gl("tree/{nomatch1,nomatch2}", GLOB_BRACE, 0, 0);
    gl("tree/{nomatch1,nomatch2}", GLOB_BRACE | GLOB_NOCHECK, 0, 0);
    gl("tree/{nomatch1,nomatch2}", GLOB_BRACE | GLOB_NOMAGIC, 0, 0);
    gl("tree/{file1", GLOB_BRACE, 0, 0);
    gl("tree/file1}", GLOB_BRACE, 0, 0);
    gl("tree/{a,b}", 0, 0, 0);
    gl("tree/\\{a,b}", GLOB_BRACE, 0, 0);
    gl("tree/{a\\,b}", GLOB_BRACE, 0, 0);
    gl("tree/{a,b}", GLOB_BRACE, 0, 0);
    gl("tree/{d1,d2}/*", GLOB_BRACE, 0, 0);
    gl("tree/{d1/f1,file1}", GLOB_BRACE, 0, 0);
    gl("tree/[{]a,b}", GLOB_BRACE, 0, 0);
    gl("tree/{a,b}{,}", GLOB_BRACE, 0, 0);
    gl("tree/{{a,b}}", GLOB_BRACE, 0, 0);
    gl("{tree,sort}/a", GLOB_BRACE, 0, 0);
    gl("tree/{file1,file1}", GLOB_BRACE, 0, 0);
    gl("tree/{file1,file1}", GLOB_BRACE | GLOB_DOOFFS, 1, 0);
    gl("tree/{q*,b*}", GLOB_BRACE | GLOB_NOESCAPE, 0, 0);
    gl("tree/{[ab],q?}", GLOB_BRACE, 0, 0);
    puts("-- GLOB_TILDE, GLOB_TILDE_CHECK");
    {
        char home[4200];
        snprintf(home, sizeof home, "%s/tilde", g_dir);
        setenv("HOME", home, 1);
        gl("~", GLOB_TILDE, 0, 0);
        gl("~/", GLOB_TILDE, 0, 0);
        gl("~/*", GLOB_TILDE, 0, 0);
        gl("~/t1", GLOB_TILDE, 0, 0);
        gl("~/nomatch", GLOB_TILDE, 0, 0);
        gl("~/nomatch", GLOB_TILDE | GLOB_NOCHECK, 0, 0);
        gl("~", 0, 0, 0);
        gl("~", GLOB_TILDE_CHECK, 0, 0);
        gl("~root", GLOB_TILDE, 0, 0);
        gl("~root/", GLOB_TILDE, 0, 0);
        gl("~root", GLOB_TILDE | GLOB_MARK, 0, 0);
        gl("~nosuchuser_zq", GLOB_TILDE, 0, 0);
        gl("~nosuchuser_zq", GLOB_TILDE | GLOB_NOCHECK, 0, 0);
        gl("~nosuchuser_zq/x", GLOB_TILDE_CHECK, 0, 0);
        gl("~nosuchuser_zq", GLOB_TILDE_CHECK | GLOB_NOCHECK, 0, 0);
        gl("~nosuchuser_zq", GLOB_TILDE_CHECK | GLOB_TILDE, 0, 0);
        gl("\\~", GLOB_TILDE, 0, 0);
        gl("\\~/t1", GLOB_TILDE, 0, 0);
        gl("~\\/t1", GLOB_TILDE, 0, 0);
        gl("tree/~", GLOB_TILDE, 0, 0);
        gl("x~", GLOB_TILDE | GLOB_NOCHECK, 0, 0);
        gl("~*", GLOB_TILDE | GLOB_NOCHECK, 0, 0);
        gl("~ro*", GLOB_TILDE | GLOB_NOCHECK, 0, 0);
        gl("{~,tree}", GLOB_TILDE | GLOB_BRACE, 0, 0);
        gl("~/t*", GLOB_TILDE | GLOB_NOESCAPE, 0, 0);
        setenv("HOME", "", 1);
        gl("~", GLOB_TILDE, 0, 0);
        gl("~/x", GLOB_TILDE | GLOB_NOCHECK, 0, 0);
        unsetenv("HOME");
        gl("~", GLOB_TILDE, 0, 0);
        gl("~", GLOB_TILDE_CHECK, 0, 0);
        setenv("HOME", home, 1);
    }
    puts("-- glob_pattern_p");
    {
        static const char *pp[] = {"abc", "a*", "a?", "a[b]", "a[", "a]", "a\\*", "\\", "a\\", "", "{a,b}", "~", "a[!b", "[]", "*\\"};
        for (size_t i = 0; i < sizeof pp / sizeof *pp; i++)
            for (int quote = 0; quote <= 1; quote++) {
                fputs("glob_pattern_p ", stdout);
                q(pp[i]);
                printf(" quote=%d = %d", quote, glob_pattern_p(pp[i], quote));
                nl();
            }
    }
    puts("-- invalid flags");
    {
        glob_t g;
        memset(&g, 0, sizeof g);
        errno = 0;
        int r = glob("tree/file*", 1 << 20, NULL, &g);
        printf("glob flags=1<<20 -> %d errno=%d pathc=%zu\n", r, errno, r == 0 ? g.gl_pathc : 0);
        if (r == 0)
            globfree(&g);
        memset(&g, 0, sizeof g);
        errno = 0;
        r = glob("tree/file*", -1 & ~0x7fff, NULL, &g);
        printf("glob flags=~0x7fff -> %d errno=%d\n", r, errno);
        if (r == 0)
            globfree(&g);
    }
    puts("-- GLOB_ALTDIRFUNC (\"opened\" lists the directories passed to gl_opendir: informative)");
    gl_alt("v/*", 0);
    gl_alt("v/*", GLOB_PERIOD);
    gl_alt("v/*", GLOB_MARK);
    gl_alt("v/*", GLOB_ONLYDIR);
    gl_alt("v/*/*", 0);
    gl_alt("v/x", 0);
    gl_alt("v/nomatch", 0);
    gl_alt("v/nomatch", GLOB_NOCHECK);
    gl_alt("*", 0);
    gl_alt("v/locked/*", 0);
    gl_alt("v/locked/*", GLOB_ERR);
    gl_alt("v/?", 0);
    gl_alt("v/u*", GLOB_MARK);
    gl_alt("v/*/", 0);
    gl_alt("v/dang", 0);
    gl_alt("v/dang", GLOB_MARK);
    gl_alt("v/dang", GLOB_NOCHECK);
    gl_alt("v/da*", 0);
    gl_alt("v/da*", GLOB_MARK);
    gl_alt("v/da*", GLOB_ONLYDIR);
    gl_alt("v/{dang,x}", GLOB_BRACE);
    puts("-- sorting under each locale (results sorted by the library)");
    for (size_t l = 0; l < NLOC; l++) {
        if (!use_locale(locales[l]))
            continue;
        gl("sort/*", 0, 0, 0);
        gl("sort/*", GLOB_PERIOD, 0, 0);
        gl("sort/{z,a,Z,A}", GLOB_BRACE, 0, 0);
        gl("sort/[[:upper:]]", 0, 0, 0);
        gl("sort/[a-z]", 0, 0, 0);
        gl("sort/[[=e=]]", 0, 0, 0);
        gl("sort/?", 0, 0, 0);
        gl("sort/[E\xc3\x89]", 0, 0, 0);
    }
    use_locale("C");
    puts("-- directory order (GLOB_NOSORT: the order of the scratch file system named on the fs: line)");
    fs_line();
    gl("sort/*", GLOB_NOSORT, 0, 0);
    gl("tree/*", GLOB_NOSORT, 0, 0);
    gl("tree/{d*,file*}", GLOB_NOSORT | GLOB_BRACE, 0, 0);
    gl("tree/*/*", GLOB_NOSORT, 0, 0);
    /* glob64 is the same interface */
    {
        glob64_t g;
        memset(&g, 0, sizeof g);
        int r = glob64("tree/file*", 0, NULL, &g);
        printf("glob64 [tree/file*] -> %d pathc=%zu %s %s\n", r, g.gl_pathc, g.gl_pathc > 0 ? g.gl_pathv[0] : "-", g.gl_pathc > 1 ? g.gl_pathv[1] : "-");
        globfree64(&g);
    }
}

#ifdef PATTERNS_COMPAT_GLOB
/* glob@GLIBC_2.2.5 (host dynamic builds only) */
int glob_old(const char *, int, int (*)(const char *, int), glob_t *);
__asm__(".symver glob_old,glob@GLIBC_2.2.5");

static void cmp_compat(const char *pat, int flags)
{
    glob_t g;
    memset(&g, 0, sizeof g);
    int r = glob_old(pat, flags, NULL, &g);
    glob_show("glob@2.2.5", pat, flags, r, &g);
    globfree(&g);
    gl(pat, flags, 0, 0);
}

static void sec_compat(void)
{
    puts("== glob compat (glob@GLIBC_2.2.5 then the default glob)");
    fs_line();
    use_locale("C");
    if (chdir(g_dir) != 0)
        return;
    cmp_compat("tree/dangling", 0);
    cmp_compat("tree/dangling", GLOB_NOCHECK);
    cmp_compat("tree/dangling", GLOB_MARK);
    cmp_compat("tree/dangling", GLOB_NOMAGIC);
    cmp_compat("tree/dang*", 0);
    cmp_compat("tree/dang*", GLOB_MARK);
    cmp_compat("tree/dang*", GLOB_ONLYDIR);
    cmp_compat("tree/loop", 0);
    cmp_compat("tree/l*", 0);
    cmp_compat("tree/l*", GLOB_MARK);
    cmp_compat("tree/{dangling,file1}", GLOB_BRACE);
    cmp_compat("tree/\\dangling", 0);
    cmp_compat("tree/link_d1", 0);
    cmp_compat("tree/file*", 0);
    puts("-- with GLOB_ALTDIRFUNC (v/dang: gl_lstat succeeds, gl_stat fails with ENOENT)");
    static const struct {
        const char *p;
        int f;
    } alt[] = {{"v/dang", 0}, {"v/dang", GLOB_MARK}, {"v/dang", GLOB_NOCHECK}, {"v/da*", 0}, {"v/da*", GLOB_MARK},
               {"v/da*", GLOB_ONLYDIR}, {"v/{dang,x}", GLOB_BRACE}, {"v/x", 0}, {"v/*", 0}};
    show_dang = 1;
    for (size_t i = 0; i < sizeof alt / sizeof *alt; i++) {
        glob_fn = glob_old;
        glob_alt_label = "glob-alt@2.2.5";
        gl_alt(alt[i].p, alt[i].f);
        glob_fn = glob;
        glob_alt_label = "glob-alt";
        gl_alt(alt[i].p, alt[i].f);
    }
    show_dang = 0;
}
#endif

/* ------------------------------------------------------------------ regex */

static void re_flags(int f)
{
    printf("%s%s%s%s", f & REG_EXTENDED ? "E" : "B", f & REG_ICASE ? "I" : "", f & REG_NEWLINE ? "N" : "", f & REG_NOSUB ? "S" : "");
    if (f & ~(REG_EXTENDED | REG_ICASE | REG_NEWLINE | REG_NOSUB))
        printf("+0x%x", f & ~(REG_EXTENDED | REG_ICASE | REG_NEWLINE | REG_NOSUB));
}

static void ee_flags(int f)
{
    if (!f) {
        fputs("-", stdout);
        return;
    }
    printf("%s%s%s", f & REG_NOTBOL ? "NOTBOL" : "", f & REG_NOTEOL ? (f & REG_NOTBOL ? "|NOTEOL" : "NOTEOL") : "",
           f & REG_STARTEND ? (f & (REG_NOTBOL | REG_NOTEOL) ? "|STARTEND" : "STARTEND") : "");
}

static void pm(const regmatch_t *m, size_t n)
{
    for (size_t i = 0; i < n; i++)
        printf("(%d,%d)", (int)m[i].rm_so, (int)m[i].rm_eo);
}

/* compile + one regexec; nmatch = re_nsub + 1 (at most 10) */
static void rx(int cf, const char *pat, const char *subj, int ef)
{
    regex_t re;
    int c = regcomp(&re, pat, cf);
    fputs("re ", stdout);
    re_flags(cf);
    putchar(' ');
    q(pat);
    putchar(' ');
    q(subj);
    putchar(' ');
    ee_flags(ef);
    if (c) {
        printf(" -> regcomp=%d", c);
        nl();
        return;
    }
    regmatch_t m[10];
    for (int i = 0; i < 10; i++)
        m[i].rm_so = m[i].rm_eo = -7;
    size_t n = re.re_nsub + 1 > 10 ? 10 : re.re_nsub + 1;
    int r = regexec(&re, subj, n, m, ef);
    printf(" -> %d nsub=%zu ", r, re.re_nsub);
    if (r == 0)
        pm(m, n);
    nl();
    regfree(&re);
}

/* Every successive match, as a global search loop does: after a match ending at e, the next
 * search starts at e (at e+1 after an empty match) with REG_NOTBOL. */
static void rx_all(int cf, const char *pat, const char *subj)
{
    regex_t re;
    int c = regcomp(&re, pat, cf);
    fputs("re-all ", stdout);
    re_flags(cf);
    putchar(' ');
    q(pat);
    putchar(' ');
    q(subj);
    if (c) {
        printf(" -> regcomp=%d", c);
        nl();
        return;
    }
    size_t len = strlen(subj), pos = 0;
    int ef = 0;
    putchar(' ');
    while (pos <= len) {
        regmatch_t m[4];
        size_t n = re.re_nsub + 1 > 4 ? 4 : re.re_nsub + 1;
        if (regexec(&re, subj + pos, n, m, ef))
            break;
        for (size_t i = 0; i < n; i++)
            printf("%s(%d,%d)", i ? "" : " ", m[i].rm_so < 0 ? -1 : (int)(m[i].rm_so + (regoff_t)pos),
                   m[i].rm_eo < 0 ? -1 : (int)(m[i].rm_eo + (regoff_t)pos));
        size_t e = pos + (size_t)m[0].rm_eo;
        pos = m[0].rm_eo == m[0].rm_so ? e + 1 : e;
        ef = REG_NOTBOL;
    }
    nl();
    regfree(&re);
}

/* REG_STARTEND on a buffer with embedded NULs */
static void rx_se(int cf, const char *pat, const char *buf, size_t blen, int so, int eo, int ef)
{
    regex_t re;
    int c = regcomp(&re, pat, cf);
    fputs("re-startend ", stdout);
    re_flags(cf);
    putchar(' ');
    q(pat);
    fputs(" [", stdout);
    rawesc(buf, blen);
    printf("] so=%d eo=%d ", so, eo);
    ee_flags(ef | REG_STARTEND);
    if (c) {
        printf(" -> regcomp=%d", c);
        nl();
        return;
    }
    regmatch_t m[4];
    for (int i = 0; i < 4; i++)
        m[i].rm_so = m[i].rm_eo = -7;
    m[0].rm_so = so;
    m[0].rm_eo = eo;
    size_t n = re.re_nsub + 1 > 4 ? 4 : re.re_nsub + 1;
    int r = regexec(&re, buf, n, m, ef | REG_STARTEND);
    printf(" -> %d ", r);
    pm(m, n);
    nl();
    regfree(&re);
}

struct rc {
    int cf;
    const char *p, *s;
    int ef;
};
#define E REG_EXTENDED
#define I REG_ICASE
#define N REG_NEWLINE
static const struct rc rcases[] = {
    /* leftmost-longest and subexpression selection (ERE) */
    {E, "(a|ab)(c|bcd)(d*)", "abcd", 0}, {E, "(a|ab)(bcd|c)(d*)", "abcd", 0}, {E, "(ab|a)(c|bcd)(d*)", "abcd", 0},
    {E, "(a*)*", "b", 0}, {E, "(a*)*", "aa", 0}, {E, "(a*)*", "", 0}, {E, "(a*)+", "b", 0}, {E, "(a*)+", "aa", 0},
    {E, "(a|b)*", "abab", 0}, {E, "(a|b)*", "", 0}, {E, "((a)|b)+", "ab", 0}, {E, "((a)|b)+", "ba", 0}, {E, "((a)|b)+", "aba", 0},
    {E, "((a)|b)*", "ab", 0}, {E, "(a|ab)*", "abab", 0}, {E, "(ab|a)*", "abab", 0}, {E, "(a|ab)*c", "ababc", 0},
    {E, "(a|ab)(b*)", "abb", 0}, {E, "(ab|a)(b*)", "abb", 0}, {E, "(a*)(a*)", "aaa", 0}, {E, "(a*)(a+)", "aaa", 0},
    {E, "(a+)(a*)", "aaa", 0}, {E, "(a*)(ab)*(b*)", "abb", 0}, {E, "(a*)(ab)?(b*)", "abb", 0}, {E, "(a|ab)(c|bcd)", "abcd", 0},
    {E, "(.*)(.*)", "ab", 0}, {E, "(.*)c(.*)", "abcdcef", 0}, {E, "(.?)(.?)(.?)", "ab", 0}, {E, "a(b|c)*d", "abcbd", 0},
    {E, "(a(b)?)+", "aba", 0}, {E, "(a(b)?)+", "abab", 0}, {E, "(a(b)?)+", "aab", 0}, {E, "(a(b)*)*", "abaa", 0},
    {E, "((a)|(b))*", "ab", 0}, {E, "((a)|(b))*", "ba", 0}, {E, "((a)|(b))+", "aab", 0}, {E, "(a|(b))+", "ba", 0},
    {E, "(a)|b", "b", 0}, {E, "(a)|(b)", "b", 0}, {E, "(a)?b", "b", 0}, {E, "(a)?(b)?", "", 0}, {E, "(a?)?", "", 0},
    {E, "(a?)?", "a", 0}, {E, "(a?)*", "a", 0}, {E, "(a?)+", "a", 0}, {E, "(a?)?b", "b", 0}, {E, "(a)?(a)", "a", 0},
    {E, "(a)*(a)", "aa", 0}, {E, "()", "", 0}, {E, "()*", "", 0}, {E, "()*", "x", 0}, {E, "()+", "x", 0}, {E, "(()|a)*", "aa", 0},
    {E, "(a|())*", "aa", 0}, {E, "(()|a)+", "a", 0}, {E, "((a*)*)*", "a", 0}, {E, "(a*)*(b)", "aab", 0}, {E, "x*", "", 0},
    {E, "x*", "y", 0}, {E, "x*", "xxy", 0}, {E, "x*", "yxx", 0}, {E, "x*$", "yxx", 0}, {E, "(x*)$", "yx", 0}, {E, "$", "abc", 0},
    {E, "^", "abc", 0}, {E, "(a|b|c)*c", "abcabc", 0}, {E, "(.)(.)\\2\\1", "xabba", 0},
    /* bounded repetition */
    {E, "(a){2}", "aaa", 0}, {E, "(a){0,2}", "aaa", 0}, {E, "(a){0}", "a", 0}, {E, "(a){0}b", "ab", 0}, {E, "(a|ab){1,2}(c)?", "abac", 0},
    {E, "(a|ab){2}", "abab", 0}, {E, "(a*){2,3}", "aa", 0}, {E, "(a*){2}", "aa", 0}, {E, "(a?){3}", "aa", 0}, {E, "a{,2}", "aaa", 0},
    {E, "a{1}{2}", "aa", 0}, {E, "a{2}*", "aaaaa", 0}, {E, "a{1,}", "aaaa", 0}, {E, "(ab){1,3}", "ababab", 0}, {E, "(a|b){2,3}", "abab", 0},
    {E, "a{2,2}", "aaa", 0}, {E, "a{0,0}", "a", 0}, {E, "a{3}", "aa", 0}, {E, "(a{2})*", "aaaaa", 0}, {E, "((a)|b){2}", "ab", 0},
    {E, "((a)|b){2}", "ba", 0}, {E, "a{1,2}a{1,2}", "aaa", 0}, {E, "a{", "a{", 0}, {E, "a{x", "a{x", 0}, {E, "a{1", "a{1", 0},
    {E, "a{1,x}", "a", 0}, {E, "{1}", "{1}", 0}, {E, "a{}", "a{}", 0}, {E, "a{,}", "a", 0}, {E, "a{32767}", "a", 0},
    {E, "a{32768}", "a", 0}, {E, "a{2,1}", "a", 0}, {E, "a{65536}", "a", 0},
    /* back-references */
    {E, "(a*)\\1", "aaaa", 0}, {E, "(a*)\\1", "aaa", 0}, {E, "(a*)\\1b", "aaab", 0}, {E, "((a)|b)\\2", "aa", 0}, {E, "((a)|b)\\2", "ba", 0},
    {E, "((a)|b)*\\2", "aba", 0}, {E, "(a|ab)\\1", "abab", 0}, {E, "(a|ab)(c|bcd)\\2", "abcdbcd", 0}, {E, "(a*)(a*)\\2", "aaaa", 0},
    {E, "(a*)(a*)\\1", "aaaa", 0}, {E, "(a)\\1*", "aaa", 0}, {E, "(a)|\\1", "x", 0}, {E, "(a)?\\1", "", 0}, {E, "(a)?\\1", "aa", 0},
    {E, "(a*)*\\1", "aa", 0}, {E, "(a|b)*\\1", "abb", 0}, {E, "(a|b)*\\1", "aba", 0}, {E, "((a)|b)+\\2", "aba", 0}, {E, "(a)\\1{2}", "aaaa", 0},
    {E, "(.)\\1", "abba", 0}, {E, "(\\1)", "a", 0}, {E, "(a)\\2", "aa", 0}, {E, "\\1(a)", "aa", 0}, {E, "(a)(b)(c)(d)(e)(f)(g)(h)(i)\\9", "abcdefghii", 0},
    {E, "(a\\1)", "aa", 0}, {E, "(x)?(a)\\1", "a", 0},
    {0, "\\(a*\\)\\1", "aaaa", 0}, {0, "\\(a*\\)*\\1", "aa", 0}, {0, "\\(a\\|b\\)*\\1", "abb", 0}, {0, "\\(a\\)\\(b\\)\\2\\1", "abba", 0},
    {0, "\\(\\(a\\)\\|b\\)*\\2", "aba", 0}, {0, "\\(a*\\)\\{2\\}", "aaaa", 0}, {0, "\\(a\\)\\1\\{2,\\}", "aaaa", 0}, {0, "\\(a\\)\\2", "a", 0},
    /* BRE versus ERE */
    {0, "a\\{2\\}", "aaa", 0}, {0, "a{2}", "a{2}", 0}, {0, "a{2}", "aa", 0}, {E, "a\\{2\\}", "a{2}", 0}, {0, "\\(ab\\)*", "abab", 0},
    {0, "(ab)*", "(ab)", 0}, {0, "a\\+", "aaa", 0}, {0, "a+", "a+", 0}, {0, "a+", "aa", 0}, {0, "a\\?", "a", 0}, {0, "a?", "a?", 0},
    {0, "*a", "*a", 0}, {0, "\\(*a\\)", "*a", 0}, {0, "^*", "*", 0}, {0, "^*a", "*a", 0}, {0, "a\\|b", "b", 0}, {0, "a|b", "a|b", 0},
    {0, "a**", "aa", 0}, {0, "a^b", "a^b", 0}, {0, "a$b", "a$b", 0}, {0, "\\(^a\\)", "a", 0}, {0, "\\(a$\\)", "a", 0},
    {0, "x\\(^a\\)", "x^a", 0}, {0, "\\(a\\)$", "a", 0}, {0, "a\\{1,2\\}\\{2\\}", "aaaa", 0}, {0, "\\{1\\}", "{1}", 0},
    {0, "a\\{,2\\}", "aaa", 0}, {0, "\\(\\)", "", 0}, {0, "a\\{1", "a", 0}, {0, "a\\{1\\}\\{", "a", 0}, {0, "\\)", ")", 0},
    {0, "\\(", "(", 0}, {0, "a\\", "a", 0}, {0, "[a", "a", 0}, {0, "\\{", "{", 0}, {0, "a\\{x\\}", "a", 0}, {0, "a*\\{2\\}", "aa", 0},
    {E, "a|*b", "*b", 0}, {E, "a||b", "", 0}, {E, "(|a)", "a", 0}, {E, "(a|)", "", 0}, {E, "|a", "a", 0}, {E, "a|", "", 0},
    {E, "a**", "aa", 0}, {E, "a+*", "aa", 0}, {E, "a*+", "aa", 0}, {E, "a?*", "aa", 0}, {E, "^*", "*", 0}, {E, "*a", "a", 0},
    {E, "+a", "a", 0}, {E, "?a", "a", 0}, {E, "(*a)", "a", 0}, {E, "(+a)", "a", 0}, {E, "a(*b)", "ab", 0}, {E, "$a", "a", 0},
    {E, "a^", "a", 0}, {E, "a$b", "a$b", 0}, {E, "a\\$b", "a$b", 0}, {E, "(^a)", "a", 0}, {E, "x(^a)", "xa", 0}, {E, "a)", "a)", 0},
    {E, ")", ")", 0}, {E, "(", "(", 0}, {E, "(a", "a", 0}, {E, "()a", "a", 0}, {E, "a(", "a", 0}, {E, "\\(a\\)", "(a)", 0},
    {E, "a\\", "a", 0}, {E, "[", "[", 0}, {E, "]", "]", 0}, {E, "a]", "a]", 0}, {E, "{", "{", 0}, {E, "a\\{", "a{", 0}, {E, "}", "}", 0},
    {E, "a{1}}", "a}", 0}, {E, "\\a", "a", 0}, {E, "\\.", "a", 0}, {E, "\\n", "n", 0}, {E, "\\t", "t", 0}, {E, "\\%", "%", 0},
    /* bracket expressions */
    {E, "[]a]", "]", 0}, {E, "[^]a]", "b", 0}, {E, "[a-]", "-", 0}, {E, "[]-a]", "-", 0}, {E, "[a-c]", "b", 0}, {E, "[c-a]", "b", 0},
    {E, "[a-a]", "a", 0}, {E, "[a-c-e]", "d", 0}, {E, "[--/]", ".", 0}, {E, "[\\]", "\\", 0}, {E, "[\\n]", "n", 0}, {E, "[[:alpha:]]", "a", 0},
    {E, "[[:foo:]]", "a", 0}, {E, "[[:alpha:]", "a", 0}, {E, "[[:alpha]", "a", 0}, {E, "[[:ALPHA:]]", "a", 0}, {E, "[[=a=]]", "a", 0},
    {E, "[[=ab=]]", "a", 0}, {E, "[[.a.]]", "a", 0}, {E, "[[.hyphen.]]", "-", 0}, {E, "[[.space.]]", " ", 0}, {E, "[[.foo.]]", "a", 0},
    {E, "[[.a.]-c]", "b", 0}, {E, "[a-[.c.]]", "b", 0}, {E, "[[:alpha:]-z]", "-", 0}, {E, "[a-[:alpha:]]", "b", 0}, {E, "[^a]", "\n", 0},
    {E | N, "[^a]", "\n", 0}, {E, "[[.]", "[", 0}, {E, "[[=]", "[", 0}, {E, "[[:]", "[", 0}, {E, "[]", "]", 0}, {E, "[^]", "^", 0},
    {E, "[[]", "[", 0}, {E, "[a[]", "[", 0}, {E, "[[.].]]", "]", 0}, {E, "[[.^.]a]", "^", 0}, {E, "[Z-a]", "_", 0}, {E, "[a-Z]", "_", 0},
    /* REG_ICASE */
    {E | I, "[a-c]+", "ABC", 0}, {E | I, "(a)\\1", "aA", 0}, {E | I, "[^a]", "A", 0}, {E | I, "[[:lower:]]", "A", 0},
    {E | I, "[[:upper:]]", "a", 0}, {E | I, "abc", "xAbC", 0}, {E | I, "[A-C]", "b", 0}, {E | I, "[Z-a]", "z", 0}, {E | I, "[Z-a]", "A", 0},
    {E | I, "[^[:lower:]]", "a", 0}, {E | I, "[^A-Z]", "q", 0},
    /* REG_NEWLINE, REG_NOTBOL, REG_NOTEOL */
    {E, "^b", "a\nb", 0}, {E | N, "^b", "a\nb", 0}, {E, "a$", "a\nb", 0}, {E | N, "a$", "a\nb", 0}, {E, "a.b", "a\nb", 0},
    {E | N, "a.b", "a\nb", 0}, {E, "a[^x]b", "a\nb", 0}, {E | N, "a[^x]b", "a\nb", 0}, {E | N, "a[\n]b", "a\nb", 0},
    {E | N, "a\nb", "a\nb", 0}, {E | N, ".*", "ab\ncd", 0}, {E | N, "^", "\nb", REG_NOTBOL}, {E | N, "^b", "b\nb", REG_NOTBOL},
    {E | N, "a$", "a\na", REG_NOTEOL}, {E, "^a", "a", REG_NOTBOL}, {E, "^a", "a", 0}, {E, "a$", "a", REG_NOTEOL}, {E, "$", "", REG_NOTEOL},
    {E, "^", "", REG_NOTBOL}, {E, "^$", "", REG_NOTBOL | REG_NOTEOL}, {E, "^*a", "a", REG_NOTBOL}, {E | N, "^$", "a\n\nb", 0},
    {E | N, "$", "a\nb", 0}, {E, "\\`a", "a", REG_NOTBOL}, {E, "a\\'", "a", REG_NOTEOL}, {E | N, "\\`b", "a\nb", 0},
    {E | N, "a\\'", "a\nb", 0}, {E | N | I, "^B", "a\nb", 0}, {E | N, "(^|x)b", "a\nb", 0}, {E | N, "a(\n)*b", "a\n\nb", 0},
    {E | N, "[[:space:]]", "\n", 0}, {E | N, "[[:cntrl:]]", "\n", 0}, {E | N, "[^[:alpha:]]", "\n", 0},
    /* GNU operators through regcomp */
    {E, "\\bfoo\\b", "a foo b", 0}, {E, "\\<a", "ba a", 0}, {E, "a\\>", "ab a", 0}, {E, "\\w+", "--ab_1--", 0}, {E, "\\W+", "ab--cd", 0},
    {E, "\\Ba", "ba a", 0}, {E, "\\s+", "a \t b", 0}, {E, "\\S+", "  ab ", 0}, {E, "\\`", "a", 0}, {E, "a\\'", "a", 0}, {0, "\\bfoo", "foo", 0},
    {0, "\\w\\+", "ab", 0}, {E, "\\b", "", 0}, {E, "\\<", "", 0}, {E, "\\B", "", 0},
    /* pathological patterns without back-references (results only) */
    {E, "(a*)*b", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", 0}, {E, "(x+x+)+y", "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", 0},
    {E, "(a|aa)*c", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", 0}, {E, "(a?){30}a{30}", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", 0},
    {E, "(.*)*x", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", 0},
};

static const struct rc rloc[] = {
    {E, ".", "\xc3\xa9", 0}, {E, "^.$", "\xc3\xa9", 0}, {E, "[\xc3\xa9]", "x\xc3\xa9", 0}, {E, "[a-z]", "\xc3\xa9", 0},
    {E | I, "\xc3\xa9", "\xc3\x89", 0}, {E | I, "[\xc3\xa9]", "\xc3\x89", 0}, {E, "[[=e=]]", "\xc3\xa9", 0}, {E, "[[=e=]]", "E", 0},
    {E, "[[:alpha:]]+", "\xc3\xa9t\xc3\xa9", 0}, {E, ".", "\xff", 0}, {E, "a.b", "a\xff" "b", 0}, {E, "a..b", "a\xc3\xa9" "b", 0},
    {E, "[^a]", "\xc3\xa9", 0}, {E, "[^a]", "\xff", 0}, {E, "\xc3", "\xc3\xa9", 0}, {E, "\xa9", "\xc3\xa9", 0}, {E, "\xc3.", "\xc3\xa9", 0},
    {E, "(.)\\1", "\xc3\xa9\xc3\xa9", 0}, {E, "\\w+", "\xc3\xa9t\xc3\xa9", 0}, {E, "\\bt", "\xc3\xa9t", 0}, {E, "[[.\xc3\xa9.]]", "\xc3\xa9", 0},
    {E, "[\xc3\xa0-\xc3\xbf]", "\xc3\xa9", 0}, {E, "[b-\xc3\xa9]", "\xc3\xa0", 0}, {E, "[b-\xc3\xa9]", "C", 0}, {E, "[A-z]", "b", 0},
    {E, "[a-Z]", "b", 0}, {E, "[[.ch.]]", "ch", 0}, {E | I, "[[:upper:]]", "\xc3\xa9", 0}, {E, "[[:digit:]]", "\xd9\xa3", 0},
    {E, "x*", "\xc3\xa9x", 0}, {E, "\xe2\x82\xac+", "\xe2\x82\xac\xe2\x82\xac", 0}, {E, "[\xff]", "\xff", 0}, {E, "\xff", "\xff", 0},
    {E, "a{2}", "aa", 0},
};

/* The corpora: every pattern of up to MAXTOK tokens, compiled; for those that compile, the
 * match of each subject. Line: "rc <flags> [pattern] <regcomp code> <nsub> <results>", where
 * results are the subjects' outcomes in order, separated by spaces: "-" no match, otherwise
 * so.eo for each of the nsub+1 pmatch entries, joined by ','. */
static char rsubj[128][8];
static int nrsubj;

static void rsubjects(int maxlen)
{
    nrsubj = 0;
    for (int len = 0; len <= maxlen; len++)
        for (int k = 0; k < (1 << len); k++) {
            for (int i = 0; i < len; i++)
                rsubj[nrsubj][i] = (k >> (len - 1 - i)) & 1 ? 'b' : 'a';
            rsubj[nrsubj][len] = 0;
            nrsubj++;
        }
}

static void rcorpus_one(int cf, const char *pat)
{
    regex_t re;
    int c = regcomp(&re, pat, cf);
    fputs("rc ", stdout);
    re_flags(cf);
    putchar(' ');
    q(pat);
    printf(" %d", c);
    if (c) {
        nl();
        return;
    }
    size_t n = re.re_nsub + 1 > 10 ? 10 : re.re_nsub + 1;
    printf(" %zu", re.re_nsub);
    for (int s = 0; s < nrsubj; s++) {
        regmatch_t m[10];
        putchar(' ');
        if (regexec(&re, rsubj[s], n, m, 0)) {
            putchar('-');
            continue;
        }
        for (size_t i = 0; i < n; i++)
            printf("%s%d.%d", i ? "," : "", (int)m[i].rm_so, (int)m[i].rm_eo);
    }
    nl();
    regfree(&re);
}

static void rcorpus(int cf, const char *const *tok, int ntok, char *buf, int depth, int maxdepth)
{
    if (depth == maxdepth) {
        rcorpus_one(cf, buf);
        return;
    }
    size_t l = strlen(buf);
    for (int t = 0; t < ntok; t++) {
        strcpy(buf + l, tok[t]);
        rcorpus(cf, tok, ntok, buf, depth + 1, maxdepth);
        buf[l] = 0;
    }
}

static void sec_regex(void)
{
    puts("== regex");
    printf("constants EXTENDED=%d ICASE=%d NEWLINE=%d NOSUB=%d NOTBOL=%d NOTEOL=%d STARTEND=%d\n", REG_EXTENDED, REG_ICASE, REG_NEWLINE,
           REG_NOSUB, REG_NOTBOL, REG_NOTEOL, REG_STARTEND);
    printf("codes NOMATCH=%d BADPAT=%d ECOLLATE=%d ECTYPE=%d EESCAPE=%d ESUBREG=%d EBRACK=%d EPAREN=%d EBRACE=%d BADBR=%d ERANGE=%d "
           "ESPACE=%d BADRPT=%d EEND=%d ESIZE=%d ERPAREN=%d\n",
           REG_NOMATCH, REG_BADPAT, REG_ECOLLATE, REG_ECTYPE, REG_EESCAPE, REG_ESUBREG, REG_EBRACK, REG_EPAREN, REG_EBRACE, REG_BADBR,
           REG_ERANGE, REG_ESPACE, REG_BADRPT, REG_EEND, REG_ESIZE, REG_ERPAREN);
    printf("sizeof regoff_t=%zu regmatch_t=%zu\n", sizeof(regoff_t), sizeof(regmatch_t));
    use_locale("C");
    puts("-- regerror");
    for (int code = -1; code <= 18; code++) {
        char label[32];
        snprintf(label, sizeof label, "regerror %d", code);
        IN_CHILD(label, {
            char buf[200];
            memset(buf, 'Z', sizeof buf);
            size_t r = regerror(code, NULL, buf, sizeof buf);
            printf("regerror %d = %zu ", code, r);
            q(buf);
            nl();
        });
    }
    IN_CHILD("regerror 1000", {
        char buf[200];
        size_t r = regerror(1000, NULL, buf, sizeof buf);
        printf("regerror 1000 = %zu\n", r);
    });
    {
        char small[6];
        memset(small, 'Z', sizeof small);
        size_t r = regerror(REG_EBRACK, NULL, small, 5);
        printf("regerror EBRACK size=5 = %zu [", r);
        rawesc(small, 6);
        puts("]");
        r = regerror(REG_EBRACK, NULL, NULL, 0);
        printf("regerror EBRACK NULL,0 = %zu\n", r);
        memset(small, 'Z', sizeof small);
        r = regerror(REG_EBRACK, NULL, small, 1);
        printf("regerror EBRACK size=1 = %zu [", r);
        rawesc(small, 2);
        puts("]");
        regex_t re;
        int c = regcomp(&re, "a", 0);
        char buf[100];
        r = regerror(REG_NOMATCH, &re, buf, sizeof buf);
        printf("regerror NOMATCH with a compiled regex = %zu [%s] (regcomp=%d)\n", r, buf, c);
        regfree(&re);
    }
    puts("-- regcomp errors");
    {
        static const struct {
            int cf;
            const char *p;
        } errs[] = {
            {E, "[[:foo:]]"}, {0, "[[:foo:]]"}, {E, "a\\"}, {0, "a\\"}, {0, "\\(a\\)\\2"}, {E, "(a)\\2"}, {E, "\\1"}, {0, "\\1"},
            {E, "[a"}, {0, "[a"}, {E, "[]"}, {E, "[^]"}, {E, "(a"}, {E, "a)"}, {0, "\\(a"}, {0, "a\\)"}, {0, "a\\{1"}, {E, "a{1"},
            {0, "a\\{1,2"}, {E, "a{1,2"}, {E, "a{2,1}"}, {0, "a\\{2,1\\}"}, {E, "a{x}"}, {0, "a\\{x\\}"}, {E, "a{-1}"},
            {E, "a{1,2,3}"}, {E, "[b-a]"}, {0, "[b-a]"}, {E, "[a-[:alpha:]]"}, {E, "[[:alpha:]-z]"}, {E, "[--a]"}, {E, "*a"},
            {E, "a**"}, {E, "(*)"}, {E, "a|*"}, {E, "^*"}, {E, "+"}, {E, "?"}, {E, "{1}"}, {E, "a{1}{2}"}, {E, "a*{2}"},
            {E, "[[.foo.]]"}, {0, "[[.foo.]]"}, {E, "[[=foo=]]"}, {E, "[[.ab.]]"}, {E, "a{32768}"}, {E, "a{32767}"}, {E, "a{1,32768}"},
            {E, "()"}, {E, ""}, {0, ""}, {E, "(|)"}, {E, "\\"}, {0, "\\"}, {E, "[\\"}, {E, "a{99999999999}"}, {E, "[[:alpha:"},
            {E, "[[="}, {E, "[[."}, {E, "[a-"}, {E, "[[.a"}, {0, "*"}, {0, "\\(*\\)"}, {0, "\\{1\\}"}, {0, "a\\{1\\}\\{2\\}"},
            {E | I, "[b-a]"}, {E, "a(?:b)"}, {E, "(?i)a"}, {E, "a{,}"}, {E, "a{,1}"},
        };
        for (size_t i = 0; i < sizeof errs / sizeof *errs; i++) {
            regex_t re;
            int c = regcomp(&re, errs[i].p, errs[i].cf);
            fputs("regcomp ", stdout);
            re_flags(errs[i].cf);
            putchar(' ');
            q(errs[i].p);
            printf(" = %d", c);
            if (!c) {
                printf(" nsub=%zu", re.re_nsub);
                regfree(&re);
            }
            nl();
        }
        regex_t re;
        int c = regcomp(&re, "a", 0x100);
        printf("regcomp unknown cflag 0x100 = %d\n", c);
        if (!c)
            regfree(&re);
    }
    puts("-- matching");
    for (size_t i = 0; i < sizeof rcases / sizeof *rcases; i++)
        rx(rcases[i].cf, rcases[i].p, rcases[i].s, rcases[i].ef);
    puts("-- successive matches (search loop with REG_NOTBOL)");
    static const struct rc alls[] = {
        {E, "x*", "axxbx", 0}, {E, "a|b", "abab", 0}, {E, "(a*)", "baab", 0}, {E, "^a", "aaa", 0}, {E, "$", "ab", 0}, {E, "\\<", "ab cd", 0},
        {E, "b*", "abc", 0}, {E, "()", "ab", 0}, {E | N, "^.", "ab\ncd", 0}, {E | N, "$", "a\nb", 0}, {0, "a*", "baaac", 0},
    };
    for (size_t i = 0; i < sizeof alls / sizeof *alls; i++)
        rx_all(alls[i].cf, alls[i].p, alls[i].s);
    puts("-- REG_NOSUB, nmatch");
    {
        regex_t re;
        regcomp(&re, "(a)(b)?", E | REG_NOSUB);
        regmatch_t m[3] = {{77, 77}, {77, 77}, {77, 77}};
        int r = regexec(&re, "xab", 3, m, 0);
        printf("NOSUB regexec = %d nsub=%zu ", r, re.re_nsub);
        pm(m, 3);
        nl();
        r = regexec(&re, "xyz", 3, m, 0);
        printf("NOSUB regexec nomatch = %d\n", r);
        r = regexec(&re, "xab", 0, NULL, 0);
        printf("NOSUB regexec nmatch=0 NULL = %d\n", r);
        regfree(&re);
        regcomp(&re, "(a)(b)(c)?", E);
        regmatch_t m5[6];
        for (int i = 0; i < 6; i++)
            m5[i].rm_so = m5[i].rm_eo = 77;
        r = regexec(&re, "xab", 1, m5, 0);
        printf("nmatch=1 = %d ", r);
        pm(m5, 6);
        nl();
        for (int i = 0; i < 6; i++)
            m5[i].rm_so = m5[i].rm_eo = 77;
        r = regexec(&re, "xab", 6, m5, 0);
        printf("nmatch=6 = %d ", r);
        pm(m5, 6);
        nl();
        r = regexec(&re, "xab", 0, NULL, 0);
        printf("nmatch=0 = %d\n", r);
        for (int i = 0; i < 6; i++)
            m5[i].rm_so = m5[i].rm_eo = 77;
        r = regexec(&re, "xyz", 6, m5, 0);
        printf("nomatch nmatch=6 = %d ", r);
        pm(m5, 6);
        nl();
        regfree(&re);
    }
    puts("-- REG_STARTEND");
    {
        static const char b1[] = "xa\0ab\0c";
        rx_se(E, "a", b1, 7, 0, 7, 0);
        rx_se(E, "ab", b1, 7, 0, 7, 0);
        rx_se(E, "ab", b1, 7, 3, 7, 0);
        rx_se(E, "^a", b1, 7, 3, 7, 0);
        rx_se(E, "^a", b1, 7, 3, 7, REG_NOTBOL);
        rx_se(E, "^a", b1, 7, 1, 7, 0);
        rx_se(E, "b$", b1, 7, 0, 5, 0);
        rx_se(E, "b$", b1, 7, 0, 5, REG_NOTEOL);
        rx_se(E, "c$", b1, 7, 0, 7, 0);
        rx_se(E, "a.a", b1, 7, 0, 7, 0);
        rx_se(E, "a[^x]a", b1, 7, 0, 7, 0);
        rx_se(E, "a(.)a", b1, 7, 0, 7, 0);
        rx_se(E, ".*", b1, 7, 0, 7, 0);
        rx_se(E, ".*", b1, 7, 2, 2, 0);
        rx_se(E, "x*", b1, 7, 2, 2, 0);
        rx_se(E, "a", b1, 7, 4, 3, 0);
        rx_se(E, "(a)(b)", b1, 7, 0, 7, 0);
        rx_se(E | N, "^c", b1, 7, 0, 7, 0);
        rx_se(E | N, "\\`a", b1, 7, 3, 7, 0);
        rx_se(E, "\\<a", b1, 7, 3, 7, 0);
        rx_se(E, "\\ba", "xa", 2, 1, 2, 0);
        rx_se(E, "\\Ba", "xa", 2, 1, 2, 0);
        rx_se(E | N, "^a", "x\na", 3, 2, 3, REG_NOTBOL);
        rx_se(E, "[[:cntrl:]]", b1, 7, 0, 7, 0);
        rx_se(E | I, "A", b1, 7, 2, 7, 0);
        rx_se(0, "\\(a\\).\\1", b1, 7, 0, 7, 0);
    }
    puts("-- character classes (bytes 1-255 that [[:class:]] matches, C locale, as hex ranges)");
    {
        static const char *cls[] = {"alpha", "digit", "alnum", "upper", "lower", "space", "blank", "punct", "print", "graph", "cntrl", "xdigit"};
        for (int lc = 0; lc < 2; lc++) {
            if (!use_locale(lc ? "C.UTF-8" : "C"))
                continue;
            for (size_t c = 0; c < sizeof cls / sizeof *cls; c++)
                for (int ic = 0; ic <= 1; ic++) {
                    char pat[32];
                    snprintf(pat, sizeof pat, "[[:%s:]]", cls[c]);
                    regex_t re;
                    if (regcomp(&re, pat, E | (ic ? I : 0) | REG_NOSUB)) {
                        printf("class %s: regcomp failed\n", cls[c]);
                        continue;
                    }
                    printf("class %s%s:", cls[c], ic ? " ICASE" : "");
                    int start = -1;
                    for (int b = 1; b <= 256; b++) {
                        int hit = 0;
                        if (b < 256) {
                            char s[2] = {(char)b, 0};
                            hit = regexec(&re, s, 0, NULL, 0) == 0;
                        }
                        if (hit && start < 0)
                            start = b;
                        if (!hit && start >= 0) {
                            if (start == b - 1)
                                printf(" %02x", start);
                            else
                                printf(" %02x-%02x", start, b - 1);
                            start = -1;
                        }
                    }
                    nl();
                    regfree(&re);
                }
        }
        use_locale("C");
    }
    puts("-- locale dependent");
    for (size_t l = 0; l < NLOC; l++) {
        if (!use_locale(locales[l]))
            continue;
        for (size_t i = 0; i < sizeof rloc / sizeof *rloc; i++)
            rx(rloc[i].cf, rloc[i].p, rloc[i].s, rloc[i].ef);
    }
    use_locale("C");
}

static void sec_regex_corpus(void)
{
    puts("== regex corpus");
    use_locale("C");
    char buf[128] = "";
    static const char *ere[] = {"a", "b", "(", ")", "|", "*", "+", "?"};
    rsubjects(3);
    fputs("subjects", stdout);
    for (int s = 0; s < nrsubj; s++) {
        putchar(' ');
        q(rsubj[s]);
    }
    nl();
    puts("-- ERE: tokens a b ( ) | * + ?, up to 5 tokens");
    for (int d = 0; d <= 5; d++)
        rcorpus(E, ere, 8, buf, 0, d);
    puts("-- ERE with {m,n} and back-references: tokens a b ( ) | {0,1} {2} \\1, up to 4 tokens");
    static const char *ere2[] = {"a", "b", "(", ")", "|", "{0,1}", "{2}", "\\1"};
    for (int d = 0; d <= 4; d++)
        rcorpus(E, ere2, 8, buf, 0, d);
    rsubjects(4);
    fputs("subjects", stdout);
    for (int s = 0; s < nrsubj; s++) {
        putchar(' ');
        q(rsubj[s]);
    }
    nl();
    puts("-- BRE: tokens a b \\( \\) * \\1, up to 5 tokens");
    static const char *bre[] = {"a", "b", "\\(", "\\)", "*", "\\1"};
    for (int d = 0; d <= 5; d++)
        rcorpus(0, bre, 6, buf, 0, d);
    puts("-- ERE REG_ICASE: tokens a B ( ) | * \\1, up to 4 tokens");
    static const char *eri[] = {"a", "B", "(", ")", "|", "*", "\\1"};
    for (int d = 0; d <= 4; d++)
        rcorpus(E | I, eri, 7, buf, 0, d);
}

/* ------------------------------------------------------------------ GNU regex interface */

static void gnu_search(const char *label, reg_syntax_t syn, const char *pat, const char *s, int slen, int start, int range)
{
    struct re_pattern_buffer b;
    memset(&b, 0, sizeof b);
    re_syntax_options = syn;
    const char *err = re_compile_pattern(pat, strlen(pat), &b);
    printf("re_search %s ", label);
    q(pat);
    fputs(" [", stdout);
    rawesc(s, (size_t)slen);
    printf("] start=%d range=%d", start, range);
    if (err) {
        fputs(" -> compile ", stdout);
        q(err);
        nl();
        return;
    }
    struct re_registers regs;
    memset(&regs, 0, sizeof regs);
    regoff_t r = re_search(&b, s, slen, start, range, &regs);
    printf(" -> %d nsub=%zu", (int)r, b.re_nsub);
    if (r >= 0) {
        printf(" regs=%u", (unsigned)(regs.num_regs > 10 ? 10 : regs.num_regs));
        for (unsigned i = 0; i < regs.num_regs && i < b.re_nsub + 1 && i < 10; i++)
            printf(" (%d,%d)", (int)regs.start[i], (int)regs.end[i]);
    }
    regoff_t m = re_match(&b, s, slen, start, NULL);
    printf(" re_match@start=%d", (int)m);
    nl();
    free(regs.start);
    free(regs.end);
    regfree(&b);
}

static void sec_gnu(void)
{
    puts("== GNU regex interface");
    use_locale("C");
    printf("re_syntax_options initially 0x%lx\n", (unsigned long)re_syntax_options);
    printf("syntaxes POSIX_EXTENDED=0x%lx POSIX_BASIC=0x%lx EMACS=0x%lx GREP=0x%lx EGREP=0x%lx POSIX_EGREP=0x%lx AWK=0x%lx\n",
           (unsigned long)RE_SYNTAX_POSIX_EXTENDED, (unsigned long)RE_SYNTAX_POSIX_BASIC, (unsigned long)RE_SYNTAX_EMACS,
           (unsigned long)RE_SYNTAX_GREP, (unsigned long)RE_SYNTAX_EGREP, (unsigned long)RE_SYNTAX_POSIX_EGREP, (unsigned long)RE_SYNTAX_AWK);
    gnu_search("POSIX_EXTENDED", RE_SYNTAX_POSIX_EXTENDED, "a|ab", "xab", 3, 0, 3);
    gnu_search("POSIX_EXTENDED", RE_SYNTAX_POSIX_EXTENDED, "(a|ab)(c|bcd)(d*)", "abcd", 4, 0, 4);
    gnu_search("EMACS", RE_SYNTAX_EMACS, "\\(a\\|ab\\)\\(c\\|bcd\\)\\(d*\\)", "abcd", 4, 0, 4);
    gnu_search("EMACS", RE_SYNTAX_EMACS, "a|b", "a|b", 3, 0, 3);
    gnu_search("EMACS", RE_SYNTAX_EMACS, "a+", "aa+", 3, 0, 3);
    gnu_search("POSIX_BASIC", RE_SYNTAX_POSIX_BASIC, "\\(a*\\)\\1", "aaaa", 4, 0, 4);
    gnu_search("POSIX_EXTENDED", RE_SYNTAX_POSIX_EXTENDED, "(", "(", 1, 0, 1);
    gnu_search("POSIX_EXTENDED", RE_SYNTAX_POSIX_EXTENDED, "[a", "a", 1, 0, 1);
    gnu_search("POSIX_EXTENDED", RE_SYNTAX_POSIX_EXTENDED, "a{2,1}", "a", 1, 0, 1);
    gnu_search("POSIX_EXTENDED", RE_SYNTAX_POSIX_EXTENDED, "b", "abcabc", 6, 5, -5);
    gnu_search("POSIX_EXTENDED", RE_SYNTAX_POSIX_EXTENDED, "b", "abcabc", 6, 0, 6);
    gnu_search("POSIX_EXTENDED", RE_SYNTAX_POSIX_EXTENDED, "b", "abcabc", 6, 2, 1);
    gnu_search("POSIX_EXTENDED", RE_SYNTAX_POSIX_EXTENDED, "b", "abcabc", 6, 7, 1);
    gnu_search("POSIX_EXTENDED", RE_SYNTAX_POSIX_EXTENDED, "b", "abcabc", 6, -1, 1);
    gnu_search("POSIX_EXTENDED", RE_SYNTAX_POSIX_EXTENDED, "b", "abcabc", 6, 3, 100);
    gnu_search("POSIX_EXTENDED", RE_SYNTAX_POSIX_EXTENDED, "a.b", "a\0b", 3, 0, 3);
    gnu_search("POSIX_EXTENDED", RE_SYNTAX_POSIX_EXTENDED, "^b", "ab", 2, 1, 1);
    gnu_search("POSIX_EXTENDED", RE_SYNTAX_POSIX_EXTENDED, "\\`b", "ab", 2, 1, 1);
    gnu_search("POSIX_EXTENDED", RE_SYNTAX_POSIX_EXTENDED, "a\\'", "aa", 2, 0, 2);
    gnu_search("POSIX_EXTENDED", RE_SYNTAX_POSIX_EXTENDED, "a$", "aa", 2, 0, 2);
    gnu_search("POSIX_EXTENDED", RE_SYNTAX_POSIX_EXTENDED, "\\<b", "a b", 3, 0, 3);
    gnu_search("POSIX_EXTENDED", RE_SYNTAX_POSIX_EXTENDED, "b\\>", "ab b", 4, 0, 4);
    gnu_search("POSIX_EXTENDED", RE_SYNTAX_POSIX_EXTENDED, "\\bb", "ab b", 4, 0, 4);
    gnu_search("POSIX_EXTENDED", RE_SYNTAX_POSIX_EXTENDED, "\\w+", "--ab--", 6, 0, 6);
    gnu_search("POSIX_EXTENDED", RE_SYNTAX_POSIX_EXTENDED, "(a)|b", "b", 1, 0, 1);
    gnu_search("POSIX_EXTENDED", RE_SYNTAX_POSIX_EXTENDED, "x*", "ab", 2, 0, 2);
    gnu_search("POSIX_EXTENDED|NO_BK_REFS", RE_SYNTAX_POSIX_EXTENDED | RE_NO_BK_REFS, "(a)\\1", "aa", 2, 0, 2);
    gnu_search("POSIX_EXTENDED|ICASE", RE_SYNTAX_POSIX_EXTENDED | RE_ICASE, "ab", "xAB", 3, 0, 3);
    gnu_search("POSIX_EXTENDED|DOT_NEWLINE", RE_SYNTAX_POSIX_EXTENDED & ~RE_DOT_NEWLINE, "a.b", "a\nb", 3, 0, 3);
    gnu_search("GREP", RE_SYNTAX_GREP, "a\\|b", "b", 1, 0, 1);
    gnu_search("EGREP", RE_SYNTAX_EGREP, "a+|c", "xaac", 4, 0, 4);
    gnu_search("POSIX_MINIMAL_EXTENDED", RE_SYNTAX_POSIX_MINIMAL_EXTENDED, "a{2}", "aa", 2, 0, 2);
    /* re_search_2, re_match_2: split buffers */
    {
        struct re_pattern_buffer b;
        memset(&b, 0, sizeof b);
        re_syntax_options = RE_SYNTAX_POSIX_EXTENDED;
        const char *err = re_compile_pattern("a(b+)c", 6, &b);
        printf("compile a(b+)c: %s\n", err ? err : "ok");
        struct re_registers regs;
        memset(&regs, 0, sizeof regs);
        regoff_t r = re_search_2(&b, "xab", 3, "bcy", 3, 0, 6, &regs, 6);
        printf("re_search_2 [xab]+[bcy] = %d", (int)r);
        if (r >= 0)
            printf(" (%d,%d) (%d,%d)", (int)regs.start[0], (int)regs.end[0], (int)regs.start[1], (int)regs.end[1]);
        nl();
        r = re_search_2(&b, "xab", 3, "bcy", 3, 0, 6, &regs, 4);
        printf("re_search_2 stop=4 = %d\n", (int)r);
        r = re_match_2(&b, "xab", 3, "bcy", 3, 1, &regs, 6);
        printf("re_match_2 at 1 = %d\n", (int)r);
        r = re_match_2(&b, "", 0, "abbc", 4, 0, &regs, 4);
        printf("re_match_2 empty first = %d\n", (int)r);
        free(regs.start);
        free(regs.end);
        /* re_set_registers with caller arrays */
        regs.num_regs = 3;
        regs.start = malloc(3 * sizeof(regoff_t));
        regs.end = malloc(3 * sizeof(regoff_t));
        re_set_registers(&b, &regs, 3, regs.start, regs.end);
        r = re_search(&b, "zabbbc", 6, 0, 6, &regs);
        printf("re_set_registers then re_search = %d num_regs=%u (%d,%d) (%d,%d)\n", (int)r, (unsigned)regs.num_regs, (int)regs.start[0],
               (int)regs.end[0], (int)regs.start[1], (int)regs.end[1]);
        free(regs.start);
        free(regs.end);
        regfree(&b);
        /* fastmap */
        memset(&b, 0, sizeof b);
        b.fastmap = malloc(256);
        err = re_compile_pattern("x[yz]", 5, &b);
        int fr = re_compile_fastmap(&b);
        r = re_search(&b, "aaxz", 4, 0, 4, NULL);
        printf("re_compile_fastmap = %d, re_search with fastmap = %d\n", fr, (int)r);
        regfree(&b);
        printf("after regfree: fastmap=%s\n", b.fastmap ? "set" : "NULL");
    }
    /* BSD interface */
    {
        re_syntax_options = 0;
        IN_CHILD("re_exec before any re_comp", printf("re_exec(abc) before any re_comp = %d\n", re_exec("abc")));
        char *m = re_comp(NULL);
        fputs("re_comp(NULL) first = ", stdout);
        q(m);
        nl();
        m = re_comp("ab*c");
        fputs("re_comp(ab*c) = ", stdout);
        q(m);
        nl();
        printf("re_exec(xabbc)=%d re_exec(xac)=%d re_exec(xb)=%d\n", re_exec("xabbc"), re_exec("xac"), re_exec("xb"));
        m = re_comp(NULL);
        fputs("re_comp(NULL) after = ", stdout);
        q(m);
        nl();
        printf("re_exec(abc) after NULL = %d\n", re_exec("abc"));
        m = re_comp("");
        fputs("re_comp(\"\") = ", stdout);
        q(m);
        printf(" re_exec(abc) = %d", re_exec("abc"));
        nl();
        /* a failing re_comp, then re_exec, each in a child */
        IN_CHILD("re_comp(\\() then re_exec", {
            char *m2 = re_comp("\\(");
            fputs("re_comp(\\() = ", stdout);
            q(m2);
            nl();
            printf("re_exec(abc) after a failed re_comp = %d\n", re_exec("abc"));
        });
        IN_CHILD("re_comp(\\() then re_comp(NULL)", {
            re_comp("\\(");
            char *m2 = re_comp(NULL);
            fputs("re_comp(NULL) after a failed re_comp = ", stdout);
            q(m2);
            nl();
        });
        m = re_comp("a|b");
        fputs("re_comp(a|b) = ", stdout);
        q(m);
        printf(" re_exec(b)=%d re_exec(a|b)=%d", re_exec("b"), re_exec("a|b"));
        nl();
        m = re_comp("a\\|b");
        fputs("re_comp(a\\|b) = ", stdout);
        q(m);
        printf(" re_exec(b)=%d", re_exec("b"));
        nl();
    }
}

/* ------------------------------------------------------------------ wordexp */

static const char *wflag_names[] = {"DOOFFS", "APPEND", "NOCMD", "REUSE", "SHOWERR", "UNDEF"};

static void w_flags(int f)
{
    if (!f) {
        fputs("0", stdout);
        return;
    }
    const char *sep = "";
    for (int i = 0; i < 6; i++)
        if (f & (1 << i)) {
            printf("%s%s", sep, wflag_names[i]);
            sep = "|";
        }
    if (f & ~0x3f)
        printf("%s0x%x", sep, f & ~0x3f);
}

static char errpath[4200];
static int err_saved = -1;

static void stderr_capture_begin(void)
{
    fflush(stderr);
    int fd = open(errpath, O_WRONLY | O_CREAT | O_TRUNC, 0644);
    err_saved = dup(2);
    dup2(fd, 2);
    close(fd);
}

static char errbuf[512];

static void stderr_capture_end(void)
{
    fflush(stderr);
    dup2(err_saved, 2);
    close(err_saved);
    int fd = open(errpath, O_RDONLY);
    ssize_t n = fd >= 0 ? read(fd, errbuf, sizeof errbuf - 1) : 0;
    if (fd >= 0)
        close(fd);
    errbuf[n > 0 ? n : 0] = 0;
}

/* what the call wrote to descriptor 2, printed after its result line */
static void stderr_show(void)
{
    if (errbuf[0]) {
        fputs("  stderr ", stdout);
        q(errbuf);
        nl();
    }
}

static void we_show(const char *label, const char *words, int flags, int r, const wordexp_t *w)
{
    printf("%s ", label);
    q(words);
    putchar(' ');
    w_flags(flags);
    printf(" -> %d wordc=%zu offs=%zu", r, w->we_wordc, w->we_offs);
    if (!w->we_wordv)
        fputs(" wordv=NULL", stdout);
    else {
        size_t n = w->we_offs + w->we_wordc;
        if (n > 4096)
            n = 0;
        for (size_t i = 0; i <= n; i++) {
            putchar(' ');
            if (i == n && w->we_wordv[i] == NULL)
                putchar('$');
            else
                q(w->we_wordv[i]);
        }
    }
    nl();
}

static void we(const char *words, int flags)
{
    wordexp_t w;
    memset(&w, 0, sizeof w);
    fflush(stdout);
    stderr_capture_begin();
    int r = wordexp(words, &w, flags);
    stderr_capture_end();
    we_show("wordexp", words, flags, r, &w);
    stderr_show();
    if (r == 0)
        wordfree(&w);
}

/* An error case: the wordexp_t starts with sentinels (we_wordc 777, we_wordv NULL); then
 * wordfree is called in a child, whose status shows whether that was safe. */
static void we_err(const char *words, int flags, size_t offs)
{
    wordexp_t w;
    memset(&w, 0, sizeof w);
    w.we_wordc = 777;
    w.we_offs = offs;
    stderr_capture_begin();
    int r = wordexp(words, &w, flags);
    stderr_capture_end();
    we_show("wordexp-state", words, flags, r, &w);
    stderr_show();
    fflush(stdout);
    pid_t p = fork();
    if (p == 0) {
        tw_alarm(10);
        wordfree(&w);
        _exit(0);
    }
    int st = 0;
    waitpid(p, &st, 0);
    printf("  wordfree afterwards: %s\n", WIFEXITED(st) && WEXITSTATUS(st) == 0 ? "ok" : WIFSIGNALED(st) ? "killed" : "failed");
    if (r == 0 || w.we_wordv) {
        /* free it here only if the child's call was fine */
        if (WIFEXITED(st) && WEXITSTATUS(st) == 0)
            wordfree(&w);
    }
}

static void sec_wordexp(void)
{
    puts("== wordexp");
    printf("constants DOOFFS=%d APPEND=%d NOCMD=%d REUSE=%d SHOWERR=%d UNDEF=%d NOSPACE=%d BADCHAR=%d BADVAL=%d CMDSUB=%d SYNTAX=%d\n",
           WRDE_DOOFFS, WRDE_APPEND, WRDE_NOCMD, WRDE_REUSE, WRDE_SHOWERR, WRDE_UNDEF, WRDE_NOSPACE, WRDE_BADCHAR, WRDE_BADVAL,
           WRDE_CMDSUB, WRDE_SYNTAX);
    use_locale("C");
    snprintf(errpath, sizeof errpath, "%s/stderr.txt", g_dir);
    char wdir[4200];
    snprintf(wdir, sizeof wdir, "%s/words", g_dir);
    if (chdir(wdir) != 0)
        return;
    char home[4200];
    snprintf(home, sizeof home, "%s/tilde", g_dir);
    setenv("HOME", home, 1);
    setenv("PATH", "/usr/bin:/bin", 1);
    setenv("X", "a b", 1);
    setenv("Y", "", 1);
    setenv("S", "  lead  trail  ", 1);
    setenv("C", "x:y::z", 1);
    setenv("G", "w*", 1);
    setenv("N", "3", 1);
    setenv("P", "path/to/file.tar.gz", 1);
    setenv("Q", "'q' \"r\"", 1);
    setenv("M", "x\xc3\xa9y\xc3\xa9z", 1);
    setenv("D", " a : b :: c ", 1);
    unsetenv("U");
    unsetenv("IFS");
    puts("-- words, quoting");
    static const char *basic[] = {"", " ", "a", "a b", "  a   b  ", "a\tb", "'a b'", "\"a b\"", "a\\ b", "'a'\"b\"c", "\"\"", "''", "a ''",
                                  "\"a\\\"b\"", "'a\\b'", "\"a\\b\"", "\"a\\$b\"", "a\\\\b", "\\a", "\"'\"", "'\"'", "#a", "a #b", "a#b",
                                  "\\#a", "=", "a=b", "--", "$", "a$", "\"$\"", "$ a", "\\", "a\\", "\"a\\\nb\"", "a\\\nb", "\xc3\xa9"};
    for (size_t i = 0; i < sizeof basic / sizeof *basic; i++)
        we(basic[i], 0);
    puts("-- parameter expansion");
    static const char *params[] = {"$X", "\"$X\"", "${X}", "${X}y", "$Xy", "$Y", "\"$Y\"", "a$Y", "$U", "\"$U\"", "${U}", "${U:-d}",
                                   "${U-d}", "${Y:-d}", "${Y-d}", "${X:-d}", "${U:-a b}", "\"${U:-a b}\"", "${U:+x}", "${X:+x}",
                                   "${Y:+x}", "${Y+x}", "${#X}", "${#U}", "${#Y}", "${P%.*}", "${P%%.*}", "${P#*/}", "${P##*/}",
                                   "${P%x}", "${P#\"path\"}", "${X%% *}", "$S", "\"$S\"", "$G", "\"$G\"", "$Q", "$N$N", "${N}0",
                                   "$1", "${1}", "$#", "$@", "\"$@\"", "$*", "$?", "$-", "$!", "$0", "$$", "${10}", "$_", "${X:2}",
                                   "${X/a/z}", "${!X}", "${X^}", "${X,}", "${X:-$N}", "${U:-$X}", "\"${U:-$X}\"", "${U:-'a b'}",
                                   "${U:-\"x y\"}", "${U:-~}", "${U:=set}", "$U", "${Y:=set2}", "$Y", "${U2:?}", "${U2:?custom}",
                                   "${U2?msg}", "${Y3:?msg}", "${X:?msg}", "$X$U", "${X}$U", "${", "${}", "${X", "${X:}", "${ X}",
                                   "${X:-}", "${#}", "${#X:-}", "$ X", "$_X", "${_}", "${X#}", "${U:-a}b", "${U:-a)b}", "${U:-a|b}",
                                   "${U:-}}", "${U:-\\}}"};
    for (size_t i = 0; i < sizeof params / sizeof *params; i++)
        we(params[i], 0);
    printf("after ${U:=set}: getenv(U)=%s\n", getenv("U") ? getenv("U") : "(unset)");
    unsetenv("U");
    setenv("Y", "", 1);
    puts("-- empty and unset variables inside double quotes (Y is set and empty, U is unset)");
    static const char *empties[] = {"\"$Y\"", "\"${Y}\"", "a\"$Y\"", "\"$Y\"a", "\"$Y\" b", "b \"$Y\"", "\"$Y$Y\"", "\"$Y\"\"$Y\"",
                                    "\"$U\"", "\"${U}\"", "\"${Y:-}\"", "\"${U:-}\"", "\"${Y-}\"", "\"${X:+}\"", "''$Y", "\"\"$Y",
                                    "\"$Y\"''", "\"x$Y\"", "\"$Y \"", "\"${#Y}\"", "\"$(true)\"", "\"$(printf '')\"", "\"`true`\"",
                                    "\"${Y%x}\""};
    for (size_t i = 0; i < sizeof empties / sizeof *empties; i++)
        we(empties[i], 0);
    puts("-- arithmetic expansion");
    static const char *arith[] = {"$((1+2))", "$((N*2))", "$(($N*2))", "$((N))", "$((U))", "$((U+1))", "$((1/0))", "$((1%0))",
                                  "$((0x10))", "$((010))", "$((-3/2))", "$((-3%2))", "$((7/2))", "$((2-3-4))", "$((2*3+4))",
                                  "$((2*(3+4)))", "$(( 1 + 2 ))", "$((1<2))", "$((1<<4))", "$((~0))", "$((!0))", "$((1==1))",
                                  "$((1?2:3))", "$((1&&0))", "$((5&3))", "$((5|3))", "$((5^3))", "$((N+=1))", "$((N++))",
                                  "$((9223372036854775807+1))", "$((2**3))", "$[1+2]", "$[N*3]", "$((1+))", "$((1", "$((a))",
                                  "$((X))", "\"$((1+2))\"", "x$((1+2))y", "$(( (1) ))", "$((08))", "$((1,2))", "$((-N))",
                                  "$((+N))", "$((4294967296*4294967296))", "$((-9223372036854775807-1))", "$((10/3*3))", "$((0x))"};
    for (size_t i = 0; i < sizeof arith / sizeof *arith; i++)
        we(arith[i], 0);
    puts("-- tilde expansion");
    static const char *tildes[] = {"~", "~/", "~/x", "a~", "~a", "\"~\"", "'~'", "\\~", "~root", "~root/x", "~nosuchuser_zq",
                                   "~nosuchuser_zq/x", "x=~", "x=~/a", "a:~", "~:x", "${U:-~}", "~ ~", "~$X", "\"~/x\""};
    for (size_t i = 0; i < sizeof tildes / sizeof *tildes; i++)
        we(tildes[i], 0);
    setenv("HOME", "", 1);
    we("~", 0);
    we("~/x", 0);
    unsetenv("HOME");
    we("~", 0);
    we("~/x", 0);
    setenv("HOME", home, 1);
    puts("-- field splitting and IFS");
    static const char *split[] = {"$X", "$S", "$C", "\"$C\"", "a$C", "$C$X", "${C}b", "$Y$Y", "a$Y b", "x${Y}y", "$U a", "'a:b' $C", "$M", "$D", "\"$D\"", "x${D}y"};
    static const char *ifss[] = {NULL, " ", ":", " :", "", "\t\n", ":x", "\xc3\xa9"};
    for (size_t k = 0; k < sizeof ifss / sizeof *ifss; k++) {
        if (ifss[k])
            setenv("IFS", ifss[k], 1);
        else
            unsetenv("IFS");
        fputs("IFS=", stdout);
        q(ifss[k]);
        nl();
        for (size_t i = 0; i < sizeof split / sizeof *split; i++)
            we(split[i], 0);
        we("a:b c", 0);
        we("$(printf 'a:b c')", 0);
    }
    unsetenv("IFS");
    puts("-- pathname expansion (in the words directory)");
    static const char *globs[] = {"*", "w?", "w\\*", "'w*'", "\"w*\"", ".*", "w[12]", "nomatch*", "$G", "\"$G\"", "x$G", "w*/",
                                  "*1 *2", "[!w]*", "w\\?", "w[*]", "{w1,w2}", "~/*", "$(echo 'w*')", "\"$(echo 'w*')\""};
    for (size_t i = 0; i < sizeof globs / sizeof *globs; i++)
        we(globs[i], 0);
    puts("-- command substitution");
    static const char *cmds[] = {"$(echo hi)", "`echo hi`", "$(echo a b)", "\"$(echo a b)\"", "$(printf 'a\\n\\n')",
                                 "$(printf 'a\\nb\\n')", "\"$(printf 'a\\nb\\n')\"", "$(exit 3)", "x$(exit 3)y", "$(echo $X)",
                                 "$(echo \"$X\")", "$(echo $(echo nested))", "`echo \\`echo nested\\``", "$(echo ')')",
                                 "$(echo '(')", "$(case a in a) echo c;; esac)", "$((1+$(echo 2)))", "$(echo x >&2)",
                                 "$(echo $U)","$(echo 'a  b')", "\"$(echo 'a  b')\"", "$(printf '%s' '*')",
                                 "$(echo w*)", "$(echo ~)", "$(printf '\\001')", "$(echo a; echo b)", "$(", "$(echo",
                                 "`echo", "$(echo a))", "$()", "``", "$( )", "$(true)a", "$(echo a|tr a b)", "`echo a|tr a b`",
                                 "$(echo {})", "$(echo \\$X)", "\"`echo \\\"a\\\"`\"", "${U:-`echo x`}", "${U:-$(echo x)}",
                                 "\"${U:-`echo x`}\"", "\"${U:-$(echo x)}\""};
    for (size_t i = 0; i < sizeof cmds / sizeof *cmds; i++)
        we(cmds[i], 0);
    puts("-- WRDE_SHOWERR");
    we("$(echo err >&2; echo out)", 0);
    we("$(echo err >&2; echo out)", WRDE_SHOWERR);
    we("$(nosuchcommand_zq)", 0);
    we("$(nosuchcommand_zq 2>&1)", 0);
    we("${U2:?msg}", 0);
    we("${U2:?msg}", WRDE_SHOWERR);
    we("$((1/0))", WRDE_SHOWERR);
    puts("-- WRDE_NOCMD");
    static const char *nocmd[] = {"$(echo x)", "`echo x`", "\"$(echo x)\"", "\"`echo x`\"", "$(echo $(echo x))", "`echo \\`x\\``",
                                  "$(($(echo 1)+1))", "$((`echo 1`+1))", "\"$(($(echo 1)))\"", "'$(echo x)'", "\\$(echo x)",
                                  "\\`echo x\\`", "${U:-$(echo x)}", "${X:+$(echo x)}", "${X:-$(echo x)}", "${U:-`echo x`}",
                                  "\"${U:-$(echo x)}\"", "a $(echo x) b", "$(", "$((1+2))", "$X", "~", "w*", "$(echo x",
                                  "\"$\"(echo x)", "$\\(echo x)", "${#X}$(echo x)", "x|$(echo x)", "$(echo x)|", "'`'", "\"\\`\"",
                                  "\\\\$(echo x)", "$((1+2))$(echo x)", "$()", "``", "${P#$(echo x)}"};
    for (size_t i = 0; i < sizeof nocmd / sizeof *nocmd; i++)
        we(nocmd[i], WRDE_NOCMD);
    puts("-- WRDE_BADCHAR (each character unquoted, quoted, escaped, in braces and in $(...))");
    static const char *bad[] = {"|", "&", ";", "<", ">", "(", ")", "{", "}", "\n"};
    for (size_t i = 0; i < sizeof bad / sizeof *bad; i++) {
        char b[64];
        snprintf(b, sizeof b, "a%sb", bad[i]);
        we(b, 0);
        snprintf(b, sizeof b, "%s", bad[i]);
        we(b, 0);
        snprintf(b, sizeof b, "'a%sb'", bad[i]);
        we(b, 0);
        snprintf(b, sizeof b, "\"a%sb\"", bad[i]);
        we(b, 0);
        snprintf(b, sizeof b, "a\\%sb", bad[i]);
        we(b, 0);
        snprintf(b, sizeof b, "${U:-a%sb}", bad[i]);
        we(b, 0);
        snprintf(b, sizeof b, "$(echo 'a%sb')", bad[i]);
        we(b, 0);
        snprintf(b, sizeof b, "a%sb", bad[i]);
        we(b, WRDE_NOCMD);
    }
    we("a|$(echo x)", 0);
    we("$(echo x)|", 0);
    we("a b|", 0);
    we("a\nb", WRDE_NOCMD);
    puts("-- WRDE_SYNTAX");
    static const char *syn[] = {"'", "\"", "`", "'a", "\"a", "a'", "a\"", "$(", "$((", "${", "${X", "$((1+", "\"$(\"", "\"${X\"",
                                "a\\", "\"\\", "$(echo 'x)", "$((1+2)", "${X:-", "`echo", "\"`echo\"", "$(()", "${X:-'}",
                                "${X#'}", "${U:-a{b", "${U:-{}}", "${U:-{}", "${U:-'}", "${U:-\"}"};
    for (size_t i = 0; i < sizeof syn / sizeof *syn; i++)
        we(syn[i], 0);
    puts("-- WRDE_UNDEF, WRDE_BADVAL");
    static const char *undef[] = {"$U", "${U}", "\"$U\"", "\"${U}\"", "'$U'", "\\$U", "${U:-x}", "${U-x}", "${U:+x}", "${#U}",
                                  "${U%x}", "$((U))", "$((U+1))", "$1", "$#", "$@", "$*", "$X", "$Y", "${Y:?}", "$(echo $U)",
                                  "`echo $U`", "${U:=d}", "~", "$?", "$$", "$!", "$-", "$0"};
    for (size_t i = 0; i < sizeof undef / sizeof *undef; i++) {
        we(undef[i], WRDE_UNDEF);
        unsetenv("U");
    }
    we("$U $(echo x)", WRDE_UNDEF | WRDE_NOCMD);
    we("$(echo x) $U", WRDE_UNDEF | WRDE_NOCMD);
    we("$U |", WRDE_UNDEF);
    we("| $U", WRDE_UNDEF);
    we("$U '", WRDE_UNDEF);
    puts("-- state left in the wordexp_t on errors (we_wordc starts at 777, we_wordv at NULL)");
    we_err("a b \"$Y\"", 0, 0);
    we_err("a b \"$Y\" c", WRDE_DOOFFS, 2);
    we_err("\"$Y\"", WRDE_DOOFFS, 1);
    we_err("a b |", 0, 0);
    we_err("a b $(", 0, 0);
    we_err("a b $U", WRDE_UNDEF, 0);
    we_err("a b $(echo x)", WRDE_NOCMD, 0);
    we_err("a b |", WRDE_DOOFFS, 2);
    we_err("a b $U", WRDE_DOOFFS | WRDE_UNDEF, 2);
    we_err("a b", WRDE_DOOFFS, 2);
    we_err("|", WRDE_DOOFFS, 3);
    {
        wordexp_t w;
        memset(&w, 0, sizeof w);
        int r = wordexp("one two", &w, 0);
        we_show("append-err#1", "one two", 0, r, &w);
        static const struct {
            const char *s;
            int f;
        } after[] = {{"three |", WRDE_APPEND}, {"three $U", WRDE_APPEND | WRDE_UNDEF}, {"three $(echo x)", WRDE_APPEND | WRDE_NOCMD},
                     {"three '", WRDE_APPEND}, {"three \"$Y\"", WRDE_APPEND}, {"three", WRDE_APPEND}};
        for (size_t i = 0; i < sizeof after / sizeof *after; i++) {
            r = wordexp(after[i].s, &w, after[i].f);
            char lab[32];
            snprintf(lab, sizeof lab, "append-err#%zu", i + 2);
            we_show(lab, after[i].s, after[i].f, r, &w);
        }
        fflush(stdout);
        pid_t p = fork();
        if (p == 0) {
            tw_alarm(10);
            wordfree(&w);
            _exit(0);
        }
        int st;
        waitpid(p, &st, 0);
        printf("  wordfree afterwards: %s\n", WIFEXITED(st) && WEXITSTATUS(st) == 0 ? "ok" : "failed");
        if (WIFEXITED(st) && WEXITSTATUS(st) == 0)
            wordfree(&w);
    }
    {
        wordexp_t w;
        memset(&w, 0, sizeof w);
        w.we_offs = 2;
        int r = wordexp("one two", &w, WRDE_DOOFFS);
        we_show("doofs-append#1", "one two", WRDE_DOOFFS, r, &w);
        r = wordexp("three", &w, WRDE_DOOFFS | WRDE_APPEND);
        we_show("doofs-append#2", "three", WRDE_DOOFFS | WRDE_APPEND, r, &w);
        r = wordexp("four |", &w, WRDE_DOOFFS | WRDE_APPEND);
        we_show("doofs-append#3", "four |", WRDE_DOOFFS | WRDE_APPEND, r, &w);
        r = wordexp("five", &w, WRDE_APPEND);
        we_show("doofs-append#4", "five", WRDE_APPEND, r, &w);
        fflush(stdout);
        pid_t p = fork();
        if (p == 0) {
            tw_alarm(10);
            wordfree(&w);
            _exit(0);
        }
        int st;
        waitpid(p, &st, 0);
        printf("  wordfree afterwards: %s\n", WIFEXITED(st) && WEXITSTATUS(st) == 0 ? "ok" : "failed");
        if (WIFEXITED(st) && WEXITSTATUS(st) == 0)
            wordfree(&w);
    }
    puts("-- WRDE_REUSE");
    {
        wordexp_t w;
        memset(&w, 0, sizeof w);
        int r = wordexp("a b c", &w, 0);
        we_show("reuse#1", "a b c", 0, r, &w);
        r = wordexp("d", &w, WRDE_REUSE);
        we_show("reuse#2", "d", WRDE_REUSE, r, &w);
        r = wordexp("e f", &w, WRDE_REUSE | WRDE_APPEND);
        we_show("reuse#3", "e f", WRDE_REUSE | WRDE_APPEND, r, &w);
        r = wordexp("g |", &w, WRDE_REUSE);
        we_show("reuse#4", "g |", WRDE_REUSE, r, &w);
        fflush(stdout);
        pid_t p = fork();
        if (p == 0) {
            tw_alarm(10);
            wordfree(&w);
            _exit(0);
        }
        int st;
        waitpid(p, &st, 0);
        printf("  wordfree afterwards: %s\n", WIFEXITED(st) && WEXITSTATUS(st) == 0 ? "ok" : "failed");
        memset(&w, 0, sizeof w);
        w.we_offs = 1;
        r = wordexp("x y", &w, WRDE_DOOFFS);
        we_show("reuse#5", "x y", WRDE_DOOFFS, r, &w);
        r = wordexp("z", &w, WRDE_REUSE | WRDE_DOOFFS);
        we_show("reuse#6", "z", WRDE_REUSE | WRDE_DOOFFS, r, &w);
        wordfree(&w);
        printf("after wordfree: wordv=%s wordc=%zu\n", w.we_wordv ? "set" : "NULL", w.we_wordc);
        wordfree(&w);
        puts("wordfree twice: ok");
        memset(&w, 0, sizeof w);
        wordfree(&w);
        puts("wordfree on a zeroed wordexp_t: ok");
    }
    puts("-- unknown flags");
    we("a", 1 << 10);
}

/* ------------------------------------------------------------------ driver */

struct section {
    const char *name;
    void (*fn)(void);
    int timeout;
};

static const struct section sections[] = {
    {"fnmatch", sec_fnmatch, 120}, {"glob", sec_glob, 60}, {"regex", sec_regex, 120}, {"regex-corpus", sec_regex_corpus, 600},
    {"gnu", sec_gnu, 60}, {"wordexp", sec_wordexp, 120},
#ifdef PATTERNS_COMPAT_GLOB
    {"compat", sec_compat, 60},
#endif
};

int main(int argc, char **argv)
{
    if (argc < 2 || argv[1][0] != '/') {
        fprintf(stderr, "usage: %s /SCRATCH [SECTION]\n", argv[0]);
        return 2;
    }
    g_argv0 = argv[0];
    snprintf(g_dir, sizeof g_dir, "%s", argv[1]);
    g_dirlen = strlen(g_dir);
    while (g_dirlen > 1 && g_dir[g_dirlen - 1] == '/')
        g_dir[--g_dirlen] = 0;
    struct passwd *pw = getpwuid(getuid());
    if (pw && pw->pw_dir)
        snprintf(g_home, sizeof g_home, "%s", pw->pw_dir);
    snprintf(g_pid, sizeof g_pid, "%ld", (long)getpid());
    setvbuf(stdout, NULL, _IOFBF, 1 << 16);
    const char *only = argc > 2 ? argv[2] : NULL;
    fixture();
    for (size_t i = 0; i < sizeof sections / sizeof *sections; i++) {
        if (only ? strcmp(only, sections[i].name) : !strcmp(sections[i].name, "compat"))
            continue;
        fflush(stdout);
        fflush(stderr);
        pid_t p = fork();
        if (p == 0) {
            prctl(PR_SET_PDEATHSIG, SIGKILL);
            prctl(PR_SET_DUMPABLE, 0); /* a crash writes no core file */
            snprintf(g_pid, sizeof g_pid, "%ld", (long)getpid());
            tw_alarm((unsigned)sections[i].timeout);
            sections[i].fn();
            fflush(stdout);
            _exit(0);
        }
        int st = 0;
        while (waitpid(p, &st, 0) < 0 && errno == EINTR) {
        }
        if (!(WIFEXITED(st) && WEXITSTATUS(st) == 0))
            printf("section %s: %s %d\n", sections[i].name, WIFSIGNALED(st) ? "killed by signal" : "exit", WIFSIGNALED(st) ? WTERMSIG(st) : WEXITSTATUS(st));
    }
    /* leave the fixture removable */
    char p1[4200];
    snprintf(p1, sizeof p1, "%s/tree/noread", g_dir);
    chmod(p1, 0755);
    snprintf(p1, sizeof p1, "%s/tree/noexec", g_dir);
    chmod(p1, 0755);
    fflush(stdout);
    return 0;
}
