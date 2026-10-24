/* printf end to end (spec §12.6; D-12.4, D-12.5, D-12.12): built against the host C library and
 * against Liella's products, the outputs must match line for line (printf_products.rs). Each
 * line is one formatting: the format, the return value and the text. */
#define _GNU_SOURCE
#include <errno.h>
#include <float.h>
#include <limits.h>
#include <locale.h>
#include <math.h>
#include <stdarg.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/wait.h>
#include <fcntl.h>
#include <unistd.h>
#include <wchar.h>

static char obuf[1 << 16];
static size_t used;
static void flush_out(void) { if (used) { (void)!write(1, obuf, used); used = 0; } }
static void ch(char c) { if (used == sizeof obuf) flush_out(); obuf[used++] = c; }
static void str(const char *s) { while (*s) ch(*s++); }
static void num(long v) {
    char b[24]; int n = 0; unsigned long u = v < 0 ? -(unsigned long)v : (unsigned long)v;
    if (v < 0) ch('-');
    do { b[n++] = (char)('0' + u % 10); u /= 10; } while (u);
    while (n) ch(b[--n]);
}
static void text(const char *p, size_t n) {
    ch('[');
    for (size_t i = 0; i < n; i++) {
        unsigned char c = (unsigned char)p[i];
        if (c >= 32 && c < 127 && c != '\\') ch((char)c);
        else { const char *d = "0123456789abcdef"; ch('\\'); ch(d[c >> 4]); ch(d[c & 15]); }
    }
    ch(']');
}

static char buf[4096];
/* One formatting through vsnprintf: the format, the result and the text. */
static void show(const char *label, const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    memset(buf, 'Z', sizeof buf);
    errno = 0;
    int r = vsnprintf(buf, sizeof buf, fmt, ap);
    va_end(ap);
    str(label); ch(' '); text(fmt, strlen(fmt)); str(" -> "); num(r);
    if (r < 0) { str(" errno="); num(errno); }
    ch(' '); text(buf, r >= 0 && (size_t)r < sizeof buf ? (size_t)r : strlen(buf)); ch('\n');
}

static const char *iflags[] = { "", "-", "+", " ", "#", "0", "-0", "+ ", "#0", "'", NULL };
static const char *widths[] = { "", "1", "8", NULL };
static const char *precs[] = { "", ".", ".0", ".1", ".5", NULL };

static void integers(void) {
    str("== integers\n");
    const char *convs = "diouxXbB";
    long vals[] = { 0, 1, -1, 42, -42, 255, 65535, 2147483647L, -2147483648L, 4294967295L, LONG_MAX, LONG_MIN };
    for (const char *c = convs; *c; c++)
        for (int f = 0; iflags[f]; f++)
            for (int w = 0; widths[w]; w++)
                for (int p = 0; precs[p]; p++) {
                    char fmt[32];
                    for (int v = 0; v < 12; v++) {
                        snprintf(fmt, sizeof fmt, "%%%s%s%s%c", iflags[f], widths[w], precs[p], *c);
                        show("i", fmt, (int)vals[v]);
                        snprintf(fmt, sizeof fmt, "%%%s%s%sl%c", iflags[f], widths[w], precs[p], *c);
                        show("l", fmt, vals[v]);
                    }
                }
    long long big = LLONG_MIN;
    show("ll", "%lld %llu %llx", big, (unsigned long long)big, (unsigned long long)big);
    show("hh", "%hhd %hhu %hhx %hhd", 300, 300, 300, -129);
    show("h", "%hd %hu %hx", 70000, 70000, -1);
    show("jzt", "%jd %zu %zd %td %ju", (intmax_t)-5, (size_t)SIZE_MAX, (ssize_t)-3, (ptrdiff_t)-7, (uintmax_t)UINTMAX_MAX);
    show("q", "%qd %Ld", -9LL, -10LL);
    show("w", "%w8d %w16u %w32x %w64d %wf8d %wf16d %wf32u %wf64x", 300, 70000, -1, LLONG_MIN, 300, -2, 7u, 255ULL);
}

static void floats(void) {
    str("== floats\n");
    const char *convs = "fFeEgGaA";
    double vals[] = { 0.0, -0.0, 1.0, -1.5, 0.1, 123456.789, 1e-7, 1e21, 5e-324, DBL_MAX, 0.5, 2.5, 9.9999999, INFINITY, -INFINITY, NAN };
    const char *fflags[] = { "", "-", "+", " ", "#", "0", "+0", "' ", NULL };
    const char *fprecs[] = { "", ".0", ".1", ".3", ".17", ".40", NULL };
    for (const char *c = convs; *c; c++)
        for (int f = 0; fflags[f]; f++)
            for (int p = 0; fprecs[p]; p++)
                for (int w = 0; widths[w]; w++)
                    for (int v = 0; v < 16; v++) {
                        char fmt[32];
                        snprintf(fmt, sizeof fmt, "%%%s%s%s%c", fflags[f], widths[w], fprecs[p], *c);
                        show("d", fmt, vals[v]);
                    }
    long double lv[] = { 0.0L, 1.0L, -1.5L, 0.1L, 1e-4000L, 1e4000L, LDBL_MAX, LDBL_MIN, LDBL_TRUE_MIN, 3.141592653589793238462643383279L };
    for (int v = 0; v < 10; v++) {
        show("L", "%Lf", lv[v]); show("L", "%.25Le", lv[v]); show("L", "%Lg", lv[v]); show("L", "%La", lv[v]); show("L", "%.3La", lv[v]);
    }
    show("big", "%.0f", 1e308);
    show("prec", "%.60f", 0.1);
}

static void chars_strings(void) {
    str("== text\n");
    show("c", "[%c|%5c|%-5c|%05c]", 'x', 'y', 'z', 'w');
    show("c", "%c%c%c", 0, 'a', 255);
    show("s", "[%s|%10s|%-10s|%.2s|%10.3s|%-10.3s|%.0s]", "abc", "abc", "abc", "abc", "abcdef", "abcdef", "x");
    show("s", "[%s|%.5s|%.6s|%10s]", (char *)NULL, (char *)NULL, (char *)NULL, (char *)NULL);
    char nonterm[3] = { 'a', 'b', 'c' };
    show("s", "%.3s", nonterm);
    show("p", "[%p|%20p|%-20p|%p|%+p|%#p]", (void *)0x1234, (void *)0x1234, (void *)0x1234, (void *)0, (void *)1, (void *)1);
    show("pct", "100%% [%5%] [%-5%]");
    show("bad", "[%y] [%-5y] [%k%d]", 7);
    show("trail", "end %");
    wchar_t w[] = { 'w', 'i', 'd', 'e', 0 };
    show("ls", "[%ls|%8ls|%.2ls|%lc|%C|%S]", w, w, w, (wint_t)'Q', (wint_t)'R', w);
    show("ls", "[%ls|%.3ls]", (wchar_t *)NULL, (wchar_t *)NULL);
    wchar_t uni[] = { 0xe9, 0x20ac, 0 };
    show("ls_c", "[%ls] [%lc]", uni, (wint_t)0xe9);
    /* %.Nls (C17 7.21.6.1): no more than N bytes, no partial character, and nothing converted
     * past the precision: an unconvertible character after it does not fail the call. */
    static const wchar_t bad3[] = {'a', 'b', 0xD800, 0}, bad2[] = {'a', 0xD800, 'c', 0}, euro[] = {'a', 0x20AC, 'b', 0};
    for (int k = 0; k < 2; k++) {
        if (k == 1 && !setlocale(LC_ALL, "C.UTF-8"))
            break;
        str(k ? "ls_prec C.UTF-8\n" : "ls_prec C\n");
        show("ls_prec", "[%.2ls]", bad3);
        show("ls_prec", "[%.3ls]", bad3);
        show("ls_prec", "[%.1ls]", bad2);
        show("ls_prec", "[%.2ls]", bad2);
        show("ls_prec", "[%ls]", bad3);
        show("ls_prec", "[%.0ls]", bad2 + 1);
        for (int p = 1; p <= 5; p++) {
            char f[16];
            snprintf(f, sizeof f, "[%%.%dls]", p);
            show("ls_prec", f, euro);
        }
    }
    setlocale(LC_ALL, "C");
    /* An array without a terminator, ending at an unmapped page: the precision bounds the read. */
    long pg = sysconf(_SC_PAGESIZE);
    char *m = mmap(NULL, (size_t)(2 * pg), PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (m != MAP_FAILED && mprotect(m + pg, (size_t)pg, PROT_NONE) == 0) {
        wchar_t *e = (wchar_t *)(m + pg - 2 * sizeof(wchar_t));
        e[0] = 'x';
        e[1] = 'y';
        show("ls_page", "[%.2ls|%.1ls]", e, e);
        munmap(m, (size_t)(2 * pg));
    }
}

static void positional(void) {
    str("== positional\n");
    show("pos", "%2$s %1$s %2$s", "world", "hello");
    show("pos", "%3$*1$.*2$f|", 10, 2, 3.14159);
    show("star", "[%*d|%-*d|%.*d|%*.*f]", 6, 42, 6, 42, 5, 42, 10, 3, 2.71828);
    show("star", "[%*d|%.*d|%.*f]", -6, 42, -1, 42, -1, 1.5);
    show("pos", "%1$d %1$x %1$o %1$c", 65);
    int n1 = -1, n2 = -1; short hn = -1; char cn = -1; long ln = -1;
    show("n", "abc%nde%n%hn%hhn%ln!", &n1, &n2, &hn, &cn, &ln);
    str("n values "); num(n1); ch(' '); num(n2); ch(' '); num(hn); ch(' '); num(cn); ch(' '); num(ln); ch('\n');
    errno = ENOENT;
    show("m", "[%m] [%.5m] [%20m]");
    errno = 9999;
    show("m", "[%m]");
    /* Positions up to NL_ARGMAX (4096); past it Liella fails with EINVAL (POSIX's range).
       Every position is used (a gap is undefined, and the fortified reference aborts on it):
       the zeros print nothing through `%k$.0d`. */
#define Z8 0, 0, 0, 0, 0, 0, 0, 0
#define Z64 Z8, Z8, Z8, Z8, Z8, Z8, Z8, Z8
#define Z512 Z64, Z64, Z64, Z64, Z64, Z64, Z64, Z64
#define Z4096 Z512, Z512, Z512, Z512, Z512, Z512, Z512, Z512
    static char af[12 * 4100];
    for (int top = 4096; top <= 4097; top++) {
        for (int star = 0; star <= 1; star++) {
            int at = star ? sprintf(af, "%%1$*%d$d|", top) : sprintf(af, "%%%d$.0d%%1$d|", top);
            for (int k = 2; k < top; k++) at += sprintf(af + at, "%%%d$.0d", k);
            show("argmax", af, 7, Z4096);
        }
    }
}

/* `*` widths in otherwise simple formats (ls -l's "%s %*s "): every sign and size of width, through
   vsnprintf and the fortified sprintf (volatile pointers: the library has to run). */
int __sprintf_chk(char *, int, size_t, const char *, ...);
static void star_simple(void) {
    static const int ws[] = { -12, -3, -1, 0, 1, 3, 12 };
    int (*volatile chk)(char *, int, size_t, const char *, ...) = __sprintf_chk;
    const char *volatile ls = "%s %*s ", *volatile mode = "-rw-r--r--", *volatile one = "1";
    for (unsigned k = 0; k < sizeof ws / sizeof ws[0]; k++) {
        int w = ws[k];
        show("star1", "%s %*s ", "drwxr-xr-x", w, "12");
        show("star1", "[%-*s|%0*d|%*lu|%*x|%*c|%*d]", w, "ab", w, -42, w, 7ul, w, 255u, w, 'q', w, -7);
        show("star1", "a%*sb%*dc%%%*s", w, (char *)0, -w, 5, w, "");
        memset(buf, 'Z', sizeof buf);
        int r = chk(buf, 1, sizeof buf, ls, mode, w, one);
        str("star1 chk -> "); num(r); ch(' '); text(buf, r >= 0 ? (size_t)r : 0); ch('\n');
    }
}

static void destinations(void) {
    str("== destinations\n");
    char small[8];
    memset(small, 'Z', sizeof small);
    int r = snprintf(small, sizeof small, "%s", "truncate me");
    str("snprintf_trunc "); num(r); ch(' '); text(small, sizeof small); ch('\n');
    r = snprintf(NULL, 0, "%d-%s", 12345, "measure");
    str("snprintf_null "); num(r); ch('\n');
    r = snprintf(small, 0, "%d", 5);
    str("snprintf_zero "); num(r); ch(' '); text(small, 1); ch('\n');
    { /* a format of exactly "%s": null, empty, truncation, exact fit, no room */
        char b[8]; const char *nul = 0;
        /* volatile: the compiler must not fold the call away, the library has to run */
        const char *volatile e = "", *volatile s7 = "1234567", *volatile s8 = "12345678", *volatile abc = "abc", *volatile abcdef = "abcdef", *volatile ms = "measure";
        r = snprintf(b, sizeof b, "%s", nul); str("s_null "); num(r); ch(' '); text(b, sizeof b); ch('\n');
        r = snprintf(b, sizeof b, "%s", e); str("s_empty "); num(r); ch(' '); text(b, sizeof b); ch('\n');
        r = snprintf(b, sizeof b, "%s", s7); str("s_fit "); num(r); ch(' '); text(b, sizeof b); ch('\n');
        r = snprintf(b, sizeof b, "%s", s8); str("s_over "); num(r); ch(' '); text(b, sizeof b); ch('\n');
        r = snprintf(b, 1, "%s", abc); str("s_one "); num(r); ch(' '); text(b, 1); ch('\n');
        r = snprintf(b, 4, "%s", nul); str("s_null4 "); num(r); ch(' '); text(b, 4); ch('\n');
        r = snprintf(b, sizeof b, "%.3s", abcdef); str("s_prec "); num(r); ch(' '); text(b, sizeof b); ch('\n');
        r = snprintf(b, sizeof b, "%.3s", nul); str("s_prec_null "); num(r); ch(' '); text(b, sizeof b); ch('\n');
        r = snprintf(NULL, 0, "%s", ms); str("s_measure "); num(r); ch('\n');
    }
    r = sprintf(buf, "%05.1f|%s", 3.14159, "sprintf");
    str("sprintf "); num(r); ch(' '); text(buf, (size_t)r); ch('\n');
    char *a = NULL;
    r = asprintf(&a, "%s=%d", "asprintf", 7);
    str("asprintf "); num(r); ch(' '); text(a, strlen(a)); ch('\n');
    free(a);
    fflush(stdout); flush_out();
    r = printf("printf %d %s\n", 1, "stdout");
    fflush(stdout);
    str("printf_ret "); num(r); ch('\n'); flush_out();
    r = dprintf(1, "dprintf %x\n", 255);
    str("dprintf_ret "); num(r); ch('\n');
    FILE *f = tmpfile();
    if (f) {
        r = fprintf(f, "%s %d %.2f", "file", -3, 2.5);
        long pos = ftell(f);
        rewind(f);
        char back[64] = {0};
        size_t k = fread(back, 1, sizeof back - 1, f);
        str("fprintf "); num(r); ch(' '); num(pos); ch(' '); text(back, k); ch('\n');
        fclose(f);
    }
    wchar_t wb[32];
    r = swprintf(wb, 32, L"%ls %d %s", L"wide", 9, "narrow");
    str("swprintf "); num(r); ch(' ');
    for (int i = 0; i < r; i++) ch((char)wb[i]);
    ch('\n');
    r = swprintf(wb, 4, L"%s", "toolong");
    str("swprintf_trunc "); num(r); ch('\n');
}

static void locales(void) {
    str("== locales\n");
    if (setlocale(LC_ALL, "de_DE.UTF-8")) {
        show("de", "%'d %'ld %'.2f %f %'g", 1234567, -9876543210L, 1234567.891, 1.5, 1234567.0);
        show("de", "%'15d|%'-15d|%'015d", 1234567, 1234567, 1234567);
        show("de_ls", "%ls %lc", L"\xe9t\xe9", (wint_t)0x20ac);
    } else str("de unavailable\n");
    if (setlocale(LC_ALL, "ps_AF.UTF-8")) {
        show("ps", "%f %'d %Id", 3.5, 1234567, 12345);
    } else str("ps unavailable\n");
    if (setlocale(LC_ALL, "en_US.UTF-8")) {
        show("en", "%'d %'.3f %'e", 1234567, 1234567.5, 1234567.5);
    } else str("en unavailable\n");
    setlocale(LC_ALL, "C");
    show("C", "%'d %f", 1234567, 1.5);
}

/* Owner ruling R3 (2026-10-05): a width or precision beyond INT_MAX fails every member of the
 * family with EOVERFLOW. Only the result and errno are printed (what a failed call leaves in a
 * buffer is unspecified). */
static int big_va(int kind, const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    char b[64];
    char *a = NULL;
    int r = -2;
    switch (kind) {
    case 0: r = vsnprintf(b, sizeof b, fmt, ap); break;
    case 1: r = vsprintf(b, fmt, ap); break;
    case 2: r = vasprintf(&a, fmt, ap); break;
    case 3: { FILE *f = tmpfile(); r = vfprintf(f, fmt, ap); fclose(f); } break;
    case 4: { int fd = open("/dev/null", 1); r = vdprintf(fd, fmt, ap); close(fd); } break;
    case 5: r = vprintf(fmt, ap); break;
    }
    va_end(ap);
    return r;
}
static int big_wva(int kind, const wchar_t *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    wchar_t b[64];
    int r = -2;
    switch (kind) {
    case 0: r = vswprintf(b, 64, fmt, ap); break;
    case 1: { FILE *f = tmpfile(); r = vfwprintf(f, fmt, ap); fclose(f); } break;
    case 2: r = vwprintf(fmt, ap); break;
    }
    va_end(ap);
    return r;
}
static void big_line(const char *who, int r, int e) {
    str("big "); str(who); str(" -> "); num(r);
    if (r < 0) { str(" errno="); num(e); }
    ch('\n');
}
/* The stdout members run in a child whose stdout is /dev/null, so that nothing they might write
 * reaches the comparison and a wide orientation does not touch this program's stdout. */
static void big_child(const char *who, int wide, int kind, int prec) {
    int p[2];
    if (pipe(p) != 0) return;
    fflush(stdout); flush_out();
    pid_t c = fork();
    if (c == 0) {
        int fd = open("/dev/null", 1);
        dup2(fd, 1);
        errno = 0;
        int r;
        if (wide) r = kind ? big_wva(2, prec ? L"ab%.2147483648lsc" : L"ab%2147483648lsc", L"x") : (prec ? wprintf(L"ab%.2147483648lsc", L"x") : wprintf(L"ab%2147483648lsc", L"x"));
        else r = kind ? big_va(5, prec ? "ab%.2147483648sc" : "ab%2147483648sc", "x") : (prec ? printf("ab%.2147483648sc", "x") : printf("ab%2147483648sc", "x"));
        int msg[2] = {r, errno};
        (void)!write(p[1], msg, sizeof msg);
        _exit(0);
    }
    close(p[1]);
    int msg[2] = {-9, -9};
    (void)!read(p[0], msg, sizeof msg);
    close(p[0]);
    waitpid(c, NULL, 0);
    big_line(who, msg[0], msg[1]);
}
static void big_counts(void) {
    str("== counts beyond INT_MAX\n");
    for (int prec = 1; prec >= 0; prec--) {
        const char *f = prec ? "ab%.2147483648sc" : "ab%2147483648sc";
        const wchar_t *wf = prec ? L"ab%.2147483648lsc" : L"ab%2147483648lsc";
        str(prec ? "precision\n" : "width\n");
        char b[64];
        char *a = NULL;
        int r;
        errno = 0; r = snprintf(b, sizeof b, f, "x"); big_line("snprintf", r, errno);
        errno = 0; r = big_va(0, f, "x"); big_line("vsnprintf", r, errno);
        errno = 0; r = sprintf(b, f, "x"); big_line("sprintf", r, errno);
        errno = 0; r = big_va(1, f, "x"); big_line("vsprintf", r, errno);
        errno = 0; r = asprintf(&a, f, "x"); big_line("asprintf", r, errno);
        errno = 0; r = big_va(2, f, "x"); big_line("vasprintf", r, errno);
        FILE *t = tmpfile();
        errno = 0; r = fprintf(t, f, "x"); big_line("fprintf", r, errno);
        fclose(t);
        errno = 0; r = big_va(3, f, "x"); big_line("vfprintf", r, errno);
        int fd = open("/dev/null", 1);
        errno = 0; r = dprintf(fd, f, "x"); big_line("dprintf", r, errno);
        close(fd);
        errno = 0; r = big_va(4, f, "x"); big_line("vdprintf", r, errno);
        big_child("printf", 0, 0, prec);
        big_child("vprintf", 0, 1, prec);
        wchar_t wb[64];
        errno = 0; r = swprintf(wb, 64, wf, L"x"); big_line("swprintf", r, errno);
        errno = 0; r = big_wva(0, wf, L"x"); big_line("vswprintf", r, errno);
        t = tmpfile();
        errno = 0; r = fwprintf(t, wf, L"x"); big_line("fwprintf", r, errno);
        fclose(t);
        errno = 0; r = big_wva(1, wf, L"x"); big_line("vfwprintf", r, errno);
        big_child("wprintf", 1, 0, prec);
        big_child("vwprintf", 1, 1, prec);
    }
    /* A precision of exactly INT_MAX is allowed. */
    char b[64];
    errno = 0; int r = snprintf(b, sizeof b, "ab%.2147483647sc", "x"); big_line("snprintf INT_MAX precision", r, errno);
}

int main(void) {
    big_counts();
    integers();
    floats();
    chars_strings();
    positional();
    star_simple();
    destinations();
    locales();
    flush_out();
    return 0;
}
