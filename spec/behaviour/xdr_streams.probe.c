/* Black-box probe of the XDR record and stdio streams and the composite filters (spec §29.1,
 * RFC 4506, RFC 5531 §11 record marking). glibc 2.39 installs no <rpc/xdr.h>; the declarations
 * are the historical public header's shapes, every name bound at its compat version with .symver.
 * One line per case: the result, positions, and the bytes each writeit call received or the
 * values read. Build: cc -std=gnu17 -O1 xdr_streams.probe.c -o xdr_streams_probe; no arguments. */
#include <stdio.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

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
bool_t xdr_hyper(XDR *, long long *); V(xdr_hyper);
bool_t xdr_opaque(XDR *, char *, unsigned); V(xdr_opaque);
bool_t xdr_wrapstring(XDR *, char **); V(xdr_wrapstring);
bool_t xdr_array(XDR *, char **, unsigned *, unsigned, unsigned, xdrproc_t); V(xdr_array);
bool_t xdr_vector(XDR *, char *, unsigned, unsigned, xdrproc_t); V(xdr_vector);
bool_t xdr_union(XDR *, int *, char *, const struct xdr_discrim *, xdrproc_t); V(xdr_union);
bool_t xdr_reference(XDR *, char **, unsigned, xdrproc_t); V(xdr_reference);
bool_t xdr_pointer(XDR *, char **, unsigned, xdrproc_t); V(xdr_pointer);
unsigned long xdr_sizeof(xdrproc_t, void *); V(xdr_sizeof);
bool_t xdr_netobj(XDR *, struct netobj *); V(xdr_netobj);

static void hex(const unsigned char *p, size_t n) { for (size_t i = 0; i < n; i++) printf("%02x", p[i]); }
#define P(f) ((xdrproc_t)(f))
#define POS(x) ((x)->x_ops->x_getpostn(x))

/* writeit: prints each call's bytes; a negative w_fail makes the call fail. */
static int w_fail;
static int writeit(char *h, char *buf, int len) {
    printf(" w%d=", len); hex((unsigned char *)buf, len);
    (void)h; return w_fail ? -1 : len;
}
/* readit: serves r_data in chunks of at most r_chunk bytes; -1 at the end (as a TCP reader
 * does: a reader returning 0 at the end makes the stream retry without end). */
static const unsigned char *r_data; static int r_len, r_off, r_chunk;
static int readit(char *h, char *buf, int len) {
    (void)h; int n = r_len - r_off; if (n <= 0) return -1; if (n > len) n = len; if (n > r_chunk) n = r_chunk;
    memcpy(buf, r_data + r_off, n); r_off += n; return n;
}
static void feed(const char *hx, int chunk) {
    static unsigned char d[256]; int n = 0;
    for (; hx[0] && hx[1]; hx += 2) { while (*hx == ' ') hx++; sscanf(hx, "%2hhx", &d[n++]); }
    r_data = d; r_len = n; r_off = 0; r_chunk = chunk;
}

static void rec_enc(unsigned sendsz, int nints, int sendnow) {
    XDR x; xdrrec_create(&x, sendsz, 0, NULL, readit, writeit); x.x_op = XDR_ENCODE;
    printf("rec enc send=%u n=%d now=%d pos0=%u:", sendsz, nints, sendnow, POS(&x));
    int ok = 1; for (int i = 1; i <= nints; i++) ok &= xdr_int(&x, &i);
    unsigned p1 = POS(&x); int e = xdrrec_endofrecord(&x, sendnow);
    printf(" | ok=%d pos=%u eor=%d pos2=%u\n", ok, p1, e, POS(&x));
    x.x_ops->x_destroy(&x);
}

static void rec_dec(const char *name, const char *data, int chunk, int nints) {
    XDR x; feed(data, chunk); xdrrec_create(&x, 0, 0, NULL, readit, writeit); x.x_op = XDR_DECODE;
    printf("rec dec %-14s chunk=%d: skip0=%d", name, chunk, xdrrec_skiprecord(&x));
    for (int i = 0; i < nints; i++) { int v = 0x5a5a; int ok = xdr_int(&x, &v); printf(" %d/%x", ok, v); }
    printf(" eof=%d skip=%d", xdrrec_eof(&x), xdrrec_skiprecord(&x));
    { int v = 0x5a5a; int ok = xdr_int(&x, &v); printf(" next=%d/%x eof=%d", ok, v, xdrrec_eof(&x)); }
    printf("\n"); x.x_ops->x_destroy(&x);
}

static int int_pair(XDR *x, int *p) { return xdr_int(x, p) && xdr_int(x, p + 1); }
static int first_int(XDR *x, int *p) { return xdr_int(x, p); }

static void mem(XDR *x, unsigned char *b, unsigned n, enum xdr_op op) { memset(b, 0xaa, n); xdrmem_create(x, (char *)b, n, op); }

int main(void) {
    unsigned char b[64]; XDR x;

    /* --- record stream: encode --- */
    rec_enc(0, 2, 1);
    rec_enc(0, 2, 0);
    rec_enc(1, 3, 1);
    rec_enc(8, 3, 1);
    rec_enc(12, 5, 1);
    rec_enc(13, 5, 1);
    rec_enc(100, 0, 1);
    { XDR r; xdrrec_create(&r, 12, 0, NULL, readit, writeit); r.x_op = XDR_ENCODE;
      char o[10] = "abcdefghij"; printf("rec enc opaque10 send=12:"); int ok = xdr_opaque(&r, o, 10);
      printf(" | ok=%d eor=%d\n", ok, xdrrec_endofrecord(&r, 1)); r.x_ops->x_destroy(&r); }
    { XDR r; xdrrec_create(&r, 0, 0, NULL, readit, writeit); r.x_op = XDR_ENCODE;
      int32_t *ip = r.x_ops->x_inline(&r, 8); int32_t *big = r.x_ops->x_inline(&r, 100000);
      printf("rec enc inline8=%s inline100000=%s setpostn0=%d\n", ip ? "ptr" : "NULL", big ? "ptr" : "NULL", r.x_ops->x_setpostn(&r, 0));
      r.x_ops->x_destroy(&r); }
    { XDR r; w_fail = 1; xdrrec_create(&r, 0, 0, NULL, readit, writeit); r.x_op = XDR_ENCODE; int v = 7;
      printf("rec enc writefail:"); int ok = xdr_int(&r, &v); int e = xdrrec_endofrecord(&r, 1);
      printf(" | ok=%d eor=%d\n", ok, e); r.x_ops->x_destroy(&r); w_fail = 0; }
    { XDR r; xdrrec_create(&r, 8, 0, NULL, readit, writeit); r.x_op = XDR_ENCODE; int v = 7;
      printf("rec enc two records send=8:"); xdr_int(&r, &v); int e1 = xdrrec_endofrecord(&r, 0); v = 8; xdr_int(&r, &v);
      int e2 = xdrrec_endofrecord(&r, 1); printf(" | eor=%d,%d\n", e1, e2); r.x_ops->x_destroy(&r); }

    /* --- record stream: decode --- */
    { XDR r; feed("80000004 00000001", 64); xdrrec_create(&r, 0, 0, NULL, readit, writeit); r.x_op = XDR_DECODE; int v = 0x5a;
      int ok = xdr_int(&r, &v); printf("rec dec no-skip first=%d/%x pos=%u\n", ok, v, POS(&r)); r.x_ops->x_destroy(&r); }
    { XDR r; feed("80000004 00000001", 64); xdrrec_create(&r, 0, 0, NULL, readit, writeit); r.x_op = XDR_DECODE; int v = 0x5a;
      int e = xdrrec_eof(&r); int ok = xdr_int(&r, &v); int e2 = xdrrec_eof(&r); int sk = xdrrec_skiprecord(&r); int ok2 = xdr_int(&r, &v);
      printf("rec dec eof-first eof=%d int=%d/%x eof=%d skip=%d int=%d\n", e, ok, v, e2, sk, ok2); r.x_ops->x_destroy(&r); }
    rec_dec("one", "80000008 00000001 00000002 80000004 00000063", 64, 2);
    rec_dec("one-chunk3", "80000008 00000001 00000002 80000004 00000063", 3, 2);
    rec_dec("two-frags", "00000004 00000001 80000004 00000002 80000004 00000063", 64, 2);
    rec_dec("past-end", "80000004 00000001 80000004 00000063", 64, 2);
    rec_dec("skip-unread", "80000008 00000001 00000002 80000004 00000063", 64, 0);
    rec_dec("empty-last", "80000000 80000004 00000063", 64, 1);
    rec_dec("empty-frag", "00000000 80000004 00000001 80000004 00000063", 64, 1);
    rec_dec("short-data", "80000008 00000001", 64, 2);
    rec_dec("no-data", "", 64, 1);
    rec_dec("half-header", "8000", 64, 1);
    { XDR r; feed("0000000361626380000003646566 80000004 00000063", 64); xdrrec_create(&r, 0, 0, NULL, readit, writeit); r.x_op = XDR_DECODE; xdrrec_skiprecord(&r);
      char o[8] = {0}; int ok = xdr_opaque(&r, o, 6); printf("rec dec opaque6 over frags: ok=%d bytes=", ok); hex((unsigned char *)o, 8);
      int v = 0; int ok2 = xdr_int(&r, &v); printf(" then=%d/%x\n", ok2, v); r.x_ops->x_destroy(&r); }
    { XDR r; feed("80000010 00000001 00000002 00000003 00000004", 64); xdrrec_create(&r, 0, 0, NULL, readit, writeit); r.x_op = XDR_DECODE; xdrrec_skiprecord(&r);
      int v; xdr_int(&r, &v); unsigned p = POS(&r); int32_t *ip = r.x_ops->x_inline(&r, 8);
      printf("rec dec pos-after-int=%u inline8=%s", p, ip ? "ptr" : "NULL"); if (ip) printf(" [%08x %08x]", (unsigned)__builtin_bswap32(ip[0]), (unsigned)__builtin_bswap32(ip[1]));
      int32_t *big = r.x_ops->x_inline(&r, 8); printf(" inline-past=%s setpostn0=%d\n", big ? "ptr" : "NULL", r.x_ops->x_setpostn(&r, 0)); r.x_ops->x_destroy(&r); }

    /* --- stdio stream --- */
    { FILE *f = tmpfile(); XDR s; xdrstdio_create(&s, f, XDR_ENCODE);
      int v = -2; long long h = 0x0102030405060708LL; char *str = "hello";
      int ok = xdr_int(&s, &v); unsigned p1 = POS(&s); ok &= xdr_hyper(&s, &h); ok &= xdr_wrapstring(&s, &str);
      unsigned p2 = POS(&s); int32_t *ip = s.x_ops->x_inline(&s, 4);
      int sp = s.x_ops->x_setpostn(&s, 0); unsigned p3 = POS(&s); v = 9; int ok2 = xdr_int(&s, &v);
      s.x_ops->x_destroy(&s); rewind(f); unsigned char fb[64]; size_t n = fread(fb, 1, sizeof fb, f);
      printf("stdio enc ok=%d pos=%u,%u inline=%s setpostn0=%d pos=%u ok2=%d file(%zu)=", ok, p1, p2, ip ? "ptr" : "NULL", sp, p3, ok2, n); hex(fb, n); printf("\n");
      rewind(f); xdrstdio_create(&s, f, XDR_DECODE); int a = 0; long long hh = 0; char *rs = NULL; int extra = 0x5a;
      int d1 = xdr_int(&s, &a), d2 = xdr_hyper(&s, &hh), d3 = xdr_wrapstring(&s, &rs), d4 = xdr_int(&s, &extra);
      printf("stdio dec %d:%d %d:%llx %d:%s %d:%x pos=%u\n", d1, a, d2, hh, d3, rs ? rs : "(null)", d4, extra, POS(&s));
      s.x_ops->x_destroy(&s); free(rs); fclose(f); }
    { FILE *f = tmpfile(); XDR s; xdrstdio_create(&s, f, XDR_DECODE); int v = 0x5a; char o[3];
      printf("stdio dec empty int=%d/%x opaque3=%d pos=%u\n", xdr_int(&s, &v), v, xdr_opaque(&s, o, 3), POS(&s)); s.x_ops->x_destroy(&s); fclose(f); }
    { FILE *f = tmpfile(); fwrite("\0\0\0\1\0\0", 1, 6, f); rewind(f); XDR s; xdrstdio_create(&s, f, XDR_DECODE); int a = 0, b2 = 0x5a;
      int r1 = xdr_int(&s, &a), r2 = xdr_int(&s, &b2); printf("stdio dec short %d:%d %d:%x pos=%u\n", r1, a, r2, b2, POS(&s)); s.x_ops->x_destroy(&s); fclose(f); }

    /* --- arrays and vectors --- */
    { int a[3] = {1, -1, 0x7fffffff}; char *ap = (char *)a; unsigned n = 3;
      mem(&x, b, 24, XDR_ENCODE); int ok = xdr_array(&x, &ap, &n, 3, sizeof(int), P(xdr_int)); printf("array enc 3/max3 ok=%d pos=%u bytes=", ok, POS(&x)); hex(b, 20); printf("\n");
      mem(&x, b, 24, XDR_ENCODE); ok = xdr_array(&x, &ap, &n, 2, sizeof(int), P(xdr_int)); printf("array enc 3/max2 ok=%d pos=%u bytes=", ok, POS(&x)); hex(b, 8); printf("\n");
      mem(&x, b, 12, XDR_ENCODE); ok = xdr_array(&x, &ap, &n, 3, sizeof(int), P(xdr_int)); printf("array enc 3/room12 ok=%d pos=%u bytes=", ok, POS(&x)); hex(b, 12); printf("\n"); }
    { unsigned char w[] = {0,0,0,2, 0,0,0,5, 0xff,0xff,0xff,0xfe}; char *ap = NULL; unsigned n = 77;
      xdrmem_create(&x, (char *)w, sizeof w, XDR_DECODE); int ok = xdr_array(&x, &ap, &n, 9, sizeof(int), P(xdr_int));
      printf("array dec null ok=%d n=%u vals=%d,%d pos=%u\n", ok, n, ap ? ((int *)ap)[0] : 0, ap ? ((int *)ap)[1] : 0, POS(&x));
      x.x_op = XDR_FREE; ok = xdr_array(&x, &ap, &n, 9, sizeof(int), P(xdr_int)); printf("array free ok=%d -> %s n=%u\n", ok, ap ? "ptr" : "NULL", n);
      unsigned max = 1; ap = NULL; n = 77; xdrmem_create(&x, (char *)w, sizeof w, XDR_DECODE); ok = xdr_array(&x, &ap, &n, max, sizeof(int), P(xdr_int));
      printf("array dec 2/max1 ok=%d n=%u ap=%s pos=%u\n", ok, n, ap ? "ptr" : "NULL", POS(&x));
      ap = NULL; n = 77; xdrmem_create(&x, (char *)w, 8, XDR_DECODE); ok = xdr_array(&x, &ap, &n, 9, sizeof(int), P(xdr_int));
      printf("array dec truncated ok=%d n=%u ap=%s v0=%d pos=%u\n", ok, n, ap ? "ptr" : "NULL", ap ? ((int *)ap)[0] : 0, POS(&x)); free(ap);
      unsigned char z[] = {0,0,0,0}; ap = NULL; n = 77; xdrmem_create(&x, (char *)z, 4, XDR_DECODE); ok = xdr_array(&x, &ap, &n, 9, sizeof(int), P(xdr_int));
      printf("array dec zero ok=%d n=%u ap=%s\n", ok, n, ap ? "ptr" : "NULL"); free(ap);
      unsigned char hg[] = {0x40,0,0,1}; ap = NULL; n = 77; xdrmem_create(&x, (char *)hg, 4, XDR_DECODE); ok = xdr_array(&x, &ap, &n, ~0u, sizeof(int), P(xdr_int));
      printf("array dec 0x40000001x4 ok=%d n=%u ap=%s\n", ok, n, ap ? "ptr" : "NULL"); free(ap);
      int keep[2] = {0, 0}; ap = (char *)keep; n = 77; xdrmem_create(&x, (char *)w, sizeof w, XDR_DECODE); ok = xdr_array(&x, &ap, &n, 9, sizeof(int), P(xdr_int));
      printf("array dec into caller ok=%d same=%d vals=%d,%d\n", ok, ap == (char *)keep, keep[0], keep[1]); }
    { unsigned short v[3] = {1, 0xffff, 0x1234}; mem(&x, b, 16, XDR_ENCODE); int ok = xdr_vector(&x, (char *)v, 3, sizeof(short), P(xdr_u_short));
      printf("vector enc ok=%d pos=%u bytes=", ok, POS(&x)); hex(b, 16); printf("\n");
      unsigned short r[3] = {0}; xdrmem_create(&x, (char *)b, 12, XDR_DECODE); ok = xdr_vector(&x, (char *)r, 3, sizeof(short), P(xdr_u_short));
      printf("vector dec ok=%d vals=%x,%x,%x\n", ok, r[0], r[1], r[2]);
      unsigned short q[3] = {7, 7, 7}; xdrmem_create(&x, (char *)b, 8, XDR_DECODE); ok = xdr_vector(&x, (char *)q, 3, sizeof(short), P(xdr_u_short));
      printf("vector dec short ok=%d vals=%x,%x,%x pos=%u\n", ok, q[0], q[1], q[2], POS(&x));
      mem(&x, b, 16, XDR_ENCODE); ok = xdr_vector(&x, (char *)v, 0, sizeof(short), P(xdr_u_short)); printf("vector enc 0 ok=%d pos=%u\n", ok, POS(&x)); }

    /* --- unions --- */
    { static const struct xdr_discrim ch[] = {{1, P(xdr_int)}, {2, P(xdr_hyper)}, {0, NULL}};
      int d = 1; long long u = 0x11223344; mem(&x, b, 24, XDR_ENCODE); int ok = xdr_union(&x, &d, (char *)&u, ch, NULL);
      printf("union enc d=1 ok=%d pos=%u bytes=", ok, POS(&x)); hex(b, 12); printf("\n");
      d = 2; u = -2; mem(&x, b, 24, XDR_ENCODE); ok = xdr_union(&x, &d, (char *)&u, ch, NULL); printf("union enc d=2 ok=%d pos=%u bytes=", ok, POS(&x)); hex(b, 12); printf("\n");
      d = 3; mem(&x, b, 24, XDR_ENCODE); ok = xdr_union(&x, &d, (char *)&u, ch, NULL); printf("union enc d=3 nodefault ok=%d pos=%u bytes=", ok, POS(&x)); hex(b, 8); printf("\n");
      d = 3; u = 5; mem(&x, b, 24, XDR_ENCODE); ok = xdr_union(&x, &d, (char *)&u, ch, P(xdr_int)); printf("union enc d=3 default ok=%d pos=%u bytes=", ok, POS(&x)); hex(b, 12); printf("\n");
      d = 0; mem(&x, b, 24, XDR_ENCODE); ok = xdr_union(&x, &d, (char *)&u, ch, NULL); printf("union enc d=0 ok=%d pos=%u\n", ok, POS(&x));
      unsigned char w[] = {0,0,0,2, 0,0,0,0, 0,0,0,9}; d = 77; u = 0; xdrmem_create(&x, (char *)w, 12, XDR_DECODE); ok = xdr_union(&x, &d, (char *)&u, ch, NULL);
      printf("union dec ok=%d d=%d u=%llx\n", ok, d, u);
      unsigned char w2[] = {0,0,0,1}; d = 77; xdrmem_create(&x, (char *)w2, 4, XDR_DECODE); ok = xdr_union(&x, &d, (char *)&u, ch, NULL);
      printf("union dec arm-missing ok=%d d=%d pos=%u\n", ok, d, POS(&x)); }

    /* --- reference and pointer --- */
    { unsigned char w[] = {0,0,0,5, 0,0,0,6}; char *rp = NULL; xdrmem_create(&x, (char *)w, 8, XDR_DECODE);
      int ok = xdr_reference(&x, &rp, 2 * sizeof(int), P(first_int)); printf("reference dec null ok=%d vals=%d,%d pos=%u\n", ok, rp ? ((int *)rp)[0] : -1, rp ? ((int *)rp)[1] : -1, POS(&x));
      xdr_free(P(int_pair), rp); /* proc frees nothing for ints */
      char *rp2 = rp; xdrmem_create(&x, (char *)w, 8, XDR_DECODE); x.x_op = XDR_FREE; ok = xdr_reference(&x, &rp2, 2 * sizeof(int), P(first_int));
      printf("reference free ok=%d -> %s\n", ok, rp2 ? "ptr" : "NULL");
      /* Encoding through a NULL reference is the caller's error (xdr_pointer handles NULL): not probed. */
      xdrmem_create(&x, (char *)w, 4, XDR_DECODE); rp = NULL; ok = xdr_reference(&x, &rp, 8, P(int_pair)); printf("reference dec short ok=%d rp=%s\n", ok, rp ? "ptr" : "NULL"); free(rp); }
    { int v = 0x42; char *pp = (char *)&v; mem(&x, b, 16, XDR_ENCODE); int ok = xdr_pointer(&x, &pp, sizeof(int), P(xdr_int));
      printf("pointer enc ok=%d pos=%u bytes=", ok, POS(&x)); hex(b, 12); printf("\n");
      pp = NULL; mem(&x, b, 16, XDR_ENCODE); ok = xdr_pointer(&x, &pp, sizeof(int), P(xdr_int)); printf("pointer enc null ok=%d pos=%u bytes=", ok, POS(&x)); hex(b, 8); printf("\n");
      unsigned char w[] = {0,0,0,1, 0,0,0,7}; pp = NULL; xdrmem_create(&x, (char *)w, 8, XDR_DECODE); ok = xdr_pointer(&x, &pp, sizeof(int), P(xdr_int));
      printf("pointer dec ok=%d val=%d\n", ok, pp ? *(int *)pp : -1); free(pp);
      unsigned char w0[] = {0,0,0,0}; pp = (char *)&v; xdrmem_create(&x, (char *)w0, 4, XDR_DECODE); ok = xdr_pointer(&x, &pp, sizeof(int), P(xdr_int));
      printf("pointer dec 0 ok=%d -> %s\n", ok, pp ? "ptr" : "NULL");
      unsigned char w5[] = {0,0,0,5, 0,0,0,7}; pp = NULL; xdrmem_create(&x, (char *)w5, 8, XDR_DECODE); ok = xdr_pointer(&x, &pp, sizeof(int), P(xdr_int));
      printf("pointer dec 5 ok=%d val=%d\n", ok, pp ? *(int *)pp : -1); free(pp); }

    /* --- sizeof and netobj --- */
    { int v = 1; long long h = 1; char *s = "abcde"; char *e = ""; int a[3] = {0};
      printf("sizeof int=%lu hyper=%lu str5=%lu str0=%lu\n", xdr_sizeof(P(xdr_int), &v), xdr_sizeof(P(xdr_hyper), &h), xdr_sizeof(P(xdr_wrapstring), &s), xdr_sizeof(P(xdr_wrapstring), &e));
      printf("sizeof pair=%lu\n", xdr_sizeof(P(int_pair), a)); }
    { struct netobj o = {3, "xyz"}; mem(&x, b, 16, XDR_ENCODE); int ok = xdr_netobj(&x, &o); printf("netobj enc ok=%d pos=%u bytes=", ok, POS(&x)); hex(b, 8); printf("\n");
      static char big[1025]; struct netobj o2 = {1025, big}; mem(&x, b, 16, XDR_ENCODE); ok = xdr_netobj(&x, &o2); printf("netobj enc 1025 ok=%d pos=%u\n", ok, POS(&x));
      unsigned char hb[1100] = {0,0,4,0}; struct netobj o3 = {0, NULL}; xdrmem_create(&x, (char *)hb, sizeof hb, XDR_DECODE); ok = xdr_netobj(&x, &o3);
      printf("netobj dec 1024 ok=%d len=%u ptr=%s pos=%u\n", ok, o3.n_len, o3.n_bytes ? "ptr" : "NULL", POS(&x)); free(o3.n_bytes); }
    return 0;
}
