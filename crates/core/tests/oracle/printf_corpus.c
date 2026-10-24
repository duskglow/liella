/* printf corpus replayer (validation item C; printf_corpus_products.rs). Reads a corpus written
 * by validation/harvest/harvest_reduce.py (printf-family calls the validation packages made
 * against glibc: format, typed arguments, locale, errno) from argv[1] and formats each call
 * again: built against the host C library and against Liella's products, the outputs must
 * agree. Output: "<n>\t<total length>[ n=<%n value>]\t<escaped output>" or "<n>\tskip\t<why>".
 *
 * Each conversion is formatted on its own: the format is split into literal text and single
 * conversions, a positional conversion ("%2$s", "%*1$d") is rewritten without positions, and
 * each piece goes through one snprintf call with typed arguments; the pieces are concatenated.
 * This needs no knowledge of the variadic calling convention, and both libraries replay the
 * same pieces. It does not reproduce whole-call failures (a total length beyond INT_MAX). */
#define _GNU_SOURCE
#include <errno.h>
#include <locale.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <wchar.h>

#pragma GCC diagnostic ignored "-Wformat-nonliteral"
#pragma GCC diagnostic ignored "-Wformat-security"

static char line[1 << 20];

static size_t unesc(char *s)
{
    if (strcmp(s, "\\e") == 0) {
        s[0] = 0;
        return 0;
    }
    char *w = s;
    for (char *r = s; *r;) {
        if (r[0] == '\\' && r[1] == 'x' && r[2] && r[3]) {
            char h[3] = {r[2], r[3], 0};
            *w++ = (char)strtol(h, NULL, 16);
            r += 4;
        } else {
            *w++ = *r++;
        }
    }
    *w = 0;
    return (size_t)(w - s);
}

static uint64_t hex(const char *s, int digits)
{
    uint64_t v = 0;
    for (int i = 0; i < digits && s[i]; i++) {
        int c = s[i];
        v = v * 16 + (uint64_t)(c <= '9' ? c - '0' : (c | 0x20) - 'a' + 10);
    }
    return v;
}

enum kind { K_INT, K_LLONG, K_DBL, K_LDBL, K_PTR, K_NONE };

struct val {
    char tag; /* i d L s S N n p */
    long long i;
    double d;
    long double ld;
    const void *p;
};

static wchar_t wide[64][1024];
static struct val args[128];
static int nargs;

static char out[1 << 17];
static size_t used;

static void emit(const char *s, size_t n)
{
    if (used + n >= sizeof out)
        n = sizeof out - 1 - used;
    memcpy(out + used, s, n);
    used += n;
}

/* Format one conversion (a mini format without positions) with up to two int stars. */
static int one(const char *mf, int nstar, const int *star, enum kind k, const struct val *v)
{
    static char piece[1 << 16];
    int r = -1;
#define CALL(X)                                                                      \
    do {                                                                             \
        if (nstar == 0)                                                              \
            r = snprintf(piece, sizeof piece, mf, X);                                \
        else if (nstar == 1)                                                         \
            r = snprintf(piece, sizeof piece, mf, star[0], X);                       \
        else                                                                         \
            r = snprintf(piece, sizeof piece, mf, star[0], star[1], X);              \
    } while (0)
    switch (k) {
    case K_INT: CALL((int)v->i); break;
    case K_LLONG: CALL(v->i); break;
    case K_DBL: CALL(v->d); break;
    case K_LDBL: CALL(v->ld); break;
    case K_PTR: CALL(v->p); break;
    case K_NONE: CALL(0); break;
    }
#undef CALL
    if (r > 0)
        emit(piece, (size_t)r < sizeof piece ? (size_t)r : sizeof piece - 1);
    return r;
}

/* Replay format f with args[]: the total length, -1 (errno kept) or -2 (not replayable). */
static long replay(const char *f, int *nval)
{
    int seq = 0;
    long total = 0;
    *nval = -1;
    for (const char *p = f; *p;) {
        if (*p != '%') {
            const char *q = strchr(p, '%');
            size_t n = q ? (size_t)(q - p) : strlen(p);
            emit(p, n);
            total += (long)n;
            p += n;
            continue;
        }
        if (p[1] == '%') {
            emit("%", 1);
            total++;
            p += 2;
            continue;
        }
        /* %[pos$][flags][width|*[pos$]][.prec|.*[pos$]][len]conv */
        const char *s = p + 1;
        char mf[64];
        size_t m = 0;
        mf[m++] = '%';
        int pos = 0, n = 0;
        const char *q = s;
        while (*q >= '0' && *q <= '9')
            n = n * 10 + (*q++ - '0');
        if (*q == '$' && q > s) {
            pos = n;
            s = q + 1;
        }
        while (*s && strchr("-+ #0'I", *s) && m < 40)
            mf[m++] = *s++;
        int nstar = 0, star[2] = {0, 0};
        for (int part = 0; part < 2; part++) {
            if (part == 1) {
                if (*s != '.')
                    break;
                mf[m++] = *s++;
            }
            if (*s == '*') {
                s++;
                q = s;
                n = 0;
                while (*q >= '0' && *q <= '9')
                    n = n * 10 + (*q++ - '0');
                int at;
                if (*q == '$' && q > s) {
                    at = n - 1;
                    s = q + 1;
                } else {
                    at = seq++;
                }
                if (at < 0 || at >= nargs || args[at].tag != 'i')
                    return -2;
                star[nstar++] = (int)args[at].i;
                mf[m++] = '*';
            } else {
                while (*s >= '0' && *s <= '9' && m < 50)
                    mf[m++] = *s++;
            }
        }
        int ll = 0;
        while (*s && strchr("hlLqjzZt", *s) && m < 56) {
            if (strchr("lqjzZt", *s))
                ll = 1;
            mf[m++] = *s++;
        }
        char conv = *s++;
        if (!conv)
            return -2;
        mf[m++] = conv;
        mf[m] = 0;
        p = s;
        if (conv == 'm') {
            int r = one(mf, nstar, star, K_NONE, NULL);
            if (r < 0)
                return -1;
            total += r;
            continue;
        }
        int at = pos ? pos - 1 : seq++;
        if (at < 0 || at >= nargs)
            return -2;
        const struct val *v = &args[at];
        if (conv == 'n') {
            *nval = (int)total;
            continue;
        }
        enum kind k;
        switch (v->tag) {
        case 'i': k = (ll && conv != 'c' && conv != 'C') ? K_LLONG : K_INT; break;
        case 'd': k = K_DBL; break;
        case 'L': k = K_LDBL; break;
        case 's': case 'S': case 'N': case 'p': k = K_PTR; break;
        default: return -2;
        }
        int r = one(mf, nstar, star, k, v);
        if (r < 0)
            return -1;
        total += r;
    }
    return total;
}

int main(int argc, char **argv)
{
    setvbuf(stdout, NULL, _IOLBF, 0); /* a crash shows its corpus line */
    FILE *in = fopen(argc > 1 ? argv[1] : "", "r");
    if (!in) {
        perror("corpus");
        return 2;
    }
    long ln = 0;
    while (fgets(line, sizeof line, in)) {
        ln++;
        line[strcspn(line, "\n")] = 0;
        char *f[132];
        int nf = 0;
        f[nf++] = line;
        for (char *p = line; *p && nf < 132; p++)
            if (*p == '\t') {
                *p = 0;
                f[nf++] = p + 1;
            }
        if (nf > 0 && f[nf - 1][0] == '@')
            nf--;
        if (nf < 4 || f[0][0] != 'F') {
            printf("%ld\tskip\tbad line\n", ln);
            continue;
        }
        unesc(f[1]);
        if (!setlocale(LC_ALL, f[1])) {
            printf("%ld\tskip\tlocale %s\n", ln, f[1]);
            setlocale(LC_ALL, "C");
            continue;
        }
        unesc(f[3]);
        nargs = 0;
        int nw = 0, bad = 0;
        for (int i = 4; i < nf && nargs < 128; i++) {
            char *a = f[i];
            struct val *v = &args[nargs++];
            memset(v, 0, sizeof *v);
            v->tag = a[0];
            if (a[0] == 'i' && a[1] == ':') {
                v->i = strtoll(a + 2, NULL, 10);
            } else if (a[0] == 'p' && a[1] == ':') {
                v->p = (const void *)(uintptr_t)hex(a + 2, 16);
            } else if (a[0] == 's' && a[1] == ':') {
                unesc(a + 2);
                v->p = a + 2;
            } else if ((a[0] == 'N' || a[0] == 'n') && a[1] == 0) {
                v->p = NULL;
            } else if (a[0] == 'S' && a[1] == ':') {
                if (nw == 64) {
                    bad = 1;
                    break;
                }
                int k = 0;
                for (char *p = a + 2; *p && *p != '-' && k < 1023;) {
                    wide[nw][k++] = (wchar_t)hex(p, 16);
                    p += 16;
                    if (*p == ',')
                        p++;
                }
                wide[nw][k] = 0;
                v->p = wide[nw++];
            } else if (a[0] == 'd' && a[1] == ':') {
                uint64_t bits = hex(a + 2, 16);
                memcpy(&v->d, &bits, 8);
            } else if (a[0] == 'L' && a[1] == ':') {
                unsigned char x[16] = {0};
                for (int k = 0; k < 10; k++)
                    x[k] = (unsigned char)hex(a + 2 + 2 * k, 2);
                memcpy(&v->ld, x, sizeof v->ld < 16 ? sizeof v->ld : 16);
            } else {
                bad = 1;
                break;
            }
        }
        if (bad) {
            printf("%ld\tskip\targuments\n", ln);
            continue;
        }
        used = 0;
        errno = (int)strtol(f[2], NULL, 10);
        int nval = -1;
        long r = replay(f[3], &nval);
        int e = errno;
        if (r == -2) {
            printf("%ld\tskip\tformat\n", ln);
            setlocale(LC_ALL, "C");
            continue;
        }
        printf("%ld\t%ld", ln, r);
        if (r < 0)
            printf(" errno=%d", e);
        if (nval >= 0)
            printf(" n=%d", nval);
        putchar('\t');
        for (size_t i = 0; i < used; i++) {
            unsigned char c = (unsigned char)out[i];
            if (c > 0x20 && c < 0x7f && c != '\\')
                putchar(c);
            else
                printf("\\x%02x", c);
        }
        putchar('\n');
        setlocale(LC_ALL, "C");
    }
    return 0;
}
