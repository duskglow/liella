/* Corpus harvest shim (validation item C): an LD_PRELOAD library for the dynamically linked
 * glibc builds of the validation packages. It interposes the regex entry points (regcomp,
 * regexec, regfree, re_compile_pattern, re_search, re_match) and the printf family (the plain,
 * v, fortified __*_chk forms, dprintf, asprintf), logs each call's inputs and glibc's results,
 * and then performs the call through the next definition (RTLD_NEXT). Black box: it only sees
 * arguments and results at the interface.
 *
 * Environment:
 *   HARVEST_DIR   directory for the logs (one file per process, harvest-<pid>.log);
 *   HARVEST_TREE  only processes whose executable lies under this path log anything (the
 *                 package's build tree; make, sh, diff and the rest pass through silently);
 *   HARVEST_BUILD a label written into every log (which build the calls came from).
 *
 * The shim never calls the printf family itself (it formats by hand and writes with write(2)),
 * keeps its descriptor at 900 or above with FD_CLOEXEC (tests check descriptors), saves and
 * restores errno, and keeps at most HARVEST_PER_FORMAT (200) records per format string and
 * process, so loops such as seq's do not flood the logs. Duplicate records within a process are
 * dropped by a hash set; cross-process duplicates are removed by harvest_reduce.py.
 *
 * Log lines (tab-separated fields; strings escaped by esc(): bytes 0x21..0x7e except '\' as
 * themselves, everything else as \xHH):
 *   B  <build label> <exe>
 *   RC <id> <cflags> <pattern> <ret>
 *   RE <id> <eflags> <nmatch> <subject> <startend so,eo or -> <ret> <so,eo;...>
 *   RP <id> <syntax> <translate hex or -> <pattern> <result message or ->
 *   RS <id> <not_bol> <not_eol> <newline_anchor> <subject> <start> <range> <regs?> <ret> <so,eo;...>
 *   RM <id> <not_bol> <not_eol> <newline_anchor> <subject> <start> <regs?> <ret> <so,eo;...>
 *   PF <func> <locale> <errno> <format> <arg>...   arg: i:<int64> u:<uint64> d:<f64 bits hex>
 *      L:<x87 10 bytes hex> s:<escaped> S:<wide code points hex,...> N (NULL string) n (%n)
 *      p:<hex>
 *   PU <func> <format>    a format the parser does not understand (not replayed)
 */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <fcntl.h>
#include <locale.h>
#include <regex.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <wchar.h>

#define HARVEST_PER_FORMAT 200

/* regexec's header declares pmatch as a variable-length array parameter. */
#pragma GCC diagnostic ignored "-Wvla-parameter"

static int enabled = -1;
static int logfd = -1;
static __thread int busy;

/* ------------------------------------------------------------------ output buffer */

struct buf {
    char d[65536];
    size_t n;
    int over;
};

static void put(struct buf *b, const char *s, size_t n)
{
    if (b->n + n > sizeof b->d - 2) {
        b->over = 1;
        return;
    }
    memcpy(b->d + b->n, s, n);
    b->n += n;
}

static void str(struct buf *b, const char *s) { put(b, s, strlen(s)); }

static void hexbyte(struct buf *b, unsigned char c)
{
    static const char h[] = "0123456789abcdef";
    char x[2] = {h[c >> 4], h[c & 15]};
    put(b, x, 2);
}

static void esc(struct buf *b, const char *s, size_t n)
{
    if (n == 0) {
        str(b, "\\e"); /* the empty string, so that a field is never empty */
        return;
    }
    for (size_t i = 0; i < n; i++) {
        unsigned char c = (unsigned char)s[i];
        if (c > 0x20 && c < 0x7f && c != '\\') {
            put(b, (const char *)&c, 1);
        } else {
            str(b, "\\x");
            hexbyte(b, c);
        }
    }
}

static void u64(struct buf *b, uint64_t v)
{
    char t[24];
    int i = 0;
    do {
        t[i++] = (char)('0' + v % 10);
        v /= 10;
    } while (v);
    while (i)
        put(b, &t[--i], 1);
}

static void i64(struct buf *b, int64_t v)
{
    if (v < 0) {
        put(b, "-", 1);
        u64(b, (uint64_t)0 - (uint64_t)v);
    } else {
        u64(b, (uint64_t)v);
    }
}

static void hex64(struct buf *b, uint64_t v)
{
    for (int i = 7; i >= 0; i--)
        hexbyte(b, (unsigned char)(v >> (i * 8)));
}

static void tab(struct buf *b) { put(b, "\t", 1); }

/* ------------------------------------------------------------------ dedup */

static uint64_t seen[1 << 16];
static struct { uint64_t h; unsigned n; } perfmt[1 << 12];

static uint64_t fnv(const char *s, size_t n)
{
    uint64_t h = 1469598103934665603ull;
    for (size_t i = 0; i < n; i++)
        h = (h ^ (unsigned char)s[i]) * 1099511628211ull;
    return h | 1;
}

static int fresh(uint64_t h)
{
    size_t m = sizeof seen / sizeof seen[0];
    for (size_t i = h % m, k = 0; k < 64; i = (i + 1) % m, k++) {
        if (seen[i] == h)
            return 0;
        if (seen[i] == 0) {
            seen[i] = h;
            return 1;
        }
    }
    return 1;
}

static int under_cap(const char *fmt)
{
    uint64_t h = fnv(fmt, strlen(fmt));
    size_t m = sizeof perfmt / sizeof perfmt[0];
    for (size_t i = h % m, k = 0; k < 32; i = (i + 1) % m, k++) {
        if (perfmt[i].h == h)
            return ++perfmt[i].n <= HARVEST_PER_FORMAT;
        if (perfmt[i].h == 0) {
            perfmt[i].h = h;
            perfmt[i].n = 1;
            return 1;
        }
    }
    return 1;
}

static void emit(struct buf *b)
{
    if (b->over || logfd < 0)
        return;
    if (!fresh(fnv(b->d, b->n)))
        return;
    b->d[b->n++] = '\n';
    for (size_t off = 0; off < b->n;) {
        ssize_t w = write(logfd, b->d + off, b->n - off);
        if (w <= 0)
            return;
        off += (size_t)w;
    }
}

/* ------------------------------------------------------------------ setup */

static void setup(void)
{
    if (enabled != -1)
        return;
    enabled = 0;
    const char *dir = getenv("HARVEST_DIR"), *tree = getenv("HARVEST_TREE");
    if (!dir || !tree)
        return;
    char exe[4096];
    ssize_t n = readlink("/proc/self/exe", exe, sizeof exe - 1);
    if (n <= 0)
        return;
    exe[n] = 0;
    if (strncmp(exe, tree, strlen(tree)) != 0)
        return;
    struct buf b = {.n = 0};
    str(&b, dir);
    str(&b, "/harvest-");
    u64(&b, (uint64_t)getpid());
    str(&b, ".log");
    b.d[b.n] = 0;
    int fd = open(b.d, O_WRONLY | O_CREAT | O_APPEND | O_CLOEXEC, 0644);
    if (fd < 0)
        return;
    int hi = fcntl(fd, F_DUPFD_CLOEXEC, 900);
    close(fd);
    if (hi < 0)
        return;
    logfd = hi;
    enabled = 1;
    struct buf h = {.n = 0};
    str(&h, "B\t");
    const char *lab = getenv("HARVEST_BUILD");
    esc(&h, lab ? lab : "-", lab ? strlen(lab) : 1);
    tab(&h);
    esc(&h, exe, (size_t)n);
    emit(&h);
}

/* A fork child must not share the parent's dedup state's file: reopen per process. */
static pid_t owner;

static int on(void)
{
    if (busy)
        return 0;
    if (enabled == 1 && owner != getpid()) {
        close(logfd);
        logfd = -1;
        enabled = -1;
        memset(seen, 0, sizeof seen);
        memset(perfmt, 0, sizeof perfmt);
    }
    if (enabled == -1) {
        setup();
        owner = getpid();
    }
    return enabled == 1;
}

#define NEXT(name) ((__typeof__(&name))dlsym(RTLD_NEXT, #name))

/* ------------------------------------------------------------------ regex */

static struct { const void *p; unsigned id; } ids[4096];
static unsigned next_id;

static unsigned id_of(const void *p, int create)
{
    size_t m = sizeof ids / sizeof ids[0];
    uintptr_t h = ((uintptr_t)p >> 4) % m;
    for (size_t k = 0; k < m; k++, h = (h + 1) % m) {
        if (ids[h].p == p) {
            if (create)
                ids[h].id = ++next_id;
            return ids[h].id;
        }
        if (ids[h].p == NULL) {
            if (!create)
                return 0;
            ids[h].p = p;
            ids[h].id = ++next_id;
            return ids[h].id;
        }
    }
    return 0;
}

static void regs_out(struct buf *b, const regmatch_t *m, size_t n)
{
    if (n == 0) {
        str(b, "-");
        return;
    }
    for (size_t i = 0; i < n; i++) {
        if (i)
            put(b, ";", 1);
        i64(b, m[i].rm_so);
        put(b, ",", 1);
        i64(b, m[i].rm_eo);
    }
}

int regcomp(regex_t *preg, const char *pattern, int cflags)
{
    int r = NEXT(regcomp)(preg, pattern, cflags);
    int e = errno;
    if (on()) {
        busy = 1;
        struct buf b = {.n = 0};
        str(&b, "RC\t");
        u64(&b, id_of(preg, 1));
        tab(&b);
        i64(&b, cflags);
        tab(&b);
        esc(&b, pattern, strlen(pattern));
        tab(&b);
        i64(&b, r);
        emit(&b);
        busy = 0;
    }
    errno = e;
    return r;
}

int regexec(const regex_t *preg, const char *string, size_t nmatch, regmatch_t *pmatch, int eflags)
{
    regmatch_t se = {-1, -1};
    if ((eflags & REG_STARTEND) && pmatch)
        se = pmatch[0];
    int r = NEXT(regexec)(preg, string, nmatch, pmatch, eflags);
    int e = errno;
    if (on() && string) {
        busy = 1;
        struct buf b = {.n = 0};
        str(&b, "RE\t");
        u64(&b, id_of(preg, 0));
        tab(&b);
        i64(&b, eflags);
        tab(&b);
        u64(&b, nmatch);
        tab(&b);
        size_t len = (eflags & REG_STARTEND) && pmatch ? (size_t)se.rm_eo : strlen(string);
        esc(&b, string, len);
        tab(&b);
        if (eflags & REG_STARTEND) {
            i64(&b, se.rm_so);
            put(&b, ",", 1);
            i64(&b, se.rm_eo);
        } else {
            str(&b, "-");
        }
        tab(&b);
        i64(&b, r);
        tab(&b);
        size_t k = (r == 0 && pmatch && !(preg->no_sub)) ? (nmatch > 32 ? 32 : nmatch) : 0;
        regs_out(&b, pmatch, k);
        emit(&b);
        busy = 0;
    }
    errno = e;
    return r;
}

const char *re_compile_pattern(const char *pattern, size_t length, struct re_pattern_buffer *bufp)
{
    unsigned char tr[256];
    int has_tr = bufp->translate != NULL;
    if (has_tr)
        memcpy(tr, bufp->translate, 256);
    reg_syntax_t *syn = dlsym(RTLD_DEFAULT, "re_syntax_options");
    reg_syntax_t s = syn ? *syn : 0;
    const char *r = NEXT(re_compile_pattern)(pattern, length, bufp);
    int e = errno;
    if (on()) {
        busy = 1;
        struct buf b = {.n = 0};
        str(&b, "RP\t");
        u64(&b, id_of(bufp, 1));
        tab(&b);
        u64(&b, (uint64_t)s);
        tab(&b);
        if (has_tr) {
            for (int i = 0; i < 256; i++)
                hexbyte(&b, tr[i]);
        } else {
            str(&b, "-");
        }
        tab(&b);
        esc(&b, pattern, length);
        tab(&b);
        if (r)
            esc(&b, r, strlen(r));
        else
            str(&b, "-");
        emit(&b);
        busy = 0;
    }
    errno = e;
    return r;
}

static void regs_of(struct buf *b, struct re_registers *regs, regoff_t r, size_t nsub)
{
    if (!regs || r < 0) {
        str(b, "-");
        return;
    }
    size_t n = nsub + 1 < regs->num_regs ? nsub + 1 : regs->num_regs;
    for (size_t i = 0; i < n && i < 32; i++) {
        if (i)
            put(b, ";", 1);
        i64(b, regs->start[i]);
        put(b, ",", 1);
        i64(b, regs->end[i]);
    }
}

regoff_t re_search(struct re_pattern_buffer *bufp, const char *string, regoff_t size, regoff_t start, regoff_t range, struct re_registers *regs)
{
    int nb = bufp->not_bol, ne = bufp->not_eol, na = bufp->newline_anchor;
    regoff_t r = NEXT(re_search)(bufp, string, size, start, range, regs);
    int e = errno;
    if (on()) {
        busy = 1;
        struct buf b = {.n = 0};
        str(&b, "RS\t");
        u64(&b, id_of(bufp, 0));
        tab(&b);
        i64(&b, nb);
        tab(&b);
        i64(&b, ne);
        tab(&b);
        i64(&b, na);
        tab(&b);
        esc(&b, string, size > 0 ? (size_t)size : 0);
        tab(&b);
        i64(&b, start);
        tab(&b);
        i64(&b, range);
        tab(&b);
        str(&b, regs ? "1" : "0");
        tab(&b);
        i64(&b, r);
        tab(&b);
        regs_of(&b, regs, r, bufp->re_nsub);
        emit(&b);
        busy = 0;
    }
    errno = e;
    return r;
}

regoff_t re_match(struct re_pattern_buffer *bufp, const char *string, regoff_t size, regoff_t start, struct re_registers *regs)
{
    int nb = bufp->not_bol, ne = bufp->not_eol, na = bufp->newline_anchor;
    regoff_t r = NEXT(re_match)(bufp, string, size, start, regs);
    int e = errno;
    if (on()) {
        busy = 1;
        struct buf b = {.n = 0};
        str(&b, "RM\t");
        u64(&b, id_of(bufp, 0));
        tab(&b);
        i64(&b, nb);
        tab(&b);
        i64(&b, ne);
        tab(&b);
        i64(&b, na);
        tab(&b);
        esc(&b, string, size > 0 ? (size_t)size : 0);
        tab(&b);
        i64(&b, start);
        tab(&b);
        str(&b, regs ? "1" : "0");
        tab(&b);
        i64(&b, r);
        tab(&b);
        regs_of(&b, regs, r, bufp->re_nsub);
        emit(&b);
        busy = 0;
    }
    errno = e;
    return r;
}

/* ------------------------------------------------------------------ printf */

enum ty { T_NONE, T_INT, T_LONG, T_LLONG, T_SIZE, T_PTRDIFF, T_INTMAX, T_DBL, T_LDBL, T_STR, T_WSTR, T_PTR, T_N, T_WINT, T_CHAR };

/* Parse one conversion at f (just after '%'). Fills the types of the arguments it consumes (in
 * positional slots when positional) and returns the length parsed, or 0 if not understood. */
struct spec {
    int pos, wpos, ppos;      /* 1-based positions, 0 when sequential */
    int wstar, pstar, hasprec;
    long prec;
    enum ty t;
    int is_unsigned;
};

static size_t parse(const char *f, struct spec *s)
{
    const char *p = f;
    memset(s, 0, sizeof *s);
    s->prec = -1;
    if (*p == '%')
        return 0; /* "%%" handled by the caller */
    const char *q = p;
    int n = 0;
    while (*q >= '0' && *q <= '9')
        n = n * 10 + (*q++ - '0');
    if (*q == '$' && q > p) {
        s->pos = n;
        p = q + 1;
    }
    while (strchr("-+ #0'I", *p) && *p)
        p++;
    if (*p == '*') {
        s->wstar = 1;
        p++;
        q = p;
        n = 0;
        while (*q >= '0' && *q <= '9')
            n = n * 10 + (*q++ - '0');
        if (*q == '$' && q > p) {
            s->wpos = n;
            p = q + 1;
        }
    } else {
        while (*p >= '0' && *p <= '9')
            p++;
    }
    if (*p == '.') {
        p++;
        s->hasprec = 1;
        if (*p == '*') {
            s->pstar = 1;
            p++;
            q = p;
            n = 0;
            while (*q >= '0' && *q <= '9')
                n = n * 10 + (*q++ - '0');
            if (*q == '$' && q > p) {
                s->ppos = n;
                p = q + 1;
            }
        } else {
            long v = 0;
            while (*p >= '0' && *p <= '9')
                v = v * 10 + (*p++ - '0');
            s->prec = v;
        }
    }
    int len = 0; /* 1 h, 2 hh, 3 l, 4 ll, 5 j, 6 z, 7 t, 8 L */
    if (p[0] == 'h' && p[1] == 'h') { len = 2; p += 2; }
    else if (p[0] == 'h') { len = 1; p++; }
    else if (p[0] == 'l' && p[1] == 'l') { len = 4; p += 2; }
    else if (p[0] == 'l') { len = 3; p++; }
    else if (p[0] == 'q') { len = 4; p++; }
    else if (p[0] == 'j') { len = 5; p++; }
    else if (p[0] == 'z' || p[0] == 'Z') { len = 6; p++; }
    else if (p[0] == 't') { len = 7; p++; }
    else if (p[0] == 'L') { len = 8; p++; }
    char c = *p++;
    switch (c) {
    case 'd': case 'i': case 'u': case 'o': case 'x': case 'X': case 'b': case 'B':
        s->is_unsigned = c != 'd' && c != 'i';
        s->t = len == 3 ? T_LONG : len == 4 ? T_LLONG : len == 5 ? T_INTMAX : len == 6 ? T_SIZE : len == 7 ? T_PTRDIFF : len == 8 ? T_LLONG : T_INT;
        break;
    case 'f': case 'F': case 'e': case 'E': case 'g': case 'G': case 'a': case 'A':
        s->t = len == 8 ? T_LDBL : T_DBL;
        break;
    case 'c':
        s->t = len == 3 ? T_WINT : T_CHAR;
        break;
    case 'C':
        s->t = T_WINT;
        break;
    case 's':
        s->t = len == 3 ? T_WSTR : T_STR;
        break;
    case 'S':
        s->t = T_WSTR;
        break;
    case 'p':
        s->t = T_PTR;
        break;
    case 'n':
        s->t = T_N;
        break;
    case 'm':
        s->t = T_NONE;
        break;
    default:
        return 0;
    }
    return (size_t)(p - f);
}

#define MAXARGS 64

struct arg {
    enum ty t;
    union {
        int64_t i;
        uint64_t u;
        double d;
        long double ld;
        const void *p;
    } v;
};

static int collect(const char *fmt, va_list ap, struct arg *a, int *na)
{
    enum ty types[MAXARGS + 1];
    memset(types, 0, sizeof types);
    int seq = 0, positional = -1, maxpos = 0;
    for (const char *f = fmt; *f; f++) {
        if (*f != '%')
            continue;
        if (f[1] == '%') {
            f++;
            continue;
        }
        struct spec s;
        size_t n = parse(f + 1, &s);
        if (n == 0)
            return -1;
        int isp = s.pos != 0;
        if (positional == -1)
            positional = isp;
        else if (positional != isp && s.t != T_NONE)
            return -1;
        if (positional) {
            if (s.wstar) { if (!s.wpos || s.wpos > MAXARGS) return -1; types[s.wpos] = T_INT; if (s.wpos > maxpos) maxpos = s.wpos; }
            if (s.pstar) { if (!s.ppos || s.ppos > MAXARGS) return -1; types[s.ppos] = T_INT; if (s.ppos > maxpos) maxpos = s.ppos; }
            if (s.t != T_NONE) { if (s.pos > MAXARGS) return -1; types[s.pos] = s.t; if (s.pos > maxpos) maxpos = s.pos; }
        } else {
            if (s.wstar) { if (seq >= MAXARGS) return -1; types[++seq] = T_INT; }
            if (s.pstar) { if (seq >= MAXARGS) return -1; types[++seq] = T_INT; }
            if (s.t != T_NONE) { if (seq >= MAXARGS) return -1; types[++seq] = s.t; }
        }
        f += n;
    }
    int count = positional == 1 ? maxpos : seq;
    va_list c;
    va_copy(c, ap);
    for (int i = 1; i <= count; i++) {
        a[i - 1].t = types[i];
        switch (types[i]) {
        case T_INT: case T_CHAR: case T_WINT: a[i - 1].v.i = va_arg(c, int); break;
        case T_LONG: case T_LLONG: case T_SIZE: case T_PTRDIFF: case T_INTMAX: a[i - 1].v.i = va_arg(c, long long); break;
        case T_DBL: a[i - 1].v.d = va_arg(c, double); break;
        case T_LDBL: a[i - 1].v.ld = va_arg(c, long double); break;
        case T_STR: case T_WSTR: case T_PTR: case T_N: a[i - 1].v.p = va_arg(c, void *); break;
        default:
            va_end(c);
            return -1; /* a gap in the positions */
        }
    }
    va_end(c);
    *na = count;
    return 0;
}

/* The precision that applies to the string argument at position i (for a bounded read). */
static long str_prec(const char *fmt, const struct arg *a, int na, int idx)
{
    int seq = 0;
    for (const char *f = fmt; *f; f++) {
        if (*f != '%')
            continue;
        if (f[1] == '%') {
            f++;
            continue;
        }
        struct spec s;
        size_t n = parse(f + 1, &s);
        if (n == 0)
            return -1;
        long prec = s.prec;
        if (s.pstar) {
            int pi = s.ppos ? s.ppos : (s.wstar ? seq + 2 : seq + 1);
            prec = pi >= 1 && pi <= na ? a[pi - 1].v.i : -1;
        }
        if (!s.pos) {
            if (s.wstar)
                seq++;
            if (s.pstar)
                seq++;
            if (s.t != T_NONE)
                seq++;
        }
        int at = s.pos ? s.pos : seq;
        if (s.t != T_NONE && at == idx)
            return s.hasprec ? prec : -1;
        f += n;
    }
    return -1;
}

static void log_printf(const char *func, const char *fmt, va_list ap)
{
    if (!fmt || !on())
        return;
    busy = 1;
    int saved = errno;
    if (!under_cap(fmt)) {
        busy = 0;
        return;
    }
    struct buf *b = malloc(sizeof *b);
    if (!b) {
        busy = 0;
        return;
    }
    b->n = 0;
    b->over = 0;
    struct arg a[MAXARGS];
    int na = 0;
    if (collect(fmt, ap, a, &na) != 0) {
        str(b, "PU\t");
        str(b, func);
        tab(b);
        esc(b, fmt, strlen(fmt));
        emit(b);
        free(b);
        busy = 0;
        return;
    }
    str(b, "PF\t");
    str(b, func);
    tab(b);
    const char *loc = setlocale(LC_ALL, NULL);
    esc(b, loc ? loc : "-", loc ? strlen(loc) : 1);
    tab(b);
    i64(b, saved);
    tab(b);
    esc(b, fmt, strlen(fmt));
    for (int i = 0; i < na; i++) {
        tab(b);
        switch (a[i].t) {
        case T_INT: case T_CHAR: case T_WINT: case T_LONG: case T_LLONG: case T_SIZE: case T_PTRDIFF: case T_INTMAX:
            str(b, "i:");
            i64(b, a[i].v.i);
            break;
        case T_DBL: {
            uint64_t bits;
            memcpy(&bits, &a[i].v.d, 8);
            str(b, "d:");
            hex64(b, bits);
            break;
        }
        case T_LDBL: {
            unsigned char x[16] = {0};
            memcpy(x, &a[i].v.ld, 10);
            str(b, "L:");
            for (int k = 0; k < 10; k++)
                hexbyte(b, x[k]);
            break;
        }
        case T_STR: {
            if (!a[i].v.p) {
                str(b, "N");
                break;
            }
            long pr = str_prec(fmt, a, na, i + 1);
            size_t len = pr >= 0 ? strnlen(a[i].v.p, (size_t)pr) : strlen(a[i].v.p);
            str(b, "s:");
            esc(b, a[i].v.p, len);
            break;
        }
        case T_WSTR: {
            if (!a[i].v.p) {
                str(b, "N");
                break;
            }
            long pr = str_prec(fmt, a, na, i + 1);
            const wchar_t *w = a[i].v.p;
            str(b, "S:");
            /* A wide string with a precision is read no further than the precision's bytes need;
             * log at most that many characters (each converts to at least one byte). */
            size_t lim = pr >= 0 ? (size_t)pr : (size_t)-1;
            size_t k = 0;
            for (; k < lim && w[k]; k++) {
                if (k)
                    put(b, ",", 1);
                hex64(b, (uint32_t)w[k]);
            }
            if (k == 0)
                str(b, "-");
            break;
        }
        case T_PTR:
            str(b, "p:");
            hex64(b, (uint64_t)(uintptr_t)a[i].v.p);
            break;
        case T_N:
            str(b, "n");
            break;
        default:
            str(b, "?");
        }
    }
    emit(b);
    free(b);
    errno = saved;
    busy = 0;
}

#define VA_HOOK(func, logname, rettype, params, call_next, last)   \
    rettype func params                                           \
    {                                                             \
        va_list ap;                                               \
        va_start(ap, last);                                       \
        log_printf(logname, last, ap);                            \
        rettype r = call_next;                                    \
        va_end(ap);                                               \
        return r;                                                 \
    }

#define V_HOOK(func, logname, rettype, params, call_next, fmtv)   \
    rettype func params                                           \
    {                                                             \
        log_printf(logname, fmtv, ap);                            \
        return call_next;                                         \
    }

VA_HOOK(printf, "printf", int, (const char *fmt, ...), NEXT(vprintf)(fmt, ap), fmt)
VA_HOOK(fprintf, "fprintf", int, (FILE *fp, const char *fmt, ...), NEXT(vfprintf)(fp, fmt, ap), fmt)
VA_HOOK(sprintf, "sprintf", int, (char *s, const char *fmt, ...), NEXT(vsprintf)(s, fmt, ap), fmt)
VA_HOOK(snprintf, "snprintf", int, (char *s, size_t n, const char *fmt, ...), NEXT(vsnprintf)(s, n, fmt, ap), fmt)
VA_HOOK(dprintf, "dprintf", int, (int fd, const char *fmt, ...), NEXT(vdprintf)(fd, fmt, ap), fmt)
VA_HOOK(asprintf, "asprintf", int, (char **s, const char *fmt, ...), NEXT(vasprintf)(s, fmt, ap), fmt)
V_HOOK(vprintf, "vprintf", int, (const char *fmt, va_list ap), NEXT(vprintf)(fmt, ap), fmt)
V_HOOK(vfprintf, "vfprintf", int, (FILE *fp, const char *fmt, va_list ap), NEXT(vfprintf)(fp, fmt, ap), fmt)
V_HOOK(vsprintf, "vsprintf", int, (char *s, const char *fmt, va_list ap), NEXT(vsprintf)(s, fmt, ap), fmt)
V_HOOK(vsnprintf, "vsnprintf", int, (char *s, size_t n, const char *fmt, va_list ap), NEXT(vsnprintf)(s, n, fmt, ap), fmt)
V_HOOK(vdprintf, "vdprintf", int, (int fd, const char *fmt, va_list ap), NEXT(vdprintf)(fd, fmt, ap), fmt)
V_HOOK(vasprintf, "vasprintf", int, (char **s, const char *fmt, va_list ap), NEXT(vasprintf)(s, fmt, ap), fmt)

/* The fortified forms (-O2 builds with _FORTIFY_SOURCE call these). */
extern int __vprintf_chk(int, const char *, va_list);
extern int __vfprintf_chk(FILE *, int, const char *, va_list);
extern int __vsprintf_chk(char *, int, size_t, const char *, va_list);
extern int __vsnprintf_chk(char *, size_t, int, size_t, const char *, va_list);
extern int __vdprintf_chk(int, int, const char *, va_list);
extern int __vasprintf_chk(char **, int, const char *, va_list);

VA_HOOK(__printf_chk, "printf", int, (int fl, const char *fmt, ...), NEXT(__vprintf_chk)(fl, fmt, ap), fmt)
VA_HOOK(__fprintf_chk, "fprintf", int, (FILE *fp, int fl, const char *fmt, ...), NEXT(__vfprintf_chk)(fp, fl, fmt, ap), fmt)
VA_HOOK(__sprintf_chk, "sprintf", int, (char *s, int fl, size_t sl, const char *fmt, ...), NEXT(__vsprintf_chk)(s, fl, sl, fmt, ap), fmt)
VA_HOOK(__snprintf_chk, "snprintf", int, (char *s, size_t n, int fl, size_t sl, const char *fmt, ...), NEXT(__vsnprintf_chk)(s, n, fl, sl, fmt, ap), fmt)
VA_HOOK(__dprintf_chk, "dprintf", int, (int fd, int fl, const char *fmt, ...), NEXT(__vdprintf_chk)(fd, fl, fmt, ap), fmt)
VA_HOOK(__asprintf_chk, "asprintf", int, (char **s, int fl, const char *fmt, ...), NEXT(__vasprintf_chk)(s, fl, fmt, ap), fmt)
V_HOOK(__vprintf_chk, "vprintf", int, (int fl, const char *fmt, va_list ap), NEXT(__vprintf_chk)(fl, fmt, ap), fmt)
V_HOOK(__vfprintf_chk, "vfprintf", int, (FILE *fp, int fl, const char *fmt, va_list ap), NEXT(__vfprintf_chk)(fp, fl, fmt, ap), fmt)
V_HOOK(__vsprintf_chk, "vsprintf", int, (char *s, int fl, size_t sl, const char *fmt, va_list ap), NEXT(__vsprintf_chk)(s, fl, sl, fmt, ap), fmt)
V_HOOK(__vsnprintf_chk, "vsnprintf", int, (char *s, size_t n, int fl, size_t sl, const char *fmt, va_list ap), NEXT(__vsnprintf_chk)(s, n, fl, sl, fmt, ap), fmt)
V_HOOK(__vdprintf_chk, "vdprintf", int, (int fd, int fl, const char *fmt, va_list ap), NEXT(__vdprintf_chk)(fd, fl, fmt, ap), fmt)
V_HOOK(__vasprintf_chk, "vasprintf", int, (char **s, int fl, const char *fmt, va_list ap), NEXT(__vasprintf_chk)(s, fl, fmt, ap), fmt)
