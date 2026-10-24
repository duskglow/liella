/* Stdio end to end (spec §12; D-12.1, D-12.11): built against the host C library and against
 * Liella's products, the outputs must match line for line (stdio_products.rs). Output goes
 * through write(2) on a private descriptor, so the program's own stdout and stderr stay free
 * for the cases that test them. Argument: a scratch directory. */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdio_ext.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>
#include <wchar.h>

static int out_fd;
static char obuf[1 << 16];
static size_t used;

static void flush_out(void) { if (used) { (void)!write(out_fd, obuf, used); used = 0; } }
static void ch(char c) { if (used == sizeof obuf) flush_out(); obuf[used++] = c; }
static void str(const char *s) { while (*s) ch(*s++); }
static void num(long v) {
    char b[24]; int n = 0; unsigned long u = v < 0 ? -(unsigned long)v : (unsigned long)v;
    if (v < 0) ch('-');
    do { b[n++] = (char)('0' + u % 10); u /= 10; } while (u);
    while (n) ch(b[--n]);
}
static void bytes(const char *p, size_t n) {
    ch('[');
    for (size_t i = 0; i < n; i++) {
        unsigned char c = (unsigned char)p[i];
        if (c >= 32 && c < 127 && c != '\\') ch((char)c);
        else { const char *d = "0123456789abcdef"; ch('\\'); ch(d[c >> 4]); ch(d[c & 15]); }
    }
    ch(']');
}
static void kv(const char *k, long v) { ch(' '); str(k); ch('='); num(v); }
static void nl(void) { ch('\n'); }

static char dir[512];
static char *path(const char *name) {
    static char p[600];
    size_t n = strlen(dir);
    memcpy(p, dir, n);
    p[n] = '/';
    strcpy(p + n + 1, name);
    return p;
}
static long size_of(const char *p) { struct stat st; return stat(p, &st) == 0 ? (long)st.st_size : -1; }

/* ---------------------------------------------------------------- byte I/O on files */

static void files(void) {
    str("== files"); nl();
    const char *p = path("a.txt");
    FILE *f = fopen(p, "w+");
    for (int i = 0; i < 3; i++) putc_unlocked('a' + i, f);
    fputs("line one\nline two\n", f);
    fputc('x', f);
    str("tell"); kv("pos", ftell(f)); kv("on_disk", size_of(p)); kv("pending", (long)__fpending(f)); nl();
    rewind(f);
    str("getc"); kv("c", getc_unlocked(f)); nl();
    ungetc('Z', f);
    char buf[64];
    char *r = fgets(buf, sizeof buf, f);
    str("fgets"); kv("ok", r == buf); ch(' '); bytes(buf, strlen(buf)); nl();
    char *line = NULL; size_t cap = 0;
    for (int i = 0; i < 3; i++) {
        ssize_t n = getline(&line, &cap, f);
        str("getline"); kv("n", n); kv("eof", feof(f) != 0); if (n > 0) { ch(' '); bytes(line, (size_t)n); } nl();
    }
    free(line);
    fseek(f, 4, SEEK_SET);
    str("seek4"); kv("c", getc(f)); kv("tell", ftell(f)); nl();
    fseek(f, -2, SEEK_END);
    char two[3] = {0};
    size_t k = fread(two, 1, 2, f);
    str("fread_end"); kv("n", (long)k); ch(' '); bytes(two, k); kv("eof", feof(f) != 0); nl();
    clearerr(f);
    str("clearerr"); kv("eof", feof(f) != 0); kv("err", ferror(f) != 0); nl();
    /* Pushback of several bytes, and the position it gives. */
    fseek(f, 5, SEEK_SET);
    ungetc('1', f); ungetc('2', f); ungetc('3', f);
    str("ungetc3"); kv("tell", ftell(f)); kv("c1", getc(f)); kv("c2", getc(f)); kv("c3", getc(f)); kv("c4", getc(f)); nl();
    /* Reading then writing without a seek in between (Liella allows it; the reference allows
     * it for an update stream at EOF only): write at the end. */
    fseek(f, 0, SEEK_END);
    fputs("END", f);
    fflush(f);
    str("append_end"); kv("size", size_of(p)); nl();
    str("fclose"); kv("r", fclose(f)); nl();
    /* Elements, partial elements. */
    f = fopen(p, "r");
    struct { char c[7]; } rec[10];
    k = fread(rec, sizeof rec[0], 10, f);
    str("fread_elems"); kv("n", (long)k); kv("eof", feof(f) != 0); kv("tell", ftell(f)); nl();
    fclose(f);
    f = fopen(p, "a");
    k = fwrite("appended\n", 3, 3, f);
    str("fwrite_elems"); kv("n", (long)k); nl();
    fclose(f);
    str("final_size"); kv("size", size_of(p)); nl();
    /* getdelim with NULs and a custom delimiter. */
    f = fopen(p, "w+");
    fwrite("ab\0cd;ef\0;g", 1, 11, f);
    rewind(f);
    line = NULL; cap = 0;
    for (int i = 0; i < 4; i++) {
        ssize_t n = getdelim(&line, &cap, ';', f);
        str("getdelim"); kv("n", n); if (n > 0) { ch(' '); bytes(line, (size_t)n); } nl();
    }
    free(line);
    fclose(f);
    unlink(p);
}

/* ---------------------------------------------------------------- append modes */

/* A 6-byte file to append to. */
static const char *fresh(void) {
    const char *p = path("app.txt");
    int fd = open(p, O_WRONLY | O_CREAT | O_TRUNC, 0644);
    (void)!write(fd, "hello\n", 6);
    close(fd);
    return p;
}

/* The position after opening, after the first write (buffered, then written), and the size. */
static void after_open(const char *label, FILE *f, const char *p) {
    str(label); kv("ok", f != NULL);
    if (!f) { nl(); return; }
    kv("open", ftell(f));
    fputs("xy", f);
    kv("write", ftell(f));
    fflush(f);
    kv("flush", ftell(f)); kv("size", size_of(p));
    fputs("z", f);
    kv("write2", ftell(f));
    fclose(f);
    kv("final", size_of(p)); nl();
}

static void append(void) {
    str("== append"); nl();
    const char *modes[] = { "a", "ab", "ae", "a+", "a+b", "ab+", "a+e", "ax", NULL };
    for (int i = 0; modes[i]; i++) {
        const char *p = fresh();
        if (modes[i][1] == 'x') unlink(p);
        str("fopen "); str(modes[i]); after_open("", fopen(p, modes[i]), p);
    }
    /* a+: read first, then write. */
    const char *p = fresh();
    FILE *f = fopen(p, "a+");
    str("a+ read"); kv("c", getc(f)); kv("tell", ftell(f));
    fputs("xy", f); kv("write", ftell(f)); fflush(f); kv("flush", ftell(f)); kv("size", size_of(p)); nl();
    fclose(f);
    /* a: fixed at open, or the end at each ftell? The file grows behind the stream. */
    p = fresh();
    f = fopen(p, "a");
    int other = open(p, O_WRONLY | O_APPEND);
    (void)!write(other, "123", 3);
    close(other);
    str("a grown"); kv("tell", ftell(f));
    fputs("xy", f); kv("write", ftell(f)); fflush(f); kv("flush", ftell(f)); kv("size", size_of(p)); nl();
    fclose(f);
    /* a: seek to the start, then tell, then write. */
    p = fresh();
    f = fopen(p, "a");
    str("a seek0"); kv("r", fseek(f, 0, SEEK_SET)); kv("tell", ftell(f));
    fputs("xy", f); kv("write", ftell(f)); fflush(f); kv("flush", ftell(f)); kv("size", size_of(p)); nl();
    fclose(f);
    /* a: an empty file. */
    p = path("app_empty.txt");
    unlink(p);
    str("a empty"); after_open("", fopen(p, "a"), p);
    unlink(p);
    /* fdopen: an O_APPEND descriptor at offset 0, a plain one at offset 2 given "a", one given "w". */
    p = fresh();
    int fd = open(p, O_WRONLY | O_APPEND);
    str("fdopen O_APPEND a"); after_open("", fdopen(fd, "a"), p);
    p = fresh();
    fd = open(p, O_WRONLY);
    lseek(fd, 2, SEEK_SET);
    str("fdopen off2 a"); after_open("", fdopen(fd, "a"), p);
    p = fresh();
    fd = open(p, O_RDWR | O_APPEND);
    str("fdopen O_APPEND a+"); after_open("", fdopen(fd, "a+"), p);
    p = fresh();
    fd = open(p, O_WRONLY | O_APPEND);
    str("fdopen O_APPEND w"); after_open("", fdopen(fd, "w"), p);
    /* freopen with a path, and with NULL on a read-write stream. */
    p = fresh();
    f = fopen(p, "r");
    str("freopen path a"); after_open("", freopen(p, "a", f), p);
    p = fresh();
    f = fopen(p, "r+");
    str("freopen NULL a"); after_open("", freopen(NULL, "a", f), p);
    p = fresh();
    f = fopen(p, "r+");
    getc(f);
    str("freopen NULL a after getc"); after_open("", freopen(NULL, "a", f), p);
    p = fresh();
    f = fopen(p, "r+");
    str("freopen NULL a+"); after_open("", freopen(NULL, "a+", f), p);
    unlink(p);
}

/* ---------------------------------------------------------------- errors */

static void errors(void) {
    str("== errors"); nl();
    errno = 0;
    FILE *f = fopen(path("missing/x"), "r");
    str("fopen_missing"); kv("null", f == NULL); kv("errno", errno); nl();
    errno = 0;
    f = fopen(path("bad_mode"), "q");
    str("fopen_bad_mode"); kv("null", f == NULL); kv("errno", errno); nl();
    const char *p = path("ro.txt");
    f = fopen(p, "w"); fputs("data", f); fclose(f);
    f = fopen(p, "r");
    errno = 0;
    int r = fputc('x', f);
    str("write_readonly"); kv("r", r); kv("errno", errno); kv("ferror", ferror(f) != 0); nl();
    clearerr(f);
    fclose(f);
    f = fopen(p, "w");
    errno = 0;
    r = fgetc(f);
    str("read_writeonly"); kv("r", r); kv("errno", errno); kv("ferror", ferror(f) != 0); kv("feof", feof(f) != 0); nl();
    fclose(f);
    int fd = open(p, O_RDONLY);
    errno = 0;
    f = fdopen(fd, "w");
    str("fdopen_mismatch"); kv("null", f == NULL); kv("errno", errno); nl();
    f = fdopen(fd, "r");
    str("fdopen_ok"); kv("fileno_matches", fileno(f) == fd); nl();
    fclose(f);
    errno = 0;
    str("fseek_bad_whence"); kv("r", fseek(stdin, 0, 7)); kv("errno", errno); nl();
    unlink(p);
}

/* ---------------------------------------------------------------- buffering policy */

static void buffering(void) {
    str("== buffering"); nl();
    const char *p = path("buf.txt");
    const char *modes[] = { "full", "line", "none" };
    int types[] = { _IOFBF, _IOLBF, _IONBF };
    for (int m = 0; m < 3; m++) {
        FILE *f = fopen(p, "w");
        setvbuf(f, NULL, types[m], 64);
        str(modes[m]);
        fputs("ab", f); kv("after_ab", size_of(p));
        fputs("c\nd", f); kv("after_newline", size_of(p));
        char big[100]; memset(big, 'x', sizeof big);
        fwrite(big, 1, sizeof big, f); kv("after_100", size_of(p));
        kv("flbf", __flbf(f) != 0); kv("bufsize", (long)__fbufsize(f));
        fclose(f); kv("closed", size_of(p)); nl();
    }
    /* A caller buffer. */
    static char user[32];
    FILE *f = fopen(p, "w");
    str("user_buffer"); kv("setvbuf", setvbuf(f, user, _IOFBF, sizeof user));
    fputs("hello", f); kv("in_user", memcmp(user, "hello", 5) == 0 || memmem(user, sizeof user, "hello", 5) != NULL); kv("on_disk", size_of(p));
    fclose(f); kv("closed", size_of(p)); nl();
    /* setvbuf after I/O fails. */
    f = fopen(p, "w");
    fputc('x', f);
    str("setvbuf_late"); kv("r", setvbuf(f, NULL, _IONBF, 0) != 0); nl();
    fclose(f);
    /* fflush of an input stream gives read-ahead back to the descriptor (POSIX 2008). */
    f = fopen(p, "w"); fputs("0123456789", f); fclose(f);
    f = fopen(p, "r");
    getc(f); getc(f);
    str("fflush_input"); kv("fd_pos_before", (long)lseek(fileno(f), 0, SEEK_CUR)); kv("fflush", fflush(f)); kv("fd_pos_after", (long)lseek(fileno(f), 0, SEEK_CUR)); kv("next", getc(f)); nl();
    str("stdio_ext"); kv("reading", __freading(f) != 0); kv("writing", __fwriting(f) != 0); kv("readable", __freadable(f) != 0); kv("writable", __fwritable(f) != 0);
    kv("locking", __fsetlocking(f, FSETLOCKING_QUERY)); kv("set", __fsetlocking(f, FSETLOCKING_BYCALLER)); kv("now", __fsetlocking(f, FSETLOCKING_QUERY)); nl();
    __fpurge(f);
    fclose(f);
    unlink(p);
}

/* ---------------------------------------------------------------- memory streams */

static void mem(const char *mode, int nullbuf) {
    char buf[12];
    memcpy(buf, "abc\0defghij", 12);
    errno = 0;
    FILE *f = fmemopen(nullbuf ? NULL : buf, 10, mode);
    str("fmemopen "); str(mode); kv("null", nullbuf); kv("ok", f != NULL); kv("errno", errno); nl();
    if (!f) return;
    str(" open"); kv("tell", ftell(f)); nl();
    char r[32] = {0};
    size_t n = 0;
    if (mode[0] == 'r' || mode[1] == '+') {
        n = fread(r, 1, 20, f);
        /* A library-allocated buffer may start uninitialised: its bytes are not compared. */
        str(" read"); kv("n", (long)n); kv("eof", feof(f) != 0); ch(' '); if (!nullbuf) bytes(r, n); nl();
    }
    clearerr(f);
    fseek(f, 0, SEEK_END);
    str(" end"); kv("tell", ftell(f)); nl();
    fseek(f, 1, SEEK_SET);
    errno = 0;
    int w = fputs("XY", f);
    int fl = fflush(f);
    str(" fputs"); kv("r", w >= 0); kv("flush", fl); kv("tell", ftell(f)); if (!nullbuf) { ch(' '); bytes(buf, 12); } nl();
    clearerr(f);
    fseek(f, 0, SEEK_END);
    str(" end2"); kv("tell", ftell(f)); nl();
    errno = 0;
    int s = fseek(f, 11, SEEK_SET);
    str(" seek11"); kv("r", s); kv("errno", errno); nl();
    fseek(f, 8, SEEK_SET);
    size_t k = fwrite("12345", 1, 5, f);
    fl = fflush(f);
    str(" fwrite"); kv("n", (long)k); kv("flush", fl); kv("err", ferror(f) != 0); if (!nullbuf) { ch(' '); bytes(buf, 12); } nl();
    str(" close"); kv("r", fclose(f)); if (!nullbuf) { ch(' '); bytes(buf, 12); } nl();
}

static void memory(void) {
    str("== memory"); nl();
    const char *modes[] = { "r", "r+", "w", "w+", "a", "a+", NULL };
    for (int i = 0; modes[i]; i++) { mem(modes[i], 0); mem(modes[i], 1); }
    char *p = (char *)1; size_t sz = 99;
    FILE *m = open_memstream(&p, &sz);
    str("memstream"); kv("ptr_unset", p == (char *)1); kv("sz", (long)sz); nl();
    fputs("hello", m);
    str(" before_flush"); kv("sz", (long)sz); nl();
    fflush(m);
    str(" flush"); kv("sz", (long)sz); ch(' '); bytes(p, sz + 1); nl();
    fseek(m, 2, SEEK_SET); fflush(m);
    str(" seek2"); kv("sz", (long)sz); nl();
    fputs("Z", m); fflush(m);
    str(" write"); kv("sz", (long)sz); ch(' '); bytes(p, sz + 1); nl();
    fseek(m, 10, SEEK_SET); fputs("!", m); fflush(m);
    str(" past_end"); kv("sz", (long)sz); ch(' '); bytes(p, sz + 1); nl();
    fseek(m, 0, SEEK_END);
    str(" end"); kv("tell", ftell(m)); nl();
    for (int i = 0; i < 2000; i++) fputc('a' + i % 26, m);
    fclose(m);
    str(" closed"); kv("sz", (long)sz); kv("last", p[sz - 1]); kv("nul", p[sz] == 0); nl();
    free(p);
}

/* ---------------------------------------------------------------- cookie streams */

struct cookie { char data[64]; size_t len, pos; int closed; };
static ssize_t ck_read(void *c, char *b, size_t n) {
    struct cookie *k = c;
    str(" cb_read"); kv("n", (long)n); nl();
    size_t m = k->len - k->pos < n ? k->len - k->pos : n;
    memcpy(b, k->data + k->pos, m); k->pos += m; return (ssize_t)m;
}
static ssize_t ck_write(void *c, const char *b, size_t n) {
    struct cookie *k = c;
    str(" cb_write"); kv("n", (long)n); ch(' '); bytes(b, n); nl();
    memcpy(k->data + k->pos, b, n); k->pos += n; if (k->pos > k->len) k->len = k->pos; return (ssize_t)n;
}
static int ck_seek(void *c, off64_t *off, int whence) {
    struct cookie *k = c;
    str(" cb_seek"); kv("off", (long)*off); kv("whence", whence); nl();
    long base = whence == SEEK_SET ? 0 : whence == SEEK_CUR ? (long)k->pos : (long)k->len;
    k->pos = (size_t)(base + *off); *off = (off64_t)k->pos; return 0;
}
static int ck_close(void *c) { struct cookie *k = c; k->closed = 1; str(" cb_close"); nl(); return 0; }

static void cookies(void) {
    str("== cookies"); nl();
    struct cookie k = { "seed data", 9, 0, 0 };
    cookie_io_functions_t fns = { ck_read, ck_write, ck_seek, ck_close };
    FILE *f = fopencookie(&k, "r+", fns);
    setvbuf(f, NULL, _IOFBF, 4);
    char b[8] = {0};
    size_t n = fread(b, 1, 6, f);
    str("read"); kv("n", (long)n); ch(' '); bytes(b, n); nl();
    fseek(f, 0, SEEK_SET);
    fputs("XYZ", f);
    fflush(f);
    str("tell"); kv("pos", ftell(f)); nl();
    fclose(f);
    str("closed"); kv("flag", k.closed); ch(' '); bytes(k.data, k.len); nl();
    cookie_io_functions_t none = { NULL, NULL, NULL, NULL };
    f = fopencookie(&k, "r+", none);
    str("null_fns"); kv("getc", fgetc(f)); kv("eof", feof(f) != 0); kv("fputs", fputs("discarded", f) >= 0); kv("flush", fflush(f)); kv("seek", fseek(f, 0, SEEK_SET)); nl();
    fclose(f);
}

/* ---------------------------------------------------------------- standard streams */

static void standard(void) {
    str("== standard"); nl();
    str("fileno"); kv("in", fileno(stdin)); kv("out", fileno(stdout)); kv("err", fileno(stderr)); nl();
    const char *p = path("redirect.txt");
    FILE *r = freopen(p, "w", stdout);
    str("freopen"); kv("same", r == stdout); kv("fd", fileno(stdout)); nl();
    puts("to the file");
    fputs("more", stdout);
    fflush(stdout);
    str("redirected"); kv("size", size_of(p)); nl();
    str("fwide"); kv("query", fwide(stdout, 0)); kv("byte", fwide(stdout, -1)); kv("then_wide", fwide(stdout, 1)); nl();
    unlink(p);
}

/* A generated name: the test directory shown as <dir> (each build has its own), the six random
 * letters as ######, the length after the directory, whether the letters are all from the
 * generator's set, and whether the name exists. */
static void name(const char *label, const char *p) {
    str(label);
    if (!p) { str(" NULL errno="); num(errno); nl(); return; }
    size_t n = strlen(p), d = strlen(dir);
    ch(' ');
    const char *q = p;
    if (strncmp(p, dir, d) == 0) { str("<dir>"); q = p + d; }
    size_t m = strlen(q);
    for (size_t i = 0; i + 6 < m; i++) ch(q[i]);
    str("######");
    int letters = n >= 6;
    for (size_t i = n >= 6 ? n - 6 : 0; i < n; i++)
        if (!((p[i] >= 'a' && p[i] <= 'z') || (p[i] >= 'A' && p[i] <= 'Z') || (p[i] >= '0' && p[i] <= '9'))) letters = 0;
    struct stat st;
    kv("len", (long)m); kv("letters", letters); kv("exists", lstat(p, &st) == 0); nl();
}

static void names(void) {
    str("== names\n");
    char buf[L_tmpnam + 8];
    memset(buf, 'Z', sizeof buf);
    errno = 0;
    char *r = tmpnam(buf);
    kv("L_tmpnam", L_tmpnam); kv("TMP_MAX", TMP_MAX); kv("in_buf", r == buf); nl();
    name("tmpnam", r);
    r = tmpnam(NULL);
    name("tmpnam_null", r);
    kv("same_static", r == tmpnam(NULL)); nl();
    errno = 0;
    kv("tmpnam_r_null", tmpnam_r(NULL) == NULL); nl();
    name("tmpnam_r", tmpnam_r(buf));
    char *t;
    name("tempnam", t = tempnam(NULL, NULL)); free(t);
    name("tempnam_pfx", t = tempnam(NULL, "abcdefgh")); free(t);
    name("tempnam_empty_pfx", t = tempnam(NULL, "")); free(t);
    name("tempnam_dir", t = tempnam(dir, "p")); free(t);
    char slashes[600];
    snprintf(slashes, sizeof slashes, "%s///", dir);
    name("tempnam_slashes", t = tempnam(slashes, "q")); free(t);
    name("tempnam_missing_dir", t = tempnam("/nonexistent/dir", "m")); free(t);
    name("tempnam_root", t = tempnam("/", "r")); free(t);
    setenv("TMPDIR", dir, 1);
    name("tempnam_tmpdir", t = tempnam("/var/tmp", "e")); free(t);
    setenv("TMPDIR", "/nonexistent", 1);
    name("tempnam_bad_tmpdir", t = tempnam("/var/tmp", "e")); free(t);
    unsetenv("TMPDIR");
    name("tmpnam_ignores_tmpdir", tmpnam(NULL));
    char tty[L_ctermid];
    str("ctermid "); str(ctermid(tty)); kv("in_buf", ctermid(tty) == tty); ch(' '); str(ctermid(NULL)); kv("L_ctermid", L_ctermid); nl();
}

/* The libio entry points (spec §12.13): no installed header declares them. */
extern int _IO_getc(FILE *);
extern int _IO_putc(int, FILE *);
extern int _IO_peekc_locked(FILE *);
extern ssize_t _IO_padn(FILE *, int, ssize_t);
extern size_t _IO_sgetn(FILE *, void *, size_t);
extern off_t _IO_seekoff(FILE *, off_t, int, int);
extern off_t _IO_seekpos(FILE *, off_t, int);
extern size_t _IO_getline(FILE *, char *, size_t, int, int);
extern size_t _IO_getline_info(FILE *, char *, size_t, int, int, int *);
extern int _IO_feof(FILE *);
extern int _IO_ferror(FILE *);
extern int _IO_fflush(FILE *);
extern int _IO_flush_all(void);
extern FILE *_IO_iter_begin(void);
extern FILE *_IO_iter_end(void);
extern FILE *_IO_iter_next(FILE *);
extern FILE *_IO_iter_file(FILE *);
extern void _IO_list_lock(void);
extern void _IO_list_unlock(void);

static void libio(void) {
    str("== libio\n");
    char path[600];
    snprintf(path, sizeof path, "%s/libio", dir);
    FILE *f = fopen(path, "w+");
    kv("putc", _IO_putc('A', f)); kv("padn", (long)_IO_padn(f, '-', 70)); kv("padn0", (long)_IO_padn(f, 'x', 0));
    fputs("\nline one\nline two;rest\nlast", f);
    kv("flush", _IO_fflush(f)); kv("seekpos", (long)_IO_seekpos(f, 0, 3)); nl();
    kv("getc", _IO_getc(f)); kv("peek", _IO_peekc_locked(f)); kv("peek_again", _IO_peekc_locked(f));
    char b[128];
    memset(b, 'Z', sizeof b);
    size_t n = _IO_sgetn(f, b, 10);
    kv("sgetn", (long)n); ch(' '); bytes(b, n);
    kv("seekoff_cur", (long)_IO_seekoff(f, 60, SEEK_CUR, 3)); nl();
    int extracts[] = { 1, 0, -1 };
    for (int k = 0; k < 3; k++) {
        _IO_seekoff(f, 71, SEEK_SET, 3);
        for (int i = 0; i < 4; i++) {
            int eof = 7;
            memset(b, 'Z', sizeof b);
            n = _IO_getline_info(f, b, i == 1 ? 4 : sizeof b, i == 2 ? ';' : '\n', extracts[k], &eof);
            str("getline_info"); kv("extract", extracts[k]); kv("n", (long)n); ch(' '); bytes(b, n + 1); kv("eof", eof); kv("next", _IO_peekc_locked(f)); nl();
        }
    }
    _IO_seekoff(f, 71, SEEK_SET, 3);
    n = _IO_getline(f, b, sizeof b, '\n', 1);
    str("getline"); kv("n", (long)n); ch(' '); bytes(b, n); nl();
    kv("feof", _IO_feof(f)); kv("ferror", _IO_ferror(f)); kv("seekoff_end", (long)_IO_seekoff(f, 0, SEEK_END, 3)); kv("getc_end", _IO_getc(f)); kv("feof_end", _IO_feof(f)); nl();
    /* The open-streams list holds the streams opened last. */
    FILE *g = fopen(path, "r"), *h = fopen(path, "r");
    _IO_list_lock();
    int seen = 0;
    for (FILE *it = _IO_iter_begin(); it != _IO_iter_end(); it = _IO_iter_next(it)) {
        FILE *x = _IO_iter_file(it);
        if (x == f) seen |= 1;
        if (x == g) seen |= 2;
        if (x == h) seen |= 4;
    }
    _IO_list_unlock();
    kv("iter_seen", seen); kv("first_is_h", _IO_iter_begin() == h); kv("flush_all", _IO_flush_all()); nl();
    fclose(h); fclose(g); fclose(f);
    unlink(path);
}

/* ---------------------------------------------------------------- seek, rewind, refill
 * STDIO-4: rewind/fseek publish only the read window when the buffer stays and no write role
 * is involved. Each line below catches one way that can go wrong: a stale window after the file
 * changed, an indicator not cleared, output not written before a seek (or a write window left
 * open after it), a wrong position after bytes were read ahead. A seek inside the read-ahead
 * keeps the descriptor's offset in the reference library (a recorded difference), so the
 * descriptor's offset is printed only after a rewind. */
static void seeks(void) {
    str("== seeks"); nl();
    const char *p = path("seek.txt");
    int fd = open(p, O_RDWR | O_CREAT | O_TRUNC, 0644);
    (void)!write(fd, "abcdefghij\n", 11);
    FILE *f = fopen(p, "r");
    int c0 = fgetc(f);
    (void)!pwrite(fd, "XY", 2, 0);
    rewind(f);
    /* The header macro reads the published window words directly (the library's own entry
       points re-check them against the stream's state, so only this sees a stale window). */
    int c1 = getc_unlocked(f);
    str("refill"); kv("first", c0); kv("after_rewind", c1); kv("fd_at", (long)lseek(fileno(f), 0, SEEK_CUR)); nl();
    for (int i = 0; i < 3; i++) (void)fgetc(f);
    long t = ftell(f);
    fseek(f, 2, SEEK_CUR);
    int c2 = fgetc(f);
    fseek(f, 1, SEEK_SET);
    int c3 = fgetc(f);
    str("readahead"); kv("tell", t); kv("cur2", c2); kv("set1", c3); kv("tell2", ftell(f)); nl();
    while (fgetc(f) != EOF) {}
    int eof = feof(f) != 0;
    (void)fputc('q', f);
    int err = ferror(f) != 0;
    rewind(f);
    str("indicators"); kv("eof", eof); kv("err", err); kv("eof_after", feof(f) != 0); kv("err_after", ferror(f) != 0); kv("c", fgetc(f)); nl();
    fseek(f, 0, SEEK_END);
    (void)fgetc(f);
    int eof2 = feof(f) != 0;
    fseek(f, 3, SEEK_SET);
    str("seek_clears_eof"); kv("eof", eof2); kv("eof_after", feof(f) != 0); kv("c", fgetc(f)); nl();
    fclose(f);
    FILE *w = fopen(p, "w+");
    fputs("hello", w);
    rewind(w);
    long disk = size_of(p);
    fputs("HE", w);
    fflush(w);
    char got[16] = {0};
    long n = (long)pread(fd, got, sizeof got - 1, 0);
    str("write_seek"); kv("flushed_at_rewind", disk); kv("n", n); ch(' '); bytes(got, n > 0 ? (size_t)n : 0); kv("tell", ftell(w)); nl();
    fseek(w, 0, SEEK_SET);
    str("write_read"); kv("c", fgetc(w)); kv("c2", fgetc(w)); kv("tell", ftell(w)); nl();
    fseek(w, 0, SEEK_CUR);
    fputc('!', w);
    fseek(w, 0, SEEK_SET);
    memset(got, 0, sizeof got);
    n = (long)pread(fd, got, sizeof got - 1, 0);
    str("read_write"); kv("n", n); ch(' '); bytes(got, n > 0 ? (size_t)n : 0); kv("c", fgetc(w)); nl();
    fclose(w);
    FILE *u = fopen(p, "r");
    str("unused_close"); kv("flush", fflush(u)); kv("close", fclose(u)); nl();
    close(fd);
    unlink(p);
}

/* Refills of an empty read window (STDIO-5): small buffers, the header macro after a refill,
   reading then writing on an update stream, a refill that fails, end of input. */
static void refills(void) {
    str("== refills"); nl();
    const char *p = path("refill.txt");
    int fd = open(p, O_RDWR | O_CREAT | O_TRUNC, 0644);
    (void)!write(fd, "0123456789abcdefghijkl", 22);
    FILE *f = fopen(p, "r");
    static char small[4];
    setvbuf(f, small, _IOFBF, sizeof small);
    str("small");
    for (int i = 0; i < 24; i++) {
        int c = (i & 1) ? getc_unlocked(f) : fgetc(f);
        ch(' '); num(c); ch('@'); num(ftell(f));
    }
    kv("eof", feof(f) != 0); kv("err", ferror(f) != 0); nl();
    /* Only the header macro: a short last refill must not leave the previous window's end. */
    rewind(f);
    str("macro_only");
    for (int i = 0; i < 24; i++) { int c = getc_unlocked(f); ch(' '); num(c); }
    nl();
    rewind(f);
    int a = fgetc(f);
    int b = getc_unlocked(f);
    int c = getc_unlocked(f);
    str("macro_after_refill"); kv("a", a); kv("b", b); kv("c", c); kv("tell", ftell(f)); kv("fd_at", (long)lseek(fileno(f), 0, SEEK_CUR)); nl();
    fclose(f);
    FILE *u = fopen(p, "r+");
    int r0 = fgetc(u);
    int r1 = fgetc(u);
    fseek(u, 0, SEEK_CUR);
    fputc('#', u);
    fflush(u);
    char got[32] = {0};
    long n = (long)pread(fd, got, sizeof got - 1, 0);
    str("read_then_write"); kv("r0", r0); kv("r1", r1); kv("tell", ftell(u)); kv("n", n); ch(' '); bytes(got, n > 0 ? (size_t)n : 0); nl();
    fclose(u);
    /* Input straight after output on an update stream: the pending bytes go out first. */
    FILE *wr = fopen(p, "w+");
    fputs("xyz", wr);
    int after_write = fgetc(wr);
    str("write_then_read"); kv("c", after_write); kv("size", size_of(p)); kv("eof", feof(wr) != 0); nl();
    fclose(wr);
    (void)!pwrite(fd, "0123456789abcdefghijkl", 22, 0);
    /* Reading an unbuffered stream first flushes line-buffered stdout (ISO C). */
    const char *q = path("refill-out.txt");
    int ofd = open(q, O_WRONLY | O_CREAT | O_TRUNC, 0644);
    int saved = dup(1);
    dup2(ofd, 1);
    setvbuf(stdout, NULL, _IOLBF, 0);
    fputs("partial", stdout);
    long before = size_of(q);
    FILE *ub = fopen(p, "r");
    setvbuf(ub, NULL, _IONBF, 0);
    int c_ub = fgetc(ub);
    long after = size_of(q);
    fputs("more", stdout);
    int c_ub2 = fgetc(ub); /* the buffer is in place now: a refill, not a first read */
    long after2 = size_of(q);
    fflush(stdout);
    dup2(saved, 1);
    close(saved);
    close(ofd);
    fclose(ub);
    unlink(q);
    str("unbuffered_flushes_stdout"); kv("c", c_ub); kv("before", before); kv("after", after); kv("c2", c_ub2); kv("after2", after2); nl();
    FILE *d = fopen(dir, "r");
    if (d) {
        errno = 0;
        int e = fgetc(d);
        int en = errno;
        str("dir"); kv("c", e); kv("errno", en); kv("err", ferror(d) != 0); kv("eof", feof(d) != 0);
        clearerr(d);
        kv("again", getc_unlocked(d)); kv("err2", ferror(d) != 0); nl();
        fclose(d);
    }
    close(fd);
    unlink(p);
}

int main(int argc, char **argv) {
    if (argc < 2) return 2;
    strcpy(dir, argv[1]);
    out_fd = dup(1);
    files();
    append();
    errors();
    buffering();
    memory();
    cookies();
    standard();
    names();
    libio();
    seeks();
    refills();
    flush_out();
    return 0;
}
