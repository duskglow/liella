/* Message catalogs end to end (spec §7.7 gettext family, §7.8 catopen/catgets/catclose; D-7.5,
 * D-7.6): built against the host C library and against Liella's products, the outputs must match
 * line for line. Black-box: every line is a result, an errno, or a fact read back through a public
 * function.
 *
 * Output conventions: strings are printed in brackets with every byte outside printable ASCII as
 * \xNN and a backslash as \\; a run of more than 60 equal bytes is printed as <c>*<n>; the scratch
 * directory is printed as <dir>. Nothing prints a descriptor number, an address, a PID or the value
 * of _nl_msg_cat_cntr (only whether it changed).
 *
 * Usage: catalogs_diff DIR [unstable], normally through catalogs_run.sh, which first builds fixture
 * catalogs with msgfmt (GNU gettext) and gencat into DIR/fix. DIR must be an absolute path without
 * symbolic links. With `unstable` only the damaged-catalog cases marked "~" run (offsets or sizes
 * pointing outside the file); their outcome depends on memory layout, so the record leaves them out. LOCPATH must name the test locales (de_DE.UTF-8, fr_FR.UTF-8, fr_FR.ISO-8859-1,
 * cs_CZ.UTF-8, hu_HU.UTF-8, it_IT.UTF-8, nl_NL.UTF-8, ru_RU.KOI8-R, ja_JP.EUC-JP).
 *
 * The probe also writes catalogs of its own: .mo files laid out as the GNU gettext manual's "MO
 * Files" chapter describes (no hash table), and .cat files laid out as recorded in
 * spec/behaviour/catalogs-format.md. Every catalog the probe writes lives below DIR; the system's
 * locale directories are never written. Risky calls run in forked children with an alarm. */
#define _GNU_SOURCE
#undef _FORTIFY_SOURCE
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <libintl.h>
#include <limits.h>
#include <locale.h>
#include <nl_types.h>
#include <signal.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/resource.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>
#include "tscale.h"

/* Exported data of §7.7 that no installed header declares (gettext.h and autoconf tests declare
 * _nl_msg_cat_cntr this way). Sizes from the symbol table: 5, 18, 8 and 4 bytes. */
extern int _nl_msg_cat_cntr;
extern const char _nl_default_dirname[];
extern const char _libc_intl_domainname[];
extern void *_nl_domain_bindings;

static char dir[PATH_MAX];
static size_t dirlen;
static int unstable_mode;

/* ---------------------------------------------------------------- formatting helpers */

static const char *en(int e)
{
    static char b[4][24];
    static int k;
    switch (e) {
    case 0: return "0";
    case EPERM: return "EPERM";
    case ENOENT: return "ENOENT";
    case EINTR: return "EINTR";
    case EBADF: return "EBADF";
    case EAGAIN: return "EAGAIN";
    case ENOMEM: return "ENOMEM";
    case EACCES: return "EACCES";
    case EEXIST: return "EEXIST";
    case ENOTDIR: return "ENOTDIR";
    case EISDIR: return "EISDIR";
    case EINVAL: return "EINVAL";
    case ENFILE: return "ENFILE";
    case EMFILE: return "EMFILE";
    case EFBIG: return "EFBIG";
    case ENOSPC: return "ENOSPC";
    case ERANGE: return "ERANGE";
    case ENAMETOOLONG: return "ENAMETOOLONG";
    case ELOOP: return "ELOOP";
    case ENOMSG: return "ENOMSG";
    case EBADMSG: return "EBADMSG";
    case EOVERFLOW: return "EOVERFLOW";
    case EILSEQ: return "EILSEQ";
    case ENXIO: return "ENXIO";
    case E2BIG: return "E2BIG";
    case ENOEXEC: return "ENOEXEC";
    case ENODATA: return "ENODATA";
    }
    char *r = b[k++ & 3];
    snprintf(r, 24, "E#%d", e);
    return r;
}

static const char *sn(int s)
{
    static char b[24];
    switch (s) {
    case SIGSEGV: return "SIGSEGV";
    case SIGBUS: return "SIGBUS";
    case SIGFPE: return "SIGFPE";
    case SIGALRM: return "SIGALRM";
    case SIGABRT: return "SIGABRT";
    case SIGILL: return "SIGILL";
    case SIGKILL: return "SIGKILL";
    }
    snprintf(b, sizeof b, "%d", s);
    return b;
}

static void pc(unsigned char c)
{
    if (c == '\\')
        fputs("\\\\", stdout);
    else if (c >= 0x20 && c < 0x7f)
        putchar(c);
    else
        printf("\\x%02x", c);
}

/* A string in brackets (NULL as NULL), with the scratch directory as <dir>. */
static void pz(const char *s)
{
    if (!s) {
        fputs("NULL", stdout);
        return;
    }
    putchar('[');
    const char *p = s;
    while (*p) {
        if (dirlen && strncmp(p, dir, dirlen) == 0) {
            fputs("<dir>", stdout);
            p += dirlen;
            continue;
        }
        size_t run = 1;
        while (p[run] == p[0])
            run++;
        if (run > 60) {
            putchar('<');
            pc((unsigned char)p[0]);
            printf(">*%zu", run);
            p += run;
            continue;
        }
        pc((unsigned char)*p);
        p++;
    }
    putchar(']');
}

static void pl(const char *label, const char *s)
{
    printf("%s: ", label);
    pz(s);
    putchar('\n');
}

/* A path below the scratch directory (eight rotating buffers). */
static char *P(const char *fmt, ...)
{
    static char b[8][PATH_MAX * 2];
    static int k;
    char *r = b[k++ & 7];
    int n = snprintf(r, PATH_MAX * 2, "%s/", dir);
    va_list ap;
    va_start(ap, fmt);
    vsnprintf(r + n, PATH_MAX * 2 - n, fmt, ap);
    va_end(ap);
    return r;
}

static void mkparents(const char *path)
{
    char t[PATH_MAX * 2];
    snprintf(t, sizeof t, "%s", path);
    for (char *p = t + dirlen + 1; *p; p++)
        if (*p == '/') {
            *p = 0;
            mkdir(t, 0755);
            *p = '/';
        }
}

static int wfile(const char *path, const void *b, size_t n)
{
    mkparents(path);
    unlink(path);
    int fd = open(path, O_WRONLY | O_CREAT | O_TRUNC | O_CLOEXEC, 0644);
    if (fd < 0) {
        printf("setup: cannot create ");
        pz(path);
        printf(": %s\n", en(errno));
        return -1;
    }
    size_t o = 0;
    while (o < n) {
        ssize_t w = write(fd, (const char *)b + o, n - o);
        if (w <= 0)
            break;
        o += (size_t)w;
    }
    close(fd);
    return 0;
}

static unsigned char *rfile(const char *path, size_t *n)
{
    int fd = open(path, O_RDONLY | O_CLOEXEC);
    if (fd < 0)
        return NULL;
    struct stat st;
    if (fstat(fd, &st) != 0) {
        close(fd);
        return NULL;
    }
    unsigned char *b = malloc((size_t)st.st_size + 1);
    size_t o = 0;
    while (b && o < (size_t)st.st_size) {
        ssize_t r = read(fd, b + o, (size_t)st.st_size - o);
        if (r <= 0)
            break;
        o += (size_t)r;
    }
    close(fd);
    *n = o;
    return b;
}

/* The fixture built by catalogs_run.sh, or NULL with a line saying it is missing. */
static unsigned char *fixture(const char *name, size_t *n)
{
    unsigned char *b = rfile(P("fix/%s", name), n);
    if (!b)
        printf("fixture missing: %s\n", name);
    return b;
}

static void copy_fixture(const char *name, const char *dst)
{
    size_t n;
    unsigned char *b = fixture(name, &n);
    if (b) {
        wfile(dst, b, n);
        free(b);
    }
}

/* Run f(a) in a child with an alarm; report how it ended unless it exited 0 (or always).
 * `name` (the function's name, from the macro below) appears only in the stderr diagnostics. */
static void child_run(const char *name, void (*f)(void *), void *a, unsigned secs, int always)
{
    struct timespec t0, t1;
    clock_gettime(CLOCK_MONOTONIC, &t0);
    fflush(stdout);
    pid_t p = fork();
    if (p < 0) {
        printf("fork failed\n");
        return;
    }
    if (p == 0) {
        tw_alarm(secs);
        f(a);
        fflush(stdout);
        _exit(0);
    }
    int st;
    while (waitpid(p, &st, 0) < 0 && errno == EINTR)
        ;
    if (WIFEXITED(st)) {
        if (always || WEXITSTATUS(st))
            printf("  child exited %d\n", WEXITSTATUS(st));
    } else if (WIFSIGNALED(st)) {
        printf("  child killed by signal %s\n", sn(WTERMSIG(st)));
        /* Diagnostics only (stderr, not compared): a child that met its watchdog on a loaded machine
         * prints a different line count than an idle run (CATALOGS-FULL-RACE-2). */
        if (WTERMSIG(st) == SIGALRM) {
            clock_gettime(CLOCK_MONOTONIC, &t1);
            char la[64] = "?";
            int fd = open("/proc/loadavg", O_RDONLY);
            ssize_t n = fd < 0 ? -1 : read(fd, la, sizeof la - 1);
            if (fd >= 0)
                close(fd);
            if (n > 0)
                la[n - (la[n - 1] == '\n')] = 0;
            fflush(stdout);
            fprintf(stderr, "catalogs_diff: child %s(%p) met its watchdog (base %u s, scale %u) after %.1f s; loadavg %s\n",
                    name, a, secs, tw_scale(), (double)(t1.tv_sec - t0.tv_sec) + (t1.tv_nsec - t0.tv_nsec) / 1e9, la);
        }
    }
}

#define child(f, a, secs, always) child_run(#f, f, a, secs, always)

static void clear_env(void)
{
    static const char *v[] = {"LANGUAGE", "LANG", "LC_ALL", "LC_MESSAGES", "LC_CTYPE", "LC_TIME",
                              "LC_NUMERIC", "LC_COLLATE", "LC_MONETARY", "NLSPATH", "OUTPUT_CHARSET"};
    for (size_t i = 0; i < sizeof v / sizeof v[0]; i++)
        unsetenv(v[i]);
}

static void setenv_or_unset(const char *name, const char *value)
{
    if (value)
        setenv(name, value, 1);
    else
        unsetenv(name);
}

static int need_locale(int cat, const char *name)
{
    if (!setlocale(cat, name)) {
        printf("  setlocale(%d, ", cat);
        pz(name);
        printf(") failed\n");
        return 0;
    }
    return 1;
}

/* ---------------------------------------------------------------- .mo writer */

static void put32(unsigned char *p, uint32_t v, int be)
{
    if (be) {
        p[0] = (unsigned char)(v >> 24);
        p[1] = (unsigned char)(v >> 16);
        p[2] = (unsigned char)(v >> 8);
        p[3] = (unsigned char)v;
    } else {
        p[0] = (unsigned char)v;
        p[1] = (unsigned char)(v >> 8);
        p[2] = (unsigned char)(v >> 16);
        p[3] = (unsigned char)(v >> 24);
    }
}

static uint32_t get32(const unsigned char *p)
{
    return (uint32_t)p[0] | (uint32_t)p[1] << 8 | (uint32_t)p[2] << 16 | (uint32_t)p[3] << 24;
}

struct ent {
    const char *k;
    size_t kl;
    const char *v;
    size_t vl;
};
#define E(a, b) {a, sizeof(a) - 1, b, sizeof(b) - 1}

static int cmp_ent(const void *x, const void *y)
{
    const struct ent *a = x, *b = y;
    size_t m = a->kl < b->kl ? a->kl : b->kl;
    int c = memcmp(a->k, b->k, m);
    if (c)
        return c;
    return a->kl < b->kl ? -1 : a->kl > b->kl;
}

static unsigned char mobuf[1 << 20];

/* Header (28 bytes), the original-string table at 28, the translation table after it, no hash
 * table (S = 0, H = the end of the tables), then the strings, each followed by a NUL. */
static size_t build_mo(unsigned char *buf, const struct ent *in, int n, int be, uint32_t rev, int sort)
{
    struct ent e[64];
    memcpy(e, in, sizeof *e * (size_t)n);
    if (sort)
        qsort(e, (size_t)n, sizeof *e, cmp_ent);
    uint32_t O = 28, T = 28 + 8 * (uint32_t)n, H = 28 + 16 * (uint32_t)n;
    size_t off = H;
    put32(buf, 0x950412de, be);
    put32(buf + 4, rev, be);
    put32(buf + 8, (uint32_t)n, be);
    put32(buf + 12, O, be);
    put32(buf + 16, T, be);
    put32(buf + 20, 0, be);
    put32(buf + 24, H, be);
    for (int i = 0; i < n; i++) {
        put32(buf + O + 8 * i, (uint32_t)e[i].kl, be);
        put32(buf + O + 8 * i + 4, (uint32_t)off, be);
        memcpy(buf + off, e[i].k, e[i].kl);
        buf[off + e[i].kl] = 0;
        off += e[i].kl + 1;
    }
    for (int i = 0; i < n; i++) {
        put32(buf + T + 8 * i, (uint32_t)e[i].vl, be);
        put32(buf + T + 8 * i + 4, (uint32_t)off, be);
        memcpy(buf + off, e[i].v, e[i].vl);
        buf[off + e[i].vl] = 0;
        off += e[i].vl + 1;
    }
    return off;
}

static void write_mo(const char *path, const struct ent *e, int n)
{
    size_t len = build_mo(mobuf, e, n, 0, 0, 1);
    wfile(path, mobuf, len);
}

#define HDR_UTF8 "Content-Type: text/plain; charset=UTF-8\n"

static void where_mo(const char *path, const char *label)
{
    struct ent e[2] = {{"", 0, HDR_UTF8, strlen(HDR_UTF8)}, {"where", 5, label, strlen(label)}};
    write_mo(path, e, 2);
}

static const char *G(const char *dom, const char *id)
{
    return dcgettext(dom, id, LC_MESSAGES);
}

/* ================================================================ gettext: initial state */

static void initial(void)
{
    printf("== initial state (before any other call)\n");
    pl("_nl_default_dirname", _nl_default_dirname);
    pl("_libc_intl_domainname", _libc_intl_domainname);
    printf("_nl_domain_bindings NULL: %s\n", _nl_domain_bindings ? "no" : "yes");
    pl("textdomain(NULL)", textdomain(NULL));
    pl("bindtextdomain(\"messages\", NULL)", bindtextdomain("messages", NULL));
    pl("bind_textdomain_codeset(\"messages\", NULL)", bind_textdomain_codeset("messages", NULL));
    pl("bindtextdomain(\"never-bound\", NULL)", bindtextdomain("never-bound", NULL));
    printf("_nl_domain_bindings NULL after the queries: %s\n", _nl_domain_bindings ? "no" : "yes");
    pl("setlocale(LC_MESSAGES, NULL)", setlocale(LC_MESSAGES, NULL));
    static const char id[] = "hello";
    const char *r = (gettext)(id);
    pl("C locale: gettext(\"hello\")", r);
    printf("C locale: gettext returns its argument: %s\n", r == id ? "yes" : "no");
    static const char s1[] = "one", s2[] = "many";
    printf("C locale: ngettext n=0 %s, n=1 %s, n=2 %s, n=ULONG_MAX %s\n",
           (ngettext)(s1, s2, 0) == s2 ? "plural" : "other",
           (ngettext)(s1, s2, 1) == s1 ? "singular" : "other",
           (ngettext)(s1, s2, 2) == s2 ? "plural" : "other",
           (ngettext)(s1, s2, ULONG_MAX) == s2 ? "plural" : "other");
    pl("C locale: gettext(\"\")", (gettext)(""));
    printf("NL_SETD %d NL_CAT_LOCALE %d sizeof(nl_catd) %zu\n", NL_SETD, NL_CAT_LOCALE, sizeof(nl_catd));
#ifdef NL_SETMAX
    printf("NL_SETMAX %ld\n", (long)NL_SETMAX);
#endif
#ifdef NL_MSGMAX
    printf("NL_MSGMAX %ld\n", (long)NL_MSGMAX);
#endif
#ifdef NL_TEXTMAX
    printf("NL_TEXTMAX %ld\n", (long)NL_TEXTMAX);
#endif
#ifdef NL_ARGMAX
    printf("NL_ARGMAX %ld\n", (long)NL_ARGMAX);
#endif
#ifdef NL_LANGMAX
    printf("NL_LANGMAX %ld\n", (long)NL_LANGMAX);
#endif
#ifdef NL_NMAX
    printf("NL_NMAX %ld\n", (long)NL_NMAX);
#endif
}

/* ================================================================ textdomain */

static void s_textdomain(void *u)
{
    (void)u;
    char buf[16] = "alpha";
    char *r = textdomain(buf);
    pl("textdomain(\"alpha\")", r);
    printf("result is the argument: %s\n", r == buf ? "yes" : "no");
    strcpy(buf, "XXXXX");
    pl("after the argument was overwritten, textdomain(NULL)", textdomain(NULL));
    printf("textdomain(NULL) returns the same pointer as the last set: %s\n", textdomain(NULL) == r ? "yes" : "no");
    pl("textdomain(textdomain(NULL))", textdomain(textdomain(NULL)));
    errno = EINTR;
    r = textdomain("beta");
    printf("textdomain(\"beta\") errno after (preset EINTR): %s\n", en(errno));
    errno = 0;
    pl("textdomain(\"\")", textdomain(""));
    printf("  errno %s\n", en(errno));
    pl("textdomain(NULL) after \"\"", textdomain(NULL));
    pl("textdomain(\"messages\")", textdomain("messages"));
    pl("textdomain(\"a/b\")", textdomain("a/b"));
    pl("textdomain(\"../x\")", textdomain("../x"));
    static char longname[5001];
    memset(longname, 'q', 5000);
    pl("textdomain(5000 x q)", textdomain(longname));

    if (!need_locale(LC_ALL, "de_DE.UTF-8"))
        return;
    where_mo(P("td/de_DE.UTF-8/LC_MESSAGES/tdom.mo"), "tdom");
    where_mo(P("td/de_DE.UTF-8/LC_MESSAGES/messages.mo"), "messages");
    where_mo(P("td/de_DE.UTF-8/LC_MESSAGES/other.mo"), "other");
    bindtextdomain("tdom", P("td"));
    bindtextdomain("messages", P("td"));
    textdomain("tdom");
    printf("-- default domain tdom\n");
    pl("  gettext", (gettext)("where"));
    pl("  dgettext(NULL)", (dgettext)(NULL, "where"));
    pl("  dcgettext(NULL, LC_MESSAGES)", (dcgettext)(NULL, "where", LC_MESSAGES));
    pl("  ngettext n=1", (ngettext)("where", "wheres", 1));
    pl("  dngettext(NULL) n=1", (dngettext)(NULL, "where", "wheres", 1));
    pl("  dcngettext(NULL) n=1", (dcngettext)(NULL, "where", "wheres", 1, LC_MESSAGES));
    pl("  __dcgettext(NULL)", __dcgettext(NULL, "where", LC_MESSAGES));
    pl("  __dgettext(tdom)", __dgettext("tdom", "where"));
    textdomain("other");
    pl("-- default domain other (not bound, catalog in the tdom tree): gettext", (gettext)("where"));
    bindtextdomain("other", P("td"));
    pl("-- after binding other: gettext", (gettext)("where"));
    textdomain("");
    pl("-- after textdomain(\"\"): gettext", (gettext)("where"));
    pl("  textdomain(NULL)", textdomain(NULL));
    textdomain("tdom");
    pl("-- dgettext(\"\", where) with default tdom", (dgettext)("", "where"));
}

/* ================================================================ bindtextdomain */

static void s_bind(void *u)
{
    (void)u;
    if (!need_locale(LC_ALL, "de_DE.UTF-8"))
        return;
    pl("bindtextdomain(\"bd1\", NULL) before binding", bindtextdomain("bd1", NULL));
    char a[PATH_MAX];
    snprintf(a, sizeof a, "%s/b1", dir);
    char *r = bindtextdomain("bd1", a);
    pl("bindtextdomain(\"bd1\", <dir>/b1)", r);
    printf("result is the argument: %s\n", r == a ? "yes" : "no");
    strcpy(a, "/overwritten");
    char *q = bindtextdomain("bd1", NULL);
    pl("after the argument was overwritten, bindtextdomain(\"bd1\", NULL)", q);
    printf("same pointer as the result of the set: %s\n", q == r ? "yes" : "no");
    pl("bind_textdomain_codeset(\"bd1\", NULL)", bind_textdomain_codeset("bd1", NULL));

    printf("-- relative directory\n");
    where_mo(P("rel/de_DE.UTF-8/LC_MESSAGES/bd2.mo"), "relative to the cwd at bind time");
    where_mo(P("x/rel/de_DE.UTF-8/LC_MESSAGES/bd2.mo"), "relative to the cwd at lookup time");
    if (chdir(dir) != 0)
        printf("chdir failed\n");
    pl("  bindtextdomain(\"bd2\", \"rel\")", bindtextdomain("bd2", "rel"));
    if (chdir(P("x")) != 0)
        printf("chdir failed\n");
    pl("  lookup after chdir(<dir>/x)", G("bd2", "where"));
    pl("  bindtextdomain(\"bd2\", NULL)", bindtextdomain("bd2", NULL));
    if (chdir(dir) != 0)
        printf("chdir failed\n");
    where_mo(P("x/dot/de_DE.UTF-8/LC_MESSAGES/bd2b.mo"), "dot relative at lookup");
    where_mo(P("dot/de_DE.UTF-8/LC_MESSAGES/bd2b.mo"), "dot relative at bind");
    pl("  bindtextdomain(\"bd2b\", \"./dot\")", bindtextdomain("bd2b", "./dot"));
    if (chdir(P("x")) != 0)
        printf("chdir failed\n");
    pl("  lookup after chdir(<dir>/x)", G("bd2b", "where"));
    if (chdir(dir) != 0)
        printf("chdir failed\n");

    printf("-- NULL and empty arguments\n");
    errno = 0;
    pl("  bindtextdomain(NULL, <dir>)", bindtextdomain(NULL, dir));
    printf("  errno %s\n", en(errno));
    errno = 0;
    pl("  bindtextdomain(NULL, NULL)", bindtextdomain(NULL, NULL));
    printf("  errno %s\n", en(errno));
    errno = 0;
    pl("  bindtextdomain(\"\", <dir>)", bindtextdomain("", dir));
    printf("  errno %s\n", en(errno));
    errno = 0;
    pl("  bindtextdomain(\"\", NULL)", bindtextdomain("", NULL));
    printf("  errno %s\n", en(errno));
    where_mo(P("de_DE.UTF-8/LC_MESSAGES/bd3.mo"), "found relative to the cwd");
    errno = 0;
    pl("  bindtextdomain(\"bd3\", \"\")", bindtextdomain("bd3", ""));
    printf("  errno %s\n", en(errno));
    pl("  lookup bd3 with cwd <dir>", G("bd3", "where"));

    printf("-- spelling of the directory\n");
    where_mo(P("b4/de_DE.UTF-8/LC_MESSAGES/bd4.mo"), "b4");
    pl("  bindtextdomain(\"bd4\", <dir>/b4/)", bindtextdomain("bd4", P("b4/")));
    pl("  lookup", G("bd4", "where"));
    where_mo(P("b5/de_DE.UTF-8/LC_MESSAGES/bd5.mo"), "b5");
    char dbl[PATH_MAX];
    snprintf(dbl, sizeof dbl, "%s//b5/.", dir);
    pl("  bindtextdomain(\"bd5\", <dir>//b5/.)", bindtextdomain("bd5", dbl));
    pl("  lookup", G("bd5", "where"));
    static char longdir[5001];
    memset(longdir, 'd', 5000);
    longdir[0] = '/';
    r = bindtextdomain("bd-long", longdir);
    printf("  bindtextdomain with a 5000-byte directory: %s, length %zu\n", r ? "non-NULL" : "NULL", r ? strlen(r) : 0);
    pl("  lookup", G("bd-long", "where"));

    printf("-- rebinding after a catalog was loaded\n");
    where_mo(P("b6a/de_DE.UTF-8/LC_MESSAGES/bd6.mo"), "A");
    where_mo(P("b6b/de_DE.UTF-8/LC_MESSAGES/bd6.mo"), "B");
    bindtextdomain("bd6", P("b6a"));
    pl("  bound to b6a", G("bd6", "where"));
    bindtextdomain("bd6", P("b6b"));
    pl("  rebound to b6b", G("bd6", "where"));
    bindtextdomain("bd6", P("b6a"));
    pl("  rebound to b6a", G("bd6", "where"));

    printf("-- catalog file replaced on disk after it was loaded\n");
    where_mo(P("b7/de_DE.UTF-8/LC_MESSAGES/bd7.mo"), "old");
    bindtextdomain("bd7", P("b7"));
    pl("  first lookup", G("bd7", "where"));
    where_mo(P("b7/de_DE.UTF-8/LC_MESSAGES/bd7.mo"), "new");
    /* Liella checks a catalog's file at most once per second (owner ruling 2026-10-05), so a
     * lookup right after the replacement may see either file: only the one 1.1 s later is
     * printed. */
    struct timespec pause = {1, 100000000};
    nanosleep(&pause, NULL);
    pl("  lookup 1.1 s after the file was replaced", G("bd7", "where"));
    bindtextdomain("bd7", P("b7"));
    pl("  after bindtextdomain with the same directory", G("bd7", "where"));
    char same[PATH_MAX];
    snprintf(same, sizeof same, "%s/b7/", dir);
    bindtextdomain("bd7", same);
    pl("  after bindtextdomain with the directory spelled <dir>/b7/", G("bd7", "where"));
    setlocale(LC_ALL, "de_DE.UTF-8");
    pl("  after setlocale(LC_ALL, same name)", G("bd7", "where"));
    setlocale(LC_ALL, "C");
    setlocale(LC_ALL, "de_DE.UTF-8");
    pl("  after setlocale C then back", G("bd7", "where"));
    printf("_nl_domain_bindings NULL: %s\n", _nl_domain_bindings ? "no" : "yes");
}

/* ================================================================ codesets */

#define UT_EACUTE "\xc3\xa9"
#define UT_EURO "\xe2\x82\xac"

static const struct ent conv_ents[] = {
    {"", 0, HDR_UTF8 "Plural-Forms: nplurals=2; plural=n != 1;\n", sizeof(HDR_UTF8 "Plural-Forms: nplurals=2; plural=n != 1;\n") - 1},
    E("ascii", "plain"),
    E("cyr", "\xd0\x96\xd0\xb0"),
    E("e-acute", UT_EACUTE),
    E("euro", UT_EURO),
    E("file\0files", "F" UT_EACUTE "\0Fs" UT_EACUTE),
    E("invalid", "a\xff\xfe" "b"),
    E("kana", "\xe3\x81\x82"),
    E("mixed", "na\xc3\xafve \xe2\x80\x93 " UT_EURO "5"),
};
static const char *conv_ids[] = {"ascii", "e-acute", "euro", "mixed", "cyr", "kana", "invalid", NULL};

static void conv_print(const char *dom)
{
    for (int i = 0; conv_ids[i]; i++) {
        printf("  %s ", conv_ids[i]);
        errno = EINTR;
        pz(G(dom, conv_ids[i]));
        printf(" errno %s\n", en(errno));
    }
    printf("  file n=2 ");
    pz(dcngettext(dom, "file", "files", 2, LC_MESSAGES));
    putchar('\n');
}

struct cvcase {
    const char *title;
    const char *codeset; /* NULL: no bind_textdomain_codeset */
};

static void s_codeset_one(void *a)
{
    const struct cvcase *c = a;
    printf("-- UTF-8 catalog, de_DE.UTF-8 locale, codeset ");
    pz(c->codeset);
    putchar('\n');
    if (!need_locale(LC_ALL, "de_DE.UTF-8"))
        return;
    write_mo(P("cv/de_DE.UTF-8/LC_MESSAGES/conv.mo"), conv_ents, (int)(sizeof conv_ents / sizeof conv_ents[0]));
    bindtextdomain("conv", P("cv"));
    if (c->codeset) {
        errno = 0;
        pl("  bind_textdomain_codeset returns", bind_textdomain_codeset("conv", c->codeset));
        printf("  errno %s\n", en(errno));
        pl("  bind_textdomain_codeset(NULL) returns", bind_textdomain_codeset("conv", NULL));
    }
    conv_print("conv");
}

struct lccase {
    const char *title;
    const char *messages; /* LC_MESSAGES locale (catalog directory) */
    const char *ctype;    /* LC_CTYPE locale */
    const char *hdr;      /* header entry (NULL: none) */
    const char *id;       /* translated id */
    const char *val;      /* its translation */
    const char *codeset;  /* bind_textdomain_codeset, or NULL */
};

static void s_codeset_locale(void *a)
{
    const struct lccase *c = a;
    printf("-- %s\n", c->title);
    if (!need_locale(LC_ALL, c->messages) || !need_locale(LC_CTYPE, c->ctype))
        return;
    if (c->hdr == NULL && c->val == NULL) {
        write_mo(P("lc/%s/LC_MESSAGES/conv.mo", c->messages), conv_ents, (int)(sizeof conv_ents / sizeof conv_ents[0]));
        bindtextdomain("conv", P("lc"));
        if (c->codeset)
            bind_textdomain_codeset("conv", c->codeset);
        conv_print("conv");
        return;
    }
    struct ent e[2];
    int n = 0;
    if (c->hdr)
        e[n++] = (struct ent){"", 0, c->hdr, strlen(c->hdr)};
    e[n++] = (struct ent){c->id, strlen(c->id), c->val, strlen(c->val)};
    write_mo(P("lc/%s/LC_MESSAGES/one.mo", c->messages), e, n);
    bindtextdomain("one", P("lc"));
    if (c->codeset)
        bind_textdomain_codeset("one", c->codeset);
    errno = EINTR;
    printf("  %s ", c->id);
    pz(G("one", c->id));
    printf(" errno %s\n", en(errno));
}

static void s_codeset_misc(void *u)
{
    (void)u;
    printf("-- argument checks\n");
    errno = 0;
    pl("  bind_textdomain_codeset(NULL, \"UTF-8\")", bind_textdomain_codeset(NULL, "UTF-8"));
    printf("  errno %s\n", en(errno));
    errno = 0;
    pl("  bind_textdomain_codeset(\"\", \"UTF-8\")", bind_textdomain_codeset("", "UTF-8"));
    printf("  errno %s\n", en(errno));
    errno = 0;
    pl("  bind_textdomain_codeset(\"cs1\", NULL) never set", bind_textdomain_codeset("cs1", NULL));
    printf("  errno %s\n", en(errno));
    char cs[16] = "ISO-8859-1";
    char *r = bind_textdomain_codeset("cs1", cs);
    pl("  bind_textdomain_codeset(\"cs1\", \"ISO-8859-1\")", r);
    printf("  result is the argument: %s\n", r == cs ? "yes" : "no");
    strcpy(cs, "XXX");
    pl("  query after the argument was overwritten", bind_textdomain_codeset("cs1", NULL));
    pl("  bindtextdomain(\"cs1\", NULL) (only a codeset was set)", bindtextdomain("cs1", NULL));
    pl("  bind_textdomain_codeset(\"cs1\", \"\")", bind_textdomain_codeset("cs1", ""));
    pl("  query", bind_textdomain_codeset("cs1", NULL));

    printf("-- codeset changed after a lookup (same domain)\n");
    if (!need_locale(LC_ALL, "de_DE.UTF-8"))
        return;
    write_mo(P("cm/de_DE.UTF-8/LC_MESSAGES/convm.mo"), conv_ents, (int)(sizeof conv_ents / sizeof conv_ents[0]));
    bindtextdomain("convm", P("cm"));
    const char *p1 = G("convm", "e-acute");
    pl("  default", p1);
    const char *p1b = G("convm", "e-acute");
    printf("  second lookup returns the same pointer: %s\n", p1 == p1b ? "yes" : "no");
    bind_textdomain_codeset("convm", "ISO-8859-1");
    const char *p2 = G("convm", "e-acute");
    pl("  after ISO-8859-1", p2);
    pl("  first result still reads", p1);
    bind_textdomain_codeset("convm", "UTF-8");
    pl("  after UTF-8", G("convm", "e-acute"));
    printf("-- OUTPUT_CHARSET environment variable (set to ISO-8859-1) before the first lookup\n");
    setenv("OUTPUT_CHARSET", "ISO-8859-1", 1);
    write_mo(P("cm/de_DE.UTF-8/LC_MESSAGES/convo.mo"), conv_ents, (int)(sizeof conv_ents / sizeof conv_ents[0]));
    bindtextdomain("convo", P("cm"));
    pl("  e-acute", G("convo", "e-acute"));
    unsetenv("OUTPUT_CHARSET");
}

static void s_codesets(void)
{
    printf("== bind_textdomain_codeset and conversion\n");
    child(s_codeset_misc, NULL, 20, 0);
    static const struct cvcase cv[] = {
        {"none", NULL},
        {"", "UTF-8"},
        {"", "utf8"},
        {"", "ISO-8859-1"},
        {"", "iso-8859-1"},
        {"", "ISO-8859-15"},
        {"", "ASCII"},
        {"", "ISO-8859-1//TRANSLIT"},
        {"", "ASCII//TRANSLIT"},
        {"", "ISO-8859-1//IGNORE"},
        {"", "KOI8-R"},
        {"", "EUC-JP"},
        {"", "UTF-16LE"},
        {"", "NOPE"},
        {"", ""},
    };
    for (size_t i = 0; i < sizeof cv / sizeof cv[0]; i++)
        child(s_codeset_one, (void *)&cv[i], 20, 0);
    static const struct lccase lc[] = {
        {"UTF-8 catalog, fr_FR.ISO-8859-1 locale (no codeset bound)", "fr_FR.ISO-8859-1", "fr_FR.ISO-8859-1", NULL, NULL, NULL, NULL},
        {"UTF-8 catalog, LC_MESSAGES de_DE.UTF-8, LC_CTYPE C", "de_DE.UTF-8", "C", NULL, NULL, NULL, NULL},
        {"UTF-8 catalog, LC_MESSAGES de_DE.UTF-8, LC_CTYPE fr_FR.ISO-8859-1", "de_DE.UTF-8", "fr_FR.ISO-8859-1", NULL, NULL, NULL, NULL},
        {"UTF-8 catalog, LC_MESSAGES de_DE.UTF-8, LC_CTYPE C.UTF-8", "de_DE.UTF-8", "C.UTF-8", NULL, NULL, NULL, NULL},
        {"UTF-8 catalog, ru_RU.KOI8-R locale", "ru_RU.KOI8-R", "ru_RU.KOI8-R", NULL, NULL, NULL, NULL},
        {"UTF-8 catalog, ja_JP.EUC-JP locale", "ja_JP.EUC-JP", "ja_JP.EUC-JP", NULL, NULL, NULL, NULL},
        {"UTF-8 catalog, fr_FR.ISO-8859-1 locale, codeset UTF-8 bound", "fr_FR.ISO-8859-1", "fr_FR.ISO-8859-1", NULL, NULL, NULL, "UTF-8"},
        {"ISO-8859-1 catalog in a UTF-8 locale", "de_DE.UTF-8", "de_DE.UTF-8", "Content-Type: text/plain; charset=ISO-8859-1\n", "latin", "\xe9t\xe9", NULL},
        {"ISO-8859-1 catalog in a UTF-8 locale, codeset ISO-8859-1 bound", "de_DE.UTF-8", "de_DE.UTF-8", "Content-Type: text/plain; charset=ISO-8859-1\n", "latin", "\xe9t\xe9", "ISO-8859-1"},
        {"ISO-8859-1 catalog in an ISO-8859-1 locale", "fr_FR.ISO-8859-1", "fr_FR.ISO-8859-1", "Content-Type: text/plain; charset=ISO-8859-1\n", "latin", "\xe9t\xe9", NULL},
        {"no header entry, UTF-8 bytes, ISO-8859-1 locale", "fr_FR.ISO-8859-1", "fr_FR.ISO-8859-1", NULL, "raw", UT_EACUTE, NULL},
        {"no header entry, ISO-8859-1 bytes, UTF-8 locale", "de_DE.UTF-8", "de_DE.UTF-8", NULL, "raw", "\xe9", NULL},
        {"no header entry, UTF-8 bytes, ISO-8859-1 locale, codeset UTF-8 bound", "fr_FR.ISO-8859-1", "fr_FR.ISO-8859-1", NULL, "raw", UT_EACUTE, "UTF-8"},
        {"header without a charset, UTF-8 bytes, ISO-8859-1 locale", "fr_FR.ISO-8859-1", "fr_FR.ISO-8859-1", "Content-Type: text/plain\n", "raw", UT_EACUTE, NULL},
        {"charset=CHARSET, UTF-8 bytes, ISO-8859-1 locale", "fr_FR.ISO-8859-1", "fr_FR.ISO-8859-1", "Content-Type: text/plain; charset=CHARSET\n", "raw", UT_EACUTE, NULL},
        {"charset=NOPE, UTF-8 bytes, ISO-8859-1 locale", "fr_FR.ISO-8859-1", "fr_FR.ISO-8859-1", "Content-Type: text/plain; charset=NOPE\n", "raw", UT_EACUTE, NULL},
        {"charset=NOPE, UTF-8 bytes, UTF-8 locale", "de_DE.UTF-8", "de_DE.UTF-8", "Content-Type: text/plain; charset=NOPE\n", "raw", UT_EACUTE, NULL},
        {"charset=utf-8 (lowercase), ISO-8859-1 locale", "fr_FR.ISO-8859-1", "fr_FR.ISO-8859-1", "Content-Type: text/plain; charset=utf-8\n", "raw", UT_EACUTE, NULL},
        {"charset given on a later header line", "fr_FR.ISO-8859-1", "fr_FR.ISO-8859-1", "Project-Id-Version: x\nContent-Type: text/plain; charset=UTF-8\n", "raw", UT_EACUTE, NULL},
        {"charset without Content-Type", "fr_FR.ISO-8859-1", "fr_FR.ISO-8859-1", "X-Note: charset=UTF-8\n", "raw", UT_EACUTE, NULL},
        {"charset followed by a space and more text", "fr_FR.ISO-8859-1", "fr_FR.ISO-8859-1", "Content-Type: text/plain; charset=UTF-8 extra\n", "raw", UT_EACUTE, NULL},
    };
    for (size_t i = 0; i < sizeof lc / sizeof lc[0]; i++)
        child(s_codeset_locale, (void *)&lc[i], 20, 0);
}

/* ================================================================ locale selection */

struct selcase {
    const char *title;
    const char *lc_all, *lc_messages, *lang, *language;
    int mode; /* 0 setlocale(LC_ALL, ""), 1 none, 2 setlocale(LC_MESSAGES, "") only,
                 3 setlocale(LC_ALL, "") then LC_MESSAGES "C", 4 setlocale(LC_ALL, "") then LC_CTYPE "C" */
};

static void sel_print(void)
{
    printf("  LC_MESSAGES ");
    pz(setlocale(LC_MESSAGES, NULL));
    printf(" where ");
    pz(G("sel", "where"));
    printf(" only-nl ");
    pz(G("sel", "only-nl"));
    putchar('\n');
}

static void s_sel_one(void *a)
{
    const struct selcase *c = a;
    printf("-- %s\n", c->title);
    clear_env();
    setenv_or_unset("LC_ALL", c->lc_all);
    setenv_or_unset("LC_MESSAGES", c->lc_messages);
    setenv_or_unset("LANG", c->lang);
    setenv_or_unset("LANGUAGE", c->language);
    const char *r = NULL;
    switch (c->mode) {
    case 0: r = setlocale(LC_ALL, ""); break;
    case 2: r = setlocale(LC_MESSAGES, ""); break;
    case 3: r = setlocale(LC_ALL, ""); setlocale(LC_MESSAGES, "C"); break;
    case 4: r = setlocale(LC_ALL, ""); setlocale(LC_CTYPE, "C"); break;
    }
    printf("  setlocale %s\n", c->mode == 1 ? "not called" : r ? "ok" : "NULL");
    bindtextdomain("sel", P("sel"));
    sel_print();
}

static void s_sel_runtime(void *u)
{
    (void)u;
    clear_env();
    bindtextdomain("sel", P("sel"));
    printf("-- LANGUAGE changed at run time (LANG=de_DE.UTF-8, same domain)\n");
    setenv("LANG", "de_DE.UTF-8", 1);
    setenv("LANGUAGE", "it", 1);
    setlocale(LC_ALL, "");
    pl("  LANGUAGE=it", G("sel", "where"));
    setenv("LANGUAGE", "nl", 1);
    pl("  LANGUAGE=nl, nothing else", G("sel", "where"));
    pl("  another msgid only-nl", G("sel", "only-nl"));
    textdomain(textdomain(NULL));
    pl("  after textdomain(textdomain(NULL))", G("sel", "where"));
    setlocale(LC_MESSAGES, setlocale(LC_MESSAGES, NULL));
    pl("  after setlocale(LC_MESSAGES, current)", G("sel", "where"));
    unsetenv("LANGUAGE");
    pl("  LANGUAGE unset", G("sel", "where"));
    setenv("LANGUAGE", "it", 1);
    pl("  LANGUAGE=it again", G("sel", "where"));
    printf("-- environment changed after setlocale, no new setlocale\n");
    unsetenv("LANGUAGE");
    setenv("LC_MESSAGES", "fr_FR.UTF-8", 1);
    setenv("LANG", "fr_FR.UTF-8", 1);
    pl("  LC_MESSAGES/LANG now fr_FR.UTF-8", G("sel", "where"));
    printf("-- setlocale changed at run time\n");
    setlocale(LC_ALL, "fr_FR.UTF-8");
    pl("  setlocale(LC_ALL, fr_FR.UTF-8)", G("sel", "where"));
    setlocale(LC_MESSAGES, "cs_CZ.UTF-8");
    pl("  setlocale(LC_MESSAGES, cs_CZ.UTF-8)", G("sel", "where"));
    setlocale(LC_MESSAGES, "C");
    pl("  setlocale(LC_MESSAGES, C)", G("sel", "where"));
    printf("-- uselocale (global LC_MESSAGES de_DE.UTF-8)\n");
    setlocale(LC_ALL, "de_DE.UTF-8");
    locale_t fr = newlocale(LC_MESSAGES_MASK, "fr_FR.UTF-8", (locale_t)0);
    if (!fr) {
        printf("  newlocale failed\n");
        return;
    }
    pl("  global", G("sel", "where"));
    locale_t old = uselocale(fr);
    pl("  thread locale LC_MESSAGES fr_FR.UTF-8", G("sel", "where"));
    locale_t cl = newlocale(LC_ALL_MASK, "C", (locale_t)0);
    uselocale(cl);
    setenv("LANGUAGE", "it", 1);
    pl("  thread locale C, LANGUAGE=it", G("sel", "where"));
    unsetenv("LANGUAGE");
    uselocale(old);
    pl("  back to the global locale", G("sel", "where"));
    uselocale(LC_GLOBAL_LOCALE);
    freelocale(fr);
    freelocale(cl);
}

static void s_selection(void)
{
    printf("== locale selection (catalogs: de_DE.UTF-8 fr_FR.UTF-8 cs_CZ.UTF-8 C POSIX C.UTF-8 it nl, bad = corrupt)\n");
    static const char *dirs[] = {"de_DE.UTF-8", "fr_FR.UTF-8", "cs_CZ.UTF-8", "C", "POSIX", "C.UTF-8", "it", "nl"};
    for (size_t i = 0; i < sizeof dirs / sizeof dirs[0]; i++) {
        if (strcmp(dirs[i], "nl") == 0) {
            struct ent e[3] = {{"", 0, HDR_UTF8, strlen(HDR_UTF8)}, E("only-nl", "nl-only"), {"where", 5, "nl", 2}};
            write_mo(P("sel/nl/LC_MESSAGES/sel.mo"), e, 3);
        } else {
            where_mo(P("sel/%s/LC_MESSAGES/sel.mo", dirs[i]), dirs[i]);
        }
    }
    wfile(P("sel/bad/LC_MESSAGES/sel.mo"), "not a catalog at all, just text\n", 32);
    static const struct selcase c[] = {
        {"no setlocale call, LANG=de_DE.UTF-8 LANGUAGE=it", NULL, NULL, "de_DE.UTF-8", "it", 1},
        {"no setlocale call, LC_ALL=de_DE.UTF-8", "de_DE.UTF-8", NULL, NULL, NULL, 1},
        {"LANG=de_DE.UTF-8", NULL, NULL, "de_DE.UTF-8", NULL, 0},
        {"LANG=de_DE.UTF-8 LC_MESSAGES=fr_FR.UTF-8", NULL, "fr_FR.UTF-8", "de_DE.UTF-8", NULL, 0},
        {"LANG=de LC_MESSAGES=fr LC_ALL=cs", "cs_CZ.UTF-8", "fr_FR.UTF-8", "de_DE.UTF-8", NULL, 0},
        {"LC_ALL empty, LC_MESSAGES=fr, LANG=de", "", "fr_FR.UTF-8", "de_DE.UTF-8", NULL, 0},
        {"LC_ALL and LC_MESSAGES empty, LANG=de", "", "", "de_DE.UTF-8", NULL, 0},
        {"LC_MESSAGES=xx_XX.UTF-8 (not installed), LANG=de", NULL, "xx_XX.UTF-8", "de_DE.UTF-8", NULL, 0},
        {"only setlocale(LC_MESSAGES, \"\"), LC_ALL=de_DE.UTF-8", "de_DE.UTF-8", NULL, NULL, NULL, 2},
        {"LANG=C LANGUAGE=it", NULL, NULL, "C", "it", 0},
        {"LANG=POSIX LANGUAGE=it", NULL, NULL, "POSIX", "it", 0},
        {"LANG=C.UTF-8 LANGUAGE=it", NULL, NULL, "C.UTF-8", "it", 0},
        {"LANG unset LANGUAGE=it", NULL, NULL, NULL, "it", 0},
        {"LANG=de LANGUAGE=it", NULL, NULL, "de_DE.UTF-8", "it", 0},
        {"LANG=de LANGUAGE empty", NULL, NULL, "de_DE.UTF-8", "", 0},
        {"LANG=de LANGUAGE=zz (no catalog)", NULL, NULL, "de_DE.UTF-8", "zz", 0},
        {"LANG=de LANGUAGE=zz:it", NULL, NULL, "de_DE.UTF-8", "zz:it", 0},
        {"LANG=de LANGUAGE=:::it:", NULL, NULL, "de_DE.UTF-8", ":::it:", 0},
        {"LANG=de LANGUAGE=it:nl (per-message fall-through)", NULL, NULL, "de_DE.UTF-8", "it:nl", 0},
        {"LANG=de LANGUAGE=nl:it", NULL, NULL, "de_DE.UTF-8", "nl:it", 0},
        {"LANG=de LANGUAGE=C", NULL, NULL, "de_DE.UTF-8", "C", 0},
        {"LANG=de LANGUAGE=C:it", NULL, NULL, "de_DE.UTF-8", "C:it", 0},
        {"LANG=de LANGUAGE=it:C", NULL, NULL, "de_DE.UTF-8", "it:C", 0},
        {"LANG=de LANGUAGE=POSIX:it", NULL, NULL, "de_DE.UTF-8", "POSIX:it", 0},
        {"LANG=de LANGUAGE=bad:it (corrupt catalog first)", NULL, NULL, "de_DE.UTF-8", "bad:it", 0},
        {"LANG=de LANGUAGE=de_DE.UTF-8", NULL, NULL, "de_DE.UTF-8", "de_DE.UTF-8", 0},
        {"LANG=de LANGUAGE=it, then LC_MESSAGES set to C", NULL, NULL, "de_DE.UTF-8", "it", 3},
        {"LANG=de LANGUAGE=it, then LC_CTYPE set to C", NULL, NULL, "de_DE.UTF-8", "it", 4},
        {"LANG=fr LANGUAGE=../sel/it (a path in LANGUAGE)", NULL, NULL, "fr_FR.UTF-8", "../sel/it", 0},
        {"LANG=fr LANGUAGE=/it", NULL, NULL, "fr_FR.UTF-8", "/it", 0},
    };
    for (size_t i = 0; i < sizeof c / sizeof c[0]; i++)
        child(s_sel_one, (void *)&c[i], 20, 0);
    child(s_sel_runtime, NULL, 20, 0);
}

/* ================================================================ variant order */

struct vcase {
    const char *title;
    const char *msgloc;   /* setlocale(LC_ALL) name; NULL: hu_HU.UTF-8 */
    const char *language; /* LANGUAGE or NULL */
    const char *c[26];
};

static const char *vlabel(const char *c)
{
    return *c ? c : "(empty)";
}

static void s_vorder(void *a)
{
    const struct vcase *v = a;
    printf("-- %s\n", v->title);
    clear_env();
    const char *ml = v->msgloc ? v->msgloc : "hu_HU.UTF-8";
    if (!need_locale(LC_ALL, ml))
        return;
    setenv_or_unset("LANGUAGE", v->language);
    int n = 0;
    while (v->c[n])
        n++;
    int on[26];
    for (int i = 0; i < n; i++)
        on[i] = 1;
    char base[PATH_MAX * 2];
    snprintf(base, sizeof base, "%s", P("vo%d", (int)getpid()));
    printf("  order:");
    int found = 0;
    for (int it = 0; it <= n; it++) {
        char dom[16];
        snprintf(dom, sizeof dom, "v%02d", it);
        for (int i = 0; i < n; i++)
            if (on[i])
                where_mo(P("vo%d/%s/LC_MESSAGES/%s.mo", (int)getpid(), v->c[i], dom), vlabel(v->c[i]));
        bindtextdomain(dom, base);
        const char *r = G(dom, "where");
        if (strcmp(r, "where") == 0)
            break;
        int k = -1;
        for (int i = 0; i < n; i++)
            if (on[i] && strcmp(vlabel(v->c[i]), r) == 0)
                k = i;
        if (k < 0) {
            printf(" > ?");
            pz(r);
            break;
        }
        printf("%s %s", found ? " >" : "", vlabel(v->c[k]));
        found++;
        on[k] = 0;
    }
    if (!found)
        printf(" (none)");
    printf("\n  never:");
    int any = 0;
    for (int i = 0; i < n; i++)
        if (on[i]) {
            printf(" %s", vlabel(v->c[i]));
            any = 1;
        }
    if (!any)
        printf(" -");
    putchar('\n');
}

static void s_variants(void)
{
    printf("== locale name variants (catalog directories found, most preferred first; LC_ALL hu_HU.UTF-8 unless named)\n");
    static const struct vcase v[] = {
        {"LANGUAGE=xx_YY.ISO-8859-1@mod", NULL, "xx_YY.ISO-8859-1@mod",
         {"xx_YY.ISO-8859-1@mod", "xx_YY.iso88591@mod", "xx_YY@mod", "xx.ISO-8859-1@mod", "xx.iso88591@mod", "xx@mod",
          "xx_YY.ISO-8859-1", "xx_YY.iso88591", "xx_YY", "xx.ISO-8859-1", "xx.iso88591", "xx",
          "xx_YY.ISO8859-1", "xx_YY.iso-8859-1", "xx_YY.ISO-8859-1@MOD", "XX", "xx_ZZ", "yy", "mod", "xx_YY.88591",
          "xx_YY.iso88591@MOD", "hu", "hu_HU.UTF-8", NULL}},
        {"LANGUAGE=xx_YY.UTF-8", NULL, "xx_YY.UTF-8",
         {"xx_YY.UTF-8", "xx_YY.utf8", "xx_YY", "xx.UTF-8", "xx.utf8", "xx", "xx_YY.UTF8", "xx_YY.utf-8",
          "xx_YY.UTF-8@mod", "xx@mod", NULL}},
        {"LANGUAGE=xx_YY.UTF-8@mod", NULL, "xx_YY.UTF-8@mod",
         {"xx_YY.UTF-8@mod", "xx_YY.utf8@mod", "xx_YY@mod", "xx.UTF-8@mod", "xx.utf8@mod", "xx@mod",
          "xx_YY.UTF-8", "xx_YY.utf8", "xx_YY", "xx.UTF-8", "xx.utf8", "xx", NULL}},
        {"LANGUAGE=xx@mod", NULL, "xx@mod", {"xx@mod", "xx", "xx_YY@mod", "xx.utf8@mod", "xx_YY", NULL}},
        {"LANGUAGE=xx_YY", NULL, "xx_YY", {"xx_YY", "xx", "xx_YY.UTF-8", "xx_YY.utf8", "xx@mod", "xx_ZZ", NULL}},
        {"LANGUAGE=xx.utf8", NULL, "xx.utf8", {"xx.utf8", "xx.UTF-8", "xx", "xx_YY.utf8", NULL}},
        {"LANGUAGE=xx_YY.utf-8", NULL, "xx_YY.utf-8", {"xx_YY.utf-8", "xx_YY.utf8", "xx_YY.UTF-8", "xx_YY", "xx.utf-8", "xx.utf8", "xx", NULL}},
        {"LANGUAGE=xx_YY.8859-1 (codeset of digits)", NULL, "xx_YY.8859-1",
         {"xx_YY.8859-1", "xx_YY.iso88591", "xx_YY.88591", "xx_YY", "xx.8859-1", "xx.iso88591", "xx.88591", "xx", NULL}},
        {"LANGUAGE=xx_YY.EUC-JP", NULL, "xx_YY.EUC-JP",
         {"xx_YY.EUC-JP", "xx_YY.eucjp", "xx_YY.euc-jp", "xx_YY", "xx.EUC-JP", "xx.eucjp", "xx", NULL}},
        {"LANGUAGE=xx_YY.@mod (empty codeset)", NULL, "xx_YY.@mod",
         {"xx_YY.@mod", "xx_YY@mod", "xx_YY.", "xx_YY", "xx.@mod", "xx@mod", "xx.", "xx", NULL}},
        {"LANGUAGE=_YY (empty language)", NULL, "_YY", {"_YY", "", "YY", NULL}},
        {"LANGUAGE=xx_YY_ZZ", NULL, "xx_YY_ZZ", {"xx_YY_ZZ", "xx_YY", "xx", NULL}},
        {"LANGUAGE=german (an alias)", NULL, "german",
         {"german", "de_DE.ISO-8859-1", "de_DE.iso88591", "de_DE", "de.ISO-8859-1", "de.iso88591", "de", NULL}},
        {"LANGUAGE=GERMAN", NULL, "GERMAN",
         {"GERMAN", "german", "de_DE.ISO-8859-1", "de_DE.iso88591", "de_DE", "de.ISO-8859-1", "de.iso88591", "de", NULL}},
        {"LC_ALL=de_DE.UTF-8@euro, no LANGUAGE", "de_DE.UTF-8@euro", NULL,
         {"de_DE.UTF-8@euro", "de_DE.utf8@euro", "de_DE@euro", "de.UTF-8@euro", "de.utf8@euro", "de@euro",
          "de_DE.UTF-8", "de_DE.utf8", "de_DE", "de.UTF-8", "de.utf8", "de", "de_DE.UTF-8@EURO", "DE", NULL}},
        {"LC_ALL=fr_FR.ISO-8859-1, no LANGUAGE", "fr_FR.ISO-8859-1", NULL,
         {"fr_FR.ISO-8859-1", "fr_FR.iso88591", "fr_FR", "fr.ISO-8859-1", "fr.iso88591", "fr", "fr_FR.UTF-8", "fr_FR.utf8", NULL}},
        {"LC_ALL=de_DE.UTF-8 with LANGUAGE=xx (the locale is not a fall-back)", "de_DE.UTF-8", "xx",
         {"de_DE.UTF-8", "de", "xx_XX", NULL}},
    };
    for (size_t i = 0; i < sizeof v / sizeof v[0]; i++)
        child(s_vorder, (void *)&v[i], 60, 0);
}

/* ================================================================ categories */

static const struct {
    int c;
    const char *n;
} cats[] = {
    {LC_CTYPE, "LC_CTYPE"}, {LC_NUMERIC, "LC_NUMERIC"}, {LC_TIME, "LC_TIME"}, {LC_COLLATE, "LC_COLLATE"},
    {LC_MONETARY, "LC_MONETARY"}, {LC_MESSAGES, "LC_MESSAGES"}, {LC_PAPER, "LC_PAPER"}, {LC_NAME, "LC_NAME"},
    {LC_ADDRESS, "LC_ADDRESS"}, {LC_TELEPHONE, "LC_TELEPHONE"}, {LC_MEASUREMENT, "LC_MEASUREMENT"},
    {LC_IDENTIFICATION, "LC_IDENTIFICATION"},
};

static int cat_value;

static void s_cat_call(void *a)
{
    const char *name = a;
    errno = EINTR;
    const char *r = dcgettext("catd", "where", cat_value);
    printf("  %s (%d): ", name, cat_value);
    pz(r);
    printf(" errno %s", en(errno));
    static const char s1[] = "where", s2[] = "wheres";
    const char *q = dcngettext("catd", s1, s2, 2, cat_value);
    printf("; dcngettext n=2 ");
    if (q == s2)
        printf("(msgid_plural)");
    else
        pz(q);
    putchar('\n');
}

static void cat_loop(void)
{
    char nm[32];
    for (size_t i = 0; i < sizeof cats / sizeof cats[0]; i++) {
        cat_value = cats[i].c;
        child(s_cat_call, (void *)cats[i].n, 10, 0);
    }
    static const struct {
        int v;
        const char *n;
    } odd[] = {{LC_ALL, "LC_ALL"}, {-1, "-1"}, {13, "13"}, {99, "99"}, {INT_MAX, "INT_MAX"}, {INT_MIN, "INT_MIN"}};
    for (size_t i = 0; i < sizeof odd / sizeof odd[0]; i++) {
        cat_value = odd[i].v;
        snprintf(nm, sizeof nm, "%s", odd[i].n);
        child(s_cat_call, nm, 10, 0);
    }
}

static void s_categories_body(void *u)
{
    (void)u;
    clear_env();
    if (!need_locale(LC_ALL, "cs_CZ.UTF-8") || !need_locale(LC_MESSAGES, "de_DE.UTF-8") || !need_locale(LC_TIME, "fr_FR.UTF-8"))
        return;
    bindtextdomain("catd", P("cat"));
    printf("-- LC_ALL cs_CZ.UTF-8, LC_MESSAGES de_DE.UTF-8, LC_TIME fr_FR.UTF-8\n");
    cat_loop();
    printf("-- the same with LANGUAGE=it\n");
    setenv("LANGUAGE", "it", 1);
    cat_loop();
    printf("-- LC_MESSAGES C, LC_TIME fr_FR.UTF-8, LANGUAGE=it\n");
    setlocale(LC_MESSAGES, "C");
    cat_loop();
}

static void s_categories(void)
{
    printf("== dcgettext categories (catalog <locale>/<category>/catd.mo says which)\n");
    static const char *locs[] = {"cs_CZ.UTF-8", "de_DE.UTF-8", "fr_FR.UTF-8", "it", "C"};
    char label[64];
    for (size_t l = 0; l < sizeof locs / sizeof locs[0]; l++) {
        for (size_t i = 0; i < sizeof cats / sizeof cats[0]; i++) {
            snprintf(label, sizeof label, "%s/%s", locs[l], cats[i].n);
            where_mo(P("cat/%s/%s/catd.mo", locs[l], cats[i].n), label);
        }
        snprintf(label, sizeof label, "%s/LC_ALL", locs[l]);
        where_mo(P("cat/%s/LC_ALL/catd.mo", locs[l]), label);
    }
    child(s_categories_body, NULL, 60, 0);
}

/* ================================================================ plural forms */

struct pcase {
    const char *title;
    const char *pf;  /* Plural-Forms value; "-": header without it; NULL: no header entry */
    int raw;         /* pf is the whole header */
    int k;           /* number of translated forms */
};

static const unsigned long nvals[] = {0, 1, 2, 3, 4, 5, 7, 10, 11, 12, 14, 19, 21, 22, 25, 100, 101, 102, 103, 111,
                                      1000000, 4294967295UL, 4294967296UL, 4294967297UL, ULONG_MAX};

static void plural_n_line(const char *dom, const char *s1, const char *s2)
{
    printf("  ");
    for (size_t i = 0; i < sizeof nvals / sizeof nvals[0]; i++) {
        const char *r = dcngettext(dom, s1, s2, nvals[i], LC_MESSAGES);
        if (i == 13)
            printf("\n  ");
        printf("%lu:", nvals[i]);
        if (r == s1)
            printf("S");
        else if (r == s2)
            printf("P");
        else
            pz(r);
        putchar(' ');
    }
    putchar('\n');
}

static char hdrbuf[1 << 16];

static void s_plural_one(void *a)
{
    const struct pcase *c = a;
    printf("-- %s\n", c->title);
    clear_env();
    if (!need_locale(LC_ALL, "de_DE.UTF-8"))
        return;
    struct ent e[4];
    int n = 0;
    if (c->pf) {
        if (c->raw)
            snprintf(hdrbuf, sizeof hdrbuf, "%s", c->pf);
        else if (strcmp(c->pf, "-") == 0)
            snprintf(hdrbuf, sizeof hdrbuf, "%s", HDR_UTF8);
        else
            snprintf(hdrbuf, sizeof hdrbuf, "%sPlural-Forms: %s\n", HDR_UTF8, c->pf);
        e[n++] = (struct ent){"", 0, hdrbuf, strlen(hdrbuf)};
    }
    static char forms[64];
    size_t fl = 0;
    for (int i = 0; i < c->k; i++)
        fl += (size_t)sprintf(forms + fl, "f%d", i) + 1;
    e[n++] = (struct ent){"file\0files", 10, forms, fl - 1};
    e[n++] = (struct ent)E("solo", "Solo");
    write_mo(P("pl%d/de_DE.UTF-8/LC_MESSAGES/pl.mo", (int)getpid()), e, n);
    bindtextdomain("pl", P("pl%d", (int)getpid()));
    static const char s1[] = "file", s2[] = "files";
    plural_n_line("pl", s1, s2);
    static const char o1[] = "solo", o2[] = "solos";
    printf("  singular-only entry: n=1 ");
    pz(dcngettext("pl", o1, o2, 1, LC_MESSAGES));
    printf(" n=2 ");
    pz(dcngettext("pl", o1, o2, 2, LC_MESSAGES));
    printf("; gettext(file) ");
    pz(G("pl", "file"));
    printf("; ngettext(file, other, 2) ");
    pz(dcngettext("pl", "file", "other", 2, LC_MESSAGES));
    putchar('\n');
}

static void s_plural_missing(void *u)
{
    (void)u;
    printf("-- no catalog: dcngettext returns which argument\n");
    static const char s1[] = "file", s2[] = "files";
    plural_n_line("no-such-domain", s1, s2);
}

static void s_plurals(void)
{
    printf("== plural forms (S = msgid returned, P = msgid_plural returned)\n");
    static char deep[8200];
    {
        size_t o = (size_t)sprintf(deep, "nplurals=2; plural=");
        for (int i = 0; i < 4000; i++)
            deep[o++] = '(';
        deep[o++] = 'n';
        for (int i = 0; i < 4000; i++)
            deep[o++] = ')';
        strcpy(deep + o, "!=1;");
    }
    static char deep200[600];
    {
        size_t o = (size_t)sprintf(deep200, "nplurals=2; plural=");
        for (int i = 0; i < 200; i++)
            deep200[o++] = '(';
        deep200[o++] = 'n';
        for (int i = 0; i < 200; i++)
            deep200[o++] = ')';
        strcpy(deep200 + o, "!=1;");
    }
    static const char *ru = "nplurals=3; plural=(n%10==1 && n%100!=11 ? 0 : n%10>=2 && n%10<=4 && (n%100<10 || n%100>=20) ? 1 : 2);";
    struct pcase c[] = {
        {"no header entry", NULL, 0, 2},
        {"header without Plural-Forms", "-", 0, 2},
        {"nplurals=2; plural=n != 1;", "nplurals=2; plural=n != 1;", 0, 2},
        {"nplurals=2; plural=n>1; (French)", "nplurals=2; plural=n>1;", 0, 2},
        {"nplurals=1; plural=0;", "nplurals=1; plural=0;", 0, 1},
        {"Russian", ru, 0, 3},
        {"Polish", "nplurals=3; plural=(n==1 ? 0 : n%10>=2 && n%10<=4 && (n%100<10 || n%100>=20) ? 1 : 2);", 0, 3},
        {"Czech", "nplurals=3; plural=(n==1) ? 0 : (n>=2 && n<=4) ? 1 : 2;", 0, 3},
        {"Slovenian", "nplurals=4; plural=(n%100==1 ? 0 : n%100==2 ? 1 : n%100==3 || n%100==4 ? 2 : 3);", 0, 4},
        {"Irish", "nplurals=5; plural=n==1 ? 0 : n==2 ? 1 : n<7 ? 2 : n<11 ? 3 : 4;", 0, 5},
        {"Arabic", "nplurals=6; plural=n==0 ? 0 : n==1 ? 1 : n==2 ? 2 : n%100>=3 && n%100<=10 ? 3 : n%100>=11 ? 4 : 5;", 0, 6},
        {"index beyond nplurals: nplurals=2; plural=n;", "nplurals=2; plural=n;", 0, 2},
        {"fewer forms than nplurals: nplurals=3; plural=n%3;", "nplurals=3; plural=n%3;", 0, 2},
        {"more forms than nplurals: nplurals=2; plural=n%3; (3 forms)", "nplurals=2; plural=n%3;", 0, 3},
        {"syntax error: nplurals=2; plural=n !=;", "nplurals=2; plural=n !=;", 0, 2},
        {"nplurals=x; plural=n!=1;", "nplurals=x; plural=n!=1;", 0, 2},
        {"nplurals=2; (no plural)", "nplurals=2;", 0, 2},
        {"plural=n!=1; (no nplurals)", "plural=n!=1;", 0, 2},
        {"nplurals=0; plural=0;", "nplurals=0; plural=0;", 0, 2},
        {"division by zero: nplurals=2; plural=n/0;", "nplurals=2; plural=n/0;", 0, 2},
        {"remainder by zero: nplurals=2; plural=n%0;", "nplurals=2; plural=n%0;", 0, 2},
        {"division by n: nplurals=3; plural=10/n;", "nplurals=3; plural=10/n;", 0, 3},
        {"subtraction below zero: nplurals=3; plural=n-5;", "nplurals=3; plural=n-5;", 0, 3},
        {"no semicolon after the expression", "nplurals=2; plural=(n==1)?0:1", 0, 2},
        {"spaces: '  nplurals = 2 ;  plural = n == 1 ? 1 : 0 ;'", "  nplurals = 2 ;  plural = n == 1 ? 1 : 0 ;", 0, 2},
        {"precedence: nplurals=4; plural=1+2*n%3;", "nplurals=4; plural=1+2*n%3;", 0, 4},
        {"unary not: nplurals=3; plural=!n+!!n*2;", "nplurals=3; plural=!n+!!n*2;", 0, 3},
        {"unary minus: nplurals=2; plural=-n;", "nplurals=2; plural=-n;", 0, 2},
        {"constant beyond 32 bits: nplurals=2; plural=n>4294967295;", "nplurals=2; plural=n>4294967295;", 0, 2},
        {"nplurals=2; plural=n==4294967297;", "nplurals=2; plural=n==4294967297;", 0, 2},
        {"right-associative ?: nplurals=3; plural=n==1?0:n==2?1:2;", "nplurals=3; plural=n==1?0:n==2?1:2;", 0, 3},
        {"&& and ||: nplurals=2; plural=n<2||n>10&&n<20;", "nplurals=2; plural=n<2||n>10&&n<20;", 0, 2},
        {"leading zero: nplurals=2; plural=n==010;", "nplurals=2; plural=n==010;", 0, 2},
        {"no spaces: nplurals=2;plural=n!=1;", "nplurals=2;plural=n!=1;", 0, 2},
        {"trailing text: nplurals=2; plural=n!=1; extra words", "nplurals=2; plural=n!=1; extra words", 0, 2},
        {"nplurals=1000; plural=n;", "nplurals=1000; plural=n;", 0, 2},
        {"200 nested parentheses", deep200, 0, 2},
        {"4000 nested parentheses", deep, 0, 2},
        {"lowercase header name plural-forms:", HDR_UTF8 "plural-forms: nplurals=2; plural=n>1;\n", 1, 2},
        {"Plural-Forms before Content-Type", "Plural-Forms: nplurals=2; plural=n>1;\n" HDR_UTF8, 1, 2},
        {"Plural-Forms in the middle of another line", HDR_UTF8 "X-Note: Plural-Forms: nplurals=2; plural=n>1;\n", 1, 2},
        {"Plural-Forms: plural before nplurals", HDR_UTF8 "Plural-Forms: plural=n>1; nplurals=2;\n", 1, 2},
    };
    for (size_t i = 0; i < sizeof c / sizeof c[0]; i++)
        child(s_plural_one, &c[i], 10, 0);
    child(s_plural_missing, NULL, 10, 0);
}

/* ================================================================ contexts and special msgids */

static void s_ctx(void *u)
{
    (void)u;
    clear_env();
    if (!need_locale(LC_ALL, "de_DE.UTF-8"))
        return;
    struct ent e[] = {
        {"", 0, HDR_UTF8 "Plural-Forms: nplurals=2; plural=n != 1;\n", sizeof(HDR_UTF8 "Plural-Forms: nplurals=2; plural=n != 1;\n") - 1},
        E("menu\004open", "Menu-Open"),
        E("open", "Open-plain"),
        E("\004bare", "empty-context"),
        E("menu\004", "empty-id-in-context"),
        E("menu\004file\0files", "MF0\0MF1"),
        E("a\004b\004c", "two-separators"),
        E("high\xc3\xa9", "high-bytes"),
        E("empty-translation", ""),
    };
    write_mo(P("ctx/de_DE.UTF-8/LC_MESSAGES/ctx.mo"), e, (int)(sizeof e / sizeof e[0]));
    bindtextdomain("ctx", P("ctx"));
    static const char *ids[] = {"menu\004open", "open", "menu\004missing", "\004bare", "bare", "menu\004", "menu",
                                "menu\004file", "a\004b\004c", "a\004b", "high\xc3\xa9", "Open", "open ", " open",
                                "empty-translation", "\004", NULL};
    for (int i = 0; ids[i]; i++) {
        const char *r = G("ctx", ids[i]);
        printf("  ");
        pz(ids[i]);
        printf(" -> ");
        if (r == ids[i])
            printf("(the argument)");
        else
            pz(r);
        putchar('\n');
    }
    static const char p1[] = "menu\004file", p2[] = "files";
    printf("  dcngettext(menu\\x04file, files) n=1 ");
    pz(dcngettext("ctx", p1, p2, 1, LC_MESSAGES));
    printf(" n=2 ");
    pz(dcngettext("ctx", p1, p2, 2, LC_MESSAGES));
    putchar('\n');
    const char *h = G("ctx", "");
    printf("  gettext(\"\") with a header entry: ");
    pz(h);
    putchar('\n');
    const char *h2 = G("ctx", "");
    printf("  same pointer twice: %s\n", h == h2 ? "yes" : "no");
    struct ent e2[] = {E("x", "y")};
    write_mo(P("ctx/de_DE.UTF-8/LC_MESSAGES/nohdr.mo"), e2, 1);
    bindtextdomain("nohdr", P("ctx"));
    static const char empty[] = "";
    const char *r = G("nohdr", empty);
    printf("  gettext(\"\") without a header entry: ");
    pz(r);
    printf(" (the argument: %s)\n", r == empty ? "yes" : "no");
    r = G("no-such-domain", empty);
    printf("  gettext(\"\") without a catalog: ");
    pz(r);
    printf(" (the argument: %s)\n", r == empty ? "yes" : "no");
    static const char miss[] = "missing";
    printf("  not found returns the argument: %s\n", G("ctx", miss) == miss ? "yes" : "no");
    const char *t1 = G("ctx", "open"), *t2 = G("ctx", "open");
    printf("  found twice, same pointer: %s\n", t1 == t2 ? "yes" : "no");
    static char longid[20001];
    memset(longid, 'k', 20000);
    printf("  20000-byte msgid, not found, returns the argument: %s\n", G("ctx", longid) == longid ? "yes" : "no");
}

/* ================================================================ msgfmt fixtures */

static void mf_show(const char *dom)
{
    static const char s1[] = "file", s2[] = "files";
    printf("  hello ");
    pz(G(dom, "hello"));
    printf(" menu|open ");
    pz(G(dom, "menu\004open"));
    printf(" open ");
    pz(G(dom, "open"));
    printf(" file n=1 ");
    pz(dcngettext(dom, s1, s2, 1, LC_MESSAGES));
    printf(" n=2 ");
    pz(dcngettext(dom, s1, s2, 2, LC_MESSAGES));
    printf(" missing ");
    pz(G(dom, "missing"));
    putchar('\n');
}

static void s_msgfmt(void *u)
{
    (void)u;
    clear_env();
    if (!need_locale(LC_ALL, "de_DE.UTF-8"))
        return;
    static const char *mos[] = {"a.mo", "a-nohash.mo", "a-be.mo", NULL};
    for (int i = 0; mos[i]; i++) {
        char dom[32];
        snprintf(dom, sizeof dom, "mf%d", i);
        copy_fixture(mos[i], P("mf/de_DE.UTF-8/LC_MESSAGES/%s.mo", dom));
        bindtextdomain(dom, P("mf"));
        printf("-- msgfmt %s\n", mos[i]);
        mf_show(dom);
    }
    printf("-- the probe's own writer: the same entries, little- and big-endian\n");
    struct ent e[] = {
        {"", 0, HDR_UTF8 "Plural-Forms: nplurals=2; plural=n != 1;\n", sizeof(HDR_UTF8 "Plural-Forms: nplurals=2; plural=n != 1;\n") - 1},
        E("hello", "hallo"),
        E("menu\004open", "oeffnen"),
        E("file\0files", "Datei\0Dateien"),
    };
    size_t n = build_mo(mobuf, e, 4, 0, 0, 1);
    wfile(P("mf/de_DE.UTF-8/LC_MESSAGES/wle.mo"), mobuf, n);
    n = build_mo(mobuf, e, 4, 1, 0, 1);
    wfile(P("mf/de_DE.UTF-8/LC_MESSAGES/wbe.mo"), mobuf, n);
    bindtextdomain("wle", P("mf"));
    bindtextdomain("wbe", P("mf"));
    mf_show("wle");
    mf_show("wbe");

    printf("-- 500 entries (msgfmt, with and without a hash table)\n");
    static const char *big[] = {"big.mo", "big-nohash.mo", NULL};
    for (int i = 0; big[i]; i++) {
        char dom[32];
        snprintf(dom, sizeof dom, "big%d", i);
        copy_fixture(big[i], P("mf/de_DE.UTF-8/LC_MESSAGES/%s.mo", dom));
        bindtextdomain(dom, P("mf"));
        int good = 0;
        for (int k = 0; k < 500; k++) {
            char key[16], val[16];
            snprintf(key, sizeof key, "key-%03d", k);
            snprintf(val, sizeof val, "val-%03d", k);
            if (strcmp(G(dom, key), val) == 0)
                good++;
        }
        printf("  %s: %d of 500 found with the right translation; misses:", big[i], good);
        static const char *miss[] = {"key-500", "key-", "key-0000", "Key-001", "key-001 ", "", NULL};
        for (int k = 0; miss[k]; k++) {
            const char *r = G(dom, miss[k]);
            printf(" ");
            if (r == miss[k])
                printf("(arg)");
            else if (*miss[k] == 0)
                printf("(header)");
            else
                pz(r);
        }
        putchar('\n');
    }

}

/* Entries beyond N make the reference library read outside the file: "~" marks them unstable. */
static const char *hash_what[] = {"every hash entry 0 (empty)", "~every hash entry N+1 (one past the last index)",
                                  "~every hash entry N+5", "~every hash entry 0xffffffff",
                                  "every hash entry 2 (a valid index, the wrong string)", NULL};

static void s_hash_alter(void *a)
{
    int w = (int)(intptr_t)a;
    clear_env();
    if (!need_locale(LC_ALL, "de_DE.UTF-8"))
        return;
    size_t len;
    unsigned char *b = fixture("big.mo", &len);
    if (!b || len < 28)
        return;
    uint32_t S = get32(b + 20), H = get32(b + 24), N = get32(b + 8);
    if (w == 0)
        printf("  big.mo: N %u, hash size %u\n", N, S);
    uint32_t v = w == 0 ? 0 : w == 1 ? N + 1 : w == 2 ? N + 5 : w == 3 ? 0xffffffff : 2;
    for (uint32_t i = 0; i < S && H + 4 * i + 4 <= len; i++)
        put32(b + H + 4 * i, v, 0);
    wfile(P("hs%d/de_DE.UTF-8/LC_MESSAGES/hash.mo", w), b, len);
    bindtextdomain("hash", P("hs%d", w));
    printf("  %s:", hash_what[w]);
    fflush(stdout);
    int good = 0, arg = 0;
    for (int k = 0; k < 500; k++) {
        char key[16], val[16];
        snprintf(key, sizeof key, "key-%03d", k);
        snprintf(val, sizeof val, "val-%03d", k);
        const char *r = G("hash", key);
        if (strcmp(r, val) == 0)
            good++;
        else if (strcmp(r, key) == 0)
            arg++;
    }
    printf(" %d right, %d untranslated, %d other\n", good, arg, 500 - good - arg);
    free(b);
}

static void s_msgfmt_more(void *u)
{
    (void)u;
    clear_env();
    if (!need_locale(LC_ALL, "de_DE.UTF-8"))
        return;
    printf("-- msgfmt Russian catalog (3 forms)\n");
    copy_fixture("ru.mo", P("mf/de_DE.UTF-8/LC_MESSAGES/ru.mo"));
    bindtextdomain("ru", P("mf"));
    static const char s1[] = "file", s2[] = "files";
    plural_n_line("ru", s1, s2);
    printf("-- msgfmt catalog with a system-dependent string (revision 1): msgid \"count %%<PRIu64> x\"\n");
    copy_fixture("sd.mo", P("mf/de_DE.UTF-8/LC_MESSAGES/sd.mo"));
    bindtextdomain("sd", P("mf"));
    static const char *sdk[] = {"count %lu x", "count %llu x", "count %<PRIu64> x", "count % x", "count %" "l" "d x", NULL};
    for (int i = 0; sdk[i]; i++) {
        const char *r = G("sd", sdk[i]);
        printf("  ");
        pz(sdk[i]);
        printf(" -> ");
        if (r == sdk[i])
            printf("(the argument)\n");
        else {
            pz(r);
            putchar('\n');
        }
    }
    pl("  header", G("sd", ""));
    printf("-- msgfmt ISO-8859-1 catalog in a UTF-8 locale\n");
    copy_fixture("latin1.mo", P("mf/de_DE.UTF-8/LC_MESSAGES/latin1.mo"));
    bindtextdomain("latin1", P("mf"));
    pl("  latin", G("latin1", "latin"));
}

/* ================================================================ damaged catalogs */

struct dcase {
    const char *title;
    int kind;
    uint32_t off, val; /* for kind 1: put32(off, val) */
    size_t trunc;      /* for kind 2 */
};

static const struct ent dmg_ents[] = {
    {"", 0, HDR_UTF8 "Plural-Forms: nplurals=2; plural=n != 1;\n", sizeof(HDR_UTF8 "Plural-Forms: nplurals=2; plural=n != 1;\n") - 1},
    E("file\0files", "Datei\0Dateien"),
    E("hello", "hallo"),
    E("zz", "ZZ"),
};

static void dmg_show(const char *dom)
{
    static const char s1[] = "file", s2[] = "files";
    errno = EINTR;
    printf("  hello ");
    pz(G(dom, "hello"));
    printf(" zz ");
    pz(G(dom, "zz"));
    printf(" hell ");
    pz(G(dom, "hell"));
    printf(" file n=2 ");
    const char *r = dcngettext(dom, s1, s2, 2, LC_MESSAGES);
    if (r == s2)
        printf("(msgid_plural)");
    else
        pz(r);
    printf(" errno %s\n", en(errno));
}

static void s_damage_one(void *a)
{
    const struct dcase *c = a;
    printf("-- %s\n", c->title);
    clear_env();
    if (!need_locale(LC_ALL, "de_DE.UTF-8"))
        return;
    char *path = P("dm%d/de_DE.UTF-8/LC_MESSAGES/dm.mo", (int)getpid());
    char base[PATH_MAX * 2];
    snprintf(base, sizeof base, "%s", P("dm%d", (int)getpid()));
    size_t len = build_mo(mobuf, dmg_ents, 4, 0, 0, 1);
    switch (c->kind) {
    case 0: /* good */
        wfile(path, mobuf, len);
        break;
    case 1: /* one word changed */
        put32(mobuf + c->off, c->val, 0);
        wfile(path, mobuf, len);
        break;
    case 2: /* truncated */
        wfile(path, mobuf, c->trunc == (size_t)-1 ? len / 2 : c->trunc == (size_t)-2 ? len - 1 : c->trunc);
        break;
    case 3: /* magic byte-swapped, the rest little-endian */
        put32(mobuf, 0x950412de, 1);
        wfile(path, mobuf, len);
        break;
    case 4: { /* unsorted, no hash */
        struct ent u[4] = {dmg_ents[0], dmg_ents[3], dmg_ents[2], dmg_ents[1]};
        len = build_mo(mobuf, u, 4, 0, 0, 0);
        wfile(path, mobuf, len);
        break;
    }
    case 5: { /* duplicate keys */
        struct ent u[5] = {dmg_ents[0], dmg_ents[1], E("hello", "first"), E("hello", "second"), dmg_ents[3]};
        len = build_mo(mobuf, u, 5, 0, 0, 0);
        wfile(path, mobuf, len);
        break;
    }
    case 6: /* directory */
        mkparents(path);
        mkdir(path, 0755);
        break;
    case 7: /* mode 0000 */
        wfile(path, mobuf, len);
        chmod(path, 0);
        break;
    case 8: /* FIFO */
        mkparents(path);
        mkfifo(path, 0644);
        break;
    case 9: /* symbolic link to a good catalog */
        wfile(P("dm%d/good.mo", (int)getpid()), mobuf, len);
        mkparents(path);
        if (symlink("../../good.mo", path) != 0)
            printf("symlink failed\n");
        break;
    case 10: /* dangling link */
        mkparents(path);
        if (symlink("../../nothing.mo", path) != 0)
            printf("symlink failed\n");
        break;
    case 11: /* big-endian, header only consistent */
        len = build_mo(mobuf, dmg_ents, 4, 1, 0, 1);
        wfile(path, mobuf, len);
        break;
    case 12: /* hash table size set, hash offset given, but no table: S=3 at H = string start */
        put32(mobuf + 20, 3, 0);
        wfile(path, mobuf, len);
        break;
    case 13: /* translation string without its NUL at the end of the file */
        mobuf[len - 1] = 'X';
        wfile(path, mobuf, len);
        break;
    }
    bindtextdomain("dm", base);
    dmg_show("dm");
}

static void s_damage_fallback(void *u)
{
    (void)u;
    clear_env();
    printf("-- LANGUAGE=bad:good, the bad catalog has a wrong magic number\n");
    setenv("LANG", "de_DE.UTF-8", 1);
    setenv("LANGUAGE", "bad:good", 1);
    if (!setlocale(LC_ALL, "")) {
        printf("setlocale failed\n");
        return;
    }
    size_t len = build_mo(mobuf, dmg_ents, 4, 0, 0, 1);
    wfile(P("fb/good/LC_MESSAGES/fb.mo"), mobuf, len);
    put32(mobuf, 0x12345678, 0);
    wfile(P("fb/bad/LC_MESSAGES/fb.mo"), mobuf, len);
    bindtextdomain("fb", P("fb"));
    dmg_show("fb");
}

static void s_damage(void)
{
    printf("== damaged .mo files (probe-written: header, file|files, hello, zz; strings from offset 92)\n");
    /* Layout of the good file: O = 28, T = 60, H = 92, S = 0; index 0 "", 1 file, 2 hello, 3 zz. */
    static const struct dcase c[] = {
        {"good file", 0, 0, 0, 0},
        {"empty file", 2, 0, 0, 0},
        {"4 bytes (magic only)", 2, 0, 0, 4},
        {"20 bytes", 2, 0, 0, 20},
        {"27 bytes", 2, 0, 0, 27},
        {"28 bytes (header only)", 2, 0, 0, 28},
        {"half the file", 2, 0, 0, (size_t)-1},
        {"all but the last byte", 2, 0, 0, (size_t)-2},
        {"wrong magic 0x12345678", 1, 0, 0x12345678, 0},
        {"~big-endian magic, little-endian fields", 3, 0, 0, 0},
        {"big-endian file (probe writer)", 11, 0, 0, 0},
        {"revision 1 (minor 1)", 1, 4, 1, 0},
        {"revision 2 (minor 2)", 1, 4, 2, 0},
        {"revision 0x10000 (major 1)", 1, 4, 0x10000, 0},
        {"revision 0x20000 (major 2)", 1, 4, 0x20000, 0},
        {"revision 0xffffffff", 1, 4, 0xffffffff, 0},
        {"N = 0", 1, 8, 0, 0},
        {"N = 2", 1, 8, 2, 0},
        {"~N = 0x7fffffff", 1, 8, 0x7fffffff, 0},
        {"~N = 0x20000000", 1, 8, 0x20000000, 0},
        {"~O beyond the file", 1, 12, 0x100000, 0},
        {"~T beyond the file", 1, 16, 0x100000, 0},
        {"~O = 1 (misaligned)", 1, 12, 1, 0},
        {"O = T (tables swapped onto each other)", 1, 12, 60, 0},
        {"~hash size 3 at the string area (no table)", 12, 0, 0, 0},
        {"~hash size 0xffffffff", 1, 20, 0xffffffff, 0},
        {"hash size 5, offset beyond the file", 1, 24, 0x100000, 0},
        {"~key 'hello': offset beyond the file", 1, 28 + 16 + 4, 0x100000, 0},
        {"key 'hello': length beyond the file", 1, 28 + 16, 0x100000, 0},
        {"key 'hello': length 4", 1, 28 + 16, 4, 0},
        {"~translation of 'hello': offset beyond the file", 1, 60 + 16 + 4, 0x100000, 0},
        {"translation of 'hello': length beyond the file", 1, 60 + 16, 0x100000, 0},
        {"translation of 'hello': length 3", 1, 60 + 16, 3, 0},
        {"translation of 'hello': length 0", 1, 60 + 16, 0, 0},
        {"translation of 'file': length 5 (one form)", 1, 60 + 8, 5, 0},
        {"translation of 'zz' without its terminating NUL at the end of the file", 13, 0, 0, 0},
        {"keys not sorted, no hash table", 4, 0, 0, 0},
        {"duplicate keys", 5, 0, 0, 0},
        {"a directory in place of the file", 6, 0, 0, 0},
        {"mode 0000", 7, 0, 0, 0},
        {"a FIFO in place of the file (3 s limit)", 8, 0, 0, 0},
        {"symbolic link to a good file", 9, 0, 0, 0},
        {"dangling symbolic link", 10, 0, 0, 0},
    };
    for (size_t i = 0; i < sizeof c / sizeof c[0]; i++)
        if ((c[i].title[0] == '~') == unstable_mode)
            child(s_damage_one, (void *)&c[i], strstr(c[i].title, "FIFO") ? 3 : 20, 0); /* only a FIFO blocks; 3 s killed a loaded reference */
    if (!unstable_mode)
        child(s_damage_fallback, NULL, 10, 0);
}

/* ================================================================ _nl_msg_cat_cntr */

static int cntr_last;

static void ck(const char *what)
{
    int now = *(volatile int *)&_nl_msg_cat_cntr;
    printf("  %s: %s\n", what, now != cntr_last ? "changed" : "unchanged");
    cntr_last = now;
}

static void s_counter(void *u)
{
    (void)u;
    clear_env();
    cntr_last = _nl_msg_cat_cntr;
    where_mo(P("cn/de_DE.UTF-8/LC_MESSAGES/cn.mo"), "cn");
    where_mo(P("cn2/de_DE.UTF-8/LC_MESSAGES/cn.mo"), "cn2");
    textdomain("cn");
    ck("textdomain(\"cn\") (new)");
    textdomain("cn");
    ck("textdomain(\"cn\") (same)");
    textdomain(NULL);
    ck("textdomain(NULL)");
    textdomain("");
    ck("textdomain(\"\")");
    textdomain("cn");
    ck("textdomain(\"cn\")");
    bindtextdomain("cn", P("cn"));
    ck("bindtextdomain(cn, <dir>/cn) (new)");
    bindtextdomain("cn", P("cn"));
    ck("bindtextdomain(cn, <dir>/cn) (same)");
    bindtextdomain("cn", NULL);
    ck("bindtextdomain(cn, NULL)");
    bind_textdomain_codeset("cn", "UTF-8");
    ck("bind_textdomain_codeset(cn, UTF-8) (new)");
    bind_textdomain_codeset("cn", "UTF-8");
    ck("bind_textdomain_codeset(cn, UTF-8) (same)");
    bind_textdomain_codeset("cn", NULL);
    ck("bind_textdomain_codeset(cn, NULL)");
    bindtextdomain(NULL, P("cn"));
    ck("bindtextdomain(NULL, dir)");
    setlocale(LC_ALL, "de_DE.UTF-8");
    ck("setlocale(LC_ALL, de_DE.UTF-8)");
    setlocale(LC_ALL, "de_DE.UTF-8");
    ck("setlocale(LC_ALL, de_DE.UTF-8) (same)");
    setlocale(LC_ALL, NULL);
    ck("setlocale(LC_ALL, NULL)");
    setlocale(LC_MESSAGES, "fr_FR.UTF-8");
    ck("setlocale(LC_MESSAGES, fr_FR.UTF-8)");
    setlocale(LC_CTYPE, "fr_FR.UTF-8");
    ck("setlocale(LC_CTYPE, fr_FR.UTF-8)");
    setlocale(LC_NUMERIC, "fr_FR.UTF-8");
    ck("setlocale(LC_NUMERIC, fr_FR.UTF-8)");
    setlocale(LC_ALL, "xx_XX.UTF-8");
    ck("setlocale(LC_ALL, xx_XX.UTF-8) (fails)");
    setlocale(LC_ALL, "de_DE.UTF-8");
    ck("setlocale(LC_ALL, de_DE.UTF-8)");
    G("cn", "where");
    ck("first lookup (loads the catalog)");
    G("cn", "where");
    ck("second lookup");
    G("cn", "nothing");
    ck("lookup of a missing msgid");
    G("absent-domain", "where");
    ck("lookup in a domain without a catalog");
    bindtextdomain("cn", P("cn2"));
    ck("bindtextdomain(cn, <dir>/cn2)");
    G("cn", "where");
    ck("lookup (loads the new catalog)");
    locale_t l = newlocale(LC_ALL_MASK, "fr_FR.UTF-8", (locale_t)0);
    ck("newlocale");
    uselocale(l);
    ck("uselocale");
    uselocale(LC_GLOBAL_LOCALE);
    freelocale(l);
    setenv("LANGUAGE", "it", 1);
    ck("setenv LANGUAGE");
    G("cn", "where");
    ck("lookup after the LANGUAGE change");
    nl_catd cd = catopen("nothing-here", 0);
    if (cd != (nl_catd)-1)
        catclose(cd);
    ck("catopen");
}

/* ================================================================ errno */

static void s_errno(void *u)
{
    (void)u;
    clear_env();
    if (!need_locale(LC_ALL, "de_DE.UTF-8"))
        return;
    size_t len = build_mo(mobuf, dmg_ents, 4, 0, 0, 1);
    wfile(P("er/de_DE.UTF-8/LC_MESSAGES/good.mo"), mobuf, len);
    wfile(P("er/de_DE.UTF-8/LC_MESSAGES/locked.mo"), mobuf, len);
    chmod(P("er/de_DE.UTF-8/LC_MESSAGES/locked.mo"), 0);
    wfile(P("er/de_DE.UTF-8/LC_MESSAGES/short.mo"), mobuf, 10);
    write_mo(P("er/de_DE.UTF-8/LC_MESSAGES/conv.mo"), conv_ents, (int)(sizeof conv_ents / sizeof conv_ents[0]));
    static const char *doms[] = {"good", "locked", "short", "conv", "missing"};
    for (size_t i = 0; i < 5; i++)
        bindtextdomain(doms[i], P("er"));
    bind_textdomain_codeset("conv", "ASCII");
    static const int pre[] = {0, EINTR, ENOMEM, 12345};
    for (size_t p = 0; p < 4; p++) {
        printf("-- errno preset to %s\n", en(pre[p]));
        printf(" ");
        errno = pre[p]; (gettext)("x"); printf(" gettext(C domain) %s", en(errno));
        errno = pre[p]; G("good", "hello"); printf(" found %s", en(errno));
        errno = pre[p]; G("good", "nope"); printf(" not-found %s", en(errno));
        errno = pre[p]; G("missing", "hello"); printf(" no-catalog %s", en(errno));
        errno = pre[p]; G("locked", "hello"); printf(" unreadable %s", en(errno));
        errno = pre[p]; G("short", "hello"); printf(" truncated %s\n ", en(errno));
        errno = pre[p]; G("conv", "euro"); printf(" conversion %s", en(errno));
        errno = pre[p]; dcngettext("good", "file", "files", 2, LC_MESSAGES); printf(" ngettext %s", en(errno));
        errno = pre[p]; dcgettext("good", "hello", LC_ALL); printf(" LC_ALL %s", en(errno));
        errno = pre[p]; textdomain("x"); printf(" textdomain %s", en(errno));
        errno = pre[p]; textdomain(NULL); printf(" textdomain(NULL) %s\n ", en(errno));
        errno = pre[p]; bindtextdomain("good", P("er")); printf(" bindtextdomain %s", en(errno));
        errno = pre[p]; bindtextdomain("good", NULL); printf(" bindtextdomain(NULL dir) %s", en(errno));
        errno = pre[p]; bindtextdomain(NULL, NULL); printf(" bindtextdomain(NULL, NULL) %s", en(errno));
        errno = pre[p]; bind_textdomain_codeset("good", "UTF-8"); printf(" codeset %s\n", en(errno));
    }
}

/* ================================================================ X/Open catalogs: writer */

struct cent {
    int set, msg;
    const char *s;
};

static unsigned char catbuf[1 << 20];

/* magic, plane size, plane depth, then size*depth slots of (set+1, msg, offset) in little-endian
 * order, the same slots in big-endian order, then the NUL-terminated strings. A message goes to
 * slot ((set+1)*msg) % size of the first plane where that slot is free. */
static size_t build_cat(unsigned char *b, const struct cent *e, int n, uint32_t size, uint32_t depth)
{
    size_t slots = (size_t)size * depth, tbl = slots * 12;
    memset(b, 0, 12 + 2 * tbl);
    put32(b, 0x960408de, 0);
    put32(b + 4, size, 0);
    put32(b + 8, depth, 0);
    unsigned char *le = b + 12, *bt = b + 12 + tbl, *str = b + 12 + 2 * tbl;
    size_t off = 0;
    for (int i = 0; i < n; i++) {
        uint32_t key = (uint32_t)e[i].set + 1;
        uint32_t idx = (key * (uint32_t)e[i].msg) % size;
        uint32_t lvl;
        for (lvl = 0; lvl < depth; lvl++)
            if (get32(le + (idx + lvl * size) * 12) == 0)
                break;
        if (lvl == depth)
            return 0;
        size_t s = (size_t)(idx + lvl * size) * 12;
        put32(le + s, key, 0);
        put32(le + s + 4, (uint32_t)e[i].msg, 0);
        put32(le + s + 8, (uint32_t)off, 0);
        put32(bt + s, key, 1);
        put32(bt + s + 4, (uint32_t)e[i].msg, 1);
        put32(bt + s + 8, (uint32_t)off, 1);
        size_t l = strlen(e[i].s);
        memcpy(str + off, e[i].s, l + 1);
        off += l + 1;
    }
    return 12 + 2 * tbl + off;
}

static void label_cat(const char *path, const char *label)
{
    struct cent e[1] = {{1, 1, label}};
    size_t n = build_cat(catbuf, e, 1, 1, 1);
    wfile(path, catbuf, n);
}

static void cat_writer_check(void)
{
    printf("== X/Open catalogs: the probe's .cat writer against gencat\n");
    static const struct cent a[] = {{3, 1, "set three msg one"}, {3, 7, "\"quoted?\""}, {3, 8, "now quoted"},
                                     {1, 1, "Hello set one"}, {1, 2, "Second\tmessage\n"}, {1, 5, "five"}};
    static const struct cent b[] = {{10, 100, "ten-hundred"}, {2, 3, "two-three"}, {1, 1, "default set msg"}};
    static const struct cent c[] = {{1, 1, "x"}};
    struct {
        const char *f;
        const struct cent *e;
        int n;
        uint32_t size, depth;
    } t[] = {{"a.cat", a, 6, 5, 2}, {"b.cat", b, 3, 1, 3}, {"c.cat", c, 1, 1, 1}};
    for (int i = 0; i < 3; i++) {
        size_t len;
        unsigned char *g = fixture(t[i].f, &len);
        if (!g)
            continue;
        size_t n = build_cat(catbuf, t[i].e, t[i].n, t[i].size, t[i].depth);
        printf("  %s: gencat wrote %zu bytes, plane size %u, depth %u; the writer's bytes are identical: %s\n", t[i].f, len,
               len >= 12 ? get32(g + 4) : 0, len >= 12 ? get32(g + 8) : 0, n == len && memcmp(g, catbuf, n) == 0 ? "yes" : "no");
        free(g);
    }
}

/* ================================================================ X/Open catalogs: gencat fixture */

static int count_fds(void)
{
    DIR *d = opendir("/proc/self/fd");
    if (!d)
        return -1;
    int n = 0;
    struct dirent *e;
    while ((e = readdir(d)))
        if (e->d_name[0] != '.')
            n++;
    closedir(d);
    return n - 1;
}

static void s_catgets_main(void *u)
{
    (void)u;
    clear_env();
    printf("-- the gencat fixture main.cat (absolute name)\n");
    int f0 = count_fds();
    errno = 0;
    nl_catd cd = catopen(P("fix/main.cat"), 0);
    if (cd == (nl_catd)-1) {
        printf("  catopen failed: %s\n", en(errno));
        return;
    }
    printf("  catopen ok, errno %s, descriptors added %d\n", en(errno), count_fds() - f0);
    static const int q[][2] = {{1, 1}, {1, 2}, {1, 3}, {1, 4}, {1, 5}, {1, 6}, {1, 7}, {1, 8}, {1, 9}, {1, 10}, {1, 11},
                               {1, 12}, {1, 13}, {1, 14}, {1, 15}, {1, 16}, {1, 17}, {1, 18}, {1, 32767}, {1, 100000},
                               {1, 0}, {1, -1}, {2, 1}, {2, 2}, {2, 3}, {7, 1}, {9, 1}, {0, 1}, {-1, 1}, {3, 1},
                               {INT_MAX, 1}, {1, INT_MAX}, {INT_MIN, INT_MIN}, {-2, 1}};
    static const char dflt[] = "DEFAULT";
    for (size_t i = 0; i < sizeof q / sizeof q[0]; i++) {
        errno = 0;
        const char *r = catgets(cd, q[i][0], q[i][1], dflt);
        printf("  %d,%d: ", q[i][0], q[i][1]);
        if (r == dflt)
            printf("(default)");
        else
            pz(r);
        printf(" errno %s\n", en(errno));
    }
    errno = EINTR;
    const char *r1 = catgets(cd, 1, 1, dflt);
    printf("  found with errno preset to EINTR: errno %s\n", en(errno));
    const char *r2 = catgets(cd, 1, 1, dflt);
    printf("  the same message twice, same pointer: %s\n", r1 == r2 ? "yes" : "no");
    errno = EINTR;
    const char *r3 = catgets(cd, 1, 99, NULL);
    printf("  missing with a NULL default: %s, errno %s\n", r3 ? "non-NULL" : "NULL", en(errno));
    setlocale(LC_ALL, "fr_FR.UTF-8");
    pl("  after setlocale(LC_ALL, fr_FR.UTF-8), 1,1", catgets(cd, 1, 1, dflt));
    nl_catd cd2 = catopen(P("fix/main.cat"), 0);
    printf("  a second catopen of the same file: %s, distinct descriptor: %s\n", cd2 == (nl_catd)-1 ? "-1" : "ok",
           cd2 != cd ? "yes" : "no");
    errno = EINTR;
    printf("  catclose (second): %d errno %s\n", cd2 == (nl_catd)-1 ? 99 : catclose(cd2), en(errno));
    pl("  first descriptor still reads 2,1", catgets(cd, 2, 1, dflt));
    errno = EINTR;
    int rc = catclose(cd);
    printf("  catclose: %d errno %s, descriptors left over %d\n", rc, en(errno), count_fds() - f0);
    errno = 0;
    rc = catclose((nl_catd)-1);
    printf("  catclose((nl_catd)-1): %d errno %s\n", rc, en(errno));
    errno = 0;
    const char *r = catgets((nl_catd)-1, 1, 1, dflt);
    printf("  catgets((nl_catd)-1, 1, 1): %s errno %s\n", r == dflt ? "(default)" : "other", en(errno));
}

/* ================================================================ NLSPATH and locale sources */

struct ncase {
    const char *title;
    const char *nlspath; /* "<T>" stands for the case's tree; NULL: unset */
    const char *lang, *lc_all, *lc_messages; /* NULL: unset */
    const char *setloc;  /* setlocale(LC_ALL) before catopen, NULL: none */
    int flag;
    const char *name;    /* "<T>" allowed */
    int cwd_t;
    const char *files[16]; /* pairs: relative path, label ("!dir", "!000", "!bad", "!empty" for special files) */
};

static int ncase_id;

static char *expand(const char *s, const char *t)
{
    static char b[4][PATH_MAX * 3];
    static int k;
    char *r = b[k++ & 3];
    size_t o = 0, tl = strlen(t);
    while (*s && o < sizeof b[0] - tl - 1) {
        if (strncmp(s, "<T>", 3) == 0) {
            memcpy(r + o, t, tl);
            o += tl;
            s += 3;
        } else {
            r[o++] = *s++;
        }
    }
    r[o] = 0;
    return r;
}

static void s_ncase(void *a)
{
    const struct ncase *c = a;
    printf("-- %s\n", c->title);
    clear_env();
    char t[PATH_MAX * 2];
    snprintf(t, sizeof t, "%s", P("n%02d", ncase_id));
    mkdir(t, 0755);
    for (int i = 0; c->files[i]; i += 2) {
        char *p = P("n%02d/%s", ncase_id, c->files[i]);
        const char *l = c->files[i + 1];
        if (strcmp(l, "!dir") == 0) {
            mkparents(p);
            mkdir(p, 0755);
        } else if (strcmp(l, "!000") == 0) {
            label_cat(p, "unreadable");
            chmod(p, 0);
        } else if (strcmp(l, "!bad") == 0) {
            wfile(p, "this is not a catalog\n", 22);
        } else if (strcmp(l, "!empty") == 0) {
            wfile(p, "", 0);
        } else {
            label_cat(p, l);
        }
    }
    if (c->cwd_t && chdir(t) != 0)
        printf("chdir failed\n");
    if (c->setloc && !need_locale(LC_ALL, c->setloc))
        return;
    setenv_or_unset("LANG", c->lang);
    setenv_or_unset("LC_ALL", c->lc_all);
    setenv_or_unset("LC_MESSAGES", c->lc_messages);
    if (c->nlspath)
        setenv("NLSPATH", expand(c->nlspath, t), 1);
    errno = 0;
    nl_catd cd = catopen(expand(c->name, t), c->flag);
    if (cd == (nl_catd)-1) {
        printf("  catopen: -1 errno %s\n", en(errno));
        return;
    }
    static const char dflt[] = "DEFAULT";
    errno = 0;
    const char *r = catgets(cd, 1, 1, dflt);
    printf("  catopen: ok; catgets(1,1): ");
    if (r == dflt)
        printf("(default) errno %s", en(errno));
    else
        pz(r);
    putchar('\n');
    catclose(cd);
}

static void s_nlspath(void)
{
    printf("== catopen: NLSPATH, the name, the locale sources\n");
    static const struct ncase c[] = {
        {"%N", "<T>/%N", NULL, NULL, NULL, NULL, 0, "app", 0, {"app", "app", NULL}},
        {"%N.cat", "<T>/%N.cat", NULL, NULL, NULL, NULL, 0, "app", 0, {"app.cat", "app.cat", "app", "app", NULL}},
        {"%L, LANG=de_DE.UTF-8@euro", "<T>/%L/%N", "de_DE.UTF-8@euro", NULL, NULL, NULL, 0, "app", 0,
         {"de_DE.UTF-8@euro/app", "full name", "de_DE/app", "de_DE", "de/app", "de", NULL}},
        {"%l/%t/%c, LANG=de_DE.UTF-8@euro", "<T>/l=%l/t=%t/c=%c/%N", "de_DE.UTF-8@euro", NULL, NULL, NULL, 0, "app", 0,
         {"l=de/t=DE/c=UTF-8@euro/app", "c keeps the modifier", "l=de/t=DE/c=UTF-8/app", "c without the modifier", NULL}},
        {"%l/%t/%c, LANG=de", "<T>/l=%l/t=%t/c=%c/%N", "de", NULL, NULL, NULL, 0, "app", 0, {"l=de/t=/c=/app", "l=de t= c=", NULL}},
        {"%l/%t/%c, LANG=de_DE", "<T>/l=%l/t=%t/c=%c/%N", "de_DE", NULL, NULL, NULL, 0, "app", 0, {"l=de/t=DE/c=/app", "l=de t=DE c=", NULL}},
        {"%l/%t/%c, LANG=de.UTF-8", "<T>/l=%l/t=%t/c=%c/%N", "de.UTF-8", NULL, NULL, NULL, 0, "app", 0,
         {"l=de/t=/c=UTF-8/app", "l=de t= c=UTF-8", NULL}},
        {"%l/%t/%c, LANG=de@euro", "<T>/l=%l/t=%t/c=%c/%N", "de@euro", NULL, NULL, NULL, 0, "app", 0,
         {"l=de@euro/t=/c=/app", "l=de@euro t= c=", "l=de/t=/c=/app", "l=de t= c=", NULL}},
        {"%l/%t/%c, LANG=de_DE@euro", "<T>/l=%l/t=%t/c=%c/%N", "de_DE@euro", NULL, NULL, NULL, 0, "app", 0,
         {"l=de/t=DE@euro/c=/app", "l=de t=DE@euro c=", "l=de/t=DE/c=/app", "l=de t=DE c=", NULL}},
        {"%l/%t/%c, LANG=de_DE_X.Y", "<T>/l=%l/t=%t/c=%c/%N", "de_DE_X.Y", NULL, NULL, NULL, 0, "app", 0,
         {"l=de/t=DE_X/c=Y/app", "l=de t=DE_X c=Y", NULL}},
        {"%l/%t/%c, LANG=C.UTF-8", "<T>/l=%l/t=%t/c=%c/%N", "C.UTF-8", NULL, NULL, NULL, 0, "app", 0,
         {"l=C/t=/c=UTF-8/app", "l=C t= c=UTF-8", NULL}},
        {"%L/%l/%t/%c, LANG=C", "<T>/L=%L/l=%l/t=%t/c=%c/%N", "C", NULL, NULL, NULL, 0, "app", 0,
         {"L=C/l=C/t=/c=/app", "L=C l=C", NULL}},
        {"%L/%l/%t/%c, LANG=POSIX", "<T>/L=%L/l=%l/t=%t/c=%c/%N", "POSIX", NULL, NULL, NULL, 0, "app", 0,
         {"L=POSIX/l=POSIX/t=/c=/app", "L=POSIX l=POSIX", "L=C/l=C/t=/c=/app", "L=C l=C", NULL}},
        {"%L/%l/%t/%c, LANG empty", "<T>/L=%L/l=%l/t=%t/c=%c/%N", "", NULL, NULL, NULL, 0, "app", 0,
         {"L=C/l=C/t=/c=/app", "L=C l=C", "L=/l=/t=/c=/app", "all empty", NULL}},
        {"%L/%l/%t/%c, LANG unset", "<T>/L=%L/l=%l/t=%t/c=%c/%N", NULL, NULL, NULL, NULL, 0, "app", 0,
         {"L=C/l=C/t=/c=/app", "L=C l=C", "L=/l=/t=/c=/app", "all empty", NULL}},
        {"%%", "<T>/a%%b/%N", "de", NULL, NULL, NULL, 0, "app", 0, {"a%b/app", "a%b", "a%%b/app", "a%%b", NULL}},
        {"unknown %X in the only template", "<T>/u%X/%N", "de", NULL, NULL, NULL, 0, "app", 0,
         {"u%X/app", "u%X", "uX/app", "uX", "u/app", "u", NULL}},
        {"unknown %X in the first template, a good second one", "<T>/u%X/%N:<T>/ok/%N", "de", NULL, NULL, NULL, 0, "app", 0,
         {"u%X/app", "u%X", "uX/app", "uX", "u/app", "u", "ok/app", "ok", NULL}},
        {"a good template, then one with %X", "<T>/ok/%N:<T>/u%X/%N", "de", NULL, NULL, NULL, 0, "app", 0,
         {"ok/app", "ok", NULL}},
        {"trailing %", "<T>/%N%", "de", NULL, NULL, NULL, 0, "app", 0, {"app%", "app%", "app", "app", NULL}},
        {"trailing % in the first template, a good second one", "<T>/x%:<T>/ok/%N", "de", NULL, NULL, NULL, 0, "app", 0,
         {"x%", "x%", "x", "x", "ok/app", "ok", NULL}},
        {"leading colon (empty template), cwd holds app", ":<T>/none/%N", "de", NULL, NULL, NULL, 0, "app", 1,
         {"app", "cwd app", NULL}},
        {"two adjacent colons", "<T>/none/%N::<T>/b/%N", "de", NULL, NULL, NULL, 0, "app", 1,
         {"app", "cwd app", "b/app", "b", NULL}},
        {"trailing colon", "<T>/none/%N:", "de", NULL, NULL, NULL, 0, "app", 1, {"app", "cwd app", NULL}},
        {"first template misses, second finds", "<T>/a/%N:<T>/b/%N", "de", NULL, NULL, NULL, 0, "app", 0, {"b/app", "b", NULL}},
        {"first candidate is a directory", "<T>/a/%N:<T>/b/%N", "de", NULL, NULL, NULL, 0, "app", 0,
         {"a/app", "!dir", "b/app", "b", NULL}},
        {"first candidate is unreadable", "<T>/a/%N:<T>/b/%N", "de", NULL, NULL, NULL, 0, "app", 0,
         {"a/app", "!000", "b/app", "b", NULL}},
        {"first candidate is not a catalog", "<T>/a/%N:<T>/b/%N", "de", NULL, NULL, NULL, 0, "app", 0,
         {"a/app", "!bad", "b/app", "b", NULL}},
        {"first candidate is empty", "<T>/a/%N:<T>/b/%N", "de", NULL, NULL, NULL, 0, "app", 0,
         {"a/app", "!empty", "b/app", "b", NULL}},
        {"only candidate is a directory", "<T>/a/%N", "de", NULL, NULL, NULL, 0, "app", 0, {"a/app", "!dir", NULL}},
        {"only candidate is unreadable", "<T>/a/%N", "de", NULL, NULL, NULL, 0, "app", 0, {"a/app", "!000", NULL}},
        {"only candidate is not a catalog", "<T>/a/%N", "de", NULL, NULL, NULL, 0, "app", 0, {"a/app", "!bad", NULL}},
        {"only candidate is empty", "<T>/a/%N", "de", NULL, NULL, NULL, 0, "app", 0, {"a/app", "!empty", NULL}},
        {"absolute name, NLSPATH ignored", "<T>/n/%N", "de", NULL, NULL, NULL, 0, "<T>/abs/app", 0,
         {"abs/app", "absolute", NULL}},
        {"relative name with a slash", "<T>/n/%N", "de", NULL, NULL, NULL, 0, "sub/app", 1,
         {"sub/app", "relative with slash", "n/sub/app", "via NLSPATH", NULL}},
        {"absolute name missing", "<T>/n/%N", "de", NULL, NULL, NULL, 0, "<T>/abs/none", 0, {"n/none", "via NLSPATH", NULL}},
        {"absolute name, a directory", NULL, "de", NULL, NULL, NULL, 0, "<T>/d", 0, {"d", "!dir", NULL}},
        {"absolute name, unreadable", NULL, "de", NULL, NULL, NULL, 0, "<T>/u", 0, {"u", "!000", NULL}},
        {"absolute name below a file", NULL, "de", NULL, NULL, NULL, 0, "<T>/f/x", 0, {"f", "file", NULL}},
        {"name not found anywhere", "<T>/n/%N", "de", NULL, NULL, NULL, 0, "liella-no-such-catalog", 0, {NULL}},
        {"NLSPATH unset, name not found", NULL, "de", NULL, NULL, NULL, 0, "liella-no-such-catalog", 0, {NULL}},
        {"NLSPATH empty, cwd holds app", "", "de", NULL, NULL, NULL, 0, "app", 1, {"app", "cwd app", NULL}},
        {"template without %N", "<T>/fixed", "de", NULL, NULL, NULL, 0, "app", 0, {"fixed", "fixed", NULL}},
        {"empty name, NLSPATH <T>/n/%N (a directory <T>/n)", "<T>/n/%N", "de", NULL, NULL, NULL, 0, "", 0, {"n/", "!dir", NULL}},
        {"empty name, NLSPATH <T>/x%N (a catalog <T>/x)", "<T>/x%N", "de", NULL, NULL, NULL, 0, "", 0, {"x", "x", NULL}},
        {"empty name, NLSPATH unset", NULL, "de", NULL, NULL, NULL, 0, "", 0, {NULL}},
        {"name containing %L", "<T>/%N", "de", NULL, NULL, NULL, 0, "a%Lb", 0, {"a%Lb", "literal", "adeb", "substituted", NULL}},
        {"LANG with a slash", "<T>/%L/%N", "sub/de", NULL, NULL, NULL, 0, "app", 0, {"sub/de/app", "slash in LANG", NULL}},
        {"LANG=../x (parent directory)", "<T>/in/%L/%N", "../x", NULL, NULL, NULL, 0, "app", 0,
         {"in", "!dir", "x/app", "escaped", NULL}},
        {"path longer than PATH_MAX", "<T>/" "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/%N/%N/%N/%N/%N/%N/%N/%N/%N/%N/%N/%N/%N/%N/%N/%N/%N/%N/%N/%N",
         "de", NULL, NULL, NULL, 0,
         "nnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnn",
         0, {NULL}},
        {"flag 0 uses LANG (LC_ALL locale fr_FR.UTF-8, LANG=de_DE.UTF-8)", "<T>/%L/%N", "de_DE.UTF-8", NULL, NULL, "fr_FR.UTF-8", 0, "app", 0,
         {"fr_FR.UTF-8/app", "LC_MESSAGES", "de_DE.UTF-8/app", "LANG", "C/app", "C", NULL}},
        {"flag NL_CAT_LOCALE uses LC_MESSAGES (same setup)", "<T>/%L/%N", "de_DE.UTF-8", NULL, NULL, "fr_FR.UTF-8", NL_CAT_LOCALE, "app", 0,
         {"fr_FR.UTF-8/app", "LC_MESSAGES", "de_DE.UTF-8/app", "LANG", "C/app", "C", NULL}},
        {"flag 2 (same setup)", "<T>/%L/%N", "de_DE.UTF-8", NULL, NULL, "fr_FR.UTF-8", 2, "app", 0,
         {"fr_FR.UTF-8/app", "LC_MESSAGES", "de_DE.UTF-8/app", "LANG", "C/app", "C", NULL}},
        {"flag -1 (same setup)", "<T>/%L/%N", "de_DE.UTF-8", NULL, NULL, "fr_FR.UTF-8", -1, "app", 0,
         {"fr_FR.UTF-8/app", "LC_MESSAGES", "de_DE.UTF-8/app", "LANG", "C/app", "C", NULL}},
        {"flag 0 with LC_ALL=cs, LC_MESSAGES=it, LANG=de in the environment", "<T>/%L/%N", "de_DE.UTF-8", "cs_CZ.UTF-8", "it_IT.UTF-8", NULL, 0, "app", 0,
         {"cs_CZ.UTF-8/app", "LC_ALL", "it_IT.UTF-8/app", "LC_MESSAGES", "de_DE.UTF-8/app", "LANG", "C/app", "C", NULL}},
        {"flag 0 with LC_MESSAGES=it only", "<T>/%L/%N", NULL, NULL, "it_IT.UTF-8", NULL, 0, "app", 0,
         {"it_IT.UTF-8/app", "LC_MESSAGES", "C/app", "C", NULL}},
        {"flag 0 with LC_ALL=cs only", "<T>/%L/%N", NULL, "cs_CZ.UTF-8", NULL, NULL, 0, "app", 0,
         {"cs_CZ.UTF-8/app", "LC_ALL", "C/app", "C", NULL}},
        {"NL_CAT_LOCALE without setlocale, LANG=de_DE.UTF-8", "<T>/%L/%N", "de_DE.UTF-8", NULL, NULL, NULL, NL_CAT_LOCALE, "app", 0,
         {"de_DE.UTF-8/app", "LANG", "C/app", "C", NULL}},
        {"NL_CAT_LOCALE, LC_ALL locale de_DE.UTF-8@euro", "<T>/%L/l=%l/t=%t/c=%c/%N", NULL, NULL, NULL, "de_DE.UTF-8@euro", NL_CAT_LOCALE, "app", 0,
         {"de_DE.UTF-8@euro/l=de/t=DE/c=UTF-8@euro/app", "LC_MESSAGES name with modifier", NULL}},
    };
    for (size_t i = 0; i < sizeof c / sizeof c[0]; i++) {
        ncase_id = (int)i;
        child(s_ncase, (void *)&c[i], 10, 0);
    }
}

static void s_nlspath_change(void *u)
{
    (void)u;
    clear_env();
    printf("-- NLSPATH changed between two catopen calls in one process\n");
    label_cat(P("nc/one/app"), "one");
    label_cat(P("nc/two/app"), "two");
    setenv("NLSPATH", P("nc/one/%N"), 1);
    nl_catd a = catopen("app", 0);
    setenv("NLSPATH", P("nc/two/%N"), 1);
    nl_catd b = catopen("app", 0);
    printf("  first: ");
    pz(a == (nl_catd)-1 ? "(-1)" : catgets(a, 1, 1, "DEFAULT"));
    printf(" second: ");
    pz(b == (nl_catd)-1 ? "(-1)" : catgets(b, 1, 1, "DEFAULT"));
    putchar('\n');
    printf("-- the catalog file replaced after catopen\n");
    label_cat(P("nc/one/app"), "one, rewritten");
    pz(a == (nl_catd)-1 ? "(-1)" : catgets(a, 1, 1, "DEFAULT"));
    putchar('\n');
    printf("-- the catalog file removed after catopen\n");
    unlink(P("nc/two/app"));
    pz(b == (nl_catd)-1 ? "(-1)" : catgets(b, 1, 1, "DEFAULT"));
    putchar('\n');
    if (a != (nl_catd)-1)
        catclose(a);
    if (b != (nl_catd)-1)
        catclose(b);
    printf("-- catopen does not change _nl_msg_cat_cntr or the gettext state\n");
    pl("  textdomain(NULL)", textdomain(NULL));
}

/* ================================================================ X/Open catalogs: damaged */

struct ccase {
    const char *title;
    int kind;
    uint32_t off, val;
    size_t trunc;
};

static const struct cent cgood[] = {{1, 1, "one-one"}, {1, 2, "one-two"}, {2, 1, "two-one"}};

static void s_cat_damage_one(void *a)
{
    const struct ccase *c = a;
    printf("-- %s\n", c->title);
    size_t len = build_cat(catbuf, cgood, 3, 3, 1);
    /* plane size 3, depth 1: (1,1) slot 2, (1,2) slot 1, (2,1) slot 0; tables at 12 and 48;
       strings at 84. */
    char *path = P("cd%d/x.cat", (int)getpid());
    switch (c->kind) {
    case 0:
        break;
    case 1:
        put32(catbuf + c->off, c->val, 0);
        break;
    case 2:
        len = c->trunc == (size_t)-1 ? len - 1 : c->trunc;
        break;
    case 3: /* little-endian table zeroed */
        memset(catbuf + 12, 0, 36);
        break;
    case 4: /* big-endian table zeroed */
        memset(catbuf + 48, 0, 36);
        break;
    case 5: { /* a big-endian writer's file: header big-endian, big-endian table first */
        unsigned char t[36];
        memcpy(t, catbuf + 12, 36);
        memcpy(catbuf + 12, catbuf + 48, 36);
        memcpy(catbuf + 48, t, 36);
        put32(catbuf, 0x960408de, 1);
        put32(catbuf + 4, 3, 1);
        put32(catbuf + 8, 1, 1);
        break;
    }
    case 6: /* magic byte-swapped only */
        put32(catbuf, 0x960408de, 1);
        break;
    case 7: { /* entry (1,1) moved from its slot 2 to the free... no: swap slots 0 and 2 in both tables */
        unsigned char t[12];
        for (int tb = 0; tb < 2; tb++) {
            unsigned char *base = catbuf + 12 + tb * 36;
            memcpy(t, base, 12);
            memcpy(base, base + 24, 12);
            memcpy(base + 24, t, 12);
        }
        break;
    }
    case 8: /* last string without its NUL */
        catbuf[len - 1] = 'X';
        break;
    case 9:
        mkparents(path);
        mkdir(path, 0755);
        break;
    case 10:
        wfile(path, catbuf, len);
        chmod(path, 0);
        break;
    case 11:
        mkparents(path);
        mkfifo(path, 0644);
        break;
    case 12: { /* depth 2, the message in the second plane although its first-plane slot is free */
        struct cent e[1] = {{1, 1, "second plane"}};
        len = build_cat(catbuf, e, 1, 3, 2);
        /* slot 2 -> slot 5 */
        for (int tb = 0; tb < 2; tb++) {
            unsigned char *base = catbuf + 12 + tb * 72;
            memcpy(base + 60, base + 24, 12);
            memset(base + 24, 0, 12);
        }
        break;
    }
    }
    if (c->kind < 9 || c->kind == 12)
        wfile(path, catbuf, len);
    errno = 0;
    nl_catd cd = catopen(path, 0);
    if (cd == (nl_catd)-1) {
        printf("  catopen: -1 errno %s\n", en(errno));
        return;
    }
    static const char dflt[] = "DEFAULT";
    printf("  catopen: ok");
    static const int q[][2] = {{1, 1}, {1, 2}, {2, 1}, {3, 3}};
    for (int i = 0; i < 4; i++) {
        errno = 0;
        const char *r = catgets(cd, q[i][0], q[i][1], dflt);
        printf("; %d,%d ", q[i][0], q[i][1]);
        if (r == dflt)
            printf("(default) errno %s", en(errno));
        else
            pz(r);
    }
    errno = 0;
    int rc = catclose(cd);
    printf("; catclose %d errno %s\n", rc, en(errno));
}

static void s_cat_damage(void)
{
    printf("== damaged .cat files (probe-written: plane size 3, depth 1; 1,1 one-one; 1,2 one-two; 2,1 two-one)\n");
    static const struct ccase c[] = {
        {"good file", 0, 0, 0, 0},
        {"empty file", 2, 0, 0, 0},
        {"4 bytes", 2, 0, 0, 4},
        {"11 bytes", 2, 0, 0, 11},
        {"12 bytes (header only)", 2, 0, 0, 12},
        {"tables cut (40 bytes)", 2, 0, 0, 40},
        {"strings cut (90 bytes)", 2, 0, 0, 90},
        {"all but the last byte", 2, 0, 0, (size_t)-1},
        {"last string without its NUL", 8, 0, 0, 0},
        {"wrong magic", 1, 0, 0x12345678, 0},
        {"~magic byte-swapped only", 6, 0, 0, 0},
        {"a big-endian writer's file (swapped header, tables in the other order)", 5, 0, 0, 0},
        {"little-endian table zeroed", 3, 0, 0, 0},
        {"big-endian table zeroed", 4, 0, 0, 0},
        {"plane size 0", 1, 4, 0, 0},
        {"~plane depth 0", 1, 8, 0, 0},
        {"~plane size 0x40000000 (tables beyond the file)", 1, 4, 0x40000000, 0},
        {"~plane size x depth overflows 32 bits", 1, 8, 0x55555556, 0},
        {"plane size 2 (entries no longer at their hash slots)", 1, 4, 2, 0},
        {"slots 0 and 2 swapped (entries away from their hash slots)", 7, 0, 0, 0},
        {"message in the second plane, its first-plane slot free", 12, 0, 0, 0},
        {"string offset of 1,1 beyond the file", 1, 12 + 24 + 8, 0x100000, 0},
        {"string offset of 1,1 inside another string (offset 3)", 1, 12 + 24 + 8, 3, 0},
        {"set field of 1,1 stored as 1 instead of 2", 1, 12 + 24, 1, 0},
        {"a directory", 9, 0, 0, 0},
        {"mode 0000", 10, 0, 0, 0},
        {"a FIFO (3 s limit)", 11, 0, 0, 0},
    };
    for (size_t i = 0; i < sizeof c / sizeof c[0]; i++)
        if ((c[i].title[0] == '~') == unstable_mode)
            child(s_cat_damage_one, (void *)&c[i], strstr(c[i].title, "FIFO") ? 3 : 20, 0); /* only a FIFO blocks; 3 s killed a loaded reference */
}

/* ================================================================ main */

int main(int argc, char **argv)
{
    setvbuf(stdout, NULL, _IOLBF, 0);
    struct rlimit nocore = {0, 0};
    setrlimit(RLIMIT_CORE, &nocore);
    tw_alarm(900);
    if (argc < 2) {
        fprintf(stderr, "usage: catalogs_diff DIR\n");
        return 2;
    }
    snprintf(dir, sizeof dir, "%s", argv[1]);
    dirlen = strlen(dir);
    clear_env();
    if (argc > 2 && strcmp(argv[2], "unstable") == 0) {
        /* Outside the record: damaged catalogs whose offsets or sizes point outside the file.
         * What the reference library does with them depends on the memory around the mapping
         * (an -O0 and an -O1 build of this program differ), so the record leaves them out. */
        unstable_mode = 1;
        printf("== unstable cases (not part of the record)\n");
        for (int w = 0; hash_what[w]; w++)
            if (hash_what[w][0] == '~')
                child(s_hash_alter, (void *)(intptr_t)w, 10, 0);
        s_damage();
        s_cat_damage();
        printf("done\n");
        return 0;
    }
    initial();
    printf("running as root: %s\n", geteuid() == 0 ? "yes" : "no");
    printf("== textdomain\n");
    child(s_textdomain, NULL, 20, 0);
    printf("== bindtextdomain\n");
    child(s_bind, NULL, 20, 0);
    s_codesets();
    s_selection();
    s_variants();
    s_categories();
    s_plurals();
    printf("== contexts and special msgids\n");
    child(s_ctx, NULL, 20, 0);
    printf("== msgfmt catalogs, hash table and search\n");
    child(s_msgfmt, NULL, 60, 0);
    printf("-- msgfmt catalog big.mo with its hash table altered (each in a child)\n");
    for (int w = 0; hash_what[w]; w++)
        if (hash_what[w][0] != '~')
            child(s_hash_alter, (void *)(intptr_t)w, 10, 0);
    child(s_msgfmt_more, NULL, 10, 0);
    s_damage();
    printf("== _nl_msg_cat_cntr (whether it changed after each step)\n");
    child(s_counter, NULL, 20, 0);
    printf("== errno across the gettext family\n");
    child(s_errno, NULL, 20, 0);
    cat_writer_check();
    printf("== catopen/catgets/catclose on a gencat catalog\n");
    child(s_catgets_main, NULL, 20, 0);
    s_nlspath();
    child(s_nlspath_change, NULL, 10, 0);
    s_cat_damage();
    printf("done\n");
    return 0;
}
