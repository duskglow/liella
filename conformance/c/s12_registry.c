/* Spec §12.11: customizing printf through real C varargs (registration is process-wide, so it
 * has a program of its own). Self-checking: each clause ends with `end <clause>` on standard
 * error, each failed check prints `FAIL <clause>: line N: <check>`; the clause tests in
 * crates/core/tests/s12_stdio_clauses.rs read these lines. */
#define _GNU_SOURCE
#include <errno.h>
#include <printf.h>
#include <pthread.h>
#include <stdio.h>
#include <string.h>
#include <wchar.h>

static int failures;
static const char *current;

static void begin(const char *clause) { current = clause; }
static void end(void) { fprintf(stderr, "end %s\n", current); }

#define CHECK(cond) do { if (!(cond)) { failures++; fprintf(stderr, "FAIL %s: line %d: %s\n", current, __LINE__, #cond); } } while (0)

static int eq(const char *got, const char *want)
{
    if (strcmp(got, want) == 0)
        return 1;
    fprintf(stderr, "  got  [%s]\n  want [%s]\n", got, want);
    return 0;
}

#define FMT(want, ...) do { char b_[256]; int r_ = snprintf(b_, sizeof b_, __VA_ARGS__); CHECK(r_ == (int)strlen(want) && eq(b_, want)); } while (0)

/* Decimal text of a non-negative value, without printf (the handler runs inside one). */
static char *decimal(unsigned v, char *end)
{
    *--end = 0;
    do
        *--end = (char)('0' + v % 10);
    while ((v /= 10) != 0);
    return end;
}

/* Writes `<value:user>` for its int argument. */
static int handler(FILE *fp, const struct printf_info *info, const void *const *args)
{
    char a[16], u[16];
    char *va = decimal((unsigned)*(const int *)args[0], a + sizeof a);
    char *vu = decimal(info->user, u + sizeof u);
    fputs("<", fp);
    fputs(va, fp);
    fputs(":", fp);
    fputs(vu, fp);
    fputs(">", fp);
    return (int)(strlen(va) + strlen(vu) + 3);
}

static int arginfo(const struct printf_info *info, size_t n, int *types, int *size)
{
    (void)info;
    (void)size;
    if (n >= 1)
        types[0] = PA_INT;
    return 1;
}

static int arginfo3(const struct printf_info *info, size_t n, int *types)
{
    (void)info;
    if (n >= 1)
        types[0] = PA_INT;
    return 1;
}

static void va_reader(void *mem, va_list *ap)
{
    (void)mem;
    (void)ap;
}

static void *other_thread(void *arg)
{
    char *b = arg;
    snprintf(b, 64, "%W", 9);
    return NULL;
}

int main(void)
{
    begin("s12_11_c_1");
    CHECK(register_printf_specifier('W', handler, arginfo) == 0);
    FMT("[<42:0>]", "[%W]", 42);
    CHECK(register_printf_function('V', handler, arginfo3) == 0);
    FMT("[<7:0>|8]", "[%V|%d]", 7, 8);
    errno = 0;
    CHECK(register_printf_specifier(256, handler, arginfo) == -1 && errno == EINVAL);
    end();

    begin("s12_11_c_2");
    {
        int types[6] = { -7, -7, -7, -7, -7, -7 };
        CHECK(parse_printf_format("%d %s %f %c %*d", 6, types) == 6);
        CHECK(types[0] == PA_INT && types[1] == PA_STRING && types[2] == PA_DOUBLE && types[3] == PA_CHAR && types[4] == PA_INT && types[5] == PA_INT);
        int one[2] = { -7, -7 };
        CHECK(parse_printf_format("%s %d", 1, one) == 2 && one[0] == PA_STRING && one[1] == -7); /* the count, whatever the room */
        int bit = register_printf_modifier(L"QQ");
        CHECK(bit > 0);
        CHECK(register_printf_type(va_reader) >= PA_LAST);
        char want[64];
        snprintf(want, sizeof want, "<5:%d>|<6:0>", bit);
        FMT(want, "%QQW|%W", 5, 6);
    }
    end();

    begin("s12_11_t_0");
    {
        /* Global: a thread that registered nothing formats with the registration. */
        char b[64] = "";
        pthread_t t;
        CHECK(pthread_create(&t, NULL, other_thread, b) == 0 && pthread_join(t, NULL) == 0);
        CHECK(eq(b, "<9:0>"));
    }
    end();

    return failures ? 1 : 0;
}
