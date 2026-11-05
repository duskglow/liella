/* Spec §12.6: formatted output through real C varargs, into every destination. Self-checking:
 * each clause ends with `end <clause>` on standard error, each failed check prints
 * `FAIL <clause>: line N: <check>`; the clause tests in crates/core/tests/s12_stdio_clauses.rs
 * read these lines (and standard output, which holds exactly what the `printf` clause wrote). */
#undef _FORTIFY_SOURCE /* the checks call the plain functions; __fprintf_chk is called by name */
#define _GNU_SOURCE
#include <errno.h>
#include <limits.h>
#include <locale.h>
#include <obstack.h>
#include <signal.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/resource.h>
#include <sys/socket.h>
#include <sys/syscall.h>
#include <sys/wait.h>
#include <unistd.h>
#include <wchar.h>
#include "expect_crash.h"

#define obstack_chunk_alloc malloc
#define obstack_chunk_free free

extern int __fprintf_chk(FILE *, int, const char *, ...);
extern int __sprintf_chk(char *, int, size_t, const char *, ...);

static int failures;
static const char *current;

static void begin(const char *clause) { current = clause; }
static void end(void) { fprintf(stderr, "end %s\n", current); }

#define CHECK(cond) do { if (!(cond)) { failures++; fprintf(stderr, "FAIL %s: line %d: %s\n", current, __LINE__, #cond); } } while (0)

/* Whether `got` is `want`, showing both when not. */
static int eq(const char *got, const char *want)
{
    if (strcmp(got, want) == 0)
        return 1;
    fprintf(stderr, "  got  [%s]\n  want [%s]\n", got, want);
    return 0;
}

/* What a format of two strings (ls -l's "%s %*s ") gives, built by hand: `head`, `s0`, `mid`,
 * at most `prec` bytes of `s1` (all of it when `prec` < 0) in a field of `width` bytes
 * (left-justified when `minus` or `width` < 0, its size then -`width`), `tail`. Widths and
 * precisions count bytes, so a multibyte character counts as its bytes. */
static void pair_want(char *w, const char *head, const char *s0, const char *mid, const char *s1, int width, int minus, int prec, const char *tail)
{
    size_t n1 = strlen(s1);
    int left = minus || width < 0;
    size_t fw = width < 0 ? (size_t)(-(long)width) : (size_t)width;
    if (prec >= 0 && (size_t)prec < n1)
        n1 = (size_t)prec;
    size_t pad = fw > n1 ? fw - n1 : 0;
    w[0] = 0;
    strcat(w, head);
    strcat(w, s0);
    strcat(w, mid);
    char *p = w + strlen(w);
    if (left) {
        memcpy(p, s1, n1);
        p += n1;
    }
    memset(p, ' ', pad);
    p += pad;
    if (!left) {
        memcpy(p, s1, n1);
        p += n1;
    }
    strcpy(p, tail);
}

/* sprintf, __sprintf_chk (object of exactly the result and its terminator) and snprintf with
 * room for the result give `want` and nothing past its terminator; snprintf with every smaller
 * bound gives the cut prefix, terminated, and nothing past the bound. All return the length. */
#define PAIR(want, ...) do { \
    char b_[192]; \
    int n_ = (int)strlen(want); \
    memset(b_, 0x55, sizeof b_); \
    CHECK(sprintf(b_, __VA_ARGS__) == n_ && eq(b_, want) && (unsigned char)b_[n_ + 1] == 0x55); \
    memset(b_, 0x55, sizeof b_); \
    CHECK(__sprintf_chk(b_, 1, (size_t)n_ + 1, __VA_ARGS__) == n_ && eq(b_, want) && (unsigned char)b_[n_ + 1] == 0x55); \
    memset(b_, 0x55, sizeof b_); \
    CHECK(snprintf(b_, sizeof b_ - 1, __VA_ARGS__) == n_ && eq(b_, want) && (unsigned char)b_[n_ + 1] == 0x55); \
    for (int k_ = 0; k_ <= n_; k_++) { \
        memset(b_, 0x55, sizeof b_); \
        CHECK(snprintf(b_, (size_t)k_, __VA_ARGS__) == n_ \
              && (k_ == 0 || (memcmp(b_, want, (size_t)k_ - 1) == 0 && b_[k_ - 1] == 0)) \
              && (unsigned char)b_[k_] == 0x55); \
    } \
} while (0)

/* snprintf gives `want`, and returns its length. */
#define FMT(want, ...) do { char b_[2048]; int r_ = snprintf(b_, sizeof b_, __VA_ARGS__); CHECK(r_ == (int)strlen(want) && eq(b_, want)); } while (0)

/* The whole content of a stream, from its start. */
static size_t slurp(FILE *f, char *out, size_t cap)
{
    rewind(f);
    size_t n = fread(out, 1, cap - 1, f);
    out[n] = 0;
    return n;
}

/* Wait for child `pid` (`waitpid` is §18, a later phase: the system call directly). */
static int reap(pid_t pid, int *st)
{
    return syscall(SYS_wait4, pid, st, 0, NULL) == pid;
}

int main(void)
{
    char b[2048];

    begin("s12_6_c_0");
    FMT("42-x", "%d-%s", 42, "x");
    end();

    begin("s12_6_c_r1");
    {
        FILE *f = tmpfile();
        CHECK(fprintf(f, "n=%d\n", 7) == 4);
        CHECK(slurp(f, b, sizeof b) == 4 && eq(b, "n=7\n"));
        fclose(f);
    }
    /* A busy stream (its plan cache is made on the 8th call, D-12.21) prints what snprintf
       does, every call: formats rewritten in place at one address, a format that is not
       simple, the stream closed and its slot reused, and reopened. Arguments are volatile
       so the compiler cannot fold the oracle. */
    {
        static char want[8192];
        char got[8192];
        char fmt[16];
        volatile int vi = -5;
        const char *volatile vs = "ab";
        volatile double vd = 2.25;
        FILE *f = tmpfile();
        for (int round = 0; round < 3; round++) {
            size_t w = 0;
            for (int i = 0; i < 40; i++) {
                int n = 0;
                strcpy(fmt, i % 2 ? "%d<%s>\n" : "%x<%s>\n");
                switch (i % 4) {
                case 0: n = fprintf(f, fmt, vi + i, vs); w += snprintf(want + w, sizeof want - w, fmt, vi + i, vs); break;
                case 1: n = fprintf(f, fmt, vi * i, vs); w += snprintf(want + w, sizeof want - w, fmt, vi * i, vs); break;
                case 2: n = fprintf(f, "%-*s|%*d\n", vi - i, vs, i - 3, vi); w += snprintf(want + w, sizeof want - w, "%-*s|%*d\n", vi - i, vs, i - 3, vi); break;
                default: n = fprintf(f, "%5.1f %s\n", vd * i, vs); w += snprintf(want + w, sizeof want - w, "%5.1f %s\n", vd * i, vs); break;
                }
                CHECK(n > 0);
            }
            CHECK(slurp(f, got, sizeof got) == w && eq(got, want));
            if (round == 0) {
                fclose(f);
                f = tmpfile();
            } else {
                char path[] = "/tmp/s12_printf_XXXXXX";
                int fd = mkstemp(path);
                CHECK(fd >= 0);
                if (fd >= 0) {
                    close(fd);
                    f = freopen(path, "w+", f);
                    unlink(path);
                }
            }
            CHECK(f != NULL);
            if (f == NULL)
                break;
        }
        if (f != NULL)
            fclose(f);
    }
    end();

    begin("s12_6_c_r2");
    CHECK(printf("[%d]", 3) == 3);
    CHECK(fflush(stdout) == 0);
    end();

    begin("s12_6_c_r3");
    {
        FILE *f = tmpfile();
        int fd = fileno(f);
        CHECK(dprintf(fd, "fd %d", 5) == 4);
        CHECK(pread(fd, b, 8, 0) == 4 && memcmp(b, "fd 5", 4) == 0); /* no stream buffering */
        fclose(f);
    }
    end();

    begin("s12_6_c_r4");
    memset(b, 0x55, 32);
    CHECK(sprintf(b, "<%d>", 12345) == 7 && memcmp(b, "<12345>\0", 8) == 0);
    memset(b, 0x55, 32);
    CHECK(snprintf(b, 4, "<%d>", 12345) == 7 && memcmp(b, "<12\0\x55", 5) == 0);
    /* Two strings, the second in a field (ls -l's "%s %*s "), against the bytes built by hand:
     * every width 0-40 either way (a negative `*` width left-justifies), text around and between
     * or none, multibyte bytes, a string longer than its field, precision cuts. */
    {
        static const char *const s0s[] = { "-rw-r--r--", "", "d\xc3\xa9j\xc3\xa0" };
        static const char *const s1s[] = { "", "1", "12", "\xc3\xa9", "\xe2\x82\xac" "12", "0123456789abcdef0123456789abcdefX" };
        char want[192];
        for (size_t i = 0; i < sizeof s0s / sizeof s0s[0]; i++) {
            for (size_t j = 0; j < sizeof s1s / sizeof s1s[0]; j++) {
                const char *s0 = s0s[i], *s1 = s1s[j];
                for (int w = -40; w <= 40; w++) {
                    pair_want(want, "", s0, " ", s1, w, 0, -1, " ");
                    PAIR(want, "%s %*s ", s0, w, s1);
                    pair_want(want, "", s0, " ", s1, w, 1, -1, " ");
                    PAIR(want, "%s %-*s ", s0, w, s1);
                    pair_want(want, "[", s0, "|", s1, w, 0, -1, "]");
                    PAIR(want, "[%s|%*s]", s0, w, s1);
                    pair_want(want, "", s0, "", s1, w, 0, -1, "");
                    PAIR(want, "%s%*s", s0, w, s1);
                    pair_want(want, "", s0, " ", s1, w, 0, 2, " ");
                    PAIR(want, "%s %*.2s ", s0, w, s1);
                    pair_want(want, "", s0, " ", s1, w, 1, 3, ".");
                    PAIR(want, "%s %-*.3s.", s0, w, s1);
                }
                pair_want(want, "", s0, " ", s1, 5, 0, -1, " ");
                PAIR(want, "%s %5s ", s0, s1);
                pair_want(want, "", s0, " ", s1, 5, 1, -1, "|");
                PAIR(want, "%s %-5s|", s0, s1);
                pair_want(want, "<", s0, "", s1, 40, 0, -1, "");
                PAIR(want, "<%s%40s", s0, s1);
            }
        }
        /* __sprintf_chk with an object too small for the 14 bytes and the terminator: it aborts (in
         * a child, into shared memory) and writes nothing past the object (C0.5). */
        unsigned char *sh = mmap(NULL, 64, PROT_READ | PROT_WRITE, MAP_SHARED | MAP_ANONYMOUS, -1, 0);
        CHECK(sh != MAP_FAILED);
        for (size_t slen = 1; sh != MAP_FAILED && slen <= 14; slen++) {
            memset(sh, 0x55, 64);
            fflush(NULL);
            pid_t pid = fork();
            if (pid == 0) {
                expect_crash();
                __sprintf_chk((char *)sh, 1, slen, "%s %*s ", "-rw-r--r--", 2, "1");
                _exit(0);
            }
            int st = -1;
            CHECK(pid > 0 && reap(pid, &st) && WIFSIGNALED(st) && WTERMSIG(st) == SIGABRT);
            int clean = 1;
            for (size_t k = slen; k < 64; k++)
                clean &= sh[k] == 0x55;
            CHECK(clean);
        }
        if (sh != MAP_FAILED)
            munmap(sh, 64);
    }
    end();

    begin("s12_6_c_r5");
    {
        char *p = NULL;
        CHECK(asprintf(&p, "%d bottles", 99) == 10 && p && eq(p, "99 bottles"));
        free(p);
    }
    end();

    begin("s12_6_c_r6");
    {
        struct obstack ob;
        obstack_init(&ob);
        CHECK(obstack_printf(&ob, "one=%d", 1) == 5);
        CHECK(obstack_printf(&ob, ",two=%d", 2) == 6);
        CHECK(obstack_object_size(&ob) == 11 && memcmp(obstack_base(&ob), "one=1,two=2", 11) == 0);
        obstack_free(&ob, NULL);
    }
    end();

    begin("s12_6_c_p1");
    FMT("  2.2|7   |", "%5.1f|%-4d|", 2.25, 7);       /* ISO C */
    FMT("pos|  2.2", "%2$s|%1$5.1f", 2.25, "pos");     /* POSIX */
    errno = ENOENT;
    FMT("No such file or directory", "%m");          /* GNU */
    end();

    begin("s12_6_c_2");
    FMT("-1 2 10 3 ff FF", "%d %i %o %u %x %X", -1, 2, 8, 3u, 255u, 255u);
    FMT("1.500000 1.500000 1.234568e+04 1.230000E-04 0.0001 1E+20 0x1p+0 0X1P+0", "%f %F %e %E %g %G %a %A", 1.5, 1.5, 12345.678, 0.000123, 0.0001, 1e20, 1.0, 1.0);
    FMT("xyz%", "%c%s%%", 'x', "yz");
    /* One %c and its text alone (gawk's "*.*%c" float formats): the int argument's low byte, the
     * text around it as written, the count of all three pieces (SPRINTF-RENDER). */
    FMT("<A>", "<%c>", 0x141);
    FMT("*.*g", "*.*%c", 'g');
    FMT("q", "%c", 'q');
    FMT("0x1234", "%p", (void *)0x1234);
    {
        int count = -1;
        FMT("abcde", "abc%nde", &count);
        CHECK(count == 3);
    }
    errno = EINVAL;
    FMT("Invalid argument", "%m");
    FMT("101 0b101 110 0B101", "%b %#b %B %#B", 5u, 5u, 6u, 5u);
    FMT("W|wide|L|wide", "%C|%S|%lc|%ls", (wint_t)L'W', L"wide", (wint_t)L'L', L"wide");
    end();

    begin("s12_6_c_3");
    FMT("44|4464|1099511627776|1099511627776|1099511627776|1099511627776", "%hhd|%hd|%ld|%lld|%qd|%Ld", 300, 70000, 1L << 40, 1LL << 40, 1LL << 40, 1LL << 40);
    FMT("-1|7|-8", "%jd|%zu|%td", (intmax_t)-1, (size_t)7, (ptrdiff_t)-8);
    FMT("44|4464|-5|1099511627776|44|70000", "%w8d|%w16d|%w32d|%w64d|%wf8d|%wf16d", 300, 70000, (int32_t)-5, (int64_t)1 << 40, 300, (int_fast16_t)70000);
    FMT("255|65535", "%hhu|%hu", -1, -1);
    end();

    begin("s12_6_c_4");
    FMT("[42   ][+42][ 42][010][0xff][00042]", "[%-5d][%+d][% d][%#o][%#x][%05d]", 42, 42, 42, 8, 255, 42);
    /* The C locale has no grouping and ASCII digits: the flags are accepted and change nothing
     * (grouping in named locales is compared with the reference in printf_products.rs). */
    FMT("[1234567][42][1234.50]", "[%'d][%Id][%'.2f]", 1234567, 42, 1234.5);
    end();

    begin("s12_6_c_5");
    FMT("[ 12.35][007][     abc]", "[%6.2f][%.3d][%8.3s]", 12.3456, 7, "abcdef");
    FMT("[   7][7   ][7   ]", "[%*d][%-*d][%*d]", 4, 7, 4, 7, -4, 7);  /* a negative width is `-` */
    FMT("[00001][2.500000]", "[%.*d][%.*f]", 5, 1, -1, 2.5);         /* a negative precision is none */
    end();

    begin("s12_6_c_6");
    FMT("b a   7|a", "%2$s %1$s %3$*4$d|%1$s", "a", "b", 7, 3);
    FMT("1.00", "%1$.*2$f", 1.0, 2);
    end();

    begin("s12_6_g_1");
    FMT("0.1000000000000000055511151231257827021181583404541015625", "%.55f", 0.1);
    FMT("0 2 2", "%.0f %.0f %.0f", 0.5, 1.5, 2.5);                   /* ties to even */
    FMT("0.2", "%.1f", 0.25);
    FMT("9.99999999999999916114e+22", "%.20e", 1e23);
    FMT("10000000000000000000000.000000", "%f", 1e22);
    {
        int n = snprintf(b, sizeof b, "%.1074e", 4.9406564584124654e-324);
        CHECK(n == 1081 && strncmp(b, "4.9406564584124654417656879286822137236505980", 45) == 0 && strcmp(b + n - 5, "e-324") == 0);
    }
    end();

    begin("s12_6_g_2");
    {
#define G2_FMT "%-6s|%+.3e|%#x|%5.1f%%|%c|%ls"
#define G2_ARGS "text", -1234.5678, 0xbeefu, 99.95, 'k', L"wide"
        const char *want = "text  |-1.235e+03|0xbeef|100.0%|k|wide";
        int len = (int)strlen(want);
        FMT("text  |-1.235e+03|0xbeef|100.0%|k|wide", G2_FMT, G2_ARGS);
        CHECK(sprintf(b, G2_FMT, G2_ARGS) == len && eq(b, want));
        char *p = NULL;
        CHECK(asprintf(&p, G2_FMT, G2_ARGS) == len && eq(p, want));
        free(p);
        FILE *f = tmpfile();
        CHECK(fprintf(f, G2_FMT, G2_ARGS) == len && slurp(f, b, sizeof b) == (size_t)len && eq(b, want));
        fclose(f);
        f = tmpfile();
        CHECK(dprintf(fileno(f), G2_FMT, G2_ARGS) == len && slurp(f, b, sizeof b) == (size_t)len && eq(b, want));
        fclose(f);
        size_t size = 0;
        p = NULL;
        f = open_memstream(&p, &size);
        CHECK(fprintf(f, G2_FMT, G2_ARGS) == len);
        fclose(f);
        CHECK(size == (size_t)len && eq(p, want));
        free(p);
        struct obstack ob;
        obstack_init(&ob);
        CHECK(obstack_printf(&ob, G2_FMT, G2_ARGS) == len && obstack_object_size(&ob) == (size_t)len && memcmp(obstack_base(&ob), want, len) == 0);
        obstack_free(&ob, NULL);
        wchar_t wb[128];
        CHECK(swprintf(wb, 128, L"" G2_FMT, G2_ARGS) == len && wcscmp(wb, L"text  |-1.235e+03|0xbeef|100.0%|k|wide") == 0);
    }
    end();

    begin("s12_6_g_3");
    CHECK(snprintf(b, sizeof b, "%s%d", "abc", 42) == 5);
    end();

    begin("s12_6_g_3_1");
    memset(b, 0x55, 8);
    CHECK(snprintf(b, 5, "%s", "hello world") == 11 && memcmp(b, "hell\0\x55\x55\x55", 8) == 0);
    CHECK(snprintf(b, 1, "%s", "hello world") == 11 && b[0] == 0);
    CHECK(snprintf(NULL, 0, "%s", "hello world") == 11);
    end();

    begin("s12_6_g_3_2");
    {
        char *p = NULL;
        CHECK(asprintf(&p, "%05d", 42) == 5 && eq(p, "00042"));
        free(p);
        char *sentinel = (char *)(uintptr_t)0x5A5A;
        p = sentinel;
        errno = 0;
        CHECK(asprintf(&p, "%lc", (wint_t)0x110000) == -1 && errno == EILSEQ);
        CHECK(p == sentinel); /* not set on failure */
    }
    end();

    begin("s12_6_g_3_3");
    {
        FILE *f = tmpfile();
        CHECK(fprintf(f, "%s=%d", "key", 100) == 7);
        CHECK(slurp(f, b, sizeof b) == 7);
        fclose(f);
        CHECK(sprintf(b, "%x", 0xabcdefu) == 6);
    }
    end();

    begin("s12_6_g_4");
    {
        /* Three bytes at the end of a page followed by an inaccessible one: no terminator. */
        long page = sysconf(_SC_PAGESIZE);
        char *g = mmap(NULL, 2 * page, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
        CHECK(g != MAP_FAILED && mprotect(g + page, page, PROT_NONE) == 0);
        char *s = g + page - 3;
        memcpy(s, "abc", 3);
        FMT("[abc][ab][abc]", "[%.3s][%.2s][%.*s]", s, s, 3, s);
        munmap(g, 2 * page);
    }
    end();

    begin("s12_6_g_5");
    FMT("[(null)][  (null)]", "[%s][%8s]", (char *)NULL, (char *)NULL);
    end();

    begin("s12_6_f_0");
    errno = 0;
    CHECK(snprintf(NULL, 0, "%lc", (wint_t)0x110000) < 0 && errno != 0);
    end();

    begin("s12_6_f_1");
    errno = 0;
    CHECK(snprintf(NULL, 0, "%*d", INT_MAX, 1) == INT_MAX && errno == 0); /* INT_MAX itself fits */
    CHECK(snprintf(NULL, 0, "%*d%d", INT_MAX, 1, 2) == -1 && errno == EOVERFLOW);
    end();

    begin("s12_6_f_2");
    {
        /* In a child: the address space capped at 256 MiB, a 1 GB result cannot be held. */
        fflush(NULL);
        pid_t pid = fork();
        if (pid == 0) {
            struct rlimit rl = { 256 << 20, RLIM_INFINITY };
            char *p = NULL;
            if (setrlimit(RLIMIT_AS, &rl) != 0)
                _exit(2);
            errno = 0;
            int r = asprintf(&p, "%1000000000d", 1);
            _exit(r == -1 && errno == ENOMEM && p == NULL ? 0 : 1);
        }
        int st = -1;
        CHECK(pid > 0 && reap(pid, &st) && WIFEXITED(st) && WEXITSTATUS(st) == 0);
    }
    end();

    begin("s12_6_f_3");
    {
        /* The C locale's encoding is ASCII (the reference fails the same way). */
        errno = 0;
        CHECK(snprintf(b, sizeof b, "%lc", (wint_t)0xE9) == -1 && errno == EILSEQ);
        errno = 0;
        CHECK(snprintf(b, sizeof b, "%lc", (wint_t)0x110000) == -1 && errno == EILSEQ);
        errno = 0;
        CHECK(snprintf(b, sizeof b, "%ls", L"a\x100") == -1 && errno == EILSEQ);
        locale_t u = newlocale(LC_ALL_MASK, "C.UTF-8", (locale_t)0);
        CHECK(u != (locale_t)0);
        locale_t old = uselocale(u);
        FMT("\xc3\xa9", "%lc", (wint_t)0xE9); /* converted in a UTF-8 locale */
        uselocale(old);
        freelocale(u);
    }
    end();

    begin("s12_6_f_4");
    {
        FILE *f = tmpfile();
        FILE *r = fdopen(dup(fileno(f)), "r");
        errno = 0;
        CHECK(fprintf(r, "%d", 1) == -1 && errno == EBADF);
        fclose(r);
        fclose(f);
        FILE *full = fopen("/dev/full", "w");
        CHECK(full != NULL);
        setvbuf(full, NULL, _IONBF, 0);
        errno = 0;
        CHECK(fprintf(full, "%d", 1) == -1 && errno == ENOSPC);
        fclose(full);
        /* Not fortified, or a format in read-only memory: %n is honoured. */
        char writable[] = "ab%n";
        int n = 0;
        FILE *m = tmpfile();
        CHECK(fprintf(m, writable, &n) == 2 && n == 2);
        CHECK(__fprintf_chk(m, 1, "abc%n", &n) == 3 && n == 3);
        /* Fortified, with the format in writable memory: abort (in a child). */
        fflush(NULL);
        pid_t pid = fork();
        if (pid == 0) {
            expect_crash();
            __fprintf_chk(m, 1, writable, &n);
            _exit(0);
        }
        int st = -1;
        CHECK(pid > 0 && reap(pid, &st) && WIFSIGNALED(st) && WTERMSIG(st) == SIGABRT);
        fclose(m);
    }
    end();

    /* AUD-M2 (D-12.20): one printf-family call on an unbuffered stream reaches it in one
       write(2), not one per piece; longer output in 128-byte pieces, then the rest (the
       reference's pattern under strace). A SOCK_SEQPACKET socket keeps each write a message. */
    begin("printf_unbuffered_writes");
    {
        int sv[2];
        CHECK(socketpair(AF_UNIX, SOCK_SEQPACKET, 0, sv) == 0);
        FILE *f = fdopen(sv[0], "w");
        CHECK(f != NULL && setvbuf(f, NULL, _IONBF, 0) == 0);
        char big[301];
        memset(big, 'q', 300);
        big[300] = 0;
        CHECK(fprintf(f, "x=%d y=%s\n", 7, "ab") == 9);
        CHECK(fprintf(f, "<%s>", big) == 302);
        CHECK(fprintf(f, "%s", "") == 0);
        shutdown(sv[0], SHUT_WR);
        char m[4096];
        ssize_t got[8];
        int k = 0;
        ssize_t r;
        while (k < 8 && (r = recv(sv[1], m, sizeof m, 0)) > 0) got[k++] = r;
        CHECK(k == 4);
        CHECK(k >= 1 && got[0] == 9);
        CHECK(k >= 4 && got[1] == 128 && got[2] == 128 && got[3] == 46);
        fclose(f);
        close(sv[1]);
    }
    end();

    return failures ? 1 : 0;
}
