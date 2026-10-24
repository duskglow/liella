/* Optimization stage (plan rule P10): the baseline rows for every P10 target, with inputs typical
 * of coreutils, bash and sed (file names, ls- and date-style formats, sed-style regular
 * expressions over log and configuration lines, shell words) as well as microbenchmarks, and the
 * cold-start rows. The same file builds against the products (`cargo xtask bench`) and against
 * the host C library; `cargo xtask bench --compare cbench_opt` builds both (the host library
 * dynamically and statically) and reports medians over several runs with the same-run ratio.
 * Prints `name<TAB>ns/op` lines, formatted by hand so that the measured functions do not format
 * their own results.
 *
 * Steady-state rows are the best of seven rounds, in ns per call (per line, per name or per
 * conversion where the row says so). Cold rows run this program again through execve, 31 times:
 * `cold_exec_empty` and `cold_exec_once` are the median wall time from just before execve to the
 * parent's return from waitpid, for a program that returns at once and for one that calls every
 * target once; `first_*` and `second_*` are the median cost of the first and second call of each
 * target inside the fresh process (in the order listed in `cold`). The fresh process reads its
 * own function-pointer table and inputs first (`touch_own_tables`), so a first_* row is the
 * library's first-call cost (its text and data page faults) plus this binary's own text layout,
 * not a fault on one of this program's data pages.
 *
 * Scratch objects: a directory under /tmp (mkdtemp) holding a fixture `.mo` (spec Appendix
 * D.2.1, as cbench_services) at `en_US/LC_MESSAGES/cbench.mo`, removed at exit, also after a
 * failed check. The locale rows need `en_US.UTF-8`; the zone rows America/New_York. Every call
 * is checked once before it is timed; a failure ends the program. */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <fnmatch.h>
#include <iconv.h>
#include <libintl.h>
#include <locale.h>
#include <pthread.h>
#include <regex.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/wait.h>
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
    char buf[80];
    int n = 0;
    while (name[n] && n < 48) {
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

/* CBENCH_ONLY=<prefix>[,<prefix>...] in the environment limits the rows run (for iterating on one
 * target); the cold rows run when a prefix matches `cold`. Unset: every row. */
static int wanted(const char *name)
{
    const char *only = getenv("CBENCH_ONLY");
    if (!only || !*only)
        return 1;
    while (*only) {
        size_t n = strcspn(only, ",");
        if (n && strncmp(name, only, n) == 0)
            return 1;
        only += n;
        if (*only == ',')
            only++;
    }
    return 0;
}

#include "cbench_rows.h"

#define MEASURE(name, iters, expr)                         \
    do {                                                   \
        if (cb_skip(name))                                 \
            break;                                         \
        if (!wanted(name))                                 \
            break;                                         \
        double best = 1e300;                               \
        ctr_begin();                                       \
        for (int round = 0; round < 7; round++) {          \
            double t0 = now();                             \
            long acc = 0;                                  \
            for (long i = 0; i < (iters); i++)             \
                acc += (long)(expr);                       \
            sink = acc;                                    \
            double per = (now() - t0) / (double)(iters);   \
            if (per < best)                                \
                best = per;                                \
        }                                                  \
        ctr_end(name, 7.0 * (double)(iters));              \
        put(name, best);                                   \
    } while (0)

static char *missing_argv[] = {"cbench-no-such-command", NULL};

/* 1 when execvp searched PATH and found nothing (ENOENT). */
static long exec_missing(void)
{
    return execvp(missing_argv[0], missing_argv) == -1 && errno == ENOENT ? 1 : -1;
}

static void fail(const char *what)
{
    (void)!write(2, "cbench_opt: unexpected result: ", 31);
    (void)!write(2, what, strlen(what));
    (void)!write(2, "\n", 1);
    exit(1);
}

/* Through volatile pointers, so that each call is made (no builtins, no inlining). */
static int (*volatile snprintf_fn)(char *, size_t, const char *, ...) = snprintf;
static int (*volatile fprintf_fn)(FILE *, const char *, ...) = fprintf;
int __sprintf_chk(char *, int, size_t, const char *, ...);
static int (*volatile sprintf_chk_fn)(char *, int, size_t, const char *, ...) = __sprintf_chk;
static int (*volatile strcmp_fn)(const char *, const char *) = strcmp;
static int (*volatile strcoll_fn)(const char *, const char *) = strcoll;
static size_t (*volatile strftime_fn)(char *, size_t, const char *, const struct tm *) = strftime;
static time_t (*volatile mktime_fn)(struct tm *) = mktime;
static struct tm *(*volatile localtime_r_fn)(const time_t *, struct tm *) = localtime_r;
static int (*volatile regexec_fn)(const regex_t *, const char *, size_t, regmatch_t *, int) = regexec;
static int (*volatile fnmatch_fn)(const char *, const char *, int) = fnmatch;
static regoff_t (*volatile re_search_fn)(struct re_pattern_buffer *, const char *, regoff_t, regoff_t, regoff_t, struct re_registers *) = re_search;
static char *(*volatile gettext_fn)(const char *) = gettext;
static char *(*volatile dgettext_fn)(const char *, const char *) = dgettext;
static char *(*volatile ngettext_fn)(const char *, const char *, unsigned long) = ngettext;

/* ---- inputs */

/* File names of the kind ls, sort and the shell meet: build trees, home directories, system
 * directories; a few UTF-8 names. */
static const char *const names[] = {
    "Makefile", "Makefile.am", "Makefile.in", "README", "README.md", "COPYING", "ChangeLog",
    "NEWS", "INSTALL", "AUTHORS", "configure", "configure.ac", "config.h", "config.h.in",
    "config.log", "config.status", "aclocal.m4", "libtool", "main.c", "main.o", "util.c",
    "util.h", "parse.y", "lexer.l", "test-suite.log", "test_main.c", "Cargo.toml", "Cargo.lock",
    "build.rs", "lib.rs", "mod.rs", ".gitignore", ".git", ".bashrc", ".profile", "a.out", "core",
    "libc.so.6", "libm.so.6", "libfoo.so.1.2.3", "libfoo.a", "ls", "cp", "mv", "sort", "sed",
    "bash", "grep", "x86_64-linux-gnu", "python3.12", "perl5", "index.html", "style.css",
    "app.js", "package.json", "node_modules", "Dockerfile", "docker-compose.yml", "notes.txt",
    "TODO.txt", "report-2026-10-04.pdf", "IMG_20260101_120000.jpg", "photo.JPG",
    "r\xc3\xa9sum\xc3\xa9.pdf", "Gr\xc3\xbc\xc3\x9f" "e.txt", "na\xc3\xafve.c", "\xc3\x9c" "bersicht.ods",
    "zeta.h", "alpha.h", "Alpha.h", "_build", "__init__.py", "setup.py", "requirements.txt",
    "data.csv", "data_2.csv", "data10.csv", "data9.csv", "v1.2.10", "v1.2.9", "file~",
    "#autosave#", "tags", "TAGS", "ctags", "x", "xy", "xyz",
    "very-long-file-name-with-many-parts-and-dashes.tar.gz",
    "another_long_file_name_with_underscores.tar.xz", "mixed.Case.Name.TXT", "UPPER.TXT",
    "lower.txt", "src", "include", "tests", "docs",
};
#define NAMES ((long)(sizeof names / sizeof names[0]))
static char paths[NAMES][96];

/* 256 lines of logs, configuration and code, 40-100 bytes each. */
#define LINES 256
static char lines[LINES][128];

static char text_ascii[4096 + 1], text_latin[4096 + 1], text_mixed[4096 + 1];
static char utf16_mixed[16384];
static size_t utf16_mixed_len;
static char long_a[260], long_b[260];
static char eq_a[] = "abcdefghijklmno", eq_b[] = "abcdefghijklmno";

static void make_inputs(void)
{
    for (long i = 0; i < NAMES; i++)
        snprintf(paths[i], sizeof paths[i], "%s/%s", i % 3 == 0 ? "src/lib" : i % 3 == 1 ? "doc" : "src", names[i]);
    for (int i = 0; i < LINES; i++) {
        switch (i % 4) {
        case 0:
            snprintf(lines[i], sizeof lines[i], "Oct  4 12:%02d:%02d host sshd[%d]: Accepted publickey for user%d from 10.0.%d.%d port %d", i % 60,
                     (i * 7) % 60, 1000 + i, i % 9, i % 250, (i * 3) % 250, 40000 + i);
            break;
        case 1:
            snprintf(lines[i], sizeof lines[i], "option_%d = value_%d", i, i * 13);
            break;
        case 2:
            snprintf(lines[i], sizeof lines[i], "    if (count_%d > %d.%d) return lookup(table_%d, key);", i, i, i % 10, i % 7);
            break;
        default:
            snprintf(lines[i], sizeof lines[i], "  # comment line %d: nothing to see here", i);
            break;
        }
    }
    /* 4 KiB of prose: ASCII; Latin-1 letters (2-byte UTF-8, encodable in ISO-8859-15); mixed
     * (Latin, Greek, CJK and a 4-byte emoji). */
    static const char *const words_ascii = "the quick brown fox jumps over the lazy dog, ";
    static const char *const words_latin = "Gr\xc3\xbc\xc3\x9f" "e aus K\xc3\xb6ln, caf\xc3\xa9 cr\xc3\xa8me br\xc3\xbbl\xc3\xa9" "e ";
    static const char *const words_mixed = "abc \xce\xb1\xce\xb2\xce\xb3 \xe6\x97\xa5\xe6\x9c\xac \xf0\x9f\x98\x80 caf\xc3\xa9 ";
    const char *src[3] = {words_ascii, words_latin, words_mixed};
    char *dst[3] = {text_ascii, text_latin, text_mixed};
    for (int k = 0; k < 3; k++) {
        size_t n = 0, w = strlen(src[k]);
        while (n + w <= 4096) {
            memcpy(dst[k] + n, src[k], w);
            n += w;
        }
        while (n < 4096)
            dst[k][n++] = ' ';
        dst[k][n] = 0;
    }
    memset(long_a, 'p', 256);
    memcpy(long_b, long_a, 256);
    long_a[255] = 'a';
    long_b[255] = 'b';
}

/* ---- printf */

static long ls_line(char *buf, size_t n, long i)
{
    return snprintf_fn(buf, n, "%s %2lu %-8s %-8s %8lld %s %s", "-rw-r--r--", (unsigned long)(1 + (i & 3)), "user", "staff",
                       (long long)(i * 4099), "Oct  4 12:00", names[i % NAMES]);
}

/* ---- strcmp, strcoll: neighbouring names, as a sort compares them */

static long cmp_names(int (*f)(const char *, const char *), long i)
{
    return f(names[i % NAMES], names[(i * 7 + 3) % NAMES]) > 0;
}

/* ---- iconv */

static iconv_t cd_to16, cd_from16, cd_to32, cd_to8859, cd_from8859;
static char outbuf[32768];
static char text_8859[4096];
static size_t text_8859_len;

static long convert(iconv_t cd, const char *in, size_t n)
{
    char *ip = (char *)in, *op = outbuf;
    size_t il = n, ol = sizeof outbuf;
    iconv(cd, NULL, NULL, NULL, NULL);
    if (iconv(cd, &ip, &il, &op, &ol) == (size_t)-1 || il != 0)
        return -1;
    return (long)(op - outbuf);
}

static long open_convert_close(const char *name)
{
    iconv_t cd = iconv_open("UTF-16LE", "UTF-8");
    if (cd == (iconv_t)-1)
        return -1;
    long n = convert(cd, name, strlen(name));
    iconv_close(cd);
    return n;
}

/* ---- time */

static struct tm tm_at(long i)
{
    struct tm t;
    memset(&t, 0, sizeof t);
    t.tm_year = 126;
    t.tm_mon = (int)(i % 12);
    t.tm_mday = 1 + (int)(i % 28);
    t.tm_hour = (int)(i % 24);
    t.tm_min = 30;
    t.tm_isdst = -1;
    return t;
}

static long mk(long i)
{
    struct tm t = tm_at(i);
    return (long)mktime_fn(&t);
}

static long lt(time_t t)
{
    struct tm r;
    localtime_r_fn(&t, &r);
    return r.tm_hour;
}

/* ---- regex */

static long exec_lines(const regex_t *re, long i, size_t nmatch)
{
    regmatch_t m[4];
    return regexec_fn(re, lines[i & (LINES - 1)], nmatch, nmatch ? m : NULL, 0) == 0;
}

static long comp_free(const char *pat, int flags)
{
    regex_t re;
    if (regcomp(&re, pat, flags) != 0)
        return -1;
    regfree(&re);
    return 1;
}

/* GNU re_search as sed runs it (RE-SEARCH): re_compile_pattern with sed's syntax, a fastmap,
   REGS_REALLOCATE registers kept across calls, one search per line over the whole line. The lines
   are sed workload lines (`word 123456 /usr/lib/word/N.so`): every line matches both patterns. */
static char sed_lines[LINES][48];
static int sed_len[LINES];
static struct re_registers sed_regs;

static void sed_compile(struct re_pattern_buffer *b, const char *pat, reg_syntax_t syn)
{
    memset(b, 0, sizeof *b);
    b->fastmap = malloc(256);
    re_set_syntax(syn);
    if (b->fastmap == NULL || re_compile_pattern(pat, strlen(pat), b) != NULL)
        fail(pat);
    b->regs_allocated = REGS_REALLOCATE;
}

static long sed_search(struct re_pattern_buffer *b, long i)
{
    long k = i & (LINES - 1);
    return re_search_fn(b, sed_lines[k], sed_len[k], 0, sed_len[k], &sed_regs) >= 0;
}

static long sed_exec(const regex_t *re, long i, size_t nmatch)
{
    regmatch_t m[4];
    return regexec_fn(re, sed_lines[i & (LINES - 1)], nmatch, m, 0) == 0;
}

/* Several threads searching at once (owner 2026-10-07, regex DFA series): RE_THREADS threads each
   run RE_THREAD_EXECS searches over the text, with one regex_t shared by all or one each, so that
   a cache in the matcher shows its contention. One measured operation is one such batch. */
#define RE_THREADS 4
#define RE_THREAD_EXECS 1024
struct re_job {
    const regex_t *re;
    long hits;
};

static void *re_worker(void *arg)
{
    struct re_job *j = arg;
    long hits = 0;
    for (long i = 0; i < RE_THREAD_EXECS; i++)
        hits += regexec_fn(j->re, lines[i & (LINES - 1)], 0, NULL, 0) == 0;
    j->hits = hits;
    return NULL;
}

static long re_threads(const regex_t *res, int shared)
{
    pthread_t t[RE_THREADS];
    struct re_job jobs[RE_THREADS];
    for (int k = 0; k < RE_THREADS; k++) {
        jobs[k].re = &res[shared ? 0 : k];
        jobs[k].hits = 0;
        if (pthread_create(&t[k], NULL, re_worker, &jobs[k]) != 0)
            fail("pthread_create");
    }
    long hits = 0;
    for (int k = 0; k < RE_THREADS; k++) {
        pthread_join(t[k], NULL);
        hits += jobs[k].hits;
    }
    return hits;
}

static long scan_count(const regex_t *re, size_t nmatch)
{
    long n = 0;
    for (long i = 0; i < LINES; i++)
        n += exec_lines(re, i, nmatch);
    return n;
}

/* ---- wordexp */

static long words(const char *text, int flags, size_t want)
{
    wordexp_t we;
    if (wordexp(text, &we, flags) != 0)
        return -1;
    long n = (long)we.we_wordc;
    wordfree(&we);
    return want && (size_t)n != want ? -2 : n;
}

/* ---- gettext fixture (as cbench_services) */

static char top[] = "/tmp/cbench_opt.XXXXXX";
static int have_top;
static char path_l1[96], path_l2[96], path_mo[128];

static void cleanup(void)
{
    if (have_top) {
        unlink(path_mo);
        rmdir(path_l2);
        rmdir(path_l1);
        rmdir(top);
    }
}

static void put32(unsigned char *p, uint32_t v)
{
    p[0] = (unsigned char)v;
    p[1] = (unsigned char)(v >> 8);
    p[2] = (unsigned char)(v >> 16);
    p[3] = (unsigned char)(v >> 24);
}

static void write_mo(void)
{
    static const char header[] = "Content-Type: text/plain; charset=UTF-8\nPlural-Forms: nplurals=2; plural=n != 1;\n";
    struct { const char *s; uint32_t len; } orig[3] = {
        {"", 0},
        {"apple\0apples", 12},
        {"hello", 5},
    }, trans[3] = {
        {header, sizeof header - 1},
        {"Apfel\0\xc3\x84pfel", 12},
        {"hallo", 5},
    };
    unsigned char buf[512];
    memset(buf, 0, sizeof buf);
    uint32_t n = 3, o = 28, t = o + 8 * n, at = t + 8 * n;
    put32(buf, 0x950412de);
    put32(buf + 8, n);
    put32(buf + 12, o);
    put32(buf + 16, t);
    put32(buf + 24, at);
    for (uint32_t i = 0; i < n; i++) {
        put32(buf + o + 8 * i, orig[i].len);
        put32(buf + o + 8 * i + 4, at);
        memcpy(buf + at, orig[i].s, orig[i].len);
        at += orig[i].len + 1;
    }
    for (uint32_t i = 0; i < n; i++) {
        put32(buf + t + 8 * i, trans[i].len);
        put32(buf + t + 8 * i + 4, at);
        memcpy(buf + at, trans[i].s, trans[i].len);
        at += trans[i].len + 1;
    }
    int f = open(path_mo, O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC, 0600);
    if (f < 0 || write(f, buf, at) != (ssize_t)at)
        fail(path_mo);
    close(f);
}

static void make_fixture(void)
{
    if (atexit(cleanup) != 0)
        fail("atexit");
    if (!mkdtemp(top))
        fail("mkdtemp");
    have_top = 1;
    snprintf(path_l1, sizeof path_l1, "%s/en_US", top);
    snprintf(path_l2, sizeof path_l2, "%s/en_US/LC_MESSAGES", top);
    snprintf(path_mo, sizeof path_mo, "%s/en_US/LC_MESSAGES/cbench.mo", top);
    if (mkdir(path_l1, 0700) != 0 || mkdir(path_l2, 0700) != 0)
        fail("mkdir");
    write_mo();
}

/* ---- cold start: this program again, in a fresh process */

#define COLD_ROWS 13
static const char *const cold_names[COLD_ROWS] = {
    "snprintf", "strcmp", "setlocale_en_US", "strcoll", "iconv_open_conv_close", "localtime_r_new_york", "strftime_c",
    "mktime_new_york", "regcomp_regexec_regfree", "fnmatch", "wordexp", "bindtextdomain_gettext", "gettext_nocatalog",
};

/* Read (never call) this program's own function-pointer table and the inputs the cold rows pass
 * before the timed section. Otherwise a first_* row can include a page fault on a .data or
 * .rodata page of THIS binary that nothing had touched yet (HOT-ORDER/RC2-FAULT: first_strcmp
 * 17-27x was the read of strcmp_fn, a .data layout artifact). The rows then measure the library's
 * first-call cost on both sides; the binary's own text (main, the helpers) still faults where it
 * lies, so read the cold rows as "library cost + this binary's text layout". */
static void touch_own_tables(void)
{
    static const int used[] = {3, 4, 18, 63, 69, 70};
    uintptr_t sum = (uintptr_t)snprintf_fn + (uintptr_t)fprintf_fn + (uintptr_t)sprintf_chk_fn + (uintptr_t)strcmp_fn +
                    (uintptr_t)strcoll_fn + (uintptr_t)strftime_fn + (uintptr_t)mktime_fn + (uintptr_t)localtime_r_fn +
                    (uintptr_t)regexec_fn + (uintptr_t)fnmatch_fn + (uintptr_t)re_search_fn + (uintptr_t)gettext_fn +
                    (uintptr_t)dgettext_fn + (uintptr_t)ngettext_fn;
    for (size_t i = 0; i < sizeof used / sizeof used[0]; i++)
        sum += (uintptr_t)names[used[i]] + (unsigned char)names[used[i]][0];
    volatile uintptr_t sink = sum;
    (void)sink;
}

/* In the fresh process: every target twice, in order; prints `first second` per row. */
static int cold(const char *dir)
{
    double t[COLD_ROWS][2];
    char buf[128];
    long ok = 0;
    touch_own_tables();
    for (int k = 0; k < 2; k++) {
        double a = now();
        ok += snprintf(buf, sizeof buf, "%s=%d (%.2f)", "key", 42, 3.25) == 13;
        double b = now();
        t[0][k] = b - a;
        a = now();
        ok += strcmp_fn(names[3], names[4]) < 0;
        b = now();
        t[1][k] = b - a;
        a = now();
        ok += setlocale(LC_ALL, "en_US.UTF-8") != NULL;
        b = now();
        t[2][k] = b - a;
        a = now();
        ok += strcoll_fn(names[69], names[70]) != 0;
        b = now();
        t[3][k] = b - a;
        a = now();
        ok += open_convert_close(names[63]) > 0;
        b = now();
        t[4][k] = b - a;
        a = now();
        ok += lt(1759579200) >= 0;
        b = now();
        t[5][k] = b - a;
        struct tm r;
        time_t when = 1759579200;
        localtime_r(&when, &r);
        a = now();
        ok += strftime_fn(buf, sizeof buf, "%c", &r) > 0;
        b = now();
        t[6][k] = b - a;
        a = now();
        ok += mk(5) > 0;
        b = now();
        t[7][k] = b - a;
        a = now();
        {
            regex_t re;
            regmatch_t m[3];
            if (regcomp(&re, "^\\([a-z0-9_]*\\) = \\(.*\\)$", 0) == 0) {
                ok += regexec(&re, "option_1 = value_13", 3, m, 0) == 0;
                regfree(&re);
            }
        }
        b = now();
        t[8][k] = b - a;
        a = now();
        ok += fnmatch_fn("*.c", names[18], 0) == 0;
        b = now();
        t[9][k] = b - a;
        a = now();
        ok += words("~/a $HOME b", WRDE_NOCMD, 3) == 3;
        b = now();
        t[10][k] = b - a;
        a = now();
        ok += bindtextdomain("cbench", dir) != NULL && strcmp(dgettext_fn("cbench", "hello"), "hallo") == 0;
        b = now();
        t[11][k] = b - a;
        a = now();
        ok += strcmp(dgettext_fn("cbench-absent", "no catalog"), "no catalog") == 0;
        b = now();
        t[12][k] = b - a;
    }
    if (ok != 2 * COLD_ROWS)
        return 1;
    static char out[COLD_ROWS * 80];
    size_t n = 0;
    for (int r = 0; r < COLD_ROWS; r++)
        n += (size_t)snprintf(out + n, sizeof out - n, "%s %.0f %.0f\n", cold_names[r], t[r][0], t[r][1]);
    return write(1, out, n) == (ssize_t)n ? 0 : 1;
}

static int cmp_double(const void *a, const void *b)
{
    double x = *(const double *)a, y = *(const double *)b;
    return (x > y) - (x < y);
}

#define COLD_RUNS 31

/* Runs this program with `mode` COLD_RUNS times: `wall` gets the median time from just before
 * execve (sent through the pipe by the child) to waitpid's return; with `rows`, the medians of
 * the first and second calls the fresh process reports. */
static void fresh(const char *mode, double *wall, double rows[COLD_ROWS][2])
{
    static double w[COLD_RUNS], r[COLD_ROWS][2][COLD_RUNS];
    char home[] = "HOME=/home/cbench";
    char *const env[] = {(char *)"PATH=/usr/bin:/bin", home, (char *)"TZ=America/New_York", NULL};
    for (int i = 0; i < COLD_RUNS; i++) {
        int fd[2];
        if (pipe(fd) != 0)
            fail("pipe");
        pid_t p = fork();
        if (p < 0)
            fail("fork");
        if (p == 0) {
            close(fd[0]);
            dup2(fd[1], 1);
            char *argv[] = {(char *)"cbench_opt", (char *)mode, top, NULL};
            double t0 = now();
            if (write(1, &t0, sizeof t0) != (ssize_t)sizeof t0)
                _exit(126);
            execve("/proc/self/exe", argv, env);
            _exit(127);
        }
        close(fd[1]);
        static char buf[4096];
        size_t got = 0;
        ssize_t n;
        while ((n = read(fd[0], buf + got, sizeof buf - 1 - got)) > 0)
            got += (size_t)n;
        close(fd[0]);
        int st;
        if (waitpid(p, &st, 0) != p)
            fail("waitpid");
        double t1 = now();
        if (!WIFEXITED(st) || WEXITSTATUS(st) != 0 || got < sizeof(double))
            fail("fresh process");
        double t0;
        memcpy(&t0, buf, sizeof t0);
        w[i] = t1 - t0;
        if (rows) {
            buf[got] = 0;
            char *s = buf + sizeof(double);
            for (int k = 0; k < COLD_ROWS; k++) {
                char name[64];
                double a, b;
                int used = 0;
                if (sscanf(s, "%63s %lf %lf\n%n", name, &a, &b, &used) != 3 || strcmp(name, cold_names[k]) != 0)
                    fail("cold output");
                r[k][0][i] = a;
                r[k][1][i] = b;
                s += used;
            }
        }
    }
    qsort(w, COLD_RUNS, sizeof w[0], cmp_double);
    *wall = w[COLD_RUNS / 2];
    if (rows)
        for (int k = 0; k < COLD_ROWS; k++)
            for (int j = 0; j < 2; j++) {
                qsort(r[k][j], COLD_RUNS, sizeof(double), cmp_double);
                rows[k][j] = r[k][j][COLD_RUNS / 2];
            }
}

int main(int argc, char **argv)
{
    if (argc > 1 && !strcmp(argv[1], "empty"))
        return 0;
    if (argc > 2 && !strcmp(argv[1], "cold"))
        return cold(argv[2]);

    make_inputs();
    make_fixture();
    setenv("HOME", "/home/cbench", 1);
    setenv("WORDS", "one two three four", 1);
    unsetenv("LANGUAGE");
    unsetenv("LC_ALL");
    char buf[512];

    /* ---- printf family */
    if (snprintf_fn(buf, sizeof buf, "%s=%d (%5.2f) %x", "key", 7, 1.75, 255u) != 16 || strcmp(buf, "key=7 ( 1.75) ff") != 0)
        fail("snprintf mixed");
    if (ls_line(buf, sizeof buf, 5) < 40)
        fail("snprintf ls line");
    MEASURE("snprintf_d", 500000, snprintf_fn(buf, sizeof buf, "%d", (int)(i & 0xffff) * 7919));
    MEASURE("snprintf_ld_large", 250000, snprintf_fn(buf, sizeof buf, "%ld", 1234567890123L + i));
    MEASURE("snprintf_x08", 500000, snprintf_fn(buf, sizeof buf, "%08x", (unsigned)i * 2654435761u));
    MEASURE("snprintf_s_path", 500000, snprintf_fn(buf, sizeof buf, "%s", paths[i % NAMES]));
    MEASURE("snprintf_s_d_colon", 250000, snprintf_fn(buf, sizeof buf, "%s:%d: %s", "main.c", (int)i & 1023, "error"));
    MEASURE("snprintf_ls_line", 125000, ls_line(buf, sizeof buf, i));
    /* ls -l (coreutils, fortified): mode and link count column, "%s %*s " */
    MEASURE("sprintf_chk_ls_nlink", 500000, sprintf_chk_fn(buf, 1, sizeof buf, "%s %*s ", "-rw-r--r--", 2, (i & 1) ? "1" : "12"));
    MEASURE("snprintf_mixed", 125000, snprintf_fn(buf, sizeof buf, "%s=%d (%5.2f) %x", "key", (int)i, (double)i * 0.25, (unsigned)i));
    MEASURE("snprintf_f2", 125000, snprintf_fn(buf, sizeof buf, "%.2f", (double)i * 1.37));
    MEASURE("snprintf_g", 125000, snprintf_fn(buf, sizeof buf, "%g", (double)i * 0.0137));
    MEASURE("snprintf_e", 125000, snprintf_fn(buf, sizeof buf, "%.6e", (double)i * 3.14159e-7));
    MEASURE("snprintf_f_large", 50000, snprintf_fn(buf, sizeof buf, "%f", 1e300 / (double)(1 + (i & 7))));
    {
        FILE *f = fopen("/dev/null", "w");
        if (!f)
            fail("fopen /dev/null");
        static char fbuf[8192];
        setvbuf(f, fbuf, _IOFBF, sizeof fbuf);
        MEASURE("fprintf_ls_line", 125000,
                fprintf_fn(f, "%s %2lu %-8s %-8s %8lld %s %s\n", "-rw-r--r--", 1ul, "user", "staff", (long long)i * 4099, "Oct  4 12:00", names[i % NAMES]));
        MEASURE("fprintf_du_line", 250000, fprintf_fn(f, "%lu\t%s\n", (unsigned long)i * 8, paths[i % NAMES]));
        fclose(f);
    }

    /* ---- strcmp, strcoll */
    MEASURE("strcmp_names", 1250000, cmp_names(strcmp_fn, i));
    MEASURE("strcmp_equal_16", 1250000, strcmp_fn(eq_a, eq_b));
    MEASURE("strcmp_late_256", 500000, strcmp_fn(long_a, long_b));
    MEASURE("strcoll_c_names", 1250000, cmp_names(strcoll_fn, i));
    if (!setlocale(LC_ALL, "C.UTF-8"))
        fail("setlocale C.UTF-8");
    MEASURE("strcoll_c_utf8_names", 1250000, cmp_names(strcoll_fn, i));
    if (!setlocale(LC_ALL, "en_US.UTF-8"))
        fail("setlocale en_US.UTF-8");
    if (strcoll_fn("a", "b") >= 0)
        fail("strcoll en_US");
    MEASURE("strcoll_en_us_names", 250000, cmp_names(strcoll_fn, i));
    MEASURE("strcoll_en_us_equal_16", 250000, strcoll_fn(eq_a, eq_b));
    MEASURE("strcoll_en_us_utf8_16", 250000, strcoll_fn("abcdefg \xc3\xa9 h \xe2\x82\xac!", "abcdefg \xc3\xa9 h \xe2\x82\xac."));

    /* ---- iconv (per conversion of 4 KiB, or of one file name) */
    cd_to16 = iconv_open("UTF-16LE", "UTF-8");
    cd_from16 = iconv_open("UTF-8", "UTF-16LE");
    cd_to32 = iconv_open("UTF-32LE", "UTF-8");
    if (cd_to16 == (iconv_t)-1 || cd_from16 == (iconv_t)-1 || cd_to32 == (iconv_t)-1)
        fail("iconv_open unicode");
    long n16 = convert(cd_to16, text_mixed, 4096);
    if (n16 <= 0)
        fail("iconv mixed to UTF-16");
    memcpy(utf16_mixed, outbuf, (size_t)n16);
    utf16_mixed_len = (size_t)n16;
    if (convert(cd_from16, utf16_mixed, utf16_mixed_len) != 4096 || memcmp(outbuf, text_mixed, 4096) != 0)
        fail("iconv UTF-16 round trip");
    MEASURE("iconv_utf8_utf16_ascii_4k", 5000, convert(cd_to16, text_ascii, 4096));
    MEASURE("iconv_utf8_utf16_mixed_4k", 5000, convert(cd_to16, text_mixed, 4096));
    MEASURE("iconv_utf16_utf8_mixed", 5000, convert(cd_from16, utf16_mixed, utf16_mixed_len));
    MEASURE("iconv_utf8_utf32_mixed_4k", 5000, convert(cd_to32, text_mixed, 4096));
    MEASURE("iconv_utf8_utf16_name", 500000, convert(cd_to16, names[i % NAMES], strlen(names[i % NAMES])));
    MEASURE("iconv_open_conv_close_name", 50000, open_convert_close(names[i % NAMES]));
    cd_to8859 = iconv_open("ISO-8859-15", "UTF-8");
    cd_from8859 = iconv_open("UTF-8", "ISO-8859-15");
    if (cd_to8859 != (iconv_t)-1 && cd_from8859 != (iconv_t)-1) {
        long n = convert(cd_to8859, text_latin, 4096);
        if (n <= 0)
            fail("iconv to ISO-8859-15");
        memcpy(text_8859, outbuf, (size_t)n);
        text_8859_len = (size_t)n;
        MEASURE("iconv_utf8_8859_15_latin_4k", 5000, convert(cd_to8859, text_latin, 4096));
        MEASURE("iconv_8859_15_utf8_latin", 5000, convert(cd_from8859, text_8859, text_8859_len));
    }

    /* ---- strftime, mktime */
    setenv("TZ", "America/New_York", 1);
    tzset();
    struct tm r;
    time_t when = 1759579200;
    localtime_r(&when, &r);
    if (strftime_fn(buf, sizeof buf, "%Y-%m-%d %H:%M", &r) != 16 || strcmp(buf, "2025-10-04 08:00") != 0)
        fail("strftime ls iso");
    MEASURE("strftime_ls_iso", 250000, strftime_fn(buf, sizeof buf, "%Y-%m-%d %H:%M", &r));
    MEASURE("strftime_ls_recent_en_us", 250000, strftime_fn(buf, sizeof buf, "%b %e %H:%M", &r));
    MEASURE("strftime_date_en_us", 250000, strftime_fn(buf, sizeof buf, "%a %b %e %H:%M:%S %Z %Y", &r));
    MEASURE("strftime_iso8601_tz", 250000, strftime_fn(buf, sizeof buf, "%Y-%m-%dT%H:%M:%S%z", &r));
    MEASURE("strftime_c_en_us", 250000, strftime_fn(buf, sizeof buf, "%c", &r));
    MEASURE("strftime_log", 250000, strftime_fn(buf, sizeof buf, "%d/%b/%Y:%H:%M:%S %z", &r));
    setlocale(LC_TIME, "C");
    MEASURE("strftime_ls_recent_c", 250000, strftime_fn(buf, sizeof buf, "%b %e %H:%M", &r));
    setlocale(LC_TIME, "en_US.UTF-8");
    MEASURE("mktime_new_york", 125000, mk(i));
    MEASURE("localtime_r_new_york", 500000, lt(when + i * 3607));
    setenv("TZ", "UTC0", 1);
    tzset();
    MEASURE("mktime_utc", 250000, mk(i));
    setenv("TZ", "EST5EDT,M3.2.0,M11.1.0", 1);
    tzset();
    MEASURE("mktime_posix_rule", 125000, mk(i));
    MEASURE("localtime_r_posix_rule", 250000, lt(when + i * 3607));
    setenv("TZ", "America/New_York", 1);
    tzset();

    /* ---- regular expressions: per line of the 256-line text (sed and grep scripts) */
    static const struct { const char *name, *pat; int flags; size_t nmatch; long want; } res[] = {
        {"re_grep_literal", "Accepted", REG_NOSUB, 0, 64},
        {"re_grep_comment", "^[[:space:]]*#", REG_NOSUB, 0, 64},
        {"re_grep_ipv4", "[0-9]+\\.[0-9]+\\.[0-9]+\\.[0-9]+", REG_EXTENDED | REG_NOSUB, 0, 64},
        {"re_grep_alt", "(sshd|cron|kernel)\\[[0-9]+\\]", REG_EXTENDED | REG_NOSUB, 0, 64},
        {"re_grep_icase_absent", "error|warning", REG_EXTENDED | REG_ICASE | REG_NOSUB, 0, 0},
        {"re_grep_dotstar", "user.*from.*port", REG_NOSUB, 0, 64},
        {"re_sed_kv", "^\\([a-z0-9_]*\\) = \\(.*\\)$", 0, 3, 64},
        {"re_sed_swap", "\\([a-z]*\\)_\\([0-9]*\\)", 0, 3, 128},
    };
    for (unsigned k = 0; k < sizeof res / sizeof res[0]; k++) {
        regex_t re;
        if (regcomp(&re, res[k].pat, res[k].flags) != 0)
            fail(res[k].name);
        if (scan_count(&re, res[k].nmatch) != res[k].want)
            fail(res[k].name);
        MEASURE(res[k].name, 25000, exec_lines(&re, i, res[k].nmatch));
        regfree(&re);
    }
    /* ---- GNU API: re_search per sed line (cmd1 ERE swap, cmd2 BRE suffix), the same patterns
       through regexec (same subexpressions), and a 1-byte subject (the fixed per-call cost) */
    for (int k = 0; k < LINES; k++)
        sed_len[k] = snprintf(sed_lines[k], sizeof sed_lines[k], "word %d /usr/lib/word/%d.so", 100000 + k * 37, k);
    {
        static const struct { const char *search, *exec, *pat; reg_syntax_t syn; int cflags; size_t nmatch; } gs[] = {
            {"re_search_sed_swap", "regexec_sed_swap", "([a-z]+) ([0-9]+)", RE_SYNTAX_POSIX_EXTENDED, REG_EXTENDED, 3},
            {"re_search_sed_so", "regexec_sed_so", "\\.so$", RE_SYNTAX_POSIX_BASIC, 0, 1},
        };
        for (unsigned k = 0; k < sizeof gs / sizeof gs[0]; k++) {
            struct re_pattern_buffer b;
            regex_t re;
            sed_compile(&b, gs[k].pat, (gs[k].syn & ~RE_DOT_NOT_NULL) | RE_NO_POSIX_BACKTRACKING);
            if (regcomp(&re, gs[k].pat, gs[k].cflags) != 0)
                fail(gs[k].exec);
            for (long i = 0; i < LINES; i++)
                if (!sed_search(&b, i) || !sed_exec(&re, i, gs[k].nmatch))
                    fail(gs[k].search);
            MEASURE(gs[k].search, 25000, sed_search(&b, i));
            MEASURE(gs[k].exec, 25000, sed_exec(&re, i, gs[k].nmatch));
            if (k == 0) {
                if (re_search_fn(&b, "w", 1, 0, 1, &sed_regs) != -1)
                    fail("re_search_1byte");
                MEASURE("re_search_1byte", 50000, re_search_fn(&b, "w", 1, 0, 1, &sed_regs) == -1);
            }
            regfree(&b);
            regfree(&re);
        }
        free(sed_regs.start);
        free(sed_regs.end);
    }
    MEASURE("regcomp_regfree_sed_kv", 12500, comp_free("^\\([a-z0-9_]*\\) = \\(.*\\)$", 0));
    MEASURE("regcomp_regfree_ipv4", 12500, comp_free("[0-9]+\\.[0-9]+\\.[0-9]+\\.[0-9]+", REG_EXTENDED | REG_NOSUB));
    {
        regex_t mt[RE_THREADS];
        for (int k = 0; k < RE_THREADS; k++)
            if (regcomp(&mt[k], "(sshd|cron|kernel)\\[[0-9]+\\]", REG_EXTENDED | REG_NOSUB) != 0)
                fail("re_mt4");
        if (re_threads(mt, 1) != RE_THREADS * RE_THREAD_EXECS / 4 || re_threads(mt, 0) != RE_THREADS * RE_THREAD_EXECS / 4)
            fail("re_mt4");
        MEASURE("re_mt4_shared", 40, re_threads(mt, 1));
        MEASURE("re_mt4_distinct", 40, re_threads(mt, 0));
        for (int k = 0; k < RE_THREADS; k++)
            regfree(&mt[k]);
    }

    /* ---- fnmatch: per name of the list */
    static const struct { const char *name, *pat; int flags; int paths; } fns[] = {
        {"fnmatch_names_star_c", "*.c", 0, 0},
        {"fnmatch_names_ch", "*.[ch]", 0, 0},
        {"fnmatch_names_lib_so", "lib*.so*", 0, 0},
        {"fnmatch_names_period", "*", FNM_PERIOD, 0},
        {"fnmatch_names_question", "????.txt", 0, 0},
        {"fnmatch_names_class", "[[:upper:]]*", 0, 0},
        {"fnmatch_names_casefold", "*.txt", FNM_CASEFOLD, 0},
        {"fnmatch_paths_pathname", "src/*/*.c", FNM_PATHNAME, 1},
        {"fnmatch_paths_leading_dir", "src", FNM_LEADING_DIR, 1},
    };
    for (unsigned k = 0; k < sizeof fns / sizeof fns[0]; k++) {
        long hits = 0;
        for (long j = 0; j < NAMES; j++)
            hits += fnmatch_fn(fns[k].pat, fns[k].paths ? paths[j] : names[j], fns[k].flags) == 0;
        if (hits == 0)
            fail(fns[k].name);
        MEASURE(fns[k].name, 250000, fnmatch_fn(fns[k].pat, fns[k].paths ? paths[i % NAMES] : names[i % NAMES], fns[k].flags) == 0);
    }

    /* ---- wordexp (WRDE_NOCMD: nothing runs a shell) */
    static const struct { const char *name, *text; size_t want; } wes[] = {
        {"wordexp_tilde_var", "~/a $HOME b", 3},
        {"wordexp_arith", "x $((3*(4+5)-7)) y", 3},
        {"wordexp_plain_8", "cc -O2 -Wall -o prog main.c util.c -lm", 8},
        {"wordexp_quoted", "'a b' \"c $HOME\" d\\ e", 3},
        {"wordexp_split_var", "$WORDS", 4},
        {"wordexp_param_forms", "${HOME:-/} ${UNSET_CBENCH:-x} ${#HOME}", 3},
    };
    for (unsigned k = 0; k < sizeof wes / sizeof wes[0]; k++) {
        if (words(wes[k].text, WRDE_NOCMD, wes[k].want) != (long)wes[k].want)
            fail(wes[k].name);
        MEASURE(wes[k].name, 12500, words(wes[k].text, WRDE_NOCMD, wes[k].want));
    }

    /* ---- gettext (en_US.UTF-8, LANGUAGE unset): the fixture catalog; a domain with no catalog,
     * as coreutils in an English locale; the C locale. */
    if (!bindtextdomain("cbench", top) || !textdomain("cbench") || !bindtextdomain("cbench-absent", "/usr/share/locale"))
        fail("bindtextdomain");
    if (strcmp(gettext_fn("hello"), "hallo") != 0 || strcmp(gettext_fn("missing"), "missing") != 0 ||
        strcmp(ngettext_fn("apple", "apples", 3), "\xc3\x84pfel") != 0 || strcmp(dgettext_fn("cbench-absent", "x"), "x") != 0)
        fail("gettext");
    MEASURE("gettext_found", 50000, gettext_fn("hello"));
    MEASURE("gettext_missing", 50000, gettext_fn("missing"));
    MEASURE("ngettext_found", 50000, ngettext_fn("apple", "apples", 3));
    MEASURE("gettext_no_catalog", 50000, dgettext_fn("cbench-absent", "no catalog"));
    /* Varying inputs (P10 (14)): msgids found and not found in turn; then LANGUAGE alternating
     * between two values (putenv of two fixed strings, so the row adds no allocation), each of
     * which reaches the fixture catalog. */
    static const char *const vary_ids[4] = {"hello", "apple", "missing", "not in the catalog"};
    MEASURE("gettext_vary_msgid", 50000, gettext_fn(vary_ids[i & 3]));
    static char lang_a[] = "LANGUAGE=en_US", lang_b[] = "LANGUAGE=de:en_US";
    putenv(lang_a);
    if (strcmp(gettext_fn("hello"), "hallo") != 0)
        fail("gettext LANGUAGE");
    putenv(lang_b);
    if (strcmp(gettext_fn("hello"), "hallo") != 0)
        fail("gettext LANGUAGE");
    MEASURE("gettext_language_alternating", 50000, (putenv((i & 1) ? lang_a : lang_b), gettext_fn("hello")));
    MEASURE("gettext_language_putenv_alone", 50000, putenv((i & 1) ? lang_a : lang_b));
    /* LANGUAGE set and not changing between calls (GETTEXT-MEMO-HIT-COST): the common case of a
     * desktop session, where the alternating row's putenv per call is unusual. */
    putenv(lang_b);
    if (strcmp(gettext_fn("hello"), "hallo") != 0)
        fail("gettext LANGUAGE steady");
    MEASURE("gettext_language_steady", 50000, gettext_fn("hello"));
    unsetenv("LANGUAGE");
    setlocale(LC_MESSAGES, "C");
    if (strcmp(gettext_fn("hello"), "hello") != 0)
        fail("gettext C");
    MEASURE("gettext_c_locale", 500000, gettext_fn("hello"));
    setlocale(LC_MESSAGES, "en_US.UTF-8");

    /* ---- PATH search (STRESS-ENV-AUDIT-2): execvp of a name in no PATH directory tries every one and
     * returns ENOENT in this process, so the counters see the whole search (a found command would be
     * searched in a child, which they do not count; posix_spawnp_true in cbench_process times that). */
    if (exec_missing() != 1)
        fail("execvp of a missing command");
    MEASURE("execvp_missing", 2000, exec_missing());

    /* ---- cold start */
    if (!wanted("cold"))
        return 0;
    double wall, rows[COLD_ROWS][2];
    fresh("empty", &wall, NULL);
    put("cold_exec_empty", wall);
    fresh("cold", &wall, rows);
    put("cold_exec_once", wall);
    for (int k = 0; k < COLD_ROWS; k++) {
        char name[64];
        snprintf(name, sizeof name, "first_%s", cold_names[k]);
        put(name, rows[k][0]);
        snprintf(name, sizeof name, "second_%s", cold_names[k]);
        put(name, rows[k][1]);
    }
    return 0;
}
