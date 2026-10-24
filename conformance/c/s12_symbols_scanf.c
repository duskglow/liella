/* Translation tests for spec §12.7 (crates/boundary/tests/s12_stdio_symbols.rs): every formatted
 * input entry point of the three symbol sets through real C varargs, and every `va_list` form
 * through a real `va_list` (a variadic wrapper per symbol). Each symbol is called by its own
 * name (an assembler label, so the headers' redirections to `__isoc99_`/`__isoc23_` do not
 * apply) and checked for its source (a string, a stream, `stdin`; bytes or wide characters),
 * its result convention (the assignments, 0 for a matching failure, `EOF` for an input failure
 * first) and its symbol set: `%i` reads a `0b` prefix only in the C23 set, and `%a` allocates
 * before `s` in the plain set and is a floating conversion in the others. The conversions
 * themselves are the clause tests' (s12_scanf.c). Self-checking: each symbol's checks end with
 * `end <symbol>` on standard error, each failed check prints `FAIL <symbol>: line N: <check>`;
 * the exit status is 1 when any check failed. */
#define _GNU_SOURCE
#include <errno.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <wchar.h>

static int failures;
static const char *current;

static void begin(const char *symbol) { current = symbol; }
static void end(void) { fprintf(stderr, "end %s\n", current); }

#define CHECK(cond) do { if (!(cond)) { failures++; fprintf(stderr, "FAIL %s: line %d: %s\n", current, __LINE__, #cond); } } while (0)

/* The symbol sets. */
enum { PLAIN, C99, C23 };

typedef int (*scan_t)(const char *, ...);
typedef int (*fscan_t)(FILE *, const char *, ...);
typedef int (*sscan_t)(const char *, const char *, ...);
typedef int (*wscan_t)(const wchar_t *, ...);
typedef int (*fwscan_t)(FILE *, const wchar_t *, ...);
typedef int (*swscan_t)(const wchar_t *, const wchar_t *, ...);

/* Each entry point by its exported name. */
#define SET(p, l) \
    extern int p##scanf_(const char *, ...) __asm__(l "scanf"); \
    extern int p##fscanf_(FILE *, const char *, ...) __asm__(l "fscanf"); \
    extern int p##sscanf_(const char *, const char *, ...) __asm__(l "sscanf"); \
    extern int p##wscanf_(const wchar_t *, ...) __asm__(l "wscanf"); \
    extern int p##fwscanf_(FILE *, const wchar_t *, ...) __asm__(l "fwscanf"); \
    extern int p##swscanf_(const wchar_t *, const wchar_t *, ...) __asm__(l "swscanf"); \
    extern int p##vscanf_(const char *, va_list) __asm__(l "vscanf"); \
    extern int p##vfscanf_(FILE *, const char *, va_list) __asm__(l "vfscanf"); \
    extern int p##vsscanf_(const char *, const char *, va_list) __asm__(l "vsscanf"); \
    extern int p##vwscanf_(const wchar_t *, va_list) __asm__(l "vwscanf"); \
    extern int p##vfwscanf_(FILE *, const wchar_t *, va_list) __asm__(l "vfwscanf"); \
    extern int p##vswscanf_(const wchar_t *, const wchar_t *, va_list) __asm__(l "vswscanf"); \
    static int p##w_vscanf(const char *fmt, ...) { int r; VA(p##vscanf_(fmt, ap)); return r; } \
    static int p##w_vfscanf(FILE *f, const char *fmt, ...) { int r; VA(p##vfscanf_(f, fmt, ap)); return r; } \
    static int p##w_vsscanf(const char *s, const char *fmt, ...) { int r; VA(p##vsscanf_(s, fmt, ap)); return r; } \
    static int p##w_vwscanf(const wchar_t *fmt, ...) { int r; VA(p##vwscanf_(fmt, ap)); return r; } \
    static int p##w_vfwscanf(FILE *f, const wchar_t *fmt, ...) { int r; VA(p##vfwscanf_(f, fmt, ap)); return r; } \
    static int p##w_vswscanf(const wchar_t *s, const wchar_t *fmt, ...) { int r; VA(p##vswscanf_(s, fmt, ap)); return r; }

#define VA(call) do { va_list ap; va_start(ap, fmt); r = (call); va_end(ap); } while (0)
SET(plain_, "")
SET(c99_, "__isoc99_")
SET(c23_, "__isoc23_")

/* The other names of the plain set. */
extern int vfscanf2_(FILE *, const char *, va_list) __asm__("__vfscanf");
extern int vsscanf2_(const char *, const char *, va_list) __asm__("__vsscanf");
extern int io_sscanf_(const char *, const char *, ...) __asm__("_IO_sscanf");
extern int io_vfscanf_(FILE *, const char *, va_list, int *) __asm__("_IO_vfscanf");
static int w___vfscanf(FILE *f, const char *fmt, ...) { int r; VA(vfscanf2_(f, fmt, ap)); return r; }
static int w___vsscanf(const char *s, const char *fmt, ...) { int r; VA(vsscanf2_(s, fmt, ap)); return r; }
static int w__IO_vfscanf(FILE *f, const char *fmt, ...) { int r; VA(io_vfscanf_(f, fmt, ap, NULL)); return r; }
static int errp_value;
static int w__IO_vfscanf_errp(FILE *f, const char *fmt, ...) { int r; VA(io_vfscanf_(f, fmt, ap, &errp_value)); return r; }

/* A stream reading `text` (not yet oriented). */
static FILE *input(const char *text)
{
    FILE *f = tmpfile();
    fputs(text, f);
    fflush(f);
    FILE *r = fdopen(dup(fileno(f)), "r");
    fclose(f);
    lseek(fileno(r), 0, SEEK_SET);
    return r;
}

/* The scratch file standard input reads. */
static char path[64];

/* stdin reopened on a file holding `text` (orientation reset). */
static void stdin_holds(const char *text)
{
    FILE *f = fopen(path, "w");
    fputs(text, f);
    fclose(f);
    CHECK(freopen(path, "r", stdin) == stdin);
}

/* The symbol-set check over a string source: `S` is a call taking (source, format, pointer). */
#define DIALECT(S, zb, xy, hex) do { \
    int i_ = -1; \
    CHECK(S(zb, "%i", &i_) == 1 && i_ == (set == C23 ? 5 : 0)); \
    if (set == PLAIN) { \
        char *p_ = NULL; \
        CHECK(S(xy, "%as", &p_) == 1 && p_ != NULL && strcmp(p_, "xy") == 0); \
        free(p_); \
    } else { \
        float x_ = 0; \
        CHECK(S(hex, "%a", &x_) == 1 && x_ == 8.0f); \
    } \
} while (0)

#define WDIALECT(S, zb, xy, hex) do { \
    int i_ = -1; \
    CHECK(S(zb, L"%i", &i_) == 1 && i_ == (set == C23 ? 5 : 0)); \
    if (set == PLAIN) { \
        char *p_ = NULL; \
        CHECK(S(xy, L"%as", &p_) == 1 && p_ != NULL && strcmp(p_, "xy") == 0); \
        free(p_); \
    } else { \
        float x_ = 0; \
        CHECK(S(hex, L"%a", &x_) == 1 && x_ == 8.0f); \
    } \
} while (0)

/* A string form. */
static void strings(const char *symbol, sscan_t f, int set)
{
    begin(symbol);
    int n = 0;
    char s[8] = "";
    CHECK(f("12 abc", "%d %s", &n, s) == 2 && n == 12 && strcmp(s, "abc") == 0);
    CHECK(f("x", "%d", &n) == 0);
    CHECK(f("", "%d", &n) == EOF);
#define S_(src, fmt, p) f(src, fmt, p)
    DIALECT(S_, "0b101", "xy", "0x1p3");
#undef S_
    end();
}

/* A stream form: the mismatching byte is left unread. */
static void streams(const char *symbol, fscan_t f, int set)
{
    begin(symbol);
    int n = 0;
    char s[8] = "";
    FILE *in = input("12 abc;7x");
    CHECK(f(in, "%d %s", &n, s) == 2 && n == 12 && strcmp(s, "abc;7x") == 0);
    fclose(in);
    in = input("7x");
    CHECK(f(in, "%d", &n) == 1 && n == 7 && fgetc(in) == 'x');
    CHECK(f(in, "%d", &n) == EOF && feof(in));
    fclose(in);
    in = input("x");
    CHECK(f(in, "%d", &n) == 0 && fgetc(in) == 'x');
    fclose(in);
#define S_(src, fmt, p) (in = input(src), r_ = f(in, fmt, p), fclose(in), r_)
    int r_;
    DIALECT(S_, "0b101", "xy", "0x1p3");
#undef S_
    end();
}

/* A form reading stdin. */
static void standard(const char *symbol, scan_t f, int set)
{
    begin(symbol);
    int n = 0;
    char s[8] = "";
    stdin_holds("12 abc 7x");
    CHECK(f("%d %s", &n, s) == 2 && n == 12 && strcmp(s, "abc") == 0);
    CHECK(f("%d", &n) == 1 && n == 7 && getchar() == 'x');
    CHECK(f("%d", &n) == EOF && feof(stdin));
#define S_(src, fmt, p) (stdin_holds(src), f(fmt, p))
    DIALECT(S_, "0b101", "xy", "0x1p3");
#undef S_
    end();
}

/* A wide string form. */
static void wstrings(const char *symbol, swscan_t f, int set)
{
    begin(symbol);
    int n = 0;
    wchar_t s[8] = L"";
    CHECK(f(L"12 abc", L"%d %ls", &n, s) == 2 && n == 12 && wcscmp(s, L"abc") == 0);
    CHECK(f(L"x", L"%d", &n) == 0);
    CHECK(f(L"", L"%d", &n) == EOF);
#define S_(src, fmt, p) f(src, fmt, p)
    WDIALECT(S_, L"0b101", L"xy", L"0x1p3");
#undef S_
    end();
}

/* A wide stream form: the stream becomes wide-oriented; the mismatching character is left
 * unread. */
static void wstreams(const char *symbol, fwscan_t f, int set)
{
    begin(symbol);
    int n = 0;
    wchar_t s[8] = L"";
    FILE *in = input("12 abc");
    CHECK(f(in, L"%d %ls", &n, s) == 2 && n == 12 && wcscmp(s, L"abc") == 0 && fwide(in, 0) > 0);
    fclose(in);
    in = input("7x");
    CHECK(f(in, L"%d", &n) == 1 && n == 7 && fgetwc(in) == L'x');
    CHECK(f(in, L"%d", &n) == EOF && feof(in));
    fclose(in);
#define S_(src, fmt, p) (in = input(src), r_ = f(in, fmt, p), fclose(in), r_)
    int r_;
    WDIALECT(S_, "0b101", "xy", "0x1p3");
#undef S_
    end();
}

/* A wide form reading stdin, which becomes wide-oriented. */
static void wstandard(const char *symbol, wscan_t f, int set)
{
    begin(symbol);
    int n = 0;
    wchar_t s[8] = L"";
    stdin_holds("12 abc 7x");
    CHECK(f(L"%d %ls", &n, s) == 2 && n == 12 && wcscmp(s, L"abc") == 0 && fwide(stdin, 0) > 0);
    CHECK(f(L"%d", &n) == 1 && n == 7 && getwchar() == L'x');
    CHECK(f(L"%d", &n) == EOF && feof(stdin));
#define S_(src, fmt, p) (stdin_holds(src), f(fmt, p))
    WDIALECT(S_, "0b101", "xy", "0x1p3");
#undef S_
    end();
}

int main(void)
{
    snprintf(path, sizeof path, "/tmp/liella-s12-symbols-scanf-%d", (int)getpid());

    strings("sscanf", plain_sscanf_, PLAIN);
    strings("vsscanf", plain_w_vsscanf, PLAIN);
    strings("__vsscanf", w___vsscanf, PLAIN);
    strings("_IO_sscanf", io_sscanf_, PLAIN);
    strings("__isoc99_sscanf", c99_sscanf_, C99);
    strings("__isoc99_vsscanf", c99_w_vsscanf, C99);
    strings("__isoc23_sscanf", c23_sscanf_, C23);
    strings("__isoc23_vsscanf", c23_w_vsscanf, C23);

    streams("fscanf", plain_fscanf_, PLAIN);
    streams("vfscanf", plain_w_vfscanf, PLAIN);
    streams("__vfscanf", w___vfscanf, PLAIN);
    streams("_IO_vfscanf", w__IO_vfscanf, PLAIN);
    begin("_IO_vfscanf");
    {
        /* With the error variable: the result is the same. */
        int n = 0;
        FILE *in = input("5");
        CHECK(w__IO_vfscanf_errp(in, "%d", &n) == 1 && n == 5);
        fclose(in);
        end();
    }
    streams("__isoc99_fscanf", c99_fscanf_, C99);
    streams("__isoc99_vfscanf", c99_w_vfscanf, C99);
    streams("__isoc23_fscanf", c23_fscanf_, C23);
    streams("__isoc23_vfscanf", c23_w_vfscanf, C23);

    standard("scanf", plain_scanf_, PLAIN);
    standard("vscanf", plain_w_vscanf, PLAIN);
    standard("__isoc99_scanf", c99_scanf_, C99);
    standard("__isoc99_vscanf", c99_w_vscanf, C99);
    standard("__isoc23_scanf", c23_scanf_, C23);
    standard("__isoc23_vscanf", c23_w_vscanf, C23);

    wstrings("swscanf", plain_swscanf_, PLAIN);
    wstrings("vswscanf", plain_w_vswscanf, PLAIN);
    wstrings("__isoc99_swscanf", c99_swscanf_, C99);
    wstrings("__isoc99_vswscanf", c99_w_vswscanf, C99);
    wstrings("__isoc23_swscanf", c23_swscanf_, C23);
    wstrings("__isoc23_vswscanf", c23_w_vswscanf, C23);

    wstreams("fwscanf", plain_fwscanf_, PLAIN);
    wstreams("vfwscanf", plain_w_vfwscanf, PLAIN);
    wstreams("__isoc99_fwscanf", c99_fwscanf_, C99);
    wstreams("__isoc99_vfwscanf", c99_w_vfwscanf, C99);
    wstreams("__isoc23_fwscanf", c23_fwscanf_, C23);
    wstreams("__isoc23_vfwscanf", c23_w_vfwscanf, C23);

    wstandard("wscanf", plain_wscanf_, PLAIN);
    wstandard("vwscanf", plain_w_vwscanf, PLAIN);
    wstandard("__isoc99_wscanf", c99_wscanf_, C99);
    wstandard("__isoc99_vwscanf", c99_w_vwscanf, C99);
    wstandard("__isoc23_wscanf", c23_wscanf_, C23);
    wstandard("__isoc23_vwscanf", c23_w_vwscanf, C23);

    unlink(path);
    return failures ? 1 : 0;
}
