/* Spec §12.5, D-12.11, P5: the front path of fputc/putc/fputs/fwrite in a single-threaded
 * process (the stream lock is skipped): bytes go into an open write window and nothing else
 * changes. Each stream kind and state the front path must leave to the general path is checked
 * for the general path's answers: an undecided or wide orientation, line-buffered and
 * unbuffered streams, a read-only stream, the memory and cookie streams, and stdout at its
 * first use. The functions are called through volatile pointers, never as header-inline code
 * or compiler builtins. fprintf (STDIO-HOT) holds the write window for a whole call. getc
 * (STDIO-2) takes a byte from an open read window; that never writes a line-buffered stream. */
#define _GNU_SOURCE
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/single_threaded.h>
#include <sys/stat.h>
#include <unistd.h>
#include <wchar.h>

#define CHECK(n, cond) do { if (!(cond)) return n; } while (0)

static int (*volatile p_fputc)(int, FILE *) = fputc;
static int (*volatile p_putc)(int, FILE *) = putc;
static int (*volatile p_putc_unlocked)(int, FILE *) = putc_unlocked;
static int (*volatile p_putchar)(int) = putchar;
static int (*volatile p_fputs)(const char *, FILE *) = fputs;
static size_t (*volatile p_fwrite)(const void *, size_t, size_t, FILE *) = fwrite;
static int (*volatile p_getc)(FILE *) = getc;
static int (*volatile p_getc_unlocked)(FILE *) = getc_unlocked;
static size_t (*volatile p_fread)(void *, size_t, size_t, FILE *) = fread;

static long size_of(FILE *f)
{
    struct stat st;
    return fstat(fileno(f), &st) == 0 ? (long)st.st_size : -1;
}

/* Every form, many times over the buffer's size, read back byte for byte. */
static int mixed(void)
{
    FILE *f = tmpfile();
    CHECK(10, f != NULL && fwide(f, 0) == 0);
    CHECK(11, p_fputc('<', f) == '<' && fwide(f, 0) < 0);      /* the first one decides: byte */
    long n = 1;
    for (int i = 0; i < 20000; i++) {
        char c = (char)('a' + i % 26);
        switch (i % 5) {
        case 0: CHECK(12, p_fputc(c, f) == c); n += 1; break;
        case 1: CHECK(13, p_putc(c | 0x100, f) == c); n += 1; break;
        case 2: CHECK(14, p_putc_unlocked(c, f) == c); n += 1; break;
        case 3: CHECK(15, p_fputs("xyz", f) >= 0); n += 3; break;
        default: CHECK(16, p_fwrite("0123456789", 2, 3, f) == 3); n += 6; break;
        }
    }
    CHECK(17, fflush(f) == 0 && size_of(f) == n);
    rewind(f);
    CHECK(18, fgetc(f) == '<');
    for (int i = 0; i < 20000; i++) {
        char c = (char)('a' + i % 26), got[8] = {0};
        switch (i % 5) {
        case 3: CHECK(19, fread(got, 1, 3, f) == 3 && memcmp(got, "xyz", 3) == 0); break;
        case 4: CHECK(20, fread(got, 1, 6, f) == 6 && memcmp(got, "012345", 6) == 0); break;
        default: CHECK(21, fgetc(f) == c); break;
        }
    }
    CHECK(22, fgetc(f) == EOF && feof(f) && !ferror(f));
    fclose(f);
    return 0;
}

/* A fully buffered stream holds the bytes; flockfile is recursive for the only thread. A
 * line-buffered stream writes at the newline, an unbuffered one at once. */
static int buffering(void)
{
    FILE *f = tmpfile();
    CHECK(30, f != NULL);
    CHECK(31, p_fputc('a', f) == 'a' && p_putc('\n', f) == '\n' && size_of(f) == 0);
    flockfile(f);
    CHECK(32, p_fputs("bc\n", f) >= 0 && p_fwrite("d", 1, 1, f) == 1);
    funlockfile(f);
    CHECK(33, size_of(f) == 0 && fflush(f) == 0 && size_of(f) == 6);
    fclose(f);
    FILE *l = tmpfile();
    CHECK(34, l != NULL && setvbuf(l, NULL, _IOLBF, 0) == 0);
    CHECK(35, p_putc('e', l) == 'e' && p_fputs("fg", l) >= 0 && p_fwrite("h", 1, 1, l) == 1 && size_of(l) == 0);
    CHECK(36, p_fputc('\n', l) == '\n' && size_of(l) == 5);
    CHECK(37, p_fwrite("i\n", 1, 2, l) == 2 && size_of(l) == 7 && p_fputs("j\n", l) >= 0 && size_of(l) == 9);
    fclose(l);
    FILE *u = tmpfile();
    CHECK(38, u != NULL && setvbuf(u, NULL, _IONBF, 0) == 0);
    CHECK(39, p_putc('k', u) == 'k' && size_of(u) == 1 && p_fputs("lm", u) >= 0 && size_of(u) == 3);
    CHECK(40, p_fwrite("n", 1, 1, u) == 1 && size_of(u) == 4);
    fclose(u);
    return 0;
}

/* A wide stream, even with an open window, refuses byte output: nothing is stored. */
static int wide(void)
{
    FILE *f = tmpfile();
    CHECK(70, f != NULL && fwide(f, 1) > 0 && fputwc(L'w', f) == L'w');
    errno = 0;
    CHECK(71, p_fputc('x', f) == EOF && errno == EINVAL);
    errno = 0;
    CHECK(72, p_fputs("x", f) == EOF && errno == EINVAL);
    errno = 0;
    CHECK(73, p_fwrite("x", 1, 1, f) == 0 && errno == EINVAL);
    CHECK(74, fflush(f) == 0 && size_of(f) == 1);
    fclose(f);
    FILE *r = fopen("/dev/null", "r");
    CHECK(75, r != NULL && p_putc('x', r) == EOF && ferror(r));    /* not open for writing */
    fclose(r);
    return 0;
}

static size_t cookie_bytes;
static ssize_t cookie_write(void *c, const char *b, size_t n)
{
    (void)c;
    (void)b;
    cookie_bytes += n;
    return (ssize_t)n;
}

/* Memory and cookie streams: the bytes arrive as the general path delivers them. */
static int custom(void)
{
    char buf[16] = {0};
    FILE *m = fmemopen(buf, sizeof buf, "w");
    CHECK(50, m != NULL && p_putc('m', m) == 'm' && p_fputs("em", m) >= 0 && p_fwrite("!", 1, 1, m) == 1);
    CHECK(51, fflush(m) == 0 && memcmp(buf, "mem!", 5) == 0);
    fclose(m);
    char *p = NULL;
    size_t len = 0;
    FILE *o = open_memstream(&p, &len);
    CHECK(52, o != NULL && p_fputc('o', o) == 'o' && p_fputs("ms", o) >= 0);
    CHECK(53, fflush(o) == 0 && len == 3 && memcmp(p, "oms", 4) == 0);
    fclose(o);
    free(p);
    cookie_io_functions_t io = {.write = cookie_write};
    FILE *c = fopencookie(NULL, "w", io);
    CHECK(54, c != NULL && p_putc('c', c) == 'c' && p_fwrite("ok", 1, 2, c) == 2 && p_fputs("ie", c) >= 0);
    CHECK(55, fflush(c) == 0 && cookie_bytes == 5);
    fclose(c);
    return 0;
}

/* stdout at its first use (here a regular file: fully buffered). */
static int first_stdout(void)
{
    FILE *t = tmpfile();
    CHECK(60, t != NULL && dup2(fileno(t), 1) == 1);
    CHECK(61, p_putchar('h') == 'h' && p_fputc('i', stdout) == 'i' && p_fputs("!\n", stdout) >= 0);
    CHECK(62, p_fwrite("ok", 1, 2, stdout) == 2 && size_of(t) == 0);
    CHECK(63, fflush(stdout) == 0 && size_of(t) == 6);
    fclose(t);
    return 0;
}

/* STDIO-HOT: fprintf writes its pieces into the stream's write window, held for the call.
 * Many-piece lines crossing a small caller buffer read back as snprintf renders them, and
 * fputc and ftell between the calls see the position each call left. A line-buffered stream
 * gets its bytes at the newline, an unbuffered one by the end of each call. */
#define LINE "%s %2lu %-8s %8lld %s|%c\n"
#define LINE_ARGS(i) "-rw", (unsigned long)((i) % 7), "user" + (i) % 4, (long long)(i) * 4099, "name" + (i) % 5, 'a' + (i) % 26
static int printf_window(void)
{
    static char sbuf[61];
    char want[128], got[128];
    FILE *f = tmpfile();
    CHECK(90, f != NULL && setvbuf(f, sbuf, _IOFBF, sizeof sbuf) == 0);
    long total = 0;
    for (int i = 0; i < 500; i++) {
        int n = snprintf(want, sizeof want, LINE, LINE_ARGS(i));
        CHECK(91, n > 0 && fprintf(f, LINE, LINE_ARGS(i)) == n);
        CHECK(92, p_fputc('#', f) == '#');
        total += n + 1;
        CHECK(93, ftell(f) == total);
    }
    CHECK(94, fflush(f) == 0 && size_of(f) == total);
    rewind(f);
    for (int i = 0; i < 500; i++) {
        int n = snprintf(want, sizeof want, LINE, LINE_ARGS(i));
        CHECK(95, fread(got, 1, (size_t)n, f) == (size_t)n && memcmp(got, want, (size_t)n) == 0 && getc(f) == '#');
    }
    fclose(f);
    FILE *l = tmpfile();
    CHECK(96, l != NULL && setvbuf(l, NULL, _IOLBF, 0) == 0);
    CHECK(97, fprintf(l, "%d-%s", 12, "ab") == 5 && size_of(l) == 0);
    CHECK(98, fprintf(l, "%s\n", "cd") == 3 && size_of(l) == 8 && fprintf(l, "%d", 7) == 1 && size_of(l) == 8);
    fclose(l);
    FILE *u = tmpfile();
    CHECK(99, u != NULL && setvbuf(u, NULL, _IONBF, 0) == 0);
    CHECK(100, fprintf(u, "%s=%d;", "x", 5) == 4 && size_of(u) == 4 && fprintf(u, "%s", "") == 0 && size_of(u) == 4);
    fclose(u);
    return 0;
}

/* STDIO-2: bytes from the read window, across refills of a line-buffered input stream; reading
 * a byte the window holds writes no line-buffered output (only a refill may). */
static int getc_window(void)
{
    FILE *f = tmpfile(), *l = tmpfile();
    CHECK(110, f != NULL && l != NULL && setvbuf(f, NULL, _IOLBF, 0) == 0 && setvbuf(l, NULL, _IOLBF, 0) == 0);
    for (int i = 0; i < 9000; i++)
        CHECK(111, p_fputc('a' + i % 26, f) == 'a' + i % 26);
    CHECK(112, fflush(f) == 0 && fseek(f, 0, SEEK_SET) == 0);
    for (int i = 0; i < 9000; i++) {
        CHECK(113, (i % 2 ? p_getc(f) : p_getc_unlocked(f)) == 'a' + i % 26);
        if (i % 1000 == 0) {
            CHECK(114, p_fputc('x', l) == 'x');
            long held = size_of(l);
            for (int k = 1; k < 4 && i + k < 9000; k++, i++)
                CHECK(115, p_getc(f) == 'a' + (i + 1) % 26);
            CHECK(116, size_of(l) == held);
        }
    }
    CHECK(117, p_getc(f) == EOF && feof(f) && !ferror(f) && ftell(f) == 9000);
    fclose(f);
    fclose(l);
    return 0;
}

/* FREAD-BIG-2: fread with nothing in the read window, in a single-threaded process (the stream
 * lock is skipped): requests at least the buffer's size go straight to the caller's buffer;
 * the count, the bytes, the position, end of file and a read error are the general path's. */
static int fread_empty_window(void)
{
    static char data[20000], got[8200];
    for (int i = 0; i < 20000; i++)
        data[i] = (char)(i % 253);
    FILE *f = tmpfile();
    CHECK(120, f != NULL && fwrite(data, 1, sizeof data, f) == sizeof data && fseek(f, 0, SEEK_SET) == 0);
    CHECK(121, setvbuf(f, NULL, _IOFBF, 1024) == 0);
    CHECK(122, p_fread(got, 1, 10, f) == 10 && p_fread(got, 1, 1014, f) == 1014 && memcmp(got, data + 10, 1014) == 0);
    long pos = 1024;
    for (int k = 0; k < 2; k++, pos += 8192) {
        memset(got, '#', sizeof got);
        CHECK(123, p_fread(got, 1, 8192, f) == 8192 && memcmp(got, data + pos, 8192) == 0 && got[8192] == '#');
        CHECK(124, ftell(f) == pos + 8192 && lseek(fileno(f), 0, SEEK_CUR) == pos + 8192 && !feof(f) && !ferror(f));
    }
    CHECK(125, p_fread(got, 1, 8192, f) == (size_t)(20000 - pos) && memcmp(got, data + pos, (size_t)(20000 - pos)) == 0);
    CHECK(126, feof(f) && !ferror(f) && ftell(f) == 20000 && p_fread(got, 3, 2000, f) == 0);
    CHECK(127, fseek(f, 5, SEEK_SET) == 0 && p_fread(got, 4, 1500, f) == 1500 && memcmp(got, data + 5, 6000) == 0);
    CHECK(128, p_fread(got, 1, 1, f) == 1 && got[0] == data[6005] && ftell(f) == 6006);
    fclose(f);
    FILE *d = fopen("/", "r");
    CHECK(129, d != NULL && setvbuf(d, NULL, _IOFBF, 1024) == 0);
    errno = 0;
    CHECK(130, p_fread(got, 1, 8192, d) == 0 && errno == EISDIR && ferror(d) && !feof(d));
    fclose(d);
    return 0;
}

int main(void)
{
    CHECK(1, __libc_single_threaded);
    int r = mixed();
    if (r == 0)
        r = buffering();
    if (r == 0)
        r = wide();
    if (r == 0)
        r = custom();
    if (r == 0)
        r = first_stdout();
    if (r == 0)
        r = printf_window();
    if (r == 0)
        r = getc_window();
    if (r == 0)
        r = fread_empty_window();
    return r;
}
