/* Regex corpus replayer (validation item C; regex_corpus_products.rs). Reads a corpus written by
 * validation/harvest/harvest_reduce.py from the file named by argv[1] (calls glibc made in the
 * validation packages' test suites, with glibc's recorded results) and makes each call again:
 * built against the host C library and against Liella's products, the two outputs must agree.
 * Output: one line per corpus line, "<n>\t<kind>\t<result>\t<recorded result>", where a result
 * has the corpus's own form (P: ret; E: ret regs; G: message; S/M: ret regs). */
#define _GNU_SOURCE
#include <regex.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static char line[1 << 20];

/* Undo the corpus escaping in place; returns the length (the text may hold NUL bytes). */
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

static int split(char *s, char **f, int max)
{
    int n = 0;
    f[n++] = s;
    for (char *p = s; *p && n < max; p++) {
        if (*p == '\t') {
            *p = 0;
            f[n++] = p + 1;
        }
    }
    return n;
}

static char out[1 << 16];
static size_t used;

static void add(const char *fmt, long a, long b)
{
    used += (size_t)snprintf(out + used, sizeof out - used, fmt, a, b);
    if (used >= sizeof out)
        used = sizeof out - 1;
}

/* The bytes of a pattern buffer's translate table, from its hex form. */
static unsigned char *table(const char *hex)
{
    if (strcmp(hex, "-") == 0)
        return NULL;
    static unsigned char t[256];
    for (int i = 0; i < 256 && hex[2 * i] && hex[2 * i + 1]; i++) {
        char h[3] = {hex[2 * i], hex[2 * i + 1], 0};
        t[i] = (unsigned char)strtol(h, NULL, 16);
    }
    return t;
}

static void gnu(char **f, int search)
{
    /* f: kind syntax translate pattern not_bol not_eol nl subject start [range] regs? ret regs */
    re_syntax_options = strtoul(f[1], NULL, 10);
    struct re_pattern_buffer b;
    memset(&b, 0, sizeof b);
    static char fastmap[256];
    b.fastmap = fastmap;
    b.translate = table(f[2]);
    size_t plen = unesc(f[3]);
    const char *msg = re_compile_pattern(f[3], plen, &b);
    if (msg) {
        add("compile error", 0, 0);
        return;
    }
    b.not_bol = atoi(f[4]) != 0;
    b.not_eol = atoi(f[5]) != 0;
    b.newline_anchor = atoi(f[6]) != 0;
    size_t slen = unesc(f[7]);
    int start = atoi(f[8]);
    int at = search ? 10 : 9;
    int want_regs = atoi(f[at]);
    struct re_registers regs;
    memset(&regs, 0, sizeof regs);
    b.regs_allocated = REGS_UNALLOCATED;
    regoff_t r = search ? re_search(&b, f[7], (regoff_t)slen, start, atoi(f[9]), want_regs ? &regs : NULL)
                        : re_match(&b, f[7], (regoff_t)slen, start, want_regs ? &regs : NULL);
    add("%ld\t", (long)r, 0);
    if (!want_regs || r < 0) {
        add("-", 0, 0);
    } else {
        size_t n = b.re_nsub + 1 < regs.num_regs ? b.re_nsub + 1 : regs.num_regs;
        for (size_t i = 0; i < n && i < 32; i++)
            add(i ? ";%ld,%ld" : "%ld,%ld", (long)regs.start[i], (long)regs.end[i]);
    }
    b.translate = NULL;
    b.fastmap = NULL;
    regfree(&b);
}

int main(int argc, char **argv)
{
    FILE *in = fopen(argc > 1 ? argv[1] : "", "r");
    if (!in) {
        perror("corpus");
        return 2;
    }
    long n = 0;
    setvbuf(stdout, NULL, _IOLBF, 0); /* a crash shows its corpus line */
    while (fgets(line, sizeof line, in)) {
        n++;
        line[strcspn(line, "\n")] = 0;
        char *f[32];
        int nf = split(line, f, 32);
        used = 0;
        out[0] = 0;
        const char *rec = "";
        char recbuf[1 << 12];
        if (f[0][0] == 'P' && nf >= 4) {
            regex_t re;
            unesc(f[2]);
            int r = regcomp(&re, f[2], atoi(f[1]));
            add("%ld", r, 0);
            if (r == 0)
                regfree(&re);
            rec = f[3];
        } else if (f[0][0] == 'E' && nf >= 10) {
            regex_t re;
            int cflags = atoi(f[1]);
            unesc(f[2]);
            int r = regcomp(&re, f[2], cflags);
            if (r != 0) {
                add("compile %ld", r, 0);
            } else {
                int eflags = atoi(f[3]);
                size_t nmatch = strtoul(f[4], NULL, 10);
                size_t slen = unesc(f[5]);
                regmatch_t m[64];
                size_t k = nmatch > 64 ? 64 : nmatch;
                for (size_t i = 0; i < k; i++)
                    m[i].rm_so = m[i].rm_eo = -7;
                if (eflags & REG_STARTEND) {
                    long so = 0, eo = (long)slen;
                    sscanf(f[6], "%ld,%ld", &so, &eo);
                    m[0].rm_so = so;
                    m[0].rm_eo = eo;
                    if (k == 0)
                        k = 1;
                }
                int x = regexec(&re, f[5], k, m, eflags);
                add("%ld\t", x, 0);
                size_t show = (x == 0 && !(cflags & REG_NOSUB)) ? (nmatch > 32 ? 32 : nmatch) : 0;
                if (show == 0)
                    add("-", 0, 0);
                for (size_t i = 0; i < show; i++)
                    add(i ? ";%ld,%ld" : "%ld,%ld", (long)m[i].rm_so, (long)m[i].rm_eo);
                regfree(&re);
            }
            snprintf(recbuf, sizeof recbuf, "%s\t%s", f[7], f[8]);
            rec = recbuf;
        } else if (f[0][0] == 'G' && nf >= 5) {
            re_syntax_options = strtoul(f[1], NULL, 10);
            struct re_pattern_buffer b;
            memset(&b, 0, sizeof b);
            b.translate = table(f[2]);
            size_t plen = unesc(f[3]);
            const char *msg = re_compile_pattern(f[3], plen, &b);
            used += (size_t)snprintf(out, sizeof out, "%s", msg ? msg : "-");
            b.translate = NULL;
            regfree(&b);
            unesc(f[4]);
            rec = f[4];
        } else if ((f[0][0] == 'S' && nf >= 14) || (f[0][0] == 'M' && nf >= 13)) {
            int search = f[0][0] == 'S';
            char saved[1 << 12];
            int at = search ? 11 : 10;
            snprintf(saved, sizeof saved, "%s\t%s", f[at], f[at + 1]);
            gnu(f, search);
            rec = saved;
            printf("%ld\t%s\t%s\t%s\n", n, f[0], out, rec);
            continue;
        } else {
            add("unknown", 0, 0);
        }
        printf("%ld\t%s\t%s\t%s\n", n, f[0], out, rec);
    }
    return 0;
}
