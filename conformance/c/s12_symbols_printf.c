/* Translation tests for spec §12.6 (crates/boundary/tests/s12_stdio_symbols.rs): every formatted
 * output entry point through real C varargs, and every `va_list` form through a real `va_list`
 * (a variadic wrapper per symbol). Each symbol is checked once with the same format, which
 * reads all three argument classes (an integer, a pointer, a `double`), for its destination and
 * result convention; the fortified forms also check that an overflow of the object (or `%n` in a
 * writable format) ends in the fortify failure, in a child, and the byte forms a format with `%n`
 * in read-only memory (the wide forms' is in s12_symbols_printf_chk.c, with the other cases
 * Liella does not pass yet). The conversions themselves are the clause tests' (s12_printf.c).
 * Self-checking: each symbol's checks end with `end <symbol>` on standard error, each failed
 * check prints `FAIL <symbol>: line N: <check>`; the exit status is 1 when any check failed. */
#undef _FORTIFY_SOURCE /* the plain functions are called; the fortified ones by name */
#define _GNU_SOURCE
#include <errno.h>
#include <locale.h>
#include <obstack.h>
#include <signal.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/prctl.h>
#include <sys/syscall.h>
#include <sys/wait.h>
#include <unistd.h>
#include <wchar.h>
#include "expect_crash.h"

#define obstack_chunk_alloc malloc
#define obstack_chunk_free free

/* Exported, not declared by the installed headers (or only in fortified builds). */
extern int __asprintf(char **, const char *, ...);
extern int __vsnprintf(char *, size_t, const char *, va_list);
extern int _IO_printf(const char *, ...);
extern int _IO_fprintf(FILE *, const char *, ...);
extern int _IO_sprintf(char *, const char *, ...);
extern int _IO_vfprintf(FILE *, const char *, va_list);
extern int _IO_vsprintf(char *, const char *, va_list);
extern int __printf_chk(int, const char *, ...);
extern int __vprintf_chk(int, const char *, va_list);
extern int __fprintf_chk(FILE *, int, const char *, ...);
extern int __vfprintf_chk(FILE *, int, const char *, va_list);
extern int __dprintf_chk(int, int, const char *, ...);
extern int __vdprintf_chk(int, int, const char *, va_list);
extern int __sprintf_chk(char *, int, size_t, const char *, ...);
extern int __vsprintf_chk(char *, int, size_t, const char *, va_list);
extern int __snprintf_chk(char *, size_t, int, size_t, const char *, ...);
extern int __vsnprintf_chk(char *, size_t, int, size_t, const char *, va_list);
extern int __asprintf_chk(char **, int, const char *, ...);
extern int __vasprintf_chk(char **, int, const char *, va_list);
extern int __obstack_printf_chk(struct obstack *, int, const char *, ...);
extern int __obstack_vprintf_chk(struct obstack *, int, const char *, va_list);
extern int __wprintf_chk(int, const wchar_t *, ...);
extern int __vwprintf_chk(int, const wchar_t *, va_list);
extern int __fwprintf_chk(FILE *, int, const wchar_t *, ...);
extern int __vfwprintf_chk(FILE *, int, const wchar_t *, va_list);
extern int __swprintf_chk(wchar_t *, size_t, int, size_t, const wchar_t *, ...);
extern int __vswprintf_chk(wchar_t *, size_t, int, size_t, const wchar_t *, va_list);

static int failures;
static const char *current;

static void begin(const char *symbol) { current = symbol; }
static void end(void) { fprintf(stderr, "end %s\n", current); }

#define CHECK(cond) do { if (!(cond)) { failures++; fprintf(stderr, "FAIL %s: line %d: %s\n", current, __LINE__, #cond); } } while (0)

/* The format every symbol renders, its arguments and its result. */
#define FMT "<%d|%s|%.1f>"
#define WFMT L"<%d|%s|%.1f>"
#define ARGS 42, "ab", 2.5
#define WANT "<42|ab|2.5>"
#define WWANT L"<42|ab|2.5>"
#define LEN 11

/* The fortified forms' flag (as _FORTIFY_SOURCE=2 passes it), and `%n` in writable memory. */
#define FLAG 1
static char writable[] = "ab%n";
static wchar_t wwritable[] = L"ab%n";
static int stored;

static int eq(const char *got, const char *want)
{
    if (strcmp(got, want) == 0)
        return 1;
    fprintf(stderr, "  got  [%s]\n  want [%s]\n", got, want);
    return 0;
}

static int weq(const wchar_t *got, const wchar_t *want)
{
    if (wcscmp(got, want) == 0)
        return 1;
    fprintf(stderr, "  got  [%ls]\n  want [%ls]\n", got, want);
    return 0;
}

/* The bytes written at the start of descriptor `fd` (stream buffers are not consulted). */
static const char *on_fd(int fd)
{
    static char b[256];
    ssize_t n = pread(fd, b, sizeof b - 1, 0);
    b[n > 0 ? n : 0] = 0;
    return b;
}

/* The bytes a stream wrote to its file (flushed first; any orientation). */
static const char *on_file(FILE *f)
{
    fflush(f);
    return on_fd(fileno(f));
}

/* The scratch file standard output is redirected to. */
static char path[64];

/* stdout reopened on the scratch file, emptied. */
static void redirect(void)
{
    CHECK(freopen(path, "w+", stdout) == stdout);
}

/* What was written to stdout since `redirect`. */
static const char *captured(void)
{
    return on_file(stdout);
}

/* stdout reopened read-only: output to it fails. */
static void unwritable(void)
{
    CHECK(freopen(path, "r", stdout) == stdout);
    errno = 0;
}

/* `f` run in a child ends in SIGABRT (the fortify failure); no core dump is made. */
static int aborts(void (*f)(void))
{
    fflush(NULL);
    pid_t pid = fork();
    if (pid == 0) {
        expect_crash();
        f();
        _exit(0);
    }
    int st = -1;
    return pid > 0 && syscall(SYS_wait4, pid, &st, 0, NULL) == pid && WIFSIGNALED(st) && WTERMSIG(st) == SIGABRT;
}

/* The `va_list` forms, called through a real `va_list`. */
#define VA(call) do { va_list ap; va_start(ap, fmt); r = (call); va_end(ap); } while (0)
static int w_vprintf(const char *fmt, ...) { int r; VA(vprintf(fmt, ap)); return r; }
static int w___vprintf_chk(int flag, const char *fmt, ...) { int r; VA(__vprintf_chk(flag, fmt, ap)); return r; }
static int w_vfprintf(FILE *f, const char *fmt, ...) { int r; VA(vfprintf(f, fmt, ap)); return r; }
static int w__IO_vfprintf(FILE *f, const char *fmt, ...) { int r; VA(_IO_vfprintf(f, fmt, ap)); return r; }
static int w___vfprintf_chk(FILE *f, int flag, const char *fmt, ...) { int r; VA(__vfprintf_chk(f, flag, fmt, ap)); return r; }
static int w_vdprintf(int fd, const char *fmt, ...) { int r; VA(vdprintf(fd, fmt, ap)); return r; }
static int w___vdprintf_chk(int fd, int flag, const char *fmt, ...) { int r; VA(__vdprintf_chk(fd, flag, fmt, ap)); return r; }
static int w_vsprintf(char *s, const char *fmt, ...) { int r; VA(vsprintf(s, fmt, ap)); return r; }
static int w__IO_vsprintf(char *s, const char *fmt, ...) { int r; VA(_IO_vsprintf(s, fmt, ap)); return r; }
static int w___vsprintf_chk(char *s, int flag, size_t slen, const char *fmt, ...) { int r; VA(__vsprintf_chk(s, flag, slen, fmt, ap)); return r; }
static int w_vsnprintf(char *s, size_t n, const char *fmt, ...) { int r; VA(vsnprintf(s, n, fmt, ap)); return r; }
static int w___vsnprintf(char *s, size_t n, const char *fmt, ...) { int r; VA(__vsnprintf(s, n, fmt, ap)); return r; }
static int w___vsnprintf_chk(char *s, size_t n, int flag, size_t slen, const char *fmt, ...) { int r; VA(__vsnprintf_chk(s, n, flag, slen, fmt, ap)); return r; }
static int w_vasprintf(char **p, const char *fmt, ...) { int r; VA(vasprintf(p, fmt, ap)); return r; }
static int w___vasprintf_chk(char **p, int flag, const char *fmt, ...) { int r; VA(__vasprintf_chk(p, flag, fmt, ap)); return r; }
static int w_obstack_vprintf(struct obstack *o, const char *fmt, ...) { int r; VA(obstack_vprintf(o, fmt, ap)); return r; }
static int w___obstack_vprintf_chk(struct obstack *o, int flag, const char *fmt, ...) { int r; VA(__obstack_vprintf_chk(o, flag, fmt, ap)); return r; }
static int w_vwprintf(const wchar_t *fmt, ...) { int r; VA(vwprintf(fmt, ap)); return r; }
static int w___vwprintf_chk(int flag, const wchar_t *fmt, ...) { int r; VA(__vwprintf_chk(flag, fmt, ap)); return r; }
static int w_vfwprintf(FILE *f, const wchar_t *fmt, ...) { int r; VA(vfwprintf(f, fmt, ap)); return r; }
static int w___vfwprintf_chk(FILE *f, int flag, const wchar_t *fmt, ...) { int r; VA(__vfwprintf_chk(f, flag, fmt, ap)); return r; }
static int w_vswprintf(wchar_t *s, size_t n, const wchar_t *fmt, ...) { int r; VA(vswprintf(s, n, fmt, ap)); return r; }
static int w___vswprintf_chk(wchar_t *s, size_t n, int flag, size_t slen, const wchar_t *fmt, ...) { int r; VA(__vswprintf_chk(s, n, flag, slen, fmt, ap)); return r; }

/* A byte form to stdout: the result and the text; -1 with EBADF when stdout cannot write. */
#define STDOUT_FORM(call) do { \
    redirect(); \
    CHECK((call) == LEN && eq(captured(), WANT)); \
    unwritable(); \
    CHECK((call) == -1 && errno == EBADF); \
} while (0)

/* A wide form to stdout: stdout becomes wide-oriented; -1 when it cannot write. */
#define WSTDOUT_FORM(call) do { \
    redirect(); \
    CHECK((call) == LEN && eq(captured(), WANT) && fwide(stdout, 0) > 0); \
    unwritable(); \
    CHECK((call) == -1 && errno == EBADF); \
} while (0)

/* A byte form to the stream `f`: the result and the text; -1 with EBADF on a read-only stream. */
#define STREAM_FORM(call) do { \
    FILE *f = tmpfile(); \
    CHECK((call) == LEN && eq(on_file(f), WANT)); \
    fclose(f); \
    f = fopen("/dev/null", "r"); \
    errno = 0; \
    CHECK((call) == -1 && errno == EBADF); \
    fclose(f); \
} while (0)

/* A wide form to the stream `f`: it becomes wide-oriented; -1 on a read-only stream. */
#define WSTREAM_FORM(call) do { \
    FILE *f = tmpfile(); \
    CHECK((call) == LEN && eq(on_file(f), WANT) && fwide(f, 0) > 0); \
    fclose(f); \
    f = fopen("/dev/null", "r"); \
    errno = 0; \
    CHECK((call) == -1 && errno == EBADF); \
    fclose(f); \
} while (0)

/* A form to the descriptor `fd`: written at once (no stream); -1 with EBADF on a bad one. */
#define FD_FORM(call) do { \
    FILE *t = tmpfile(); \
    int fd = fileno(t); \
    CHECK((call) == LEN && eq(on_fd(fd), WANT)); \
    fclose(t); \
    fd = 999999; \
    errno = 0; \
    CHECK((call) == -1 && errno == EBADF); \
} while (0)

/* An unbounded form to the buffer `b`: the text and its terminator. */
#define BUFFER_FORM(call) do { \
    char b[32]; \
    memset(b, 0x55, sizeof b); \
    CHECK((call) == LEN && memcmp(b, WANT "\0\x55", LEN + 2) == 0); \
} while (0)

/* A bounded form to the buffer `b` of `n` bytes: at most n - 1 bytes and a terminator, the
 * full length returned; n 0 writes nothing. */
#define BOUNDED_FORM(call) do { \
    char b[32]; \
    size_t n = 6; \
    memset(b, 0x55, sizeof b); \
    CHECK((call) == LEN && memcmp(b, "<42|a\0\x55", 7) == 0); \
    n = 0; \
    b[0] = 0x55; \
    CHECK((call) == LEN && b[0] == 0x55); \
    n = sizeof b; \
    CHECK((call) == LEN && eq(b, WANT)); \
} while (0)

/* An allocating form: `*p` is the text; on a failure it is left as it was. */
#define ALLOC_FORM(call, fail) do { \
    char *sentinel = (char *)(uintptr_t)0x5A5A, *p = NULL; \
    CHECK((call) == LEN && p != NULL && eq(p, WANT)); \
    free(p); \
    p = sentinel; \
    errno = 0; \
    CHECK((fail) == -1 && errno == EILSEQ && p == sentinel); \
} while (0)

/* An obstack form: the text grows the current object, unterminated. */
#define OBSTACK_FORM(call, more) do { \
    struct obstack ob; \
    obstack_init(&ob); \
    CHECK((call) == LEN && obstack_object_size(&ob) == LEN && memcmp(obstack_base(&ob), WANT, LEN) == 0); \
    CHECK((more) == 2 && obstack_object_size(&ob) == LEN + 2 && memcmp(obstack_base(&ob), WANT "-7", LEN + 2) == 0); \
    obstack_free(&ob, NULL); \
} while (0)

/* A wide form to the buffer `b` of `n` wide characters: the text and its terminator; a result
 * that does not fit is an error. */
#define WBUFFER_FORM(call) do { \
    wchar_t b[32]; \
    size_t n = 32; \
    CHECK((call) == LEN && weq(b, WWANT)); \
    n = 5; \
    CHECK((call) < 0); \
} while (0)

/* The fortify failures, each in a child (`aborts`). */
static void over_sprintf_chk(void) { char b[LEN]; __sprintf_chk(b, FLAG, sizeof b, FMT, ARGS); }
static void over_vsprintf_chk(void) { char b[LEN]; w___vsprintf_chk(b, FLAG, sizeof b, FMT, ARGS); }
static void over_snprintf_chk(void) { char b[8]; __snprintf_chk(b, 16, FLAG, sizeof b, FMT, ARGS); }
static void over_vsnprintf_chk(void) { char b[8]; w___vsnprintf_chk(b, 16, FLAG, sizeof b, FMT, ARGS); }
/* The overflow ends in the fortify failure before a byte lands beyond the object (C0.5 "would
 * write"): the object is the last 8 bytes of a page whose successor is inaccessible, so a byte
 * written beyond it is a SIGSEGV, not the SIGABRT `aborts` wants. */
static void over_snprintf_chk_edge(void)
{
    long pg = sysconf(_SC_PAGESIZE);
    char *m = mmap(NULL, 2 * pg, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (m == MAP_FAILED || mprotect(m + pg, pg, PROT_NONE) != 0)
        _exit(1);
    __snprintf_chk(m + pg - 8, 16, FLAG, 8, FMT, ARGS);
}
static void over_swprintf_chk(void) { wchar_t b[8]; __swprintf_chk(b, 16, FLAG, 8, WFMT, ARGS); }
static void over_vswprintf_chk(void) { wchar_t b[8]; w___vswprintf_chk(b, 16, FLAG, 8, WFMT, ARGS); }
/* SPRINTF-LS-2 (§12.6, C0.5, D-C0.5): guards for any fast route over simple formats. ls -l's
 * shape aborts on overflow; with an invalid conversion after a valid prefix that overflows, the
 * outcome must be the general route's (forced here by a leading `%.0s`). */
/* Not literals: the compiler would warn about the invalid conversion `%y`. */
static char bad_ls[] = "%s %*s %y", bad_ls_usual[] = "%.0s%s %*s %y", bad_ab[] = "ab%sc%y", bad_ab_usual[] = "%.0sab%sc%y";
static void over_ls_chk(void) { char b[8]; __sprintf_chk(b, FLAG, sizeof b, "%s %*s ", "-rw-r--r--", 2, "1"); }
static void bad_ls_chk(void) { char b[4]; __sprintf_chk(b, FLAG, sizeof b, bad_ls, "-rw-r--r--", 2, "1"); _exit(0); }
/* A position above NL_ARGMAX (D-12.18) is rejected before a byte is written, so the overflow
 * the %s before it would cause must not abort. */
static char bad_pos[] = "%s%99999$s";
static void bad_pos_chk(void) { char b[4]; __sprintf_chk(b, FLAG, sizeof b, bad_pos, "-rw-r--r--", "1"); _exit(0); }
static void bad_ls_chk_usual(void) { char b[4]; __sprintf_chk(b, FLAG, sizeof b, bad_ls_usual, "", "-rw-r--r--", 2, "1"); _exit(0); }
static void n_printf_chk(void) { __printf_chk(FLAG, writable, &stored); }
static void n_vprintf_chk(void) { w___vprintf_chk(FLAG, writable, &stored); }
static void n_fprintf_chk(void) { __fprintf_chk(stderr, FLAG, writable, &stored); }
static void n_vfprintf_chk(void) { w___vfprintf_chk(stderr, FLAG, writable, &stored); }
static void n_dprintf_chk(void) { __dprintf_chk(2, FLAG, writable, &stored); }
static void n_vdprintf_chk(void) { w___vdprintf_chk(2, FLAG, writable, &stored); }
static void n_sprintf_chk(void) { char b[32]; __sprintf_chk(b, FLAG, sizeof b, writable, &stored); }
static void n_vsprintf_chk(void) { char b[32]; w___vsprintf_chk(b, FLAG, sizeof b, writable, &stored); }
static void n_snprintf_chk(void) { char b[32]; __snprintf_chk(b, sizeof b, FLAG, sizeof b, writable, &stored); }
static void n_vsnprintf_chk(void) { char b[32]; w___vsnprintf_chk(b, sizeof b, FLAG, sizeof b, writable, &stored); }
static void n_asprintf_chk(void) { char *p; __asprintf_chk(&p, FLAG, writable, &stored); }
static void n_vasprintf_chk(void) { char *p; w___vasprintf_chk(&p, FLAG, writable, &stored); }
static void n_obstack_printf_chk(void) { struct obstack ob; obstack_init(&ob); __obstack_printf_chk(&ob, FLAG, writable, &stored); }
static void n_obstack_vprintf_chk(void) { struct obstack ob; obstack_init(&ob); w___obstack_vprintf_chk(&ob, FLAG, writable, &stored); }
static void n_wprintf_chk(void) { __wprintf_chk(FLAG, wwritable, &stored); }
static void n_vwprintf_chk(void) { w___vwprintf_chk(FLAG, wwritable, &stored); }
static void n_fwprintf_chk(void) { FILE *f = tmpfile(); __fwprintf_chk(f, FLAG, wwritable, &stored); }
static void n_vfwprintf_chk(void) { FILE *f = tmpfile(); w___vfwprintf_chk(f, FLAG, wwritable, &stored); }
static void n_swprintf_chk(void) { wchar_t b[32]; __swprintf_chk(b, 32, FLAG, 32, wwritable, &stored); }
static void n_vswprintf_chk(void) { wchar_t b[32]; w___vswprintf_chk(b, 32, FLAG, 32, wwritable, &stored); }

int main(void)
{
    int n;
    snprintf(path, sizeof path, "/tmp/liella-s12-symbols-printf-%d", (int)getpid());

    /* ---------------------------------------------------------------- standard output */

    begin("printf");
    STDOUT_FORM(printf(FMT, ARGS));
    end();

    begin("_IO_printf");
    STDOUT_FORM(_IO_printf(FMT, ARGS));
    end();

    begin("vprintf");
    STDOUT_FORM(w_vprintf(FMT, ARGS));
    end();

    begin("__printf_chk");
    STDOUT_FORM(__printf_chk(FLAG, FMT, ARGS));
    redirect();
    CHECK(__printf_chk(FLAG, "ab%n", &n) == 2 && n == 2 && eq(captured(), "ab")); /* a read-only format */
    CHECK(aborts(n_printf_chk));
    end();

    begin("__vprintf_chk");
    STDOUT_FORM(w___vprintf_chk(FLAG, FMT, ARGS));
    redirect();
    CHECK(w___vprintf_chk(FLAG, "ab%n", &n) == 2 && n == 2 && eq(captured(), "ab"));
    CHECK(aborts(n_vprintf_chk));
    end();

    begin("wprintf");
    WSTDOUT_FORM(wprintf(WFMT, ARGS));
    end();

    begin("vwprintf");
    WSTDOUT_FORM(w_vwprintf(WFMT, ARGS));
    end();

    begin("__wprintf_chk");
    WSTDOUT_FORM(__wprintf_chk(FLAG, WFMT, ARGS));
    CHECK(aborts(n_wprintf_chk));
    end();

    begin("__vwprintf_chk");
    WSTDOUT_FORM(w___vwprintf_chk(FLAG, WFMT, ARGS));
    CHECK(aborts(n_vwprintf_chk));
    end();

    /* ---------------------------------------------------------------- streams */

    begin("fprintf");
    STREAM_FORM(fprintf(f, FMT, ARGS));
    end();

    begin("_IO_fprintf");
    STREAM_FORM(_IO_fprintf(f, FMT, ARGS));
    end();

    begin("vfprintf");
    STREAM_FORM(w_vfprintf(f, FMT, ARGS));
    end();

    begin("_IO_vfprintf");
    STREAM_FORM(w__IO_vfprintf(f, FMT, ARGS));
    end();

    begin("__fprintf_chk");
    STREAM_FORM(__fprintf_chk(f, FLAG, FMT, ARGS));
    {
        FILE *f = tmpfile();
        CHECK(__fprintf_chk(f, FLAG, "ab%n", &n) == 2 && n == 2 && eq(on_file(f), "ab"));
        fclose(f);
    }
    CHECK(aborts(n_fprintf_chk));
    end();

    begin("__vfprintf_chk");
    STREAM_FORM(w___vfprintf_chk(f, FLAG, FMT, ARGS));
    {
        FILE *f = tmpfile();
        CHECK(w___vfprintf_chk(f, FLAG, "ab%n", &n) == 2 && n == 2 && eq(on_file(f), "ab"));
        fclose(f);
    }
    CHECK(aborts(n_vfprintf_chk));
    end();

    begin("fwprintf");
    WSTREAM_FORM(fwprintf(f, WFMT, ARGS));
    {
        /* Encoded by the codec of the locale the stream became wide in. */
        locale_t u = newlocale(LC_ALL_MASK, "C.UTF-8", (locale_t)0);
        CHECK(u != (locale_t)0);
        locale_t old = uselocale(u);
        FILE *f = tmpfile();
        CHECK(fwprintf(f, L"%lc", (wint_t)0xE9) == 1 && eq(on_file(f), "\xc3\xa9"));
        fclose(f);
        uselocale(old);
        freelocale(u);
    }
    end();

    begin("vfwprintf");
    WSTREAM_FORM(w_vfwprintf(f, WFMT, ARGS));
    end();

    begin("__fwprintf_chk");
    WSTREAM_FORM(__fwprintf_chk(f, FLAG, WFMT, ARGS));
    CHECK(aborts(n_fwprintf_chk));
    end();

    begin("__vfwprintf_chk");
    WSTREAM_FORM(w___vfwprintf_chk(f, FLAG, WFMT, ARGS));
    CHECK(aborts(n_vfwprintf_chk));
    end();

    /* ---------------------------------------------------------------- descriptors */

    begin("dprintf");
    FD_FORM(dprintf(fd, FMT, ARGS));
    end();

    begin("vdprintf");
    FD_FORM(w_vdprintf(fd, FMT, ARGS));
    end();

    begin("__dprintf_chk");
    FD_FORM(__dprintf_chk(fd, FLAG, FMT, ARGS));
    {
        FILE *t = tmpfile();
        CHECK(__dprintf_chk(fileno(t), FLAG, "ab%n", &n) == 2 && n == 2 && eq(on_fd(fileno(t)), "ab"));
        fclose(t);
    }
    CHECK(aborts(n_dprintf_chk));
    end();

    begin("__vdprintf_chk");
    FD_FORM(w___vdprintf_chk(fd, FLAG, FMT, ARGS));
    {
        FILE *t = tmpfile();
        CHECK(w___vdprintf_chk(fileno(t), FLAG, "ab%n", &n) == 2 && n == 2 && eq(on_fd(fileno(t)), "ab"));
        fclose(t);
    }
    CHECK(aborts(n_vdprintf_chk));
    end();

    /* ---------------------------------------------------------------- caller buffers */

    begin("sprintf");
    BUFFER_FORM(sprintf(b, FMT, ARGS));
    end();

    begin("_IO_sprintf");
    BUFFER_FORM(_IO_sprintf(b, FMT, ARGS));
    end();

    begin("vsprintf");
    BUFFER_FORM(w_vsprintf(b, FMT, ARGS));
    end();

    begin("_IO_vsprintf");
    BUFFER_FORM(w__IO_vsprintf(b, FMT, ARGS));
    end();

    begin("__sprintf_chk");
    BUFFER_FORM(__sprintf_chk(b, FLAG, sizeof b, FMT, ARGS));
    {
        char b[LEN + 1];
        CHECK(__sprintf_chk(b, FLAG, sizeof b, FMT, ARGS) == LEN && eq(b, WANT)); /* exactly the object */
        CHECK(__sprintf_chk(b, FLAG, sizeof b, "ab%n", &n) == 2 && n == 2 && eq(b, "ab"));
    }
    CHECK(aborts(over_sprintf_chk));
    CHECK(aborts(n_sprintf_chk));
    {
        /* SPRINTF-LS-2 (§12.6, C0.5): ls -l's shape; an overflow aborts only as the general route does. */
        char b[16];
        CHECK(__sprintf_chk(b, FLAG, sizeof b, "%s %*s ", "-rw-r--r--", 2, "1") == 14 && eq(b, "-rw-r--r--  1 "));
        CHECK(aborts(over_ls_chk));
        CHECK(aborts(bad_ls_chk) == aborts(bad_ls_chk_usual));
        CHECK(!aborts(bad_pos_chk));
        /* An invalid conversion after a valid prefix: the usual route's result and errno, the
         * buffer terminated where it terminates it, nothing beyond the object touched. The
         * unknown `%y` is copied as text (16 bytes), so the object is 24: its terminator fits. */
        char u[32], o[32];
        memset(u, 'Z', sizeof u);
        memset(o, 'Z', sizeof o);
        errno = 0;
        int ru = __sprintf_chk(u, FLAG, 24, bad_ls_usual, "", "-rw-r--r--", 2, "1");
        int eu = errno;
        errno = 0;
        int ro = __sprintf_chk(o, FLAG, 24, bad_ls, "-rw-r--r--", 2, "1");
        CHECK(ro == ru && errno == eu);
        CHECK(ro < 0 || memcmp(o, u, (size_t)ro + 1) == 0);
        CHECK(ro >= 0 || strlen(o) == strlen(u));
        CHECK(o[24] == 'Z' && o[31] == 'Z');
        memset(u, 'Z', sizeof u);
        memset(o, 'Z', sizeof o);
        errno = 0;
        ru = sprintf(u, bad_ab_usual, "", "XY");
        eu = errno;
        errno = 0;
        ro = sprintf(o, bad_ab, "XY");
        CHECK(ro == ru && errno == eu);
        CHECK(ro < 0 || memcmp(o, u, (size_t)ro + 1) == 0);
        CHECK(ro >= 0 || strlen(o) == strlen(u));
        CHECK(o[16] == 'Z' && o[31] == 'Z');
    }
    end();

    begin("__vsprintf_chk");
    BUFFER_FORM(w___vsprintf_chk(b, FLAG, sizeof b, FMT, ARGS));
    {
        char b[LEN + 1];
        CHECK(w___vsprintf_chk(b, FLAG, sizeof b, FMT, ARGS) == LEN && eq(b, WANT));
        CHECK(w___vsprintf_chk(b, FLAG, sizeof b, "ab%n", &n) == 2 && n == 2 && eq(b, "ab"));
    }
    CHECK(aborts(over_vsprintf_chk));
    CHECK(aborts(n_vsprintf_chk));
    end();

    begin("snprintf");
    BOUNDED_FORM(snprintf(n ? b : NULL, n, FMT, ARGS));
    end();

    begin("vsnprintf");
    BOUNDED_FORM(w_vsnprintf(n ? b : NULL, n, FMT, ARGS));
    end();

    begin("__vsnprintf");
    BOUNDED_FORM(w___vsnprintf(n ? b : NULL, n, FMT, ARGS));
    end();

    begin("__snprintf_chk");
    BOUNDED_FORM(__snprintf_chk(b, n, FLAG, sizeof b, FMT, ARGS));
    {
        char b[6];
        CHECK(__snprintf_chk(b, sizeof b, FLAG, sizeof b, FMT, ARGS) == LEN && eq(b, "<42|a")); /* truncated within the object */
        CHECK(__snprintf_chk(b, sizeof b, FLAG, sizeof b, "ab%n", &n) == 2 && n == 2 && eq(b, "ab"));
    }
    CHECK(aborts(over_snprintf_chk));
    CHECK(aborts(over_snprintf_chk_edge));
    CHECK(aborts(n_snprintf_chk));
    end();

    begin("__vsnprintf_chk");
    BOUNDED_FORM(w___vsnprintf_chk(b, n, FLAG, sizeof b, FMT, ARGS));
    {
        char b[6];
        CHECK(w___vsnprintf_chk(b, sizeof b, FLAG, sizeof b, FMT, ARGS) == LEN && eq(b, "<42|a"));
        CHECK(w___vsnprintf_chk(b, sizeof b, FLAG, sizeof b, "ab%n", &n) == 2 && n == 2 && eq(b, "ab"));
    }
    CHECK(aborts(over_vsnprintf_chk));
    CHECK(aborts(n_vsnprintf_chk));
    end();

    begin("swprintf");
    WBUFFER_FORM(swprintf(b, n, WFMT, ARGS));
    end();

    begin("vswprintf");
    WBUFFER_FORM(w_vswprintf(b, n, WFMT, ARGS));
    end();

    begin("__swprintf_chk");
    WBUFFER_FORM(__swprintf_chk(b, n, FLAG, 32, WFMT, ARGS));
    {
        wchar_t b[LEN + 1];
        CHECK(__swprintf_chk(b, LEN + 1, FLAG, LEN + 1, WFMT, ARGS) == LEN && weq(b, WWANT)); /* exactly the object */
    }
    CHECK(aborts(over_swprintf_chk));
    CHECK(aborts(n_swprintf_chk));
    end();

    begin("__vswprintf_chk");
    WBUFFER_FORM(w___vswprintf_chk(b, n, FLAG, 32, WFMT, ARGS));
    {
        wchar_t b[LEN + 1];
        CHECK(w___vswprintf_chk(b, LEN + 1, FLAG, LEN + 1, WFMT, ARGS) == LEN && weq(b, WWANT));
    }
    CHECK(aborts(over_vswprintf_chk));
    CHECK(aborts(n_vswprintf_chk));
    end();

    /* ---------------------------------------------------------------- allocations */

    begin("asprintf");
    ALLOC_FORM(asprintf(&p, FMT, ARGS), asprintf(&p, "%lc", (wint_t)0x110000));
    end();

    begin("__asprintf");
    ALLOC_FORM(__asprintf(&p, FMT, ARGS), __asprintf(&p, "%lc", (wint_t)0x110000));
    end();

    begin("vasprintf");
    ALLOC_FORM(w_vasprintf(&p, FMT, ARGS), w_vasprintf(&p, "%lc", (wint_t)0x110000));
    end();

    begin("__asprintf_chk");
    ALLOC_FORM(__asprintf_chk(&p, FLAG, FMT, ARGS), __asprintf_chk(&p, FLAG, "%lc", (wint_t)0x110000));
    {
        char *p = NULL;
        CHECK(__asprintf_chk(&p, FLAG, "ab%n", &n) == 2 && n == 2 && p != NULL && eq(p, "ab"));
        free(p);
    }
    CHECK(aborts(n_asprintf_chk));
    end();

    begin("__vasprintf_chk");
    ALLOC_FORM(w___vasprintf_chk(&p, FLAG, FMT, ARGS), w___vasprintf_chk(&p, FLAG, "%lc", (wint_t)0x110000));
    {
        char *p = NULL;
        CHECK(w___vasprintf_chk(&p, FLAG, "ab%n", &n) == 2 && n == 2 && p != NULL && eq(p, "ab"));
        free(p);
    }
    CHECK(aborts(n_vasprintf_chk));
    end();

    /* ---------------------------------------------------------------- obstacks */

    begin("obstack_printf");
    OBSTACK_FORM(obstack_printf(&ob, FMT, ARGS), obstack_printf(&ob, "%d", -7));
    end();

    begin("obstack_vprintf");
    OBSTACK_FORM(w_obstack_vprintf(&ob, FMT, ARGS), w_obstack_vprintf(&ob, "%d", -7));
    end();

    begin("__obstack_printf_chk");
    OBSTACK_FORM(__obstack_printf_chk(&ob, FLAG, FMT, ARGS), __obstack_printf_chk(&ob, FLAG, "%d", -7));
    {
        struct obstack ob;
        obstack_init(&ob);
        CHECK(__obstack_printf_chk(&ob, FLAG, "ab%n", &n) == 2 && n == 2 && obstack_object_size(&ob) == 2);
        obstack_free(&ob, NULL);
    }
    CHECK(aborts(n_obstack_printf_chk));
    end();

    begin("__obstack_vprintf_chk");
    OBSTACK_FORM(w___obstack_vprintf_chk(&ob, FLAG, FMT, ARGS), w___obstack_vprintf_chk(&ob, FLAG, "%d", -7));
    {
        struct obstack ob;
        obstack_init(&ob);
        CHECK(w___obstack_vprintf_chk(&ob, FLAG, "ab%n", &n) == 2 && n == 2 && obstack_object_size(&ob) == 2);
        obstack_free(&ob, NULL);
    }
    CHECK(aborts(n_obstack_vprintf_chk));
    end();

    fclose(stdout);
    unlink(path);
    return failures ? 1 : 0;
}
