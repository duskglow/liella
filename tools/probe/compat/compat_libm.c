/* LEGACY-PROBE-COMPAT (LG-0b): the 35 superseded libm names (old version against the default):
 * result, errno and exception flags, whether a program-defined matherr is called, and the effect of
 * _LIB_VERSION@GLIBC_2.2.5. Black-box: old versions are bound with .symver; every line starts with the
 * versioned name it observes. */
#define _GNU_SOURCE
#include <errno.h>
#include <fenv.h>
#include <float.h>
#include <math.h>
#include <stdio.h>
#include <string.h>

#define OLD1(T, n, v) extern T n##_old(T); __asm__(".symver " #n "_old, " #n "@" #v);
#define OLD2(T, n, v) extern T n##_old(T, T); __asm__(".symver " #n "_old, " #n "@" #v);
OLD1(double, exp, GLIBC_2.2.5) OLD1(double, exp2, GLIBC_2.2.5) OLD1(double, exp10, GLIBC_2.2.5)
OLD1(double, log, GLIBC_2.2.5) OLD1(double, log2, GLIBC_2.2.5) OLD1(double, lgamma, GLIBC_2.2.5)
OLD1(float, expf, GLIBC_2.2.5) OLD1(float, exp2f, GLIBC_2.2.5) OLD1(float, exp10f, GLIBC_2.2.5)
OLD1(float, logf, GLIBC_2.2.5) OLD1(float, log2f, GLIBC_2.2.5) OLD1(float, lgammaf, GLIBC_2.2.5)
OLD1(long double, lgammal, GLIBC_2.2.5)
OLD2(double, pow, GLIBC_2.2.5) OLD2(double, fmod, GLIBC_2.2.5) OLD2(double, hypot, GLIBC_2.2.5)
OLD2(float, powf, GLIBC_2.2.5) OLD2(float, fmodf, GLIBC_2.2.5) OLD2(float, hypotf, GLIBC_2.2.5)
/* totalorder*@2.25-2.27 called by value (the TS 18661-1 form); run in a child would hide nothing more, a
 * pointer-taking entry would fault on these calls and the run script reports the signal. */
#define OLDT(T, n, v) extern int n##_old(T, T); __asm__(".symver " #n "_old, " #n "@" #v);
OLDT(double, totalorder, GLIBC_2.25) OLDT(float, totalorderf, GLIBC_2.25) OLDT(long double, totalorderl, GLIBC_2.25)
OLDT(_Float128, totalorderf128, GLIBC_2.26) OLDT(_Float32, totalorderf32, GLIBC_2.27) OLDT(_Float32x, totalorderf32x, GLIBC_2.27)
OLDT(_Float64, totalorderf64, GLIBC_2.27) OLDT(_Float64x, totalorderf64x, GLIBC_2.27)
OLDT(double, totalordermag, GLIBC_2.25) OLDT(float, totalordermagf, GLIBC_2.25) OLDT(long double, totalordermagl, GLIBC_2.25)
OLDT(_Float128, totalordermagf128, GLIBC_2.26) OLDT(_Float32, totalordermagf32, GLIBC_2.27) OLDT(_Float32x, totalordermagf32x, GLIBC_2.27)
OLDT(_Float64, totalordermagf64, GLIBC_2.27) OLDT(_Float64x, totalordermagf64x, GLIBC_2.27)
extern int LIBV_old; __asm__(".symver LIBV_old, _LIB_VERSION@GLIBC_2.2.5");

/* The SVID exception record (as documented for matherr in the old manual pages). */
struct exception { int type; char *name; double arg1, arg2, retval; };
static int mcalls, mtype; static char mname[16];
int matherr(struct exception *e) { mcalls++; mtype = e->type; snprintf(mname, sizeof mname, "%s", e->name ? e->name : "?"); return 0; }
/* The library's own matherr@GLIBC_2.2.5 entry (the default handler), called directly with a DOMAIN record. */
extern int matherr_old(struct exception *); __asm__(".symver matherr_old, matherr@GLIBC_2.2.5");

static const char *en(int e) { const char *s = strerrorname_np(e); return e == 0 ? "0" : s ? s : "?"; }
static char *fl(char *b, int f) { char *p = b; if (f & FE_INVALID) *p++ = 'I'; if (f & FE_DIVBYZERO) *p++ = 'Z'; if (f & FE_OVERFLOW) *p++ = 'O'; if (f & FE_UNDERFLOW) *p++ = 'U'; if (f & FE_INEXACT) *p++ = 'X'; if (p == b) *p++ = '-'; *p = 0; return b; }

struct obs { long double r; int e, f, mc, mt; char mn[16]; };
static int erroring;
static void begin(void) { errno = 0; feclearexcept(FE_ALL_EXCEPT); mcalls = 0; mname[0] = 0; }
static void end(struct obs *o, long double r) { o->r = r; o->e = errno; o->f = fetestexcept(FE_ALL_EXCEPT); o->mc = mcalls; o->mt = mtype; strcpy(o->mn, mname); }
static void show(const char *n, const char *args, struct obs *a, struct obs *b, int only_err) {
    char f0[8], f1[8];
    int err = a->e || b->e || (a->f & (FE_INVALID | FE_DIVBYZERO | FE_OVERFLOW)) || a->mc;
    if (only_err && !err) return;
    printf("%s@GLIBC_2.2.5 (%s): r=%La errno=%s flags=%s matherr=", n, args, a->r, en(a->e), fl(f0, a->f));
    if (a->mc) printf("%d:type%d:%s", a->mc, a->mt, a->mn); else printf("-");
    printf(" | default: r=%La errno=%s flags=%s matherr=%s\n", b->r, en(b->e), fl(f1, b->f), b->mc ? "CALLED" : "-");
}
#define RUN1(n, T, x, oe) do { struct obs a, b; T v = (x); begin(); T r0 = n##_old(v); end(&a, r0); begin(); T r1 = n(v); end(&b, r1); \
    char s[64]; snprintf(s, sizeof s, "%La", (long double)v); show(#n, s, &a, &b, oe); } while (0)
#define RUN2(n, T, x, y, oe) do { struct obs a, b; T v = (x), w = (y); begin(); T r0 = n##_old(v, w); end(&a, r0); begin(); T r1 = n(v, w); end(&b, r1); \
    char s[96]; snprintf(s, sizeof s, "%La, %La", (long double)v, (long double)w); show(#n, s, &a, &b, oe); } while (0)

static void batch(int oe) {
    static const double d1[] = {0, 1, -1, 0.5, 710, -746, -708.5, 1024, -1080, 309, -330, 0x1p-1074, -2.5, 1e306, INFINITY, -INFINITY, NAN, -0.0};
    static const float f1[] = {0, 1, -1, 0.5f, 89, -104, -90, 128, -150, 39, -46, 0x1p-149f, -2.5f, 1e37f, INFINITY, -INFINITY, NAN, -0.0f};
    for (unsigned i = 0; i < sizeof d1 / sizeof *d1; i++) {
        RUN1(exp, double, d1[i], oe); RUN1(exp2, double, d1[i], oe); RUN1(exp10, double, d1[i], oe);
        RUN1(log, double, d1[i], oe); RUN1(log2, double, d1[i], oe); RUN1(lgamma, double, d1[i], oe);
        RUN1(lgammal, long double, (long double)d1[i], oe);
        RUN1(expf, float, f1[i], oe); RUN1(exp2f, float, f1[i], oe); RUN1(exp10f, float, f1[i], oe);
        RUN1(logf, float, f1[i], oe); RUN1(log2f, float, f1[i], oe); RUN1(lgammaf, float, f1[i], oe);
    }
    static const double p2[][2] = {{2, 3}, {0, -1}, {-0.0, -1}, {0, -2}, {-1, 0.5}, {10, 400}, {10, -400}, {-8, 1.0 / 3}, {NAN, 0}, {1, NAN}, {0, -INFINITY}, {-1, INFINITY}, {5.5, 2}, {1, 0}, {INFINITY, 1}, {1, INFINITY}, {-0.0, 1}, {0, 0}, {3, 4}, {1e308, 1e308}, {INFINITY, NAN}, {NAN, INFINITY}, {0x1p-1074, 0x1p-1074}, {DBL_MAX, DBL_MAX}};
    for (unsigned i = 0; i < sizeof p2 / sizeof *p2; i++) {
        RUN2(pow, double, p2[i][0], p2[i][1], oe); RUN2(fmod, double, p2[i][0], p2[i][1], oe); RUN2(hypot, double, p2[i][0], p2[i][1], oe);
        float x = (float)p2[i][0], y = (float)p2[i][1];
        if (p2[i][0] == 10 && p2[i][1] == 400) y = 40; if (p2[i][0] == 10 && p2[i][1] == -400) y = -50;
        RUN2(powf, float, x, y, oe); RUN2(fmodf, float, x, y, oe); RUN2(hypotf, float, x, y, oe);
    }
}

#define TOT(n, T, ver) do { T v[] = {-0.0, 0.0, 1, 2, -INFINITY, INFINITY, NAN, -NAN, __builtin_nans("")}; int nv = sizeof v / sizeof *v; \
    printf("%s@%s:", #n, ver); for (int i = 0; i < nv; i++) for (int j = 0; j < nv; j++) printf("%d", n##_old(v[i], v[j]) != 0); \
    printf(" | default:"); for (int i = 0; i < nv; i++) for (int j = 0; j < nv; j++) printf("%d", n(&v[i], &v[j]) != 0); printf("\n"); } while (0)

int main(void) {
    setvbuf(stdout, NULL, _IOLBF, 0);
    printf("_LIB_VERSION@GLIBC_2.2.5 initial value=%d\n", LIBV_old);
    {
        struct exception x = {1, "probe", 0.0, 0.0, 1.5};
        begin();
        int r = matherr_old(&x);
        printf("matherr@GLIBC_2.2.5 (library entry, DOMAIN record): returns=%d errno=%s retval=%a program-matherr=%s same-as-program=%s\n",
               r, en(errno), x.retval, mcalls ? "CALLED" : "-", (void *)matherr_old == (void *)matherr ? "yes" : "no");
    }
    batch(0);
    static const int modes[] = {-1, 0, 1, 2, 3};
    int init = LIBV_old;
    for (unsigned m = 0; m < 5; m++) {
        LIBV_old = modes[m];
        printf("_LIB_VERSION@GLIBC_2.2.5 set to %d: error cases only\n", modes[m]);
        batch(1);
    }
    LIBV_old = init;
    printf("# totalorder lines: 81 results (0/1) for every ordered pair of -0,+0,1,2,-inf,+inf,nan,-nan,snan, row-major\n");
    TOT(totalorder, double, "GLIBC_2.25"); TOT(totalorderf, float, "GLIBC_2.25"); TOT(totalorderl, long double, "GLIBC_2.25"); TOT(totalorderf128, _Float128, "GLIBC_2.26");
    TOT(totalorderf32, _Float32, "GLIBC_2.27"); TOT(totalorderf32x, _Float32x, "GLIBC_2.27"); TOT(totalorderf64, _Float64, "GLIBC_2.27"); TOT(totalorderf64x, _Float64x, "GLIBC_2.27");
    TOT(totalordermag, double, "GLIBC_2.25"); TOT(totalordermagf, float, "GLIBC_2.25"); TOT(totalordermagl, long double, "GLIBC_2.25"); TOT(totalordermagf128, _Float128, "GLIBC_2.26");
    TOT(totalordermagf32, _Float32, "GLIBC_2.27"); TOT(totalordermagf32x, _Float32x, "GLIBC_2.27"); TOT(totalordermagf64, _Float64, "GLIBC_2.27"); TOT(totalordermagf64x, _Float64x, "GLIBC_2.27");
    return 0;
}
