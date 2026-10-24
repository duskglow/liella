/* XDR-STREAMS-2 follow-up probe (black box, host glibc): reader call log for record decode,
 * fragment split on encode, getpostn with an fd handle. cc -std=gnu17 -O1 rec2.probe.c */
#include <stdio.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <fcntl.h>
#include <signal.h>
#include <sys/wait.h>

enum xdr_op { XDR_ENCODE = 0, XDR_DECODE = 1, XDR_FREE = 2 };
typedef int bool_t;
typedef struct XDR XDR;
struct xdr_ops {
    bool_t (*x_getlong)(XDR *, long *);
    bool_t (*x_putlong)(XDR *, const long *);
    bool_t (*x_getbytes)(XDR *, char *, unsigned);
    bool_t (*x_putbytes)(XDR *, const char *, unsigned);
    unsigned (*x_getpostn)(const XDR *);
    bool_t (*x_setpostn)(XDR *, unsigned);
    int32_t *(*x_inline)(XDR *, unsigned);
    void (*x_destroy)(XDR *);
    bool_t (*x_getint32)(XDR *, int32_t *);
    bool_t (*x_putint32)(XDR *, const int32_t *);
};
struct XDR { enum xdr_op x_op; struct xdr_ops *x_ops; char *x_public; char *x_private; char *x_base; unsigned x_handy; };
typedef bool_t (*xdrproc_t)(XDR *, void *, ...);
struct xdr_discrim { int value; xdrproc_t proc; };
struct netobj { unsigned n_len; char *n_bytes; };

#define V(n) __asm__(".symver " #n "," #n "@GLIBC_2.2.5")
void xdrmem_create(XDR *, char *, unsigned, enum xdr_op); V(xdrmem_create);
void xdrrec_create(XDR *, unsigned, unsigned, char *, int (*)(char *, char *, int), int (*)(char *, char *, int)); V(xdrrec_create);
bool_t xdrrec_endofrecord(XDR *, bool_t); V(xdrrec_endofrecord);
bool_t xdrrec_skiprecord(XDR *); V(xdrrec_skiprecord);
bool_t xdrrec_eof(XDR *); V(xdrrec_eof);
void xdrstdio_create(XDR *, FILE *, enum xdr_op); V(xdrstdio_create);
void xdr_free(xdrproc_t, char *); V(xdr_free);
bool_t xdr_int(XDR *, int *); V(xdr_int);
bool_t xdr_u_short(XDR *, unsigned short *); V(xdr_u_short);

static unsigned char rd[256]; static int r_len, r_off, r_chunk, r_calls;
static int readit(char *h, char *buf, int len) {
    (void)h; int n = r_len - r_off; r_calls++;
    if (n <= 0) { printf(" [R%d->-1]", len); return -1; }
    if (n > len) n = len; if (n > r_chunk) n = r_chunk;
    memcpy(buf, rd + r_off, n); r_off += n; printf(" [R%d->%d]", len, n); return n;
}
static int writeit(char *h, char *buf, int len) {
    (void)h; printf(" [W%d", len); for (int i = 0; i < len && i < 8; i++) printf("%s%02x", i ? "" : ":", (unsigned char)buf[i]);
    printf("]"); return len;
}
static void feed(const char *hx, int chunk) {
    int n = 0; for (const char *p = hx; *p;) { if (*p == ' ') { p++; continue; } unsigned v; sscanf(p, "%2x", &v); rd[n++] = v; p += 2; }
    r_len = n; r_off = 0; r_chunk = chunk; r_calls = 0;
}
static void dec(const char *name, const char *data, int chunk, int recv) {
    XDR x; feed(data, chunk); xdrrec_create(&x, 0, recv, NULL, readit, writeit); x.x_op = XDR_DECODE;
    printf("dec %s chunk=%d recv=%d:", name, chunk, recv);
    printf(" skip=%d", xdrrec_skiprecord(&x));
    for (int i = 0; i < 2; i++) { int v = 0x5a; int ok = xdr_int(&x, &v); printf(" int=%d/%x", ok, v); }
    printf(" eof=%d", xdrrec_eof(&x));
    printf(" skip=%d", xdrrec_skiprecord(&x));
    { int v = 0x5a; int ok = xdr_int(&x, &v); printf(" next=%d/%x", ok, v); }
    printf(" eof=%d off=%d\n", xdrrec_eof(&x), r_off); x.x_ops->x_destroy(&x);
}
int main(void) {
    setvbuf(stdout, NULL, _IONBF, 0);
    dec("one", "80000008 00000001 00000002 80000004 00000063", 64, 0);
    dec("one", "80000008 00000001 00000002 80000004 00000063", 3, 0);
    dec("one", "80000008 00000001 00000002 80000004 00000063", 64, 8);
    { XDR x; feed("80000004 00000001", 64); xdrrec_create(&x, 0, 0, NULL, readit, writeit); x.x_op = XDR_DECODE; int v = 0x5a;
      printf("dec eof-first:"); int e = xdrrec_eof(&x); printf(" eof=%d", e); int ok = xdr_int(&x, &v); printf(" int=%d/%x", ok, v);
      printf(" eof=%d", xdrrec_eof(&x)); printf(" skip=%d", xdrrec_skiprecord(&x)); ok = xdr_int(&x, &v); printf(" int=%d/%x\n", ok, v); x.x_ops->x_destroy(&x); }
    { XDR x; feed("80000004 00000001", 64); xdrrec_create(&x, 0, 0, NULL, readit, writeit); x.x_op = XDR_DECODE; int v = 0x5a;
      printf("dec int-first:"); int ok = xdr_int(&x, &v); printf(" int=%d/%x skip=%d", ok, v, xdrrec_skiprecord(&x));
      ok = xdr_int(&x, &v); printf(" int=%d/%x\n", ok, v); x.x_ops->x_destroy(&x); }
    int sizes[] = {0, 1, 24, 50, 100, 101, 104, 200, 4000, 5000};
    for (unsigned i = 0; i < sizeof sizes / sizeof *sizes; i++) {
        XDR x; xdrrec_create(&x, sizes[i], 0, NULL, readit, writeit); x.x_op = XDR_ENCODE;
        printf("enc send=%d n=1100:", sizes[i]); int ok = 1;
        for (int k = 0; k < 1100; k++) { int v = k; ok &= xdr_int(&x, &v); }
        printf(" | ok=%d", ok); printf(" eor=%d\n", xdrrec_endofrecord(&x, 1)); x.x_ops->x_destroy(&x);
    }
    for (int dir = 0; dir < 2; dir++) {
        pid_t p = fork();
        if (p == 0) { alarm(5); int fd = open("/etc/passwd", O_RDONLY); lseek(fd, 100, SEEK_SET);
            XDR x; xdrrec_create(&x, 0, 0, (char *)(intptr_t)fd, readit, writeit); x.x_op = dir ? XDR_DECODE : XDR_ENCODE;
            printf("getpostn fd-handle op=%d: %u\n", dir, x.x_ops->x_getpostn(&x)); _exit(0); }
        int st; waitpid(p, &st, 0); if (!WIFEXITED(st)) printf("getpostn fd-handle op=%d: child signal %d\n", dir, WTERMSIG(st));
    }
    return 0;
}
