/* Error text and diagnostics end to end (spec §2.2, §2.3; D-2.2, D-2.3): built against the
 * host C library and against Liella's products, the outputs must match line for line
 * (diag_products.rs). Standard error is captured in a file and shown with escapes; the
 * functions that exit run in child processes. */
#define _GNU_SOURCE
#include <err.h>
#include <errno.h>
#include <error.h>
#include <fcntl.h>
#include <fmtmsg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/syscall.h>
#include <sys/wait.h>
#include <unistd.h>
#include <wchar.h>

extern int __xpg_strerror_r(int, char *, size_t);

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

/* Standard error into a file: the descriptor saved, the file's descriptor in its place. */
static int saved_err = -1, capture_fd = -1;
static void capture(void) {
    fflush(stderr);
    char path[64];
    snprintf(path, sizeof path, "/tmp/diag_diff_%d", (int)getpid());
    capture_fd = open(path, O_RDWR | O_CREAT | O_TRUNC, 0600);
    unlink(path);
    saved_err = dup(2);
    dup2(capture_fd, 2);
}
static void captured(void) {
    fflush(stderr);
    dup2(saved_err, 2);
    close(saved_err);
    char b[1024];
    ssize_t n = pread(capture_fd, b, sizeof b, 0);
    close(capture_fd);
    str(" stderr="); text(b, n > 0 ? (size_t)n : 0);
}

static void texts(void) {
    str("== strerror\n");
    for (int e = -2; e <= 140; e++) {
        num(e); ch(' '); ztext(strerror(e)); ch(' '); ztext(strerrorname_np(e)); ch(' '); ztext(strerrordesc_np(e)); ch('\n');
    }
    str("== strerror_r\n");
    int errs[] = { 0, 2, 22, 133, 134, -1, 1000 };
    size_t sizes[] = { 0, 1, 5, 12, 64 };
    for (int i = 0; i < 7; i++)
        for (int k = 0; k < 5; k++) {
            char buf[80];
            memset(buf, 'Z', sizeof buf); buf[79] = 0;
            char *r = strerror_r(errs[i], buf, sizes[k]);
            str("gnu "); num(errs[i]); ch(' '); num((long)sizes[k]); str(" in_buf="); num(r == buf); ch(' ');
            ztext(r); ch(' '); text(buf, 16); ch('\n');
            memset(buf, 'Z', sizeof buf); buf[79] = 0;
            errno = 0;
            int x = __xpg_strerror_r(errs[i], buf, sizes[k]);
            str("xpg "); num(errs[i]); ch(' '); num((long)sizes[k]); str(" -> "); num(x); str(" errno="); num(errno); ch(' '); text(buf, 16); ch('\n');
        }
}

static void perrors(void) {
    str("== perror\n");
    const char *ss[] = { NULL, "", "ctx", "two words", NULL };
    int es[] = { 0, 2, 13, 999 };
    for (int i = 0; i < 4; i++)
        for (int k = 0; k < 4; k++) {
            capture();
            errno = es[i];
            perror(ss[k]);
            int after = errno;
            str("perror "); num(es[i]); ch(' '); ztext(ss[k]); str(" errno_after="); num(after);
            captured(); ch('\n');
        }
}

typedef void (*job)(void);
/* `f` in a child: its exit status and what it wrote to standard error. */
static void child(const char *label, job f) {
    flush_out();
    int p[2];
    (void)!pipe(p);
    pid_t pid = fork();
    if (pid == 0) {
        dup2(p[1], 2);
        close(p[0]);
        f();
        fflush(stdout);
        _exit(77);
    }
    close(p[1]);
    char b[2048];
    size_t n = 0;
    ssize_t k;
    while (n < sizeof b && (k = read(p[0], b + n, sizeof b - n)) > 0) n += (size_t)k;
    close(p[0]);
    int st;
    /* wait4 through syscall(): the products have no waitpid until §18. */
    syscall(SYS_wait4, pid, &st, 0, NULL);
    str(label); str(" exit="); num(WIFEXITED(st) ? WEXITSTATUS(st) : -1); str(" stderr="); text(b, n); ch('\n');
}

static void j_warn(void) { errno = ENOENT; warn("file %s", "x.txt"); errno = EACCES; warn(NULL); }
static void j_warnx(void) { errno = ENOENT; warnx("plain %d", 42); warnx(NULL); warnx("%s", ""); }
static void j_err(void) { errno = EPERM; err(3, "cannot %s", "go"); }
static void j_err_null(void) { errno = 9999; err(4, NULL); }
static void j_errx(void) { errx(5, "bye %c", '!'); }
static void j_error0(void) {
    error(0, 0, "note %d", 1);
    error(0, ENOENT, "missing %s", "f");
    error(0, 9999, "odd");
    printf("count=%u\n", error_message_count);
}
static void j_error_exit(void) { printf("pending stdout "); error(6, EINTR, "stop"); }
static void j_at_line(void) {
    error_at_line(0, 0, "a.c", 10, "first");
    error_at_line(0, EBADF, "a.c", 10, "same");
    error_at_line(0, 0, NULL, 3, "nofile");
    error_one_per_line = 1;
    error_at_line(0, 0, "b.c", 7, "once");
    error_at_line(0, 0, "b.c", 7, "skipped");
    char other[] = "b.c";
    error_at_line(0, 0, other, 7, "skipped by name");
    error_at_line(0, 0, "b.c", 8, "next line");
    error_at_line(0, 0, "a.c", 8, "other file");
    printf("count=%u\n", error_message_count);
}
static void hook(void) { fputs("HOOK|", stderr); }
static void j_hook(void) {
    error_print_progname = hook;
    error(0, 0, "with hook");
    error_at_line(0, ENOENT, "h.c", 5, "at line");
    warnx("bsd ignores the hook");
}
static void j_orient(void) {
    printf("before=%d ", fwide(stderr, 0));
    perror("p");
    printf("after_perror=%d ", fwide(stderr, 0));
    warnx("w");
    printf("after_warnx=%d\n", fwide(stderr, 0));
}
static void j_wide(void) {
    fwide(stderr, 1);
    errno = ENOENT;
    perror("wide");
    warnx("wide %s", "warnx");
    error(0, EIO, "wide error");
}

/* fmtmsg cases; the results go to standard output. */
static void fmt_cases(void) {
    const char *labels[] = { "UX:cat", NULL, "nocolon", "ABCDEFGHIJK:x", "ABCDEFGHIJ:abcdefghijklmn", "a:abcdefghijklmno", ":", NULL };
    for (int i = 0; i < 7; i++) printf("label%d=%d ", i, fmtmsg(MM_PRINT, labels[i], MM_ERROR, "text", "act", "UX:cat:001"));
    printf("\n");
    for (int sev = -1; sev <= 10; sev++) printf("sev%d=%d ", sev, fmtmsg(MM_PRINT, "UX:cat", sev, "t", NULL, NULL));
    printf("\n");
    printf("fields: %d %d %d %d %d %d\n",
           fmtmsg(MM_PRINT, NULL, MM_NOSEV, NULL, NULL, NULL),
           fmtmsg(MM_PRINT, "UX:cat", MM_NOSEV, NULL, NULL, NULL),
           fmtmsg(MM_PRINT, NULL, MM_INFO, NULL, "fix it", NULL),
           fmtmsg(MM_PRINT, NULL, MM_NOSEV, "only text", NULL, "TAG"),
           fmtmsg(MM_PRINT | MM_SOFT | MM_UTIL, "UX:cat", MM_HALT, NULL, NULL, "TAG"),
           fmtmsg(0, "UX:cat", MM_HALT, "not printed", NULL, NULL));
    printf("add: %d %d %d %d %d %d\n", addseverity(6, "SIX"), fmtmsg(MM_PRINT, "UX:cat", 6, "six", NULL, NULL),
           addseverity(6, NULL), fmtmsg(MM_PRINT, "UX:cat", 6, "gone", NULL, NULL), addseverity(4, "X"), addseverity(8, NULL));
}
static void j_fmt_default(void) { fmt_cases(); }
static void j_fmt_verb(void) { setenv("MSGVERB", "text:action", 1); fmt_cases(); }
static void j_fmt_verb_order(void) { setenv("MSGVERB", "tag:severity:label", 1); fmt_cases(); }
static void j_fmt_badverb(void) { setenv("MSGVERB", "text:bogus", 1); fmt_cases(); }
static void j_fmt_sev(void) {
    setenv("SEV_LEVEL", "x,5,FIVE:y,9,NINE:bad:z,3,LOW:w,0x7,SEVEN:v,12", 1);
    printf("before init: %d\n", addseverity(9, "EARLY"));
    for (int sev = 3; sev <= 12; sev++) printf("sev%d=%d ", sev, fmtmsg(MM_PRINT, "UX:cat", sev, "t", NULL, NULL));
    printf("\n");
}

int main(void) {
    program_invocation_name = (char *)"/usr/bin/prog";
    program_invocation_short_name = (char *)"prog";
    texts();
    perrors();
    str("== families\n");
    child("warn", j_warn);
    child("warnx", j_warnx);
    child("err", j_err);
    child("err_null", j_err_null);
    child("errx", j_errx);
    child("error0", j_error0);
    child("error_exit", j_error_exit);
    child("at_line", j_at_line);
    child("hook", j_hook);
    child("orient", j_orient);
    child("wide", j_wide);
    child("fmt_default", j_fmt_default);
    child("fmt_verb", j_fmt_verb);
    child("fmt_verb_order", j_fmt_verb_order);
    child("fmt_badverb", j_fmt_badverb);
    child("fmt_sev", j_fmt_sev);
    flush_out();
    return 0;
}
