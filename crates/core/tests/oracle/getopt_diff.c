/* getopt end to end (spec §1.6, §1.6a; D-1.6): built against the host C library and against
 * Liella's products, the outputs must match line for line (getopt_products.rs). Each case
 * parses a fresh copy of an argument vector to the end: every code with optarg, optind, optopt
 * and the long index, then the vector as permuted and the diagnostics written. */
#define _GNU_SOURCE
#include <fcntl.h>
#include <getopt.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

extern int __posix_getopt(int, char *const *, const char *);

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
static void ztext(const char *p) { if (p) text(p, strlen(p)); else str("NULL"); }

static int saved_err = -1, capture_fd = -1;
static void capture(void) {
    fflush(stderr);
    char path[64];
    snprintf(path, sizeof path, "/tmp/getopt_diff_%d", (int)getpid());
    capture_fd = open(path, O_RDWR | O_CREAT | O_TRUNC, 0600);
    unlink(path);
    saved_err = dup(2);
    dup2(capture_fd, 2);
}
static void captured(void) {
    fflush(stderr);
    dup2(saved_err, 2);
    close(saved_err);
    char b[2048];
    ssize_t n = pread(capture_fd, b, sizeof b, 0);
    close(capture_fd);
    str(" stderr="); text(b, n > 0 ? (size_t)n : 0);
}

static int flagvar;
static const struct option longs[] = {
    { "verbose", no_argument, NULL, 'v' },
    { "file", required_argument, NULL, 'f' },
    { "color", optional_argument, NULL, 'c' },
    { "colour", optional_argument, NULL, 'c' },
    { "flag", no_argument, &flagvar, 42 },
    { "fill", required_argument, NULL, 'F' },
    { "x", no_argument, NULL, 'X' },
    { NULL, 0, NULL, 0 },
};

enum mode { SHORT, LONG, LONG_ONLY, POSIX };
static const char *mode_name[] = { "short", "long", "long_only", "posix" };

/* Parse `vec` (NULL-terminated) with `spec` to the end. */
static void run(enum mode m, const char *spec, const char *const *vec, int opterr_value) {
    char *argv[32];
    char store[32][64];
    int argc = 0;
    for (; vec[argc]; argc++) { strcpy(store[argc], vec[argc]); argv[argc] = store[argc]; }
    argv[argc] = NULL;
    optind = 0;
    opterr = opterr_value;
    optopt = '?';
    flagvar = 0;
    str(mode_name[m]); ch(' '); ztext(spec); str(" opterr="); num(opterr_value); str(" argv=");
    for (int i = 0; i < argc; i++) { ztext(vec[i]); ch(' '); }
    ch('\n');
    capture();
    for (int step = 0; step < 40; step++) {
        int li = -7;
        int c;
        switch (m) {
        case SHORT: c = getopt(argc, argv, spec); break;
        case LONG: c = getopt_long(argc, argv, spec, longs, &li); break;
        case LONG_ONLY: c = getopt_long_only(argc, argv, spec, longs, &li); break;
        default: c = __posix_getopt(argc, argv, spec); break;
        }
        str("  "); num(c); str(" optind="); num(optind); str(" optopt="); num(optopt); str(" optarg="); ztext(optarg);
        str(" li="); num(li); str(" flag="); num(flagvar); ch('\n');
        if (c == -1) {
            /* Calls after the end read on from optind (POSIX describes getopt by optind alone): -1
               again where the options are exhausted, the next options after a `--`. */
            for (int again = 0; again < 2; again++) {
                c = m == SHORT ? getopt(argc, argv, spec) : m == LONG ? getopt_long(argc, argv, spec, longs, &li)
                  : m == LONG_ONLY ? getopt_long_only(argc, argv, spec, longs, &li) : __posix_getopt(argc, argv, spec);
                str("  again "); num(c); str(" optind="); num(optind); ch('\n');
            }
            /* optind = 1 (not 0) at an element boundary: the next call reads element 1 again. Only
               when the last call ended (a call inside a cluster keeps glibc's cursor: D-1.6). */
            if (c != -1) break;
            optind = 1;
            c = m == SHORT ? getopt(argc, argv, spec) : m == LONG ? getopt_long(argc, argv, spec, longs, &li)
              : m == LONG_ONLY ? getopt_long_only(argc, argv, spec, longs, &li) : __posix_getopt(argc, argv, spec);
            str("  optind=1: "); num(c); str(" optind="); num(optind); str(" optarg="); ztext(optarg); ch('\n');
            break;
        }
    }
    str("  final:");
    for (int i = 0; i < argc; i++) { ch(' '); ztext(argv[i]); }
    captured();
    ch('\n');
}

static const char *const vecs[][12] = {
    { "prog", NULL },
    { "prog", "-a", NULL },
    { "prog", "-ab", "-c", "carg", "op1", "-dxyz", "op2", NULL },
    { "prog", "op1", "-a", "op2", "-b", "--", "-c", "op3", NULL },
    { "prog", "-c", NULL },
    { "prog", "-q", "-a", NULL },
    { "prog", "-:", "-;", NULL },
    { "prog", "-d", "-dval", "-", "op", NULL },
    { "prog", "--verbose", "--file=f.txt", "--file", "g.txt", "x", NULL },
    { "prog", "--col", "--colo=red", "--color", "blue", NULL },
    { "prog", "--fi", "--fil=1", "--flag", "--fla", NULL },
    { "prog", "--verbose=no", "--file", NULL },
    { "prog", "--nope", "--", "--verbose", NULL },
    { "prog", "-verbose", "-file", "x", "-fill=3", "-x", "-ab", NULL },
    { "prog", "-W", "verbose", "-Wfile=w", "-W", NULL },
    { "prog", "a", "b", "-a", "c", "-b", "d", "-c", "e", NULL },
    /* Added for the getopt rewrite (COLD-REWRITE2-W-far-07): separate arguments behind operands,
       a missing argument with operands pending, and blocks whose sizes share a factor. */
    { "prog", "op", "-c", "x", "op2", "-ab", "op3", "-c", NULL },
    { "prog", "o1", "o2", "o3", "-ac", "v", "o4", "-d", "--verbose", "o5", NULL },
    { "prog", "o1", "o2", "-a", "o3", "o4", "-b", "o5", "o6", "-c", "o7", NULL },
    /* -W with an unknown, ambiguous, equivalent-prefix, separate-argument and argument-refusing name. */
    { "prog", "-W", "nope", "-Wfi", "x", "-W", "fil", "-Wverbose=1", "-Wcol", "-Wfile", "y", NULL },
    /* Empty long names, exact single-letter entries, arguments that look like options. */
    { "prog", "--=x", "--x", "-x=1", "--fla=2", "-fl", NULL },
    { "prog", "-c", "--", "-a", "--file", "--", "op", NULL },
    { "prog", "op1", "--", NULL },
    { "prog", "-", "-a", "-", "--fil", "-abd", "-ddd", "-dc", "-cd", NULL },
};
static const char *specs[] = { "abc:d::", ":abc:d::", "+abc:d::", "-abc:d::", "abc:d::W;", "ab:", NULL };

int main(void) {
    for (int m = SHORT; m <= POSIX; m++)
        for (int s = 0; specs[s]; s++)
            for (size_t v = 0; v < sizeof vecs / sizeof vecs[0]; v++) {
                run((enum mode)m, specs[s], vecs[v], 1);
                if (s == 0 && v % 4 == 2) run((enum mode)m, specs[s], vecs[v], 0);
            }
    /* POSIXLY_CORRECT stops at the first operand. */
    setenv("POSIXLY_CORRECT", "1", 1);
    run(SHORT, "abc:", vecs[3], 1);
    run(LONG, "abc:", vecs[15], 1);
    unsetenv("POSIXLY_CORRECT");

    str("== getsubopt\n");
    char *const tokens[] = { "ro", "rw", "size", NULL };
    const char *inputs[] = { "ro,size=10,bogus,rw", "", "size=", "=x,ro", "rw,", "unknown=1", NULL };
    for (int i = 0; inputs[i]; i++) {
        char buf[64];
        strcpy(buf, inputs[i]);
        char *p = buf, *val;
        ztext(inputs[i]); ch(':');
        for (int k = 0; k < 8 && *p; k++) {
            val = (char *)1;
            int r = getsubopt(&p, tokens, &val);
            ch(' '); num(r); ch('/'); if (val == (char *)1) str("unset"); else ztext(val); ch('@'); num((long)(p - buf));
        }
        ch(' '); text(buf, strlen(inputs[i]) + 1);
        ch('\n');
    }
    flush_out();
    return 0;
}
