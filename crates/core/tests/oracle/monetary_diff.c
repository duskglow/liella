/* strfmon end to end (spec §7.5; D-7.11): built against the host C library and against
 * Liella's products, run with the same LOCPATH; the outputs must match line for line
 * (monetary_products.rs). Each line is one formatting: the format, the amount, the result,
 * errno on failure, and the text. */
#define _GNU_SOURCE
#include <errno.h>
#include <locale.h>
#include <math.h>
#include <monetary.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

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

static const char *fmts[] = {
    "%n", "%i", "%=*#10n", "%^n", "%+n", "%(n", "%!n", "%-14n", "%14n", "%#5n", "%#5.0n", "%.3i",
    "%=0#8n", "%(#5i", "%^#9.1n", "%!(#7n", "%+#6i", "%-#6.2n|", "a %n b %% c", "%=x-16.1i|",
    "%q", "%#n", "%.n", "%=", "%+(n", "%(+n", "%L", NULL,
};
static const double vals[] = { 0, 1234.567, -1234.567, 1234567.891, -0.0, 0.005, 1e20, -5, 999.995, 12 };

static void one(const char *fmt, double v, size_t max) {
    char buf[256];
    memset(buf, 'Z', sizeof buf);
    errno = 0;
    ssize_t r = strfmon(buf, max, fmt, v, v);
    text(fmt, strlen(fmt)); ch(' '); num((long)(v * 1000)); str(" max="); num((long)max); str(" -> "); num((long)r);
    if (r < 0) { str(" errno="); num(errno); }
    else { ch(' '); text(buf, (size_t)r + 1); }
    ch('\n');
}

int main(void) {
    const char *locs[] = { "C", "en_US.UTF-8", "de_DE.UTF-8", "fr_FR.UTF-8", "ja_JP.UTF-8", "it_IT.UTF-8", "de_CH.UTF-8", "nl_NL.UTF-8", "en_IN.UTF-8", "xx_NOSEP.UTF-8", NULL };
    for (const char **l = locs; *l; l++) {
        if (!setlocale(LC_ALL, *l)) { str("== no locale "); str(*l); ch('\n'); continue; }
        str("== "); str(*l); ch('\n');
        for (const char **f = fmts; *f; f++)
            for (size_t k = 0; k < sizeof vals / sizeof vals[0]; k++) one(*f, vals[k], 200);
        for (size_t max = 0; max < 14; max++) one("%n", -1234.567, max);
        char buf[200];
        long double ld = 1234.5L;
        ssize_t r = strfmon(buf, sizeof buf, "%Ln|%Li", ld, -ld);
        str("long double -> "); num((long)r); ch(' '); text(buf, r >= 0 ? (size_t)r : 0); ch('\n');
        r = strfmon(buf, sizeof buf, "%n %n %i", 1.0, -2.0, 3.5);
        str("several -> "); num((long)r); ch(' '); text(buf, r >= 0 ? (size_t)r : 0); ch('\n');
        locale_t c = newlocale(LC_ALL_MASK, "de_DE.UTF-8", 0);
        if (c) {
            r = strfmon_l(buf, sizeof buf, c, "%n|%i", 1234.5, -1234.5);
            str("strfmon_l -> "); num((long)r); ch(' '); text(buf, r >= 0 ? (size_t)r : 0); ch('\n');
            freelocale(c);
        }
    }
    flush_out();
    return 0;
}
