/* Wide-character streams end to end (spec §12.10; D-12.7, D-12.14): built against the host C
 * library and against Liella's products, the outputs must match line for line
 * (wstream_products.rs). Each line is one operation: what it returned, errno where the contract
 * sets it, the indicators and the position. */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <locale.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <wchar.h>

static char obuf[1 << 16];
static size_t used;
static void flush_out(void) { if (used) { (void)!write(1, obuf, used); used = 0; } }
static int linebuf; /* WSTREAM_DIFF_LINEBUF set: flush each line (to find a crashing case) */
static void ch(char c) { if (used == sizeof obuf) flush_out(); obuf[used++] = c; if (c == 10 && linebuf) flush_out(); }
static void str(const char *s) { while (*s) ch(*s++); }
static void num(long v) {
    char b[24]; int n = 0; unsigned long u = v < 0 ? -(unsigned long)v : (unsigned long)v;
    if (v < 0) ch('-');
    do { b[n++] = (char)('0' + u % 10); u /= 10; } while (u);
    while (n) ch(b[--n]);
}
static void hex(const unsigned char *p, size_t n) {
    const char *d = "0123456789abcdef";
    ch('<');
    for (size_t i = 0; i < n; i++) { ch(d[p[i] >> 4]); ch(d[p[i] & 15]); }
    ch('>');
}
/* The indicators only: the reference library cannot always tell the position while
 * characters given back are pending. */
static void flags(FILE *f) {
    str(" err="); num(ferror(f)); str(" eof="); num(feof(f)); ch('\n');
}
static void state(FILE *f) {
    str(" err="); num(ferror(f)); str(" eof="); num(feof(f)); str(" pos="); num(ftell(f)); ch('\n');
}

/* A stream reading `in` (n bytes), with no orientation yet. */
static FILE *with(const char *in, size_t n) {
    FILE *f = tmpfile();
    (void)!write(fileno(f), in, n);
    rewind(f);
    return f;
}
/* The bytes a stream wrote. */
static void written(FILE *f) {
    fflush(f);
    unsigned char b[256];
    ssize_t n = pread(fileno(f), b, sizeof b, 0);
    str(" bytes="); hex(b, n > 0 ? (size_t)n : 0);
}

static void reads(const char *label, const char *in, size_t n) {
    FILE *f = with(in, n);
    for (int i = 0; i < 6; i++) {
        errno = 0;
        wint_t w = fgetwc(f);
        str(label); ch(' '); num(i); str(": "); num((long)(int)w);
        if (w == WEOF) { str(" errno="); num(errno == EILSEQ ? EILSEQ : 0); }
        state(f);
    }
    fclose(f);
}

static void inputs(void) {
    str("== reads\n");
    reads("ascii", "abc", 3);
    reads("utf8", "h\xc3\xa9\xe2\x82\xac!", 7);
    reads("invalid", "h\xc3\xa9\xff!", 5);
    reads("cut", "h\xc3", 2);
    reads("lone-cont", "\x80x", 2);
    reads("empty", "", 0);
    reads("nul", "a\0b", 3);

    str("== ungetwc\n");
    FILE *f = with("ab\xc3\xa9z", 5);
    num((long)(int)fgetwc(f)); ch(' ');
    num((long)(int)ungetwc(L'Q', f)); ch(' ');
    num((long)(int)fgetwc(f)); ch(' '); num((long)(int)fgetwc(f)); ch(' ');
    num((long)(int)fgetwc(f)); flags(f);
    num((long)(int)ungetwc(L'Q', f)); ch(' '); num((long)(int)ungetwc(L'R', f)); ch(' ');
    num((long)(int)ungetwc(L'S', f)); ch(' ');
    num((long)(int)fgetwc(f)); ch(' '); num((long)(int)fgetwc(f)); ch(' '); num((long)(int)fgetwc(f)); ch(' ');
    num((long)(int)fgetwc(f)); ch(' '); num((long)(int)fgetwc(f)); state(f);
    num((long)(int)ungetwc(WEOF, f)); state(f);
    rewind(f);
    num((long)(int)fgetwc(f)); ch(' '); num((long)(int)fgetwc(f)); ch(' '); num((long)(int)fgetwc(f));
    str(" at="); num(ftell(f)); ch(' ');
    num((long)(int)ungetwc(0xe9, f)); flags(f);
    num((long)(int)fgetwc(f)); ch(' '); num((long)(int)fgetwc(f)); state(f);
    num((long)(int)fgetwc(f)); state(f);
    num((long)(int)ungetwc(L'y', f)); flags(f);
    num((long)(int)fgetwc(f)); state(f);
    fseek(f, 1, SEEK_SET); num((long)(int)fgetwc(f)); state(f);
    fclose(f);

    str("== fgetws\n");
    int sizes[] = { 1, 2, 3, 8, 64 };
    for (int k = 0; k < 5; k++) {
        f = with("line one\nline \xc3\xa9two\nend", 23);
        wchar_t buf[64];
        str("n="); num(sizes[k]); ch(':');
        for (int i = 0; i < 12; i++) {
            wchar_t *r = fgetws(buf, sizes[k], f);
            if (!r) { str(" null"); break; }
            ch(' '); ch('[');
            for (wchar_t *p = buf; *p; p++) { if (*p == L'\n') str("\\n"); else if (*p < 128) ch((char)*p); else { ch('#'); num(*p); } }
            ch(']');
        }
        state(f);
        fclose(f);
    }
}

static void outputs(void) {
    str("== writes\n");
    FILE *f = tmpfile();
    num((long)(int)fputwc(L'a', f)); ch(' ');
    num((long)(int)fputwc(0xe9, f)); ch(' ');
    num((long)(int)fputwc(0x20ac, f)); ch(' ');
    num(fputws(L"x\x00e9y\n", f) >= 0); ch(' ');
    num((long)(int)putwc(L'z', f)); ch(' ');
    num(fwprintf(f, L"%d %ls %s|", 42, L"w\x00e9", "b\xc3\xa9"));
    written(f); state(f);
    fclose(f);

    str("== wmemstream\n");
    wchar_t *wb; size_t sz;
    FILE *m = open_wmemstream(&wb, &sz);
    num(fwide(m, 0)); ch(' ');
    fputwc(L'A', m); fputws(L"\x00e9z", m); fflush(m);
    num((long)sz); ch(' '); for (size_t i = 0; i <= sz; i++) { num(wb[i]); ch(','); }
    num(fwprintf(m, L"%d|%ls", 42, L"\x20ac")); ch(' ');
    fseek(m, 1, SEEK_SET); fputwc(L'B', m); fflush(m);
    num((long)sz); ch(' ');
    fseek(m, 0, SEEK_END);
    fclose(m);
    num((long)sz); ch(' '); for (size_t i = 0; i <= sz; i++) { num(wb[i]); ch(','); }
    ch('\n');
    free(wb);
}

static void orientation(void) {
    str("== orientation\n");
    FILE *f = tmpfile();
    num(fwide(f, 0)); ch(' '); num(fwide(f, 5)); ch(' '); num(fwide(f, -3)); ch(' '); num(fwide(f, 0)); ch('\n');
    str("fgetc on wide: "); num(fgetc(f)); state(f);
    str("fputs on wide: "); num(fputs("bc", f)); state(f);
    str("fwrite on wide: "); num((long)fwrite("de", 1, 2, f)); state(f);
    str("fprintf on wide: "); num(fprintf(f, "x")); state(f);
    str("fputc on wide: "); num(fputc('y', f)); state(f);
    fclose(f);
    f = tmpfile();
    fputs("hi", f);
    num(fwide(f, 0)); ch(' '); num(fwide(f, 1)); ch('\n');
    str("fgetwc on byte: "); num((long)(int)fgetwc(f)); state(f);
    str("fputwc on byte: "); num((long)(int)fputwc(L'x', f)); state(f);
    str("fwprintf on byte: "); num(fwprintf(f, L"x")); state(f);
    fclose(f);
    f = with("ab", 2);
    num((long)(int)fgetwc(f)); ch(' '); num(fwide(f, 0)); ch(' '); num(fwide(f, -1)); ch('\n');
    fclose(f);
    f = with("ab", 2);
    num(fgetc(f)); ch(' '); num(fwide(f, 0)); ch('\n');
    fclose(f);
}

/* fopen's ",ccs=": the stream is wide-oriented on that charset from the start. */
static void charsets(void) {
    str("== ccs\n");
    char path[64];
    snprintf(path, sizeof path, "/tmp/wstream_diff_%d", (int)getpid());
    const char *sets[] = { "UTF-8", "UTF-16LE", "UTF-16BE", "UTF-16", "ISO-8859-1", "UCS-4LE", "UTF-32", NULL };
    for (const char **c = sets; *c; c++) {
        char mode[64];
        snprintf(mode, sizeof mode, "w,ccs=%s", *c);
        FILE *f = fopen(path, mode);
        str(*c); ch(':');
        if (!f) { str(" open failed\n"); continue; }
        ch(' '); num(fwide(f, 0)); ch(' ');
        num((long)(int)fputwc(L'h', f)); ch(' '); num((long)(int)fputwc(0xe9, f)); ch(' ');
        num(fputws(L"\x20ac!", f) >= 0);
        fclose(f);
        unsigned char b[64];
        int fd = open(path, O_RDONLY);
        ssize_t n = read(fd, b, sizeof b);
        close(fd);
        str(" bytes="); hex(b, n > 0 ? (size_t)n : 0); ch('\n');
        snprintf(mode, sizeof mode, "r,ccs=%s", *c);
        f = fopen(path, mode);
        if (!f) { str(" reopen failed\n"); continue; }
        str("  read:");
        for (int i = 0; i < 6; i++) { ch(' '); num((long)(int)fgetwc(f)); }
        state(f);
        fclose(f);
    }
    errno = 0;
    FILE *f = fopen(path, "w,ccs=NO-SUCH-CHARSET");
    str("unknown: "); num(f != NULL); ch(' '); num(errno); ch('\n');
    if (f) fclose(f);
    unlink(path);
}

int main(void) {
    linebuf = getenv("WSTREAM_DIFF_LINEBUF") != NULL;
    const char *locs[] = { "C.UTF-8", "en_US.UTF-8", "C", NULL };
    for (const char **l = locs; *l; l++) {
        if (!setlocale(LC_ALL, *l)) { str("== no locale "); str(*l); ch('\n'); continue; }
        str("== "); str(*l); ch('\n');
        inputs();
        if (strcmp(*l, "C") != 0) outputs();
        orientation();
    }
    setlocale(LC_ALL, "C.UTF-8");
    charsets();
    /* The codec is fixed when the stream becomes wide-oriented. */
    str("== fixed codec\n");
    setlocale(LC_ALL, "C.UTF-8");
    FILE *f = with("\xc3\xa9\xc3\xa9", 4);
    num((long)(int)fgetwc(f)); ch(' ');
    setlocale(LC_ALL, "C");
    num((long)(int)fgetwc(f)); state(f);
    fclose(f);
    flush_out();
    return 0;
}
