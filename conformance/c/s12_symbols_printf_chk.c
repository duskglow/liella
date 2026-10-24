/* Translation tests for spec C0.5 and §12.6 (crates/boundary/tests/s12_stdio_symbols.rs): the
 * fortified printf forms where the operation does not write beyond the object and does not use
 * `%n` in writable memory, so they must return normally (C0.5):
 *   - the bounded forms `__snprintf_chk`, `__vsnprintf_chk`, `__swprintf_chk` and
 *     `__vswprintf_chk` with a bound larger than the object (or a zero-sized object) and a
 *     result that fits in it (D-C0.5: where the amount written depends on the input, a bound
 *     larger than the object alone does not terminate);
 *   - the wide forms with `%n` in a format in read-only memory (a string literal).
 * Liella terminates in each of these cases (the tests that run this program are ignored until
 * it does not). Each call runs in a child, which exits with the call's result. Self-checking:
 * each symbol's checks end with `end <symbol>` on standard error, each failed check prints
 * `FAIL <symbol>: line N: <check>`; the exit status is 1 when any check failed. */
#undef _FORTIFY_SOURCE
#define _GNU_SOURCE
#include <stdarg.h>
#include <stdio.h>
#include <sys/prctl.h>
#include <sys/syscall.h>
#include <sys/wait.h>
#include <unistd.h>
#include <wchar.h>

extern int __snprintf_chk(char *, size_t, int, size_t, const char *, ...);
extern int __vsnprintf_chk(char *, size_t, int, size_t, const char *, va_list);
extern int __swprintf_chk(wchar_t *, size_t, int, size_t, const wchar_t *, ...);
extern int __vswprintf_chk(wchar_t *, size_t, int, size_t, const wchar_t *, va_list);
extern int __wprintf_chk(int, const wchar_t *, ...);
extern int __vwprintf_chk(int, const wchar_t *, va_list);
extern int __fwprintf_chk(FILE *, int, const wchar_t *, ...);
extern int __vfwprintf_chk(FILE *, int, const wchar_t *, va_list);

static int failures;
static const char *current;

static void begin(const char *symbol) { current = symbol; }
static void end(void) { fprintf(stderr, "end %s\n", current); }

#define CHECK(cond) do { if (!(cond)) { failures++; fprintf(stderr, "FAIL %s: line %d: %s\n", current, __LINE__, #cond); } } while (0)

#define VA(call) do { va_list ap; va_start(ap, fmt); r = (call); va_end(ap); } while (0)
static int w___vsnprintf_chk(char *s, size_t n, int flag, size_t slen, const char *fmt, ...) { int r; VA(__vsnprintf_chk(s, n, flag, slen, fmt, ap)); return r; }
static int w___vswprintf_chk(wchar_t *s, size_t n, int flag, size_t slen, const wchar_t *fmt, ...) { int r; VA(__vswprintf_chk(s, n, flag, slen, fmt, ap)); return r; }
static int w___vwprintf_chk(int flag, const wchar_t *fmt, ...) { int r; VA(__vwprintf_chk(flag, fmt, ap)); return r; }
static int w___vfwprintf_chk(FILE *f, int flag, const wchar_t *fmt, ...) { int r; VA(__vfwprintf_chk(f, flag, fmt, ap)); return r; }

/* `f` run in a child returns normally with `want` (its exit status); no core dump
 * is made if it aborts instead. */
static int returns(int (*f)(void), int want)
{
    fflush(NULL);
    pid_t pid = fork();
    if (pid == 0) {
        syscall(SYS_prctl, PR_SET_DUMPABLE, 0, 0, 0, 0); /* no core dump handler (slow) */
        _exit(f());
    }
    int st = -1;
    if (pid <= 0 || syscall(SYS_wait4, pid, &st, 0, NULL) != pid)
        return 0;
    if (WIFEXITED(st) && WEXITSTATUS(st) == want)
        return 1;
    fprintf(stderr, "  status %#x, want exit %d\n", st, want);
    return 0;
}

/* The result of a call whose format has `%n` after "ab"; 100 when `%n` did not store 2. */
static int stored;
#define WITH_N(call) (stored = -1, r = (call), stored == 2 ? r : 100)

static int snprintf_larger(void) { char b[8]; return __snprintf_chk(b, 16, 1, sizeof b, "%s", "ab"); }
static int snprintf_empty(void) { char b[1]; return __snprintf_chk(b, 0, 1, 0, "%s", "ab"); }
static int vsnprintf_larger(void) { char b[8]; return w___vsnprintf_chk(b, 16, 1, sizeof b, "%s", "ab"); }
static int vsnprintf_empty(void) { char b[1]; return w___vsnprintf_chk(b, 0, 1, 0, "%s", "ab"); }
static int swprintf_larger(void) { wchar_t b[8]; return __swprintf_chk(b, 16, 1, 8, L"%s", "ab"); }
static int vswprintf_larger(void) { wchar_t b[8]; return w___vswprintf_chk(b, 16, 1, 8, L"%s", "ab"); }
static int wprintf_n(void) { int r; return WITH_N(__wprintf_chk(1, L"ab%n", &stored)); }
static int vwprintf_n(void) { int r; return WITH_N(w___vwprintf_chk(1, L"ab%n", &stored)); }
static int fwprintf_n(void) { int r; return WITH_N(__fwprintf_chk(tmpfile(), 1, L"ab%n", &stored)); }
static int vfwprintf_n(void) { int r; return WITH_N(w___vfwprintf_chk(tmpfile(), 1, L"ab%n", &stored)); }
static int swprintf_n(void) { int r; wchar_t b[8]; return WITH_N(__swprintf_chk(b, 8, 1, 8, L"ab%n", &stored)); }
static int vswprintf_n(void) { int r; wchar_t b[8]; return WITH_N(w___vswprintf_chk(b, 8, 1, 8, L"ab%n", &stored)); }

int main(void)
{
    begin("__snprintf_chk");
    CHECK(returns(snprintf_larger, 2));
    CHECK(returns(snprintf_empty, 2));
    end();

    begin("__vsnprintf_chk");
    CHECK(returns(vsnprintf_larger, 2));
    CHECK(returns(vsnprintf_empty, 2));
    end();

    begin("__swprintf_chk");
    CHECK(returns(swprintf_larger, 2));
    CHECK(returns(swprintf_n, 2));
    end();

    begin("__vswprintf_chk");
    CHECK(returns(vswprintf_larger, 2));
    CHECK(returns(vswprintf_n, 2));
    end();

    begin("__wprintf_chk");
    CHECK(returns(wprintf_n, 2));
    end();

    begin("__vwprintf_chk");
    CHECK(returns(vwprintf_n, 2));
    end();

    begin("__fwprintf_chk");
    CHECK(returns(fwprintf_n, 2));
    end();

    begin("__vfwprintf_chk");
    CHECK(returns(vfwprintf_n, 2));
    end();

    return failures ? 1 : 0;
}
