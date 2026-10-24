/* scanf end to end (spec §12.7; D-12.6, D-12.13): built against the host C library and against
 * Liella's products, the outputs must match line for line (scanf_products.rs). Each line is one
 * scan: the symbol set, the input, the format, the result, the bytes stored in each argument,
 * and for streams the position afterwards and the next character (what the scan left unread). */
#define _GNU_SOURCE
#include <errno.h>
#include <locale.h>
#include <stdarg.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <wchar.h>

/* The three symbol sets, named explicitly: the headers would pick one by language mode. */
extern int gnu_sscanf(const char *, const char *, ...) __asm__("sscanf");
extern int gnu_fscanf(FILE *, const char *, ...) __asm__("fscanf");
extern int gnu_swscanf(const wchar_t *, const wchar_t *, ...) __asm__("swscanf");
extern int gnu_fwscanf(FILE *, const wchar_t *, ...) __asm__("fwscanf");
extern int c99_sscanf(const char *, const char *, ...) __asm__("__isoc99_sscanf");
extern int c99_fscanf(FILE *, const char *, ...) __asm__("__isoc99_fscanf");
extern int c99_swscanf(const wchar_t *, const wchar_t *, ...) __asm__("__isoc99_swscanf");
extern int c99_fwscanf(FILE *, const wchar_t *, ...) __asm__("__isoc99_fwscanf");
extern int c23_sscanf(const char *, const char *, ...) __asm__("__isoc23_sscanf");
extern int c23_fscanf(FILE *, const char *, ...) __asm__("__isoc23_fscanf");
extern int c23_swscanf(const wchar_t *, const wchar_t *, ...) __asm__("__isoc23_swscanf");
extern int c23_fwscanf(FILE *, const wchar_t *, ...) __asm__("__isoc23_fwscanf");
extern int c99_vsscanf(const char *, const char *, va_list) __asm__("__isoc99_vsscanf");

static char obuf[1 << 16];
static size_t used;
static void flush_out(void) { if (used) { (void)!write(1, obuf, used); used = 0; } }
static int linebuf; /* SCANF_DIFF_LINEBUF set: flush each line (to find a crashing case) */
static void ch(char c) { if (used == sizeof obuf) flush_out(); obuf[used++] = c; if (c == '\n' && linebuf) flush_out(); }
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
static void hex(const unsigned char *p, size_t n) {
    const char *d = "0123456789abcdef";
    for (size_t i = 0; i < n; i++) { ch(d[p[i] >> 4]); ch(d[p[i] & 15]); }
}

#define SLOTS 4
#define SLOT 64
static unsigned char slot[SLOTS][SLOT] __attribute__((aligned(16)));
static void clear(void) { memset(slot, 0xAA, sizeof slot); }
/* The length of an allocated result: 0 for a string, else that many bytes (%Nmc). */
static size_t mlen;
/* Each argument's bytes, up to the last one the scan changed. */
static void slots(int alloc) {
    for (int k = 0; k < SLOTS; k++) {
        if (alloc & (1 << k)) {
            char *p; memcpy(&p, slot[k], sizeof p);
            str(" m"); num(k); ch('=');
            if (p == (char *)0xAAAAAAAAAAAAAAAAull) str("unset");
            else if (!p) str("null");
            else { text(p, mlen ? mlen : strlen(p)); free(p); }
            continue;
        }
        int last = -1;
        for (int i = 0; i < SLOT; i++) if (slot[k][i] != 0xAA) last = i;
        if (last < 0) continue;
        ch(' '); ch('a' + k); ch('='); hex(slot[k], (size_t)last + 1);
    }
}

typedef int (*sfn)(const char *, const char *, ...);
typedef int (*ffn)(FILE *, const char *, ...);
typedef int (*wsfn)(const wchar_t *, const wchar_t *, ...);
typedef int (*wffn)(FILE *, const wchar_t *, ...);
static const char *setname[] = { "gnu", "c99", "c23" };
static sfn S[] = { gnu_sscanf, c99_sscanf, c23_sscanf };
static ffn F[] = { gnu_fscanf, c99_fscanf, c23_fscanf };
static wsfn WS[] = { gnu_swscanf, c99_swscanf, c23_swscanf };
static wffn WF[] = { gnu_fwscanf, c99_fwscanf, c23_fwscanf };

/* A stream reading `in`, with no orientation yet (written through its descriptor). */
static FILE *file_with(const char *in) {
    FILE *f = tmpfile();
    (void)!write(fileno(f), in, strlen(in));
    rewind(f);
    return f;
}

static void head(const char *how, int set, const char *in, const char *fmt) {
    str(how); ch(' '); str(setname[set]); ch(' '); text(in, strlen(in)); ch(' '); text(fmt, strlen(fmt));
}

/* One case through every input kind: string, stream, wide string, wide stream. `alloc` marks
 * the arguments that receive an allocation (%m). */
static void one(int set, const char *in, const char *fmt, int alloc, int wide) {
    void *a = slot[0], *b = slot[1], *c = slot[2], *d = slot[3];
    clear(); errno = 0;
    int r = S[set](in, fmt, a, b, c, d);
    head("s", set, in, fmt); str(" -> "); num(r); slots(alloc); ch('\n');

    FILE *f = file_with(in);
    clear();
    r = F[set](f, fmt, a, b, c, d);
    head("f", set, in, fmt); str(" -> "); num(r); slots(alloc);
    str(" pos="); num(ftell(f)); str(" next="); num(fgetc(f)); str(" eof="); num(feof(f)); ch('\n');
    fclose(f);

    if (!wide) return;
    wchar_t win[256], wfmt[256];
    if (mbstowcs(win, in, 256) == (size_t)-1 || mbstowcs(wfmt, fmt, 256) == (size_t)-1) return;
    clear();
    r = WS[set](win, wfmt, a, b, c, d);
    head("ws", set, in, fmt); str(" -> "); num(r); slots(alloc); ch('\n');

    f = file_with(in);
    clear();
    r = WF[set](f, wfmt, a, b, c, d);
    head("wf", set, in, fmt); str(" -> "); num(r); slots(alloc);
    str(" pos="); num(ftell(f)); str(" next="); num((long)(int)fgetwc(f)); ch('\n');
    fclose(f);
}

static void grid(const char **fmts, const char **ins, int wide) {
    for (int set = 0; set < 3; set++)
        for (const char **fp = fmts; *fp; fp++)
            for (const char **ip = ins; *ip; ip++)
                one(set, *ip, *fp, 0, wide);
}

static const char *int_fmts[] = {
    "%d", "%i", "%u", "%o", "%x", "%X", "%b", "%hhd", "%hd", "%ld", "%lld", "%hhu", "%hu", "%lu",
    "%jd", "%zd", "%td", "%qd", "%Lx", "%p", "%3d", "%1x", "%2i", "%*d", "%hhi", "%lb", "%2b", NULL,
};
static const char *int_ins[] = {
    "0", "42", "-42", "+7", "  12abc", "0x1F", "0X", "0xg", "0b101", "0B2", "-0b11", "017", "08",
    "2147483647", "2147483648", "-2147483649", "4294967295", "4294967296", "99999999999999999999",
    "-1", "300", "-129", "", "   ", "abc", "-", "+", "1e5", "0x", "ff", " -0x10", "0b", "-0", NULL,
};
static const char *float_fmts[] = { "%f", "%e", "%g", "%a", "%E", "%lf", "%Lf", "%le", "%La", "%*f", "%lA", NULL };
static const char *float_ins[] = {
    "1", "-1.5", "1e10", ".5", "5.", ".", "inf", "INFINITY", "infin", "-inf", "nan",
    "nan(123)", "nan(", "NaN(abc)x", "0x1p3", "0x1.8p-2", "0x", "0xp", "1e400", "1e-400", "-0",
    "3.14159265358979323846264", "1,5", "+.e", "0x1.fffffffffffffp1023", "4.9e-324", "1.5e-45",
    "  2.5x", "", "e5", "1.e5", "0x.8", "123456789012345678901234567890", NULL,
};
/* Widths that end the field on a complete number. */
static const char *fwidth_fmts[] = { "%5f", "%2lf", "%3Lf", "%4le", NULL };
static const char *fwidth_ins[] = { "1.5", "12", "-1.25e3", "123456", "  2.5x", "inf", "nan", "-.5", NULL };
/* A field that is a proper prefix of a number: spec §12.7 says the scan consumes the prefix
 * and fails; the reference library converts the longest number in it. */
static const char *partial_fmts[] = { "%f", "%lf", "%Lf", "%5f", "%2lf", "%3Lf", NULL };
static const char *partial_ins[] = { "1e", "1e+", "1.5e-x", "4.9e-324", "0x1p3", "0xp", "0x", "infin", "1e10", "1e-400", NULL };
static const char *text_fmts[] = {
    "%s", "%c", "%3c", "%5s", "%[abc]", "%[^,]", "%[]a]", "%[a-c]", "%[c-a]", "%[-a]", "%[a-]",
    "%2[0-9]", "%ls", "%lc", "%S", "%C", "%*s", "%s%s", "%c%c", "%2lc", "%[^]]", "%1s", NULL,
};
static const char *text_ins[] = {
    "hello world", "  hi", "a,b", "]ab", "abc-", "-", "", "x", "\t\n x", "caf\xc3\xa9 ok", "c-a", "12345", "a]b", NULL,
};
static const char *combo_fmts[] = {
    "%d,%d", "%d %% %d", "%d%%%d", "a%db", "%d %n%d", "%2$d %1$d", "%d%s", " %c", "%c", "x%n",
    "%n", "%*d %d", "%d%*[ ]%d", "%hhn %d", "%d%n", " %d %d ", "%d\n%d", "%5n", "%%", " %%%d",
    "%d %d %d %d", "%1$d %1$d", "%3$s %1$d", "%lln%d", NULL,
};
static const char *combo_ins[] = {
    "1,2", "1 % 2", "1%2", "a5b", "a5c", "1 2", "5 6 7", "", " ", "x", "%5", " % 5", "12ab", "1\n2", NULL,
};

static void allocations(void) {
    str("== allocations\n");
    const char *ins[] = { "hello world", "abc", "", "  x", NULL };
    for (int set = 0; set < 3; set++)
        for (const char **ip = ins; *ip; ip++) {
            one(set, *ip, "%ms", 1, 0);
            one(set, *ip, "%m[a-z]", 1, 0);
            mlen = 3;
            one(set, *ip, "%3mc", 1, 0);
            mlen = 0;
            one(set, *ip, "%ms %ms", 3, 0);
            one(set, *ip, "%*ms", 0, 0);
        }
    /* GNU: %as allocates; C99 and C23: %a is a floating conversion. */
    for (const char **ip = ins; *ip; ip++) one(0, *ip, "%as", 1, 0);
    for (const char **ip = ins; *ip; ip++) one(0, *ip, "%a[a-z]", 1, 0);
    for (int set = 1; set < 3; set++) {
        one(set, "1.5s", "%as", 0, 0);
        one(set, "hello", "%as", 0, 0);
    }
    one(0, "1.5", "%a", 0, 0);
    one(0, "1.5", "%af", 0, 0);
}

static int vcall(const char *in, const char *fmt, ...) {
    va_list ap; va_start(ap, fmt);
    int r = c99_vsscanf(in, fmt, ap);
    va_end(ap);
    return r;
}

static void streams(void) {
    str("== streams\n");
    /* Successive scans on one stream: the unread character stays for the next. */
    FILE *f = file_with("12 abc 3.5x 7");
    int i = 0; double x = 0; char s[16] = { 0 };
    num(c99_fscanf(f, "%d", &i)); ch(' '); num(i); ch(' ');
    num(c99_fscanf(f, "%s", s)); ch(' '); str(s); ch(' ');
    num(c99_fscanf(f, "%lf", &x)); ch(' '); num((long)(x * 10)); ch(' ');
    num(c99_fscanf(f, "%d", &i)); ch(' ');
    num(fgetc(f)); ch(' ');
    num(c99_fscanf(f, "x%d", &i)); ch(' '); num(i); ch(' ');
    num(c99_fscanf(f, "%d", &i)); ch(' '); num(feof(f)); ch('\n');
    fclose(f);
    /* The v-form, and arguments read by position. */
    int p = 0, q = 0;
    num(vcall("4 9", "%2$d %1$d", &p, &q)); ch(' '); num(p); ch(' '); num(q); ch('\n');
}

int main(void) {
    linebuf = getenv("SCANF_DIFF_LINEBUF") != NULL;
    const char *locs[] = { "C", "en_US.UTF-8", "de_DE.UTF-8", NULL };
    for (const char **l = locs; *l; l++) {
        if (!setlocale(LC_ALL, *l)) { str("== no locale "); str(*l); ch('\n'); continue; }
        str("== "); str(*l); ch('\n');
        if (strcmp(*l, "C") == 0) {
            grid(int_fmts, int_ins, 1);
            grid(combo_fmts, combo_ins, 1);
            allocations();
            streams();
        }
        grid(float_fmts, float_ins, 1);
        grid(fwidth_fmts, fwidth_ins, 1);
        if (strcmp(*l, "C") == 0) { str("== partial\n"); grid(partial_fmts, partial_ins, 1); }
        grid(text_fmts, text_ins, 1);
    }
    flush_out();
    return 0;
}
