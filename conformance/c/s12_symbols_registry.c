/* Translation tests for spec §12.11 (crates/boundary/tests/s12_stdio_symbols.rs): customizing
 * printf through real C calls. Each registration is checked by its result convention and by a
 * real `snprintf` (or `parse_printf_format`) that reaches the registered functions with the C
 * objects they expect (`struct printf_info`, the argument pointers, a `va_list *` for a
 * registered type); the ready-made `printf_size`, `printf_size_info` and `__printf_fp` are
 * checked registered and called directly. Registration is process-wide, so this program has
 * its own process. Self-checking: each symbol's checks end with `end <symbol>` on standard
 * error, each failed check prints `FAIL <symbol>: line N: <check>`; the exit status is 1 when
 * any check failed. */
#define _GNU_SOURCE
#include <errno.h>
#include <printf.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <wchar.h>

/* Exported, not declared by the installed headers. */
extern int __printf_fp(FILE *, const struct printf_info *, const void *const *);

static int failures;
static const char *current;

static void begin(const char *symbol) { current = symbol; }
static void end(void) { fprintf(stderr, "end %s\n", current); }

#define CHECK(cond) do { if (!(cond)) { failures++; fprintf(stderr, "FAIL %s: line %d: %s\n", current, __LINE__, #cond); } } while (0)

static int eq(const char *got, const char *want)
{
    if (strcmp(got, want) == 0)
        return 1;
    fprintf(stderr, "  got  [%s]\n  want [%s]\n", got, want);
    return 0;
}

/* snprintf gives `want`, and returns its length. */
#define FMT(want, ...) do { char b_[256]; int r_ = snprintf(b_, sizeof b_, __VA_ARGS__); CHECK(eq(b_, want) && r_ == (int)strlen(want)); } while (0)

/* Decimal text of a value, without printf (the handlers run inside one). */
static const char *decimal(long v, char *buf, size_t size)
{
    char *p = buf + size;
    unsigned long u = v < 0 ? -(unsigned long)v : (unsigned long)v;
    *--p = 0;
    do
        *--p = (char)('0' + u % 10);
    while ((u /= 10) != 0);
    if (v < 0)
        *--p = '-';
    return p;
}

/* Writes `<value:user>` for its int argument. */
static int handler(FILE *fp, const struct printf_info *info, const void *const *args)
{
    char a[24], u[24];
    const char *va = decimal(*(const int *)args[0], a, sizeof a), *vu = decimal(info->user, u, sizeof u);
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

/* The older argument function (no sizes). */
static int arginfo3(const struct printf_info *info, size_t n, int *types)
{
    (void)info;
    if (n >= 1)
        types[0] = PA_INT;
    return 1;
}

/* A registered type: a pair of ints, read from the caller's `va_list` by `pair_reader`. */
struct pair { int a, b; };
static int pair_type = -1;

static void pair_reader(void *mem, va_list *ap)
{
    *(struct pair *)mem = va_arg(*ap, struct pair);
}

static int pair_arginfo(const struct printf_info *info, size_t n, int *types, int *size)
{
    (void)info;
    if (n >= 1) {
        types[0] = pair_type;
        size[0] = sizeof(struct pair);
    }
    return 1;
}

/* Writes `(a,b)` for its pair argument (`args[0]` points to the pointer to the object). */
static int pair_handler(FILE *fp, const struct printf_info *info, const void *const *args)
{
    char a[24], b[24];
    const struct pair *p = *(const struct pair *const *)args[0];
    const char *va = decimal(p->a, a, sizeof a), *vb = decimal(p->b, b, sizeof b);
    (void)info;
    fputs("(", fp);
    fputs(va, fp);
    fputs(",", fp);
    fputs(vb, fp);
    fputs(")", fp);
    return (int)(strlen(va) + strlen(vb) + 3);
}

/* `info` formatted by `f` (printf_size or __printf_fp) into a memory stream, for the value
 * at `value`: the text, and the result in `*r`. */
static const char *direct(int (*f)(FILE *, const struct printf_info *, const void *const *), const struct printf_info *info, const void *value, int *r)
{
    static char out[64];
    char *p = NULL;
    size_t size = 0;
    FILE *m = open_memstream(&p, &size);
    const void *args[1] = { value };
    *r = f(m, info, args);
    fclose(m);
    snprintf(out, sizeof out, "%s", p ? p : "");
    free(p);
    return out;
}

/* A `printf_info` for conversion `spec` with no flags, no width and no precision. */
static struct printf_info plain_info(wchar_t spec)
{
    struct printf_info i;
    memset(&i, 0, sizeof i);
    i.prec = -1;
    i.spec = spec;
    i.pad = L' ';
    return i;
}

int main(void)
{
    int r;

    begin("register_printf_specifier");
    CHECK(register_printf_specifier('W', handler, arginfo) == 0);
    FMT("[<42:0>]", "[%W]", 42);
    errno = 0;
    CHECK(register_printf_specifier(256, handler, arginfo) == -1 && errno == EINVAL);
    errno = 0;
    CHECK(register_printf_specifier(-1, handler, arginfo) == -1 && errno == EINVAL);
    end();

    begin("register_printf_function");
    CHECK(register_printf_function('V', handler, arginfo3) == 0);
    FMT("[<7:0>|8]", "[%V|%d]", 7, 8);
    errno = 0;
    CHECK(register_printf_function(256, handler, arginfo3) == -1 && errno == EINVAL);
    end();

    begin("register_printf_modifier");
    {
        int bit = register_printf_modifier(L"QQ");
        CHECK(bit > 0);
        char want[32];
        snprintf(want, sizeof want, "<5:%d>|<6:0>", bit);
        FMT(want, "%QQW|%W", 5, 6);
        errno = 0;
        CHECK(register_printf_modifier(L"") == -1 && errno == EINVAL);
    }
    end();

    begin("register_printf_type");
    pair_type = register_printf_type(pair_reader);
    CHECK(pair_type >= PA_LAST);
    CHECK(register_printf_specifier('P', pair_handler, pair_arginfo) == 0);
    {
        struct pair v = { 3, -4 };
        FMT("[(3,-4)|9]", "[%P|%d]", v, 9);
    }
    end();

    begin("parse_printf_format");
    {
        int types[5] = { -7, -7, -7, -7, -7 };
        CHECK(parse_printf_format("%d %s %W %f", 5, types) == 4);
        CHECK(types[0] == PA_INT && types[1] == PA_STRING && types[2] == PA_INT && types[3] == PA_DOUBLE && types[4] == -7);
        int one[2] = { -7, -7 };
        CHECK(parse_printf_format("%s %P", 1, one) == 2 && one[0] == PA_STRING && one[1] == -7); /* the count, whatever the room */
        CHECK(parse_printf_format("%P", 1, one) == 1 && one[0] == pair_type);
    }
    end();

    begin("printf_size");
    CHECK(register_printf_function('k', printf_size, printf_size_info) == 0);
    CHECK(register_printf_function('K', printf_size, printf_size_info) == 0);
    FMT("[1.500k|1.536K|1.5k|1.000k]", "[%k|%K|%.1k|%Lk]", 1536.0, 1536.0, 1536.0, 1024.0L);
    {
        struct printf_info i = plain_info(L'k');
        double v = 1536.0;
        CHECK(eq(direct(printf_size, &i, &v, &r), "1.500k") && r == 6);
    }
    end();

    begin("printf_size_info");
    {
        struct printf_info i = plain_info(L'k');
        int t = -7;
        CHECK(printf_size_info(&i, 1, &t) == 1 && t == PA_DOUBLE);
        i.is_long_double = 1;
        CHECK(printf_size_info(&i, 1, &t) == 1 && t == (PA_DOUBLE | PA_FLAG_LONG_DOUBLE));
        t = -7;
        CHECK(printf_size_info(&i, 0, &t) == 1 && t == -7); /* no room: the count only */
        int types[2] = { -7, -7 };
        CHECK(parse_printf_format("%k%Lk", 2, types) == 2 && types[0] == PA_DOUBLE && types[1] == (PA_DOUBLE | PA_FLAG_LONG_DOUBLE));
    }
    end();

    begin("__printf_fp");
    {
        struct printf_info i = plain_info(L'f');
        double v = 1.5;
        CHECK(eq(direct(__printf_fp, &i, &v, &r), "1.500000") && r == 8);
        i.prec = 2;
        i.width = 8;
        CHECK(eq(direct(__printf_fp, &i, &v, &r), "    1.50") && r == 8);
        i.left = 1;
        i.spec = L'e';
        CHECK(eq(direct(__printf_fp, &i, &v, &r), "1.50e+00") && r == 8);
        struct printf_info l = plain_info(L'f');
        long double lv = -2.25L;
        l.is_long_double = 1;
        l.prec = 1;
        CHECK(eq(direct(__printf_fp, &l, &lv, &r), "-2.2") && r == 4); /* ties to even */
    }
    end();

    return failures ? 1 : 0;
}
