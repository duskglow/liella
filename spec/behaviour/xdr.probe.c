/* Black-box probe of the XDR primitives and the memory stream (spec §29.1, RFC 4506).
 * glibc 2.39 installs no <rpc/xdr.h>; the declarations below are the historical public header's
 * shapes, and every name is bound at its compat version with .symver. Prints one line per case:
 * the result, the stream position, the x_handy field and the bytes written or the value read.
 * Build: cc -std=gnu17 -O1 xdr.probe.c -o xdr_probe; run with no arguments. */
#include <stdio.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <math.h>

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

#define V(n) __asm__(".symver " #n "," #n "@GLIBC_2.2.5")
void xdrmem_create(XDR *, char *, unsigned, enum xdr_op); V(xdrmem_create);
void xdr_free(xdrproc_t, char *); V(xdr_free);
bool_t xdr_void(void); V(xdr_void);
bool_t xdr_int(XDR *, int *); V(xdr_int);
bool_t xdr_u_int(XDR *, unsigned *); V(xdr_u_int);
bool_t xdr_long(XDR *, long *); V(xdr_long);
bool_t xdr_u_long(XDR *, unsigned long *); V(xdr_u_long);
bool_t xdr_short(XDR *, short *); V(xdr_short);
bool_t xdr_u_short(XDR *, unsigned short *); V(xdr_u_short);
bool_t xdr_char(XDR *, char *); V(xdr_char);
bool_t xdr_u_char(XDR *, unsigned char *); V(xdr_u_char);
bool_t xdr_bool(XDR *, bool_t *); V(xdr_bool);
bool_t xdr_enum(XDR *, int *); V(xdr_enum);
bool_t xdr_hyper(XDR *, long long *); V(xdr_hyper);
bool_t xdr_u_hyper(XDR *, unsigned long long *); V(xdr_u_hyper);
bool_t xdr_longlong_t(XDR *, long long *); V(xdr_longlong_t);
bool_t xdr_u_longlong_t(XDR *, unsigned long long *); V(xdr_u_longlong_t);
bool_t xdr_quad_t(XDR *, int64_t *); __asm__(".symver xdr_quad_t,xdr_quad_t@GLIBC_2.3.4");
bool_t xdr_u_quad_t(XDR *, uint64_t *); __asm__(".symver xdr_u_quad_t,xdr_u_quad_t@GLIBC_2.3.4");
bool_t xdr_int8_t(XDR *, int8_t *); V(xdr_int8_t);
bool_t xdr_uint8_t(XDR *, uint8_t *); V(xdr_uint8_t);
bool_t xdr_int16_t(XDR *, int16_t *); V(xdr_int16_t);
bool_t xdr_uint16_t(XDR *, uint16_t *); V(xdr_uint16_t);
bool_t xdr_int32_t(XDR *, int32_t *); V(xdr_int32_t);
bool_t xdr_uint32_t(XDR *, uint32_t *); V(xdr_uint32_t);
bool_t xdr_int64_t(XDR *, int64_t *); V(xdr_int64_t);
bool_t xdr_uint64_t(XDR *, uint64_t *); V(xdr_uint64_t);
bool_t xdr_float(XDR *, float *); V(xdr_float);
bool_t xdr_double(XDR *, double *); V(xdr_double);
bool_t xdr_opaque(XDR *, char *, unsigned); V(xdr_opaque);
bool_t xdr_bytes(XDR *, char **, unsigned *, unsigned); V(xdr_bytes);
bool_t xdr_string(XDR *, char **, unsigned); V(xdr_string);
bool_t xdr_wrapstring(XDR *, char **); V(xdr_wrapstring);

static unsigned char buf[64];
static XDR x;
static void enc(unsigned size) { memset(buf, 0xAA, sizeof buf); xdrmem_create(&x, (char *)buf, size, XDR_ENCODE); }
static void dec(const char *hex) {
    unsigned n = 0;
    memset(buf, 0xAA, sizeof buf);
    for (; hex[0] && hex[1]; hex += 2) { unsigned v; sscanf(hex, "%2x", &v); buf[n++] = (unsigned char)v; }
    xdrmem_create(&x, (char *)buf, n, XDR_DECODE);
}
static void show(const char *what, int ok) {
    unsigned pos = x.x_ops->x_getpostn(&x);
    printf("%-34s ok=%d pos=%u handy=%u", what, ok != 0, pos, x.x_handy);
    if (x.x_op == XDR_ENCODE) { printf(" bytes="); for (unsigned i = 0; i < pos + 4 && i < 24; i++) printf("%02x", buf[i]); }
}
#define E(T, fn, val, size) do { T v = (val); enc(size); int ok = fn(&x, &v); show(#fn " enc " #val, ok); printf("\n"); } while (0)
#define D(T, fn, hex, fmt) do { T v = (T)0x5a5a5a5a5a5a5a5aULL; dec(hex); int ok = fn(&x, &v); show(#fn " dec " hex, ok); printf(" v=" fmt "\n", v); } while (0)

int main(void) {
    printf("sizeof(XDR)=%zu xdr_void=%d\n", sizeof(XDR), xdr_void());
    E(int, xdr_int, 0, 64); E(int, xdr_int, -1, 64); E(int, xdr_int, 0x7fffffff, 64); E(int, xdr_int, (-0x7fffffff - 1), 64);
    E(unsigned, xdr_u_int, 0xffffffffu, 64);
    E(long, xdr_long, -1L, 64); E(long, xdr_long, 0x7fffffffL, 64); E(long, xdr_long, 0x80000000L, 64);
    E(long, xdr_long, 0x100000001L, 64); E(long, xdr_long, (-0x80000001L), 64);
    E(unsigned long, xdr_u_long, 0xffffffffUL, 64); E(unsigned long, xdr_u_long, 0x100000000UL, 64); E(unsigned long, xdr_u_long, (~0UL), 64);
    E(short, xdr_short, -2, 64); E(unsigned short, xdr_u_short, 65535, 64);
    E(char, xdr_char, 'A', 64); E(char, xdr_char, -1, 64); E(unsigned char, xdr_u_char, 255, 64);
    E(bool_t, xdr_bool, 0, 64); E(bool_t, xdr_bool, 1, 64); E(bool_t, xdr_bool, 2, 64); E(bool_t, xdr_bool, -1, 64);
    E(int, xdr_enum, 3, 64); E(int, xdr_enum, -1, 64);
    E(long long, xdr_hyper, 0x0102030405060708LL, 64); E(long long, xdr_hyper, -2LL, 64);
    E(unsigned long long, xdr_u_hyper, 0xfedcba9876543210ULL, 64); E(long long, xdr_longlong_t, -3LL, 64);
    E(unsigned long long, xdr_u_longlong_t, 5ULL, 64); E(int64_t, xdr_quad_t, -4LL, 64); E(uint64_t, xdr_u_quad_t, 6ULL, 64);
    E(int8_t, xdr_int8_t, -1, 64); E(uint8_t, xdr_uint8_t, 255, 64); E(int16_t, xdr_int16_t, -1, 64);
    E(uint16_t, xdr_uint16_t, 65535, 64); E(int32_t, xdr_int32_t, -5, 64); E(uint32_t, xdr_uint32_t, 0xdeadbeefu, 64);
    E(int64_t, xdr_int64_t, -6LL, 64); E(uint64_t, xdr_uint64_t, 0x1122334455667788ULL, 64);
    E(float, xdr_float, 1.5f, 64); E(float, xdr_float, -0.0f, 64); E(float, xdr_float, NAN, 64);
    E(double, xdr_double, 1.5, 64); E(double, xdr_double, -2.25, 64);
    /* short streams */
    E(int, xdr_int, 7, 3); E(long long, xdr_hyper, 7, 4); E(long long, xdr_hyper, 7, 7); E(double, xdr_double, 1.0, 6);
    { int a = 1; enc(6); int ok1 = xdr_int(&x, &a); int ok2 = xdr_int(&x, &a); show("xdr_int twice size6", ok1 * 10 + ok2); printf(" ok1=%d ok2=%d\n", ok1, ok2); }
    { char c = 9; enc(5); int ok = xdr_char(&x, &c); ok = ok * 10 + xdr_char(&x, &c); show("xdr_char twice size5", ok); printf(" r=%d\n", ok); }
    /* opaque */
    { enc(64); int ok = xdr_opaque(&x, "abcde", 5); show("xdr_opaque enc abcde", ok); printf("\n"); }
    { enc(64); int ok = xdr_opaque(&x, "", 0); show("xdr_opaque enc empty", ok); printf("\n"); }
    { enc(6); int ok = xdr_opaque(&x, "abcde", 5); show("xdr_opaque enc abcde size6", ok); printf("\n"); }
    { enc(5); int ok = xdr_opaque(&x, "abcde", 5); show("xdr_opaque enc abcde size5", ok); printf("\n"); }
    { char o[8] = {0}; dec("6162636465ffffff"); int ok = xdr_opaque(&x, o, 5); show("xdr_opaque dec nonzero pad", ok); printf(" o=%.5s\n", o); }
    { char o[8] = {0}; dec("6162636465"); int ok = xdr_opaque(&x, o, 5); show("xdr_opaque dec no pad", ok); printf(" o=%.5s\n", o); }
    /* bytes */
    { char *p = "xyz"; unsigned n = 3; enc(64); int ok = xdr_bytes(&x, &p, &n, 10); show("xdr_bytes enc xyz max10", ok); printf("\n"); }
    { char *p = "xyz"; unsigned n = 3; enc(64); int ok = xdr_bytes(&x, &p, &n, 2); show("xdr_bytes enc xyz max2", ok); printf("\n"); }
    { char *p = NULL; unsigned n = 77; dec("0000000378797a00"); int ok = xdr_bytes(&x, &p, &n, 10); show("xdr_bytes dec xyz alloc", ok);
      printf(" n=%u p=%s data=%.3s\n", n, p ? "set" : "null", p ? p : "-"); x.x_op = XDR_FREE; ok = xdr_bytes(&x, &p, &n, 10); printf("  free ok=%d p=%s\n", ok, p ? "set" : "null"); }
    { char *p = NULL; unsigned n = 77; dec("00000000"); int ok = xdr_bytes(&x, &p, &n, 10); show("xdr_bytes dec empty", ok); printf(" n=%u p=%s\n", n, p ? "set" : "null"); free(p); }
    { char *p = NULL; unsigned n = 77; dec("0000000b78797a00"); int ok = xdr_bytes(&x, &p, &n, 10); show("xdr_bytes dec len11 max10", ok); printf(" n=%u p=%s\n", n, p ? "set" : "null"); }
    { char *p = NULL; unsigned n = 77; dec("0000000878797a00"); int ok = xdr_bytes(&x, &p, &n, 10); show("xdr_bytes dec len8 short", ok); printf(" n=%u p=%s\n", n, p ? "set" : "null"); }
    { char b[4] = "QQQQ"; char *p = b; unsigned n = 77; dec("0000000278790000"); int ok = xdr_bytes(&x, &p, &n, 10); show("xdr_bytes dec into caller", ok); printf(" n=%u same=%d b=%.4s\n", n, p == b, b); }
    { char *p = NULL; unsigned n = 0; x.x_op = XDR_FREE; int ok = xdr_bytes(&x, &p, &n, 10); printf("xdr_bytes free null ok=%d\n", ok); }
    /* strings */
    { char *s = "hello"; enc(64); int ok = xdr_string(&x, &s, 10); show("xdr_string enc hello max10", ok); printf("\n"); }
    { char *s = "hello"; enc(64); int ok = xdr_string(&x, &s, 5); show("xdr_string enc hello max5", ok); printf("\n"); }
    { char *s = "hello"; enc(64); int ok = xdr_string(&x, &s, 4); show("xdr_string enc hello max4", ok); printf("\n"); }
    { char *s = ""; enc(64); int ok = xdr_string(&x, &s, 4); show("xdr_string enc empty", ok); printf("\n"); }
    { char *s = NULL; enc(64); int ok = xdr_string(&x, &s, 4); show("xdr_string enc null", ok); printf("\n"); }
    { char *s = "hi"; enc(64); int ok = xdr_wrapstring(&x, &s); show("xdr_wrapstring enc hi", ok); printf("\n"); }
    { char *s = NULL; dec("0000000568656c6c6f000000"); int ok = xdr_string(&x, &s, 10); show("xdr_string dec hello", ok);
      printf(" s=%s len=%zu\n", s ? s : "(null)", s ? strlen(s) : 0); xdr_free((xdrproc_t)xdr_wrapstring, (char *)&s); printf("  xdr_free -> s=%s\n", s ? "set" : "null"); }
    { char *s = NULL; dec("0000000568656c6c6f000000"); int ok = xdr_string(&x, &s, 4); show("xdr_string dec hello max4", ok); printf(" s=%s\n", s ? "set" : "null"); }
    { char *s = NULL; dec("00000000"); int ok = xdr_string(&x, &s, 4); show("xdr_string dec empty", ok); printf(" s=%s len=%zu\n", s ? "set" : "null", s ? strlen(s) : 99); free(s); }
    { char *s = NULL; dec("0000000568006c6c6f000000"); int ok = xdr_string(&x, &s, 10); show("xdr_string dec embedded nul", ok); printf(" s=%s len=%zu\n", s ? s : "(null)", s ? strlen(s) : 0); free(s); }
    { char *s = NULL; dec("00000005686c"); int ok = xdr_string(&x, &s, 10); show("xdr_string dec truncated", ok); printf(" s=%s\n", s ? "set" : "null"); }
    { char *s = NULL; dec("ffffffff686c"); int ok = xdr_wrapstring(&x, &s); show("xdr_wrapstring dec len ~0", ok); printf(" s=%s\n", s ? "set" : "null"); }
    { char b[8] = "QQQQQQQ"; char *s = b; dec("000000036869690000"); int ok = xdr_string(&x, &s, 10); show("xdr_string dec into caller", ok); printf(" same=%d b=%s\n", s == b, b); }
    { char *s = NULL; x.x_op = XDR_FREE; int ok = xdr_string(&x, &s, 10); printf("xdr_string free null ok=%d\n", ok); }
    /* decodes */
    D(int, xdr_int, "ffffffff", "%d"); D(int, xdr_int, "ffffff", "%d"); D(unsigned, xdr_u_int, "ffffffff", "%u");
    D(long, xdr_long, "ffffffff", "%ld"); D(long, xdr_long, "80000000", "%ld"); D(unsigned long, xdr_u_long, "ffffffff", "%lu");
    D(short, xdr_short, "00012345", "%hd"); D(short, xdr_short, "0000ffff", "%hd"); D(unsigned short, xdr_u_short, "fffe0001", "%hu");
    D(char, xdr_char, "00000141", "%d"); D(unsigned char, xdr_u_char, "000001ff", "%u");
    D(bool_t, xdr_bool, "00000000", "%d"); D(bool_t, xdr_bool, "00000001", "%d"); D(bool_t, xdr_bool, "00000002", "%d"); D(bool_t, xdr_bool, "ffffffff", "%d");
    D(int, xdr_enum, "fffffffe", "%d");
    D(long long, xdr_hyper, "0102030405060708", "%lld"); D(long long, xdr_hyper, "fffffffffffffffe", "%lld"); D(long long, xdr_hyper, "01020304050607", "%lld");
    D(unsigned long long, xdr_u_hyper, "fedcba9876543210", "%llx"); D(long long, xdr_longlong_t, "fffffffffffffffd", "%lld");
    D(unsigned long long, xdr_u_longlong_t, "0000000000000005", "%llu"); D(int64_t, xdr_quad_t, "fffffffffffffffc", "%ld"); D(uint64_t, xdr_u_quad_t, "8000000000000000", "%lx");
    D(int8_t, xdr_int8_t, "000001ff", "%d"); D(uint8_t, xdr_uint8_t, "ffffff80", "%u"); D(int16_t, xdr_int16_t, "00018000", "%d");
    D(uint16_t, xdr_uint16_t, "ffff8001", "%u"); D(int32_t, xdr_int32_t, "fffffffb", "%d"); D(uint32_t, xdr_uint32_t, "deadbeef", "%x");
    D(int64_t, xdr_int64_t, "fffffffffffffffa", "%ld"); D(uint64_t, xdr_uint64_t, "1122334455667788", "%lx");
    D(float, xdr_float, "3fc00000", "%a"); D(float, xdr_float, "ff800000", "%a"); D(double, xdr_double, "3ff8000000000000", "%a");
    D(double, xdr_double, "c002000000000000", "%a"); D(double, xdr_double, "3ff80000000000", "%a");
    /* free op and bad op on primitives */
    { int v = 5; x.x_op = XDR_FREE; printf("xdr_int XDR_FREE ok=%d v=%d\n", xdr_int(&x, &v), v); }
    { long long v = 5; x.x_op = XDR_FREE; printf("xdr_hyper XDR_FREE ok=%d\n", xdr_hyper(&x, &v)); }
    { double v = 5; x.x_op = XDR_FREE; printf("xdr_double XDR_FREE ok=%d\n", xdr_double(&x, &v)); }
    { int v = 5; enc(64); x.x_op = (enum xdr_op)7; printf("xdr_int op7 ok=%d\n", xdr_int(&x, &v)); }
    { long long v = 5; enc(64); x.x_op = (enum xdr_op)7; printf("xdr_hyper op7 ok=%d\n", xdr_hyper(&x, &v)); }
    { char *s = "a"; enc(64); x.x_op = (enum xdr_op)7; printf("xdr_string op7 ok=%d\n", xdr_string(&x, &s, 9)); }
    { short v = 5; enc(64); x.x_op = (enum xdr_op)7; printf("xdr_short op7 ok=%d\n", xdr_short(&x, &v)); }
    /* stream ops */
    enc(16);
    printf("xdrmem fields: priv-base=%td base=%d handy=%u public=%s\n", x.x_private - x.x_base, x.x_base == (char *)buf, x.x_handy, x.x_public ? "set" : "null");
    /* each call is its own statement: printf's argument order is unspecified */
    { unsigned q[] = {8, 16, 17, 0}; for (int i = 0; i < 4; i++) { int ok = x.x_ops->x_setpostn(&x, q[i]); printf("setpostn %u ok=%d pos=%u handy=%u\n", q[i], ok, x.x_ops->x_getpostn(&x), x.x_handy); } }
    { unsigned q[] = {8, 9, 3, 0}; for (int i = 0; i < 4; i++) { int32_t *p = x.x_ops->x_inline(&x, q[i]); printf("inline %u off=%td pos=%u handy=%u\n", q[i], p ? (char *)p - (char *)buf : -1, x.x_ops->x_getpostn(&x), x.x_handy); } }
    { int32_t v = 0x01020304; enc(16); int ok = x.x_ops->x_putint32(&x, &v); printf("putint32 ok=%d pos=%u bytes=%02x%02x%02x%02x\n", ok, x.x_ops->x_getpostn(&x), buf[0], buf[1], buf[2], buf[3]); }
    { long v = 0x1ffffffffL; enc(16); int ok = x.x_ops->x_putlong(&x, &v); printf("putlong 0x1ffffffff ok=%d pos=%u bytes=%02x%02x%02x%02x\n", ok, x.x_ops->x_getpostn(&x), buf[0], buf[1], buf[2], buf[3]); }
    { dec("ffffffff"); int32_t v = 0; int ok = x.x_ops->x_getint32(&x, &v); printf("getint32 ok=%d v=%d\n", ok, v); }
    { dec("ffffffff"); long v = 0; int ok = x.x_ops->x_getlong(&x, &v); printf("getlong ok=%d v=%ld\n", ok, v); }
    { dec("010203"); char o[4] = "QQQ"; int ok = x.x_ops->x_getbytes(&x, o, 4); printf("getbytes 4 of 3 ok=%d pos=%u handy=%u o=%.3s\n", ok, x.x_ops->x_getpostn(&x), x.x_handy, o); }
    { enc(3); int ok = x.x_ops->x_putbytes(&x, "abc", 3); printf("putbytes 3 of 3 ok=%d pos=%u handy=%u\n", ok, x.x_ops->x_getpostn(&x), x.x_handy); }
    { enc(3); int ok = x.x_ops->x_putbytes(&x, "abcd", 4); printf("putbytes 4 of 3 ok=%d pos=%u handy=%u\n", ok, x.x_ops->x_getpostn(&x), x.x_handy); }
    { enc(5); long v = 1; int ok = x.x_ops->x_putlong(&x, &v); ok = ok * 10 + x.x_ops->x_putlong(&x, &v); printf("putlong twice size5 r=%d pos=%u handy=%u\n", ok, x.x_ops->x_getpostn(&x), x.x_handy); }
    { enc(16); x.x_ops->x_destroy(&x); printf("destroy pos=%u handy=%u\n", x.x_ops->x_getpostn(&x), x.x_handy); }
    { XDR y; xdrmem_create(&y, (char *)buf, 8, XDR_DECODE); printf("same ops table enc/dec=%d\n", y.x_ops == x.x_ops); }
    return 0;
}
