/* The printf registry end to end (spec §12.11; D-12.8): built against the host C library and
 * against Liella's products, the outputs must match line for line (registry_products.rs).
 * Black-box: registered handlers report the `struct printf_info` and arguments they receive
 * through the public <printf.h> interface; the library's results are printed as observed. */
#define _GNU_SOURCE
#include <errno.h>
#include <locale.h>
#include <printf.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
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
static void kv(const char *k, long v) { ch(' '); str(k); ch('='); num(v); }

/* What a handler saw, as a line. */
static void show_info(const struct printf_info *i) {
    kv("prec", i->prec); kv("width", i->width); kv("spec", i->spec);
    kv("ld", i->is_long_double); kv("short", i->is_short); kv("long", i->is_long); kv("char", i->is_char);
    kv("alt", i->alt); kv("space", i->space); kv("left", i->left); kv("showsign", i->showsign);
    kv("group", i->group); kv("extra", i->extra); kv("wide", i->wide); kv("i18n", i->i18n);
    kv("user", i->user); kv("pad", i->pad);
}

/* Handlers write with fputs only: `%d` is itself overridden below. */
static int put_long(FILE *fp, long v) {
    char b[24]; int n = 0; unsigned long u = v < 0 ? -(unsigned long)v : (unsigned long)v;
    char t[24]; int k = 0;
    if (v < 0) t[k++] = '-';
    do { b[n++] = (char)('0' + u % 10); u /= 10; } while (u);
    while (n) t[k++] = b[--n];
    t[k] = 0;
    fputs(t, fp);
    return k;
}
static int put_str(FILE *fp, const char *s) { fputs(s, fp); return (int)strlen(s); }

static int arginfo_calls;
/* `Q`: one int argument; the handler prints what it saw and returns its own length. */
static int q_arginfo(const struct printf_info *info, size_t n, int *types, int *size) {
    (void)info; (void)size;
    arginfo_calls++;
    if (n > 0) types[0] = PA_INT;
    return 1;
}
static int q_handler(FILE *fp, const struct printf_info *info, const void *const *args) {
    str("  Q handler"); show_info(info); kv("arg", *(const int *)args[0]); ch('\n');
    return put_str(fp, "<Q") + put_long(fp, *(const int *)args[0]) + put_str(fp, ">");
}

/* `V`: three arguments of different types. */
static int v_arginfo(const struct printf_info *info, size_t n, int *types, int *size) {
    (void)info; (void)size;
    arginfo_calls++;
    if (n > 0) types[0] = PA_STRING;
    if (n > 1) types[1] = PA_DOUBLE;
    if (n > 2) types[2] = PA_INT | PA_FLAG_LONG;
    return 3;
}
static int v_handler(FILE *fp, const struct printf_info *info, const void *const *args) {
    (void)info;
    double d = *(const double *)args[1];
    return put_str(fp, "<V ") + put_str(fp, *(const char *const *)args[0]) + put_str(fp, " ") + put_long(fp, (long)(d * 100)) + put_str(fp, "/100 ") + put_long(fp, *(const long *)args[2]) + put_str(fp, ">");
}

/* `D`: overrides nothing; `d` overridden below declines for negative values (returns -2). */
static int d_arginfo(const struct printf_info *info, size_t n, int *types, int *size) {
    (void)info; (void)size;
    if (n > 0) types[0] = PA_INT;
    return 1;
}
static int d_handler(FILE *fp, const struct printf_info *info, const void *const *args) {
    (void)info;
    int v = *(const int *)args[0];
    if (v < 0) return -2;
    return put_str(fp, "D(") + put_long(fp, v) + put_str(fp, ")");
}

/* `E`: fails with errno set. */
static int e_handler(FILE *fp, const struct printf_info *info, const void *const *args) {
    (void)fp; (void)info; (void)args;
    errno = ERANGE;
    return -1;
}

/* `N`: arginfo declines (returns -1): the built-in rules apply. */
static int n_arginfo(const struct printf_info *info, size_t n, int *types, int *size) {
    (void)info; (void)n; (void)types; (void)size;
    return -1;
}
static int n_handler(FILE *fp, const struct printf_info *info, const void *const *args) {
    (void)info; (void)args;
    return put_str(fp, "<N>");
}

/* A registered type: a pair of ints, read from the va_list by its own function. */
struct pair { int a, b; };
static int pair_type;
static void pair_va(void *mem, va_list *ap) {
    struct pair p = va_arg(*ap, struct pair);
    memcpy(mem, &p, sizeof p);
}
static int p_arginfo(const struct printf_info *info, size_t n, int *types, int *size) {
    (void)info;
    if (n > 0) { types[0] = pair_type; size[0] = sizeof(struct pair); }
    return 1;
}
static int p_handler(FILE *fp, const struct printf_info *info, const void *const *args) {
    (void)info;
    const struct pair *p = *(const struct pair *const *)args[0];
    return put_str(fp, "(") + put_long(fp, p->a) + put_str(fp, ",") + put_long(fp, p->b) + put_str(fp, ")");
}

static int bits[20];

static void out(const char *label, int r, const char *buf) {
    str(label); str(" -> "); num(r); ch(' '); text(buf, r >= 0 ? strlen(buf) : 0); ch('\n');
}

static void parse_formats(void) {
    str("== parse_printf_format\n");
    const char *fmts[] = {
        "%i", "%hhi", "%hi", "%li", "%lli", "%qi", "%Li", "%ji", "%zi", "%ti", "%w8i", "%w16i", "%w32i", "%w64i",
        "%wf8i", "%wf16i", "%wf32i", "%wf64i", "%u %o %x %X %b %B", "%hu %lo %llx %hhX %zb %LB", "%d %ld", "%c %lc %C", "%s %ls %S", "%p %n %m",
        "%f %Lf %qf %lf %e %g %a", "%*d", "%.*f", "%*.*s", "%2$d %1$s", "%3$*1$.*2$d", "%%", "plain", "%Q", "%V %d",
        "%5Q %-Q", "%N", "%N %d", "%P", "%zQ", "%XYZd", "%XYd", "%XYZQ", "%y", "%1$d %3$d", "%",
        NULL,
    };
    for (const char **f = fmts; *f; f++) {
        int types[16];
        for (int i = 0; i < 16; i++) types[i] = -7;
        arginfo_calls = 0;
        size_t n = parse_printf_format(*f, 16, types);
        text(*f, strlen(*f)); str(" -> "); num((long)n); str(" types:");
        for (size_t i = 0; i < 16 && i < n + 1; i++) { ch(' '); num(types[i]); }
        kv("arginfo_calls", arginfo_calls); ch('\n');
        int few[1] = { -7 };
        size_t m = parse_printf_format(*f, 1, few);
        str("  n=1 -> "); num((long)m); kv("type0", few[0]); ch('\n');
    }
}

static void conversions(void) {
    str("== conversions\n");
    char buf[512];
    int r;
    r = snprintf(buf, sizeof buf, "[%Q] [%5Q] [%-8Q] [%+.3Q] [%#0'Q] [% Q] [%*Q] [%-*.*Q]", 1, 2, 3, 4, 5, 6, 7, 8, -9, 3, 10);
    out("snprintf Q", r, buf);
    r = snprintf(buf, sizeof buf, "[%hQ] [%hhQ] [%lQ] [%llQ] [%LQ] [%qQ] [%zQ] [%jQ] [%tQ] [%IQ]", 1, 2, 3, 4, 5, 6, 7, 8, 9, 10);
    out("snprintf Q lengths", r, buf);
    r = snprintf(buf, sizeof buf, "[%XYZQ] [%XYQ] [%XQ] [%XYZXYQ] [%hXYQ]", 1, 2, 3, 4, 5);
    out("snprintf Q modifiers", r, buf);
    r = snprintf(buf, sizeof buf, "%V|%d", "str", 2.5, 77L, 9);
    out("snprintf V", r, buf);
    r = snprintf(buf, sizeof buf, "%d %d %5d %-5d", 3, -3, 4, -4);
    out("snprintf d override", r, buf);
    errno = 0;
    r = snprintf(buf, sizeof buf, "ab%Ecd", 1);
    out("snprintf E", r, buf); kv("errno", errno); ch('\n');
    r = snprintf(buf, sizeof buf, "%N %5N %d", 1, 2);
    out("snprintf N", r, buf);
    struct pair pp = { 3, 4 };
    r = snprintf(buf, sizeof buf, "%P %d %P", pp, 8, (struct pair){ 5, 6 });
    out("snprintf P", r, buf);
    r = snprintf(buf, 6, "%Q%Q", 11, 22);
    out("snprintf Q truncated", r, buf);
    flush_out();
    fflush(stdout);
    r = printf("printf: %Q %V\n", 31, "x", 0.5, 2L);
    fflush(stdout);
    str("printf -> "); num(r); ch('\n');
    wchar_t wbuf[128];
    r = swprintf(wbuf, 128, L"[%Q] [%5Q]", 41, 42);
    char nb[512];
    snprintf(nb, sizeof nb, "%ls", r >= 0 ? wbuf : L"");
    out("swprintf Q", r, nb);
    char *a = NULL;
    r = asprintf(&a, "%Q-%P", 51, (struct pair){ 7, 8 });
    out("asprintf", r, a ? a : "");
    free(a);
}

static void sizes(void) {
    str("== printf_size\n");
    const char *fmts[] = { "%k", "%K", "%5k", "%-8k|", "%.0k", "%.1K", "%+k", "% k", "%08k", "%Lk", "%#k", "%'k", NULL };
    const double vals[] = { 0, 1, 999, 1000, 1023, 1024, 1536, 1048576, 1e9, 1e30, -2048, 1e300 };
    for (const char **f = fmts; *f; f++)
        for (size_t k = 0; k < sizeof vals / sizeof vals[0]; k++) {
            char buf[256];
            int r = **f && strchr(*f, 'L') ? snprintf(buf, sizeof buf, *f, (long double)vals[k]) : snprintf(buf, sizeof buf, *f, vals[k]);
            text(*f, strlen(*f)); str(" value#"); num((long)k); str(" -> "); num(r); ch(' '); text(buf, r >= 0 ? strlen(buf) : 0); ch('\n');
        }
    char buf[64];
    int r = snprintf(buf, sizeof buf, "%k %K %k", __builtin_inf(), -__builtin_inf(), __builtin_nan(""));
    out("special", r, buf);
    int types[4] = { -7, -7, -7, -7 };
    struct printf_info info;
    memset(&info, 0, sizeof info);
    info.spec = 'k';
    str("printf_size_info -> "); num((long)printf_size_info(&info, 4, types)); kv("t0", types[0]); kv("t1", types[1]);
    info.is_long_double = 1;
    num((long)printf_size_info(&info, 4, types)); kv("t0", types[0]); ch('\n');
}

static void fp_direct(void) {
    str("== __printf_fp\n");
    double vals[] = { 0, 1.5, -2.25, 1234567.891, 1e-5, 1e20 };
    const char specs[] = "feEgGaA";
    for (const char *sp = specs; *sp; sp++)
        for (size_t k = 0; k < sizeof vals / sizeof vals[0]; k++)
            for (int variant = 0; variant < 6; variant++) {
                struct printf_info info;
                memset(&info, 0, sizeof info);
                info.spec = (wchar_t)*sp;
                info.prec = variant == 1 ? 2 : variant == 2 ? 0 : -1;
                info.width = variant == 3 ? 14 : variant == 4 ? 12 : 0;
                info.left = variant == 4;
                info.showsign = variant == 5;
                info.alt = variant == 2;
                info.group = variant == 3;
                info.pad = variant == 3 ? '0' : ' ';
                const double *pv = &vals[k];
                const void *args[1] = { pv };
                char *b = NULL;
                size_t bl = 0;
                FILE *m = open_memstream(&b, &bl);
                int r = __printf_fp(m, &info, args);
                fclose(m);
                ch(*sp); kv("v", (long)(vals[k] * 1000)); kv("variant", variant); str(" -> "); num(r); ch(' '); text(b, bl); ch('\n');
                free(b);
            }
    long double lv = 3.25L;
    const long double *plv = &lv;
    const void *largs[1] = { plv };
    struct printf_info info;
    memset(&info, 0, sizeof info);
    info.spec = 'f'; info.prec = 3; info.is_long_double = 1; info.pad = ' ';
    char *b = NULL;
    size_t bl = 0;
    FILE *m = open_memstream(&b, &bl);
    int r = __printf_fp(m, &info, largs);
    fclose(m);
    str("long double -> "); num(r); ch(' '); text(b, bl); ch('\n');
    free(b);
}

int main(void) {
    setlocale(LC_ALL, "C");
    str("== registration\n");
    errno = 0; kv("spec_neg", register_printf_specifier(-1, q_handler, q_arginfo)); kv("errno", errno);
    errno = 0; kv("spec_256", register_printf_specifier(256, q_handler, q_arginfo)); kv("errno", errno); ch('\n');
    kv("Q", register_printf_specifier('Q', q_handler, q_arginfo));
    kv("V", register_printf_specifier('V', v_handler, v_arginfo));
    kv("d", register_printf_specifier('d', d_handler, d_arginfo));
    kv("E", register_printf_specifier('E', e_handler, q_arginfo));
    kv("N", register_printf_specifier('N', n_handler, n_arginfo));
    kv("k", register_printf_function('k', printf_size, printf_size_info));
    kv("K", register_printf_function('K', printf_size, printf_size_info));
    pair_type = register_printf_type(pair_va);
    kv("pair_type", pair_type);
    kv("P", register_printf_specifier('P', p_handler, p_arginfo)); ch('\n');
    errno = 0; kv("mod_empty", register_printf_modifier(L"")); kv("errno", errno);
    errno = 0; kv("mod_wide", register_printf_modifier(L"\x100")); kv("errno", errno); ch('\n');
    bits[0] = register_printf_modifier(L"XYZ");
    bits[1] = register_printf_modifier(L"XY");
    bits[2] = register_printf_modifier(L"X");
    str("modifiers:"); for (int i = 0; i < 3; i++) kv("bit", bits[i]); ch('\n');
    str("more modifiers:");
    for (int i = 3; i < 18; i++) {
        wchar_t m[4] = { L'm', (wchar_t)(L'a' + i), 0, 0 };
        errno = 0;
        bits[i] = register_printf_modifier(m);
        kv("bit", bits[i]); if (bits[i] < 0) kv("errno", errno);
    }
    ch('\n');
    parse_formats();
    conversions();
    sizes();
    fp_direct();
    flush_out();
    return 0;
}
