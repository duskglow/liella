/* wcprobe.c - WCTYPE-RETHINK black-box counters (no spec clause: measurement only).
 * Per-call user-mode counters of the wide classification entries for every class, code-point range and locale,
 * net of the harness loop and of a call to an empty function with the same signature (median of 5 runs).
 * -DLIELLA (static against Liella): instructions, cycles, taken branches, loads. Otherwise (host C library,
 * dynamic): instructions and cycles only. -DPROTO: two prototype lookup schemes whose tables are built from the
 * running library's own iswctype_l answers and verified against it for every code point.
 * usage: wcprobe count <out.tsv> | wcprobe stats | wcprobe time <rounds> <out.tsv> */
#define _GNU_SOURCE
#include <locale.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>
#include <wchar.h>
#include <wctype.h>
#include <sys/ioctl.h>
#include <asm/unistd.h>
#include <linux/perf_event.h>

#define ATTR __attribute__((noinline, noipa))
#define NCLS 12
#define NCP 0x110000u
static const char *const CLS[NCLS] = {"alnum", "alpha", "blank", "cntrl", "digit", "graph",
                                      "lower", "print", "punct", "space", "upper", "xdigit"};
typedef int (*f1_t)(wint_t);
typedef int (*fl_t)(wint_t, locale_t);
typedef int (*ft_t)(wint_t, wctype_t);
typedef int (*ftl_t)(wint_t, wctype_t, locale_t);
typedef wint_t (*g1_t)(wint_t);
typedef wint_t (*gl_t)(wint_t, locale_t);
typedef wint_t (*gt_t)(wint_t, wctrans_t);
static f1_t const ISW[NCLS] = {iswalnum, iswalpha, iswblank, iswcntrl, iswdigit, iswgraph,
                               iswlower, iswprint, iswpunct, iswspace, iswupper, iswxdigit};
static fl_t const ISWL[NCLS] = {iswalnum_l, iswalpha_l, iswblank_l, iswcntrl_l, iswdigit_l, iswgraph_l,
                                iswlower_l, iswprint_l, iswpunct_l, iswspace_l, iswupper_l, iswxdigit_l};

static wint_t IN[256];
static volatile long sinkv;
enum { K1, KL, KT, KTL, KG1, KGL, KGT, NK };
struct row { int kind; void *f; wctype_t t; wctrans_t tr; locale_t l; };

ATTR static long L1(f1_t f, long n) { long s = 0; for (long i = 0; i < n; i++) s += f(IN[i & 255]); return s; }
ATTR static long LL(fl_t f, locale_t l, long n) { long s = 0; for (long i = 0; i < n; i++) s += f(IN[i & 255], l); return s; }
ATTR static long LT(ft_t f, wctype_t t, long n) { long s = 0; for (long i = 0; i < n; i++) s += f(IN[i & 255], t); return s; }
ATTR static long LTL(ftl_t f, wctype_t t, locale_t l, long n) { long s = 0; for (long i = 0; i < n; i++) s += f(IN[i & 255], t, l); return s; }
ATTR static long G1(g1_t f, long n) { long s = 0; for (long i = 0; i < n; i++) s += (long)f(IN[i & 255]); return s; }
ATTR static long GL(gl_t f, locale_t l, long n) { long s = 0; for (long i = 0; i < n; i++) s += (long)f(IN[i & 255], l); return s; }
ATTR static long GT(gt_t f, wctrans_t t, long n) { long s = 0; for (long i = 0; i < n; i++) s += (long)f(IN[i & 255], t); return s; }
ATTR static int n1(wint_t c) { return (int)(c & 1); }
ATTR static int nl(wint_t c, locale_t l) { (void)l; return (int)(c & 1); }
ATTR static int nt(wint_t c, wctype_t t) { (void)t; return (int)(c & 1); }
ATTR static int ntl(wint_t c, wctype_t t, locale_t l) { (void)t; (void)l; return (int)(c & 1); }
ATTR static wint_t g1n(wint_t c) { return c; }
ATTR static wint_t gln(wint_t c, locale_t l) { (void)l; return c; }
ATTR static wint_t gtn(wint_t c, wctrans_t t) { (void)t; return c; }
static void *const NOP[NK] = {(void *)n1, (void *)nl, (void *)nt, (void *)ntl, (void *)g1n, (void *)gln, (void *)gtn};
static const char *const KN[NK] = {"f(c)", "f(c,l)", "f(c,t)", "f(c,t,l)", "g(c)", "g(c,l)", "g(c,tr)"};

static long run(const struct row *r, long n) {
    switch (r->kind) {
    case K1: return L1((f1_t)r->f, n);
    case KL: return LL((fl_t)r->f, r->l, n);
    case KT: return LT((ft_t)r->f, r->t, n);
    case KTL: return LTL((ftl_t)r->f, r->t, r->l, n);
    case KG1: return G1((g1_t)r->f, n);
    case KGL: return GL((gl_t)r->f, r->l, n);
    default: return GT((gt_t)r->f, r->tr, n);
    }
}

enum { R_ASCII, R_LAT1, R_CJK, R_BMPMIX, R_ASTRAL, R_BMPRND, NR };
static const char *const RN[NR] = {"ascii", "latin1", "cjk", "bmpmix", "astral", "bmprnd"};
static void fill(int rg) {
    static const unsigned bm[8] = {0x391, 0x410, 0x627, 0x915, 0xAC00, 0x4E00, 0x2010, 0xFF21};
    static const unsigned ba[4] = {0x1F600, 0x20000, 0x1D400, 0x10400};
    uint32_t x = 12345;
    for (unsigned k = 0; k < 256; k++) {
        x = x * 1103515245u + 12345u;
        switch (rg) {
        case R_ASCII: IN[k] = 0x20 + k % 95; break;
        case R_LAT1: IN[k] = 0xA0 + k % 96; break;
        case R_CJK: IN[k] = 0x4E00 + k; break;
        case R_BMPMIX: IN[k] = bm[k >> 5] + (k & 31); break;
        case R_ASTRAL: IN[k] = ba[k >> 6] + (k & 63); break;
        default: IN[k] = (x >> 8) & 0xFFFF; break;
        }
    }
}

/* ---- counters ---- */
static int gfd = -1, nev, pos[4] = {-1, -1, -1, -1};
static void pf_add(int slot, uint32_t type, uint64_t cfg) {
    struct perf_event_attr a;
    memset(&a, 0, sizeof a);
    a.size = sizeof a;
    a.type = type;
    a.config = cfg;
    a.exclude_kernel = 1;
    a.exclude_hv = 1;
    a.read_format = PERF_FORMAT_GROUP;
    a.disabled = gfd < 0;
    int fd = (int)syscall(__NR_perf_event_open, &a, 0, -1, gfd, 0);
    if (fd < 0) return;
    if (gfd < 0) gfd = fd;
    pos[slot] = nev++;
}
static void pf_init(void) {
    pf_add(0, PERF_TYPE_HARDWARE, PERF_COUNT_HW_INSTRUCTIONS);
    pf_add(1, PERF_TYPE_HARDWARE, PERF_COUNT_HW_CPU_CYCLES);
#ifdef LIELLA
    pf_add(2, PERF_TYPE_RAW, 0x20C4); /* BR_INST_RETIRED.NEAR_TAKEN (Skylake) */
    pf_add(3, PERF_TYPE_RAW, 0x81D0); /* MEM_INST_RETIRED.ALL_LOADS (Skylake) */
#endif
}
static void pf_count(const struct row *r, long n, double out[4]) {
    uint64_t buf[8] = {0};
    ioctl(gfd, PERF_EVENT_IOC_RESET, PERF_IOC_FLAG_GROUP);
    ioctl(gfd, PERF_EVENT_IOC_ENABLE, PERF_IOC_FLAG_GROUP);
    sinkv += run(r, n);
    ioctl(gfd, PERF_EVENT_IOC_DISABLE, PERF_IOC_FLAG_GROUP);
    if (read(gfd, buf, sizeof buf) < 8) buf[0] = 0;
    for (int s = 0; s < 4; s++) out[s] = pos[s] >= 0 && buf[0] ? (double)buf[1 + pos[s]] : -1;
}
#define NCALL 4096
#define REPS 5
static int cmpd(const void *a, const void *b) {
    double x = *(const double *)a, y = *(const double *)b;
    return (x > y) - (x < y);
}
static void med(const struct row *r, double m[4]) {
    double v[4][REPS];
    sinkv += run(r, NCALL); /* warm-up: first-use fills and faults are not measured */
    for (int k = 0; k < REPS; k++) {
        double o[4];
        pf_count(r, NCALL, o);
        for (int s = 0; s < 4; s++) v[s][k] = o[s];
    }
    for (int s = 0; s < 4; s++) {
        qsort(v[s], REPS, sizeof(double), cmpd);
        m[s] = v[s][REPS / 2];
    }
}
static FILE *OUT;
static double BASE[NK][4];
static void base_all(const char *loc, int rg) {
    for (int k = 0; k < NK; k++) {
        struct row r = {k, NOP[k], 0, 0, 0};
        med(&r, BASE[k]);
        if (rg == 0 && (k == K1 || k == KT)) {
            fprintf(OUT, "%s\tbase-gross\t%s\t%s", loc, KN[k], RN[rg]);
            for (int s = 0; s < 4; s++) fprintf(OUT, "\t%.2f", BASE[k][s] / NCALL);
            fputc('\n', OUT);
        }
    }
}
static void emit(const char *loc, const char *entry, const char *cls, int rg, const struct row *r) {
    double m[4];
    med(r, m);
    fprintf(OUT, "%s\t%s\t%s\t%s", loc, entry, cls, RN[rg]);
    for (int s = 0; s < 4; s++)
        if (m[s] >= 0) fprintf(OUT, "\t%.2f", (m[s] - BASE[r->kind][s]) / NCALL);
        else fprintf(OUT, "\t-");
    fputc('\n', OUT);
}
static double now_ns(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (double)ts.tv_sec * 1e9 + (double)ts.tv_nsec;
}

#ifdef PROTO
/* ---- design A: one two-stage trie of 12-bit class words shared by every class; wctype_t = the class bit ---- */
#define ASH 8
#define ANB (NCP >> ASH)
struct tabA { uint32_t s2len, pad; uint16_t s1[ANB]; uint16_t s2[]; };
/* ---- design B: Latin-1 class-word table + one bitmap trie per class (512-bit leaves shared by all classes) ---- */
#define BSH 9
#define BNB (NCP >> BSH)
struct tabB { uint16_t lat[256]; const uint16_t *s1[NCLS]; const uint64_t *leaf; };
struct ploc { const struct tabA *a; const struct tabB *b; };
static __thread const struct tabA *curA;
static __thread const struct tabB *curB;

static inline int lookA(const struct tabA *t, uint32_t c, uint32_t m) {
    if (c >= NCP) return 0;
    uint32_t i = ((uint32_t)t->s1[c >> ASH] << ASH) | (c & ((1u << ASH) - 1));
    if (i >= t->s2len) return 0; /* models a slice bounds check */
    return (t->s2[i] & m) != 0;
}
static inline int lookB(const struct tabB *t, uint32_t c, unsigned k, uint32_t m) {
    if (c < 256) return (t->lat[c] & m) != 0;
    if (c >= NCP) return 0;
    uint32_t li = t->s1[k][c >> BSH];
    uint64_t w = t->leaf[(li << 3) | ((c >> 6) & 7)];
    return (int)((w >> (c & 63)) & 1);
}
ATTR static int pA_alpha(wint_t c) { return lookA(curA, c, 1u << 1); }
ATTR static int pA_upper(wint_t c) { return lookA(curA, c, 1u << 10); }
ATTR static int pA_alpha_l(wint_t c, locale_t l) { return lookA(((const struct ploc *)l)->a, c, 1u << 1); }
ATTR static int pA_ctype(wint_t c, wctype_t d) { return lookA(curA, c, (uint32_t)d); }
ATTR static int pA_ctype_l(wint_t c, wctype_t d, locale_t l) { return lookA(((const struct ploc *)l)->a, c, (uint32_t)d); }
ATTR static int pB_alpha(wint_t c) { return lookB(curB, c, 1, 1u << 1); }
ATTR static int pB_upper(wint_t c) { return lookB(curB, c, 10, 1u << 10); }
ATTR static int pB_alpha_l(wint_t c, locale_t l) { return lookB(((const struct ploc *)l)->b, c, 1, 1u << 1); }
ATTR static int pB_ctype(wint_t c, wctype_t d) {
    return d ? lookB(curB, c, (unsigned)__builtin_ctzl(d), (uint32_t)d) : 0;
}
ATTR static int pB_ctype_l(wint_t c, wctype_t d, locale_t l) {
    return d ? lookB(((const struct ploc *)l)->b, c, (unsigned)__builtin_ctzl(d), (uint32_t)d) : 0;
}

static uint16_t *W, *WPREV;
static const char *prevname;
static uint32_t dedupb(const unsigned char *d, uint32_t nb, uint32_t bsz, uint32_t *idx, unsigned char *out) {
    uint32_t cap = 1;
    while (cap < 2 * nb) cap <<= 1;
    uint32_t *ht = malloc((size_t)cap * 4), *rep = malloc((size_t)nb * 4), nu = 0;
    memset(ht, 0xff, (size_t)cap * 4);
    for (uint32_t b = 0; b < nb; b++) {
        const unsigned char *p = d + (size_t)b * bsz;
        uint64_t h = 1469598103934665603ull;
        for (uint32_t i = 0; i < bsz; i++) h = (h ^ p[i]) * 1099511628211ull;
        uint32_t s = (uint32_t)(h >> 20) & (cap - 1);
        for (;;) {
            uint32_t e = ht[s];
            if (e == 0xffffffffu) {
                ht[s] = nu;
                rep[nu] = b;
                if (out) memcpy(out + (size_t)nu * bsz, p, bsz);
                idx[b] = nu++;
                break;
            }
            if (!memcmp(d + (size_t)rep[e] * bsz, p, bsz)) { idx[b] = e; break; }
            s = (s + 1) & (cap - 1);
        }
    }
    free(ht);
    free(rep);
    return nu;
}
static void words(locale_t l) {
    wctype_t d[NCLS];
    for (int k = 0; k < NCLS; k++) d[k] = wctype_l(CLS[k], l);
    for (uint32_t c = 0; c < NCP; c++) {
        unsigned v = 0;
        for (int k = 0; k < NCLS; k++)
            if (iswctype_l(c, d[k], l)) v |= 1u << k;
        W[c] = (uint16_t)v;
    }
}
static struct tabA *buildA(void) {
    uint32_t *idx = malloc(ANB * sizeof *idx);
    uint16_t *tmp = malloc(NCP * 2);
    uint32_t nu = dedupb((const unsigned char *)W, ANB, (1u << ASH) * 2, idx, (unsigned char *)tmp);
    struct tabA *t = malloc(sizeof *t + (size_t)nu * (2u << ASH));
    t->s2len = nu << ASH;
    t->pad = 0;
    for (uint32_t b = 0; b < ANB; b++) t->s1[b] = (uint16_t)idx[b];
    memcpy(t->s2, tmp, (size_t)nu * (2u << ASH));
    free(idx);
    free(tmp);
    return t;
}
static uint32_t nleafB;
static struct tabB *buildB(void) {
    size_t nw = NCP / 64;
    uint64_t *bm = calloc(NCLS * nw, 8);
    for (uint32_t c = 0; c < NCP; c++)
        for (int k = 0; k < NCLS; k++)
            if (W[c] >> k & 1) bm[k * nw + c / 64] |= 1ull << (c % 64);
    uint32_t *idx = malloc(NCLS * BNB * sizeof *idx);
    uint64_t *lv = malloc(NCLS * nw * 8);
    uint32_t nu = dedupb((const unsigned char *)bm, NCLS * BNB, 64, idx, (unsigned char *)lv);
    nleafB = nu;
    struct tabB *t = malloc(sizeof *t);
    for (int c = 0; c < 256; c++) t->lat[c] = W[c];
    uint16_t *s1 = malloc(NCLS * BNB * 2);
    for (uint32_t i = 0; i < NCLS * BNB; i++) s1[i] = (uint16_t)idx[i];
    for (int k = 0; k < NCLS; k++) t->s1[k] = s1 + k * BNB;
    t->leaf = lv;
    free(bm);
    free(idx);
    return t;
}
static void stats(const char *loc) {
    static unsigned char seen[65536];
    memset(seen, 0, sizeof seen);
    unsigned nd = 0;
    for (uint32_t c = 0; c < NCP; c++)
        if (!seen[W[c]]) { seen[W[c]] = 1; nd++; }
    printf("%s distinct-class-words %u\n", loc, nd);
    uint32_t *idx = malloc((NCP / 16) * sizeof *idx);
    for (unsigned sh = 5; sh <= 9; sh++) {
        uint32_t B = 1u << sh, nb = NCP >> sh;
        uint32_t nu = dedupb((const unsigned char *)W, nb, B * 2, idx, NULL);
        size_t two = (size_t)nb * 2 + (size_t)nu * B * 2;
        size_t pal = (size_t)nb * 2 + (size_t)nu * B + 512;
        uint32_t per = 4096 >> sh, nmb = NCP >> 12;
        uint16_t *mids = malloc((size_t)nb * 2);
        for (uint32_t b = 0; b < nb; b++) mids[b] = (uint16_t)idx[b];
        uint32_t *midx = malloc(nmb * sizeof *midx);
        uint32_t nmu = dedupb((const unsigned char *)mids, nmb, per * 2, midx, NULL);
        size_t three = (size_t)nmb * 2 + (size_t)nmu * per * 2 + (size_t)nu * B * 2;
        printf("%s leaf=%u distinct-leaves %u two-stage-u16 %zu two-stage-palette8 %zu three-stage-u16 %zu (mids %u)\n",
               loc, B, nu, two, pal, three, nmu);
        free(mids);
        free(midx);
    }
    free(idx);
    if (WPREV) {
        printf("%s vs %s differing code points per class:", prevname, loc);
        for (int k = 0; k < NCLS; k++) {
            unsigned long n = 0;
            for (uint32_t c = 0; c < NCP; c++) n += ((W[c] ^ WPREV[c]) >> k) & 1;
            printf(" %s=%lu", CLS[k], n);
        }
        printf("\n");
    } else {
        WPREV = malloc(NCP * 2);
    }
    memcpy(WPREV, W, NCP * 2);
    prevname = loc;
}
static unsigned long verify(const struct ploc *pl, unsigned long *entry_bad) {
    wctype_t d[NCLS];
    for (int k = 0; k < NCLS; k++) d[k] = wctype(CLS[k]);
    unsigned long bad = 0;
    for (uint64_t cc = 0; cc <= NCP + 1; cc++) {
        uint32_t c = cc == NCP + 1 ? 0xFFFFFFFFu : (uint32_t)cc;
        for (int k = 0; k < NCLS; k++) {
            int ref = iswctype(c, d[k]) != 0;
            int x = pA_ctype(c, 1ul << k) != 0, y = pB_ctype(c, 1ul << k) != 0;
            int xl = pA_ctype_l(c, 1ul << k, (locale_t)pl) != 0, yl = pB_ctype_l(c, 1ul << k, (locale_t)pl) != 0;
            if (x != ref || y != ref || xl != ref || yl != ref)
                if (bad++ < 5) printf("mismatch U+%04X %s ref %d A %d B %d\n", c, CLS[k], ref, x, y);
            if ((ISW[k](c) != 0) != ref) ++*entry_bad;
        }
    }
    return bad;
}
static void build(const char *loc, locale_t l, struct tabA **a, struct tabB **b, int dostats) {
    if (!W) W = malloc(NCP * 2);
    double t0 = now_ns();
    words(l);
    double t1 = now_ns();
    *a = buildA();
    *b = buildB();
    double t2 = now_ns();
    if (!dostats) return;
    printf("%s words-from-iswctype_l %.1f ms, tables %.1f ms\n", loc, (t1 - t0) / 1e6, (t2 - t1) / 1e6);
    stats(loc);
    printf("%s design-A bytes %zu (leaf %u), design-B bytes %zu (%u shared 512-bit leaves + %u per-class s1 + 512 Latin-1)\n",
           loc, sizeof(struct tabA) + (size_t)(*a)->s2len * 2, 1u << ASH,
           (size_t)NCLS * BNB * 2 + (size_t)nleafB * 64 + 512, nleafB, NCLS * BNB * 2);
}
#endif

static const char *const LOCS[3] = {"C", "C.UTF-8", "en_US.UTF-8"};

static int count_mode(const char *out) {
    OUT = fopen(out, "w");
    if (!OUT) return 3;
    pf_init();
    for (int li = 0; li < 3; li++) {
        const char *loc = LOCS[li];
        if (!setlocale(LC_ALL, loc)) { fprintf(stderr, "setlocale %s failed\n", loc); return 4; }
        locale_t l = newlocale(LC_ALL_MASK, loc, (locale_t)0);
        if (!l) { fprintf(stderr, "newlocale %s failed\n", loc); return 4; }
        wctype_t d[NCLS], dl[NCLS];
        for (int k = 0; k < NCLS; k++) { d[k] = wctype(CLS[k]); dl[k] = wctype_l(CLS[k], l); }
        wctrans_t tu = wctrans("toupper");
#ifdef PROTO
        struct tabA *ta;
        struct tabB *tb;
        build(loc, l, &ta, &tb, 0);
        curA = ta;
        curB = tb;
        struct ploc pl = {ta, tb};
#endif
        for (int rg = 0; rg < NR; rg++) {
            fill(rg);
            base_all(loc, rg);
            for (int k = 0; k < NCLS; k++) {
                struct row r1 = {K1, (void *)ISW[k], 0, 0, l};
                struct row r2 = {KL, (void *)ISWL[k], 0, 0, l};
                struct row r3 = {KT, (void *)iswctype, d[k], 0, l};
                struct row r4 = {KTL, (void *)iswctype_l, dl[k], 0, l};
                emit(loc, "isw", CLS[k], rg, &r1);
                emit(loc, "isw_l", CLS[k], rg, &r2);
                emit(loc, "iswctype", CLS[k], rg, &r3);
                emit(loc, "iswctype_l", CLS[k], rg, &r4);
#ifdef PROTO
                struct row a3 = {KT, (void *)pA_ctype, 1ul << k, 0, 0};
                struct row a4 = {KTL, (void *)pA_ctype_l, 1ul << k, 0, (locale_t)&pl};
                struct row b3 = {KT, (void *)pB_ctype, 1ul << k, 0, 0};
                struct row b4 = {KTL, (void *)pB_ctype_l, 1ul << k, 0, (locale_t)&pl};
                emit(loc, "pA_iswctype", CLS[k], rg, &a3);
                emit(loc, "pA_iswctype_l", CLS[k], rg, &a4);
                emit(loc, "pB_iswctype", CLS[k], rg, &b3);
                emit(loc, "pB_iswctype_l", CLS[k], rg, &b4);
#endif
            }
            struct row t1 = {KG1, (void *)towupper, 0, 0, l};
            struct row t2 = {KG1, (void *)towlower, 0, 0, l};
            struct row t3 = {KGL, (void *)towupper_l, 0, 0, l};
            struct row t4 = {KGT, (void *)towctrans, 0, tu, l};
            emit(loc, "towupper", "-", rg, &t1);
            emit(loc, "towlower", "-", rg, &t2);
            emit(loc, "towupper_l", "-", rg, &t3);
            emit(loc, "towctrans", "-", rg, &t4);
#ifdef PROTO
            struct row p1 = {K1, (void *)pA_alpha, 0, 0, 0}, p2 = {K1, (void *)pA_upper, 0, 0, 0};
            struct row p3 = {KL, (void *)pA_alpha_l, 0, 0, (locale_t)&pl};
            struct row q1 = {K1, (void *)pB_alpha, 0, 0, 0}, q2 = {K1, (void *)pB_upper, 0, 0, 0};
            struct row q3 = {KL, (void *)pB_alpha_l, 0, 0, (locale_t)&pl};
            emit(loc, "pA_isw", "alpha", rg, &p1);
            emit(loc, "pA_isw", "upper", rg, &p2);
            emit(loc, "pA_isw_l", "alpha", rg, &p3);
            emit(loc, "pB_isw", "alpha", rg, &q1);
            emit(loc, "pB_isw", "upper", rg, &q2);
            emit(loc, "pB_isw_l", "alpha", rg, &q3);
#endif
        }
    }
    fclose(OUT);
    printf("count done, sink %ld\n", (long)sinkv);
    return 0;
}

#define NT 200000
static void trow(int rd, const char *loc, const char *name, int rg, const struct row *r) {
    sinkv += run(r, NT / 8);
    double t0 = now_ns();
    sinkv += run(r, NT);
    double t1 = now_ns();
    fprintf(OUT, "%d\t%s\t%s\t%s\t%.3f\n", rd, loc, name, RN[rg], (t1 - t0) / NT);
}
static int time_mode(int rounds, const char *out) {
    static const char *const TL[2] = {"C.UTF-8", "en_US.UTF-8"};
    OUT = fopen(out, "w");
    if (!OUT) return 3;
    locale_t L[2];
#ifdef PROTO
    struct tabA *TA[2];
    struct tabB *TB[2];
    struct ploc PL[2];
#endif
    for (int li = 0; li < 2; li++) {
        if (!setlocale(LC_ALL, TL[li])) return 4;
        L[li] = newlocale(LC_ALL_MASK, TL[li], (locale_t)0);
        if (!L[li]) return 4;
#ifdef PROTO
        build(TL[li], L[li], &TA[li], &TB[li], 0);
        PL[li].a = TA[li];
        PL[li].b = TB[li];
#endif
    }
    for (int rd = 0; rd < rounds; rd++)
        for (int li = 0; li < 2; li++) {
            const char *loc = TL[li];
            setlocale(LC_ALL, loc);
            locale_t l = L[li];
            wctype_t da = wctype("alpha"), dla = wctype_l("alpha", l);
#ifdef PROTO
            curA = TA[li];
            curB = TB[li];
            locale_t pl = (locale_t)&PL[li];
#endif
            for (int rg = 0; rg < NR; rg++) {
                fill(rg);
                for (int k = 0; k < NK - 1; k++) {
                    struct row r = {k, NOP[k], 0, 0, 0};
                    trow(rd, loc, KN[k], rg, &r);
                }
                struct row e[6] = {{K1, (void *)iswalpha, 0, 0, l},        {KL, (void *)iswalpha_l, 0, 0, l},
                                   {KT, (void *)iswctype, da, 0, l},       {KTL, (void *)iswctype_l, dla, 0, l},
                                   {KG1, (void *)towupper, 0, 0, l},       {KGL, (void *)towupper_l, 0, 0, l}};
                static const char *const en[6] = {"iswalpha", "iswalpha_l", "iswctype", "iswctype_l", "towupper", "towupper_l"};
                for (int k = 0; k < 6; k++) trow(rd, loc, en[k], rg, &e[k]);
#ifdef PROTO
                struct row p[8] = {{K1, (void *)pA_alpha, 0, 0, 0},   {KL, (void *)pA_alpha_l, 0, 0, pl},
                                   {KT, (void *)pA_ctype, 2, 0, 0},   {KTL, (void *)pA_ctype_l, 2, 0, pl},
                                   {K1, (void *)pB_alpha, 0, 0, 0},   {KL, (void *)pB_alpha_l, 0, 0, pl},
                                   {KT, (void *)pB_ctype, 2, 0, 0},   {KTL, (void *)pB_ctype_l, 2, 0, pl}};
                static const char *const pn[8] = {"pA_iswalpha", "pA_iswalpha_l", "pA_iswctype", "pA_iswctype_l",
                                                  "pB_iswalpha", "pB_iswalpha_l", "pB_iswctype", "pB_iswctype_l"};
                for (int k = 0; k < 8; k++) trow(rd, loc, pn[k], rg, &p[k]);
#endif
            }
        }
    fclose(OUT);
    return 0;
}

int main(int argc, char **argv) {
    if (argc >= 3 && !strcmp(argv[1], "count")) return count_mode(argv[2]);
    if (argc >= 4 && !strcmp(argv[1], "time")) return time_mode(atoi(argv[2]), argv[3]);
#ifdef PROTO
    if (argc >= 2 && !strcmp(argv[1], "stats")) {
        for (int li = 0; li < 3; li++) {
            if (!setlocale(LC_ALL, LOCS[li])) return 4;
            locale_t l = newlocale(LC_ALL_MASK, LOCS[li], (locale_t)0);
            struct tabA *a;
            struct tabB *b;
            build(LOCS[li], l, &a, &b, 1);
            curA = a;
            curB = b;
            struct ploc pl = {a, b};
            unsigned long eb = 0, bad = verify(&pl, &eb);
            printf("%s verify: prototype mismatches %lu (of %lu checks); isw* vs iswctype disagreements %lu\n",
                   LOCS[li], bad, (unsigned long)(NCP + 2) * NCLS, eb);
        }
        return 0;
    }
#endif
    fprintf(stderr, "usage: wcprobe count <out.tsv> | stats | time <rounds> <out.tsv>\n");
    return 2;
}
