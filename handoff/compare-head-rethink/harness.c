/* COMPARE-HEAD-RETHINK black-box harness. Per-call user-space counters (perf_event_open) and time of
 * memcmp / bcmp / __memcmpeq of the libc it is linked with, and of the scheme-H prototype (proto.S),
 * on the shapes of the cbench_memref rows (A, B 64-aligned, 0x5A; head = byte 0 differs, tail = last
 * byte, diff8 = byte 8; mix = equal buffers, sizes from a fixed sequence). glibc is a black box here.
 *   ./h check           correctness (guard pages, every n 0..300 and large n, all positions, both signs)
 *   ./h count           counters; PC_EVENTS="instructions,cycles,branches;instructions,r20c4,r010e"
 *   ./h time            ns per call, median of PC_ROUNDS (default 9) rounds
 * PROTO_AVX2=0 keeps the prototype's long class on SSE2. Output: lines "R <variant> <fn> <n> <kind> ..."
 * with counters minus the proto_null row (= in-function, the call and ret included). */
#define _GNU_SOURCE
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <strings.h>
#include <stdint.h>
#include <time.h>
#include <unistd.h>
#include <sys/mman.h>
#include <sys/ioctl.h>
#include <sys/syscall.h>
#include <linux/perf_event.h>
#include <cpuid.h>

#ifndef VARIANT
#define VARIANT "unknown"
#endif

typedef int (*cmp_fn)(const void *, const void *, size_t);
extern int __memcmpeq(const void *, const void *, size_t);
extern int proto_memcmp(const void *, const void *, size_t);
extern int proto_memeq(const void *, const void *, size_t);
extern int proto_null(const void *, const void *, size_t);
extern unsigned char proto_use_avx2;

struct fn { const char *name; cmp_fn f; int ordered; int proto; };
static struct fn FNS[] = {
    {"memcmp", memcmp, 1, 0}, {"bcmp", bcmp, 0, 0}, {"__memcmpeq", __memcmpeq, 0, 0},
    {"proto_memcmp", proto_memcmp, 1, 1}, {"proto_memeq", proto_memeq, 0, 1},
};
#define NFN ((int)(sizeof FNS / sizeof FNS[0]))
static cmp_fn volatile FP[NFN + 1];

static volatile long sink;
static unsigned char *A, *B;
static size_t SZ[256];

__attribute__((noinline)) static void run(cmp_fn volatile *fp, size_t len, long n) {
    cmp_fn f = *fp;
    const unsigned char *volatile ap = A, *volatile bp = B;
    for (long i = 0; i < n; i++) sink += f(ap, bp, len);
}
__attribute__((noinline)) static void run_mix(cmp_fn volatile *fp, long n) {
    cmp_fn f = *fp;
    const unsigned char *volatile ap = A, *volatile bp = B;
    for (long i = 0; i < n; i++) sink += f(ap, bp, SZ[i & 255]);
}

/* ------------------------------------------------------------------ counters */
#define MAXP 4
#define MAXE 4
static char EVN[MAXP][MAXE][16];
static int FD[MAXP][MAXE], NPASS, NEV[MAXP];

static int raw_type(void) {
    static const char *p[] = {"/sys/bus/event_source/devices/cpu/type",
                              "/sys/bus/event_source/devices/cpu_atom/type"};
    for (int i = 0; i < 2; i++) {
        FILE *f = fopen(p[i], "r");
        int t;
        if (f) { int ok = fscanf(f, "%d", &t) == 1; fclose(f); if (ok) return t; }
    }
    return PERF_TYPE_RAW;
}

static int ev_open(const char *name, int group) {
    struct perf_event_attr a;
    memset(&a, 0, sizeof a);
    a.size = sizeof a;
    if (!strcmp(name, "instructions")) { a.type = PERF_TYPE_HARDWARE; a.config = PERF_COUNT_HW_INSTRUCTIONS; }
    else if (!strcmp(name, "cycles")) { a.type = PERF_TYPE_HARDWARE; a.config = PERF_COUNT_HW_CPU_CYCLES; }
    else if (!strcmp(name, "branches")) { a.type = PERF_TYPE_HARDWARE; a.config = PERF_COUNT_HW_BRANCH_INSTRUCTIONS; }
    else if (!strcmp(name, "brmiss")) { a.type = PERF_TYPE_HARDWARE; a.config = PERF_COUNT_HW_BRANCH_MISSES; }
    else if (name[0] == 'r') { a.type = (uint32_t)raw_type(); a.config = strtoull(name + 1, NULL, 16); }
    else { fprintf(stderr, "unknown event %s\n", name); exit(2); }
    a.disabled = group < 0;
    a.exclude_kernel = 1;
    a.exclude_hv = 1;
    a.read_format = PERF_FORMAT_GROUP;
    int fd = (int)syscall(SYS_perf_event_open, &a, 0, -1, group, 0);
    if (fd < 0) { fprintf(stderr, "perf_event_open %s failed\n", name); exit(3); }
    return fd;
}

static void parse_events(const char *s) {
    char buf[512];
    snprintf(buf, sizeof buf, "%s", s);
    char *save1, *pass = strtok_r(buf, ";", &save1);
    while (pass && NPASS < MAXP) {
        char *save2, *e = strtok_r(pass, ",", &save2);
        while (e && NEV[NPASS] < MAXE) {
            snprintf(EVN[NPASS][NEV[NPASS]], 16, "%s", e);
            FD[NPASS][NEV[NPASS]] = ev_open(e, NEV[NPASS] ? FD[NPASS][0] : -1);
            NEV[NPASS]++;
            e = strtok_r(NULL, ",", &save2);
        }
        NPASS++;
        pass = strtok_r(NULL, ";", &save1);
    }
}

static void count_once(int p, int k, size_t len, int mix, long n, double *out) {
    uint64_t buf[1 + MAXE];
    int lead = FD[p][0];
    ioctl(lead, PERF_EVENT_IOC_RESET, PERF_IOC_FLAG_GROUP);
    ioctl(lead, PERF_EVENT_IOC_ENABLE, PERF_IOC_FLAG_GROUP);
    if (mix) run_mix(&FP[k], n); else run(&FP[k], len, n);
    ioctl(lead, PERF_EVENT_IOC_DISABLE, PERF_IOC_FLAG_GROUP);
    if (read(lead, buf, sizeof buf) < (ssize_t)(8 * (1 + NEV[p]))) { fprintf(stderr, "read\n"); exit(3); }
    for (int e = 0; e < NEV[p]; e++) out[e] = (double)buf[1 + e];
}

static int dcmp(const void *x, const void *y) {
    double a = *(const double *)x, b = *(const double *)y;
    return (a > b) - (a < b);
}

/* per-iteration value of every event of every pass: median over 5 of (count(N) - count(0)) / N */
static void count_row(int k, size_t len, int mix, double out[MAXP][MAXE]) {
    const long N = 4000;
    double v[5][MAXE], c0[MAXE], c1[MAXE], col[5];
    run(&FP[k], len, 2000);
    if (mix) run_mix(&FP[k], 2000);
    for (int p = 0; p < NPASS; p++) {
        for (int r = 0; r < 5; r++) {
            count_once(p, k, len, mix, N, c1);
            count_once(p, k, len, mix, 0, c0);
            for (int e = 0; e < NEV[p]; e++) v[r][e] = (c1[e] - c0[e]) / N;
        }
        for (int e = 0; e < NEV[p]; e++) {
            for (int r = 0; r < 5; r++) col[r] = v[r][e];
            qsort(col, 5, sizeof col[0], dcmp);
            out[p][e] = col[2];
        }
    }
}

/* ------------------------------------------------------------------ time */
static double now_ns(void) {
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return t.tv_sec * 1e9 + t.tv_nsec;
}
static int ROUNDS = 9;
static double time_row(int k, size_t len, int mix) {
    long N = len >= 4096 ? 4000 : len >= 1024 ? 10000 : 40000;
    double r[64];
    if (mix) run_mix(&FP[k], N); else run(&FP[k], len, N);
    for (int i = 0; i < ROUNDS; i++) {
        double t0 = now_ns();
        if (mix) run_mix(&FP[k], N); else run(&FP[k], len, N);
        r[i] = (now_ns() - t0) / N;
    }
    qsort(r, ROUNDS, sizeof r[0], dcmp);
    return r[ROUNDS / 2];
}

/* ------------------------------------------------------------------ check */
static long nfail, nchk;
static int ref(const unsigned char *a, const unsigned char *b, size_t n) {
    for (size_t i = 0; i < n; i++) if (a[i] != b[i]) return a[i] < b[i] ? -1 : 1;
    return 0;
}
static int sgn(int x) { return (x > 0) - (x < 0); }
static void chk1(const unsigned char *a, const unsigned char *b, size_t n) {
    int r = ref(a, b, n);
    for (int k = 0; k < NFN; k++) {
        int v = FNS[k].f(a, b, n);
        int ok = FNS[k].ordered ? sgn(v) == r : (r == 0) == (v == 0);
        if (FNS[k].proto && !FNS[k].ordered) ok = ok && (v == 0 || v == 1);
        nchk++;
        if (!ok && nfail++ < 20)
            printf("FAIL %s n=%zu a=%p b=%p ref=%d got=%d proto_avx2=%d\n", FNS[k].name, n, (void *)a,
                   (void *)b, r, v, proto_use_avx2);
    }
}
static void check_n(unsigned char *a, unsigned char *b, size_t n, unsigned *seed) {
    for (size_t i = 0; i < n; i++) { *seed = *seed * 1103515245u + 12345u; a[i] = (unsigned char)(*seed >> 16); }
    memcpy(b, a, n);
    chk1(a, b, n);
    static const unsigned char pv[3][2] = {{0x80, 0x7f}, {0x7f, 0x80}, {0x00, 0xff}};
    for (size_t p = 0; p < n; p++) {
        if (n > 300 && p >= 70 && p < n - 70 && p != n / 2) continue;
        unsigned char sa = a[p];
        for (int j = 0; j < 3; j++) { a[p] = pv[j][0]; b[p] = pv[j][1]; chk1(a, b, n); }
        a[p] = b[p] = sa;
    }
}
static void check_all(void) {
    const size_t pg = 4096;
    unsigned char *m = mmap(NULL, 6 * pg, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (m == MAP_FAILED) { perror("mmap"); exit(1); }
    mprotect(m + 2 * pg, pg, PROT_NONE);
    mprotect(m + 5 * pg, pg, PROT_NONE);
    static const size_t big[] = {511, 512, 513, 1000, 1023, 1024, 1025, 2047, 2048, 2049, 4095, 4096, 4097, 8000};
    unsigned seed = 1;
    for (size_t i = 0; i <= 300 + sizeof big / sizeof big[0]; i++) {
        size_t n = i <= 300 ? i : big[i - 301];
        check_n(m + 2 * pg - n, m + 5 * pg - n, n, &seed);           /* both end at a guard page */
        check_n(m + 2 * pg - n, m + 3 * pg + 3, n, &seed);           /* b misaligned, a at the guard */
        check_n(m + 5, m + 5 * pg - n, n, &seed);                    /* a misaligned, b at the guard */
    }
    munmap(m, 6 * pg);
}

/* ------------------------------------------------------------------ rows */
static const char *KIND[] = {"eq", "head", "tail", "diff8", "mix16", "mix64"};
static void setup(size_t len, int kind) {
    memset(A, 0x5A, 8256);
    memset(B, 0x5A, 8256);
    if (kind == 1) B[0] = 0x5B;
    if (kind == 2) B[len - 1] = 0x5B;
    if (kind == 3) B[8] = 0x5B;
    if (kind >= 4) {
        unsigned s = 7, top = kind == 4 ? 16 : 64;
        for (int i = 0; i < 256; i++) { s = s * 1103515245u + 12345u; SZ[i] = 1 + (s >> 16) % top; }
    }
}

static int has_avx2(void) {
    unsigned a, b, c, d, lo, hi;
    if (!__get_cpuid(1, &a, &b, &c, &d) || !(c & (1u << 27)) || !(c & (1u << 28))) return 0;
    __asm__ volatile("xgetbv" : "=a"(lo), "=d"(hi) : "c"(0));
    if ((lo & 6) != 6) return 0;
    if (!__get_cpuid_count(7, 0, &a, &b, &c, &d)) return 0;
    return (b >> 5) & 1;
}

int main(int argc, char **argv) {
    const char *mode = argc > 1 ? argv[1] : "count";
    int avx2 = has_avx2();
    const char *pe = getenv("PROTO_AVX2");
    proto_use_avx2 = (unsigned char)(avx2 && !(pe && pe[0] == '0'));
    for (int k = 0; k < NFN; k++) FP[k] = FNS[k].f;
    FP[NFN] = proto_null;
    unsigned brand[12] = {0};
    for (unsigned i = 0; i < 3; i++)
        __get_cpuid(0x80000002u + i, &brand[4 * i], &brand[4 * i + 1], &brand[4 * i + 2], &brand[4 * i + 3]);
    printf("H variant=%s mode=%s cpu_avx2=%d proto_avx2=%d tunables=%s cpu=%.48s\n", VARIANT, mode, avx2,
           proto_use_avx2, getenv("GLIBC_TUNABLES") ? getenv("GLIBC_TUNABLES") : "-", (char *)brand);
    if (!strcmp(mode, "check")) {
        check_all();
        if (avx2) { unsigned char s = proto_use_avx2; proto_use_avx2 = !s; check_all(); proto_use_avx2 = s; }
        printf("CHECK %s: %ld checks, %ld failures\n", nfail ? "FAIL" : "ok", nchk, nfail);
        return nfail != 0;
    }
    A = aligned_alloc(64, 8320);
    B = aligned_alloc(64, 8320);
    if (getenv("PC_ROUNDS")) ROUNDS = atoi(getenv("PC_ROUNDS"));
    if (ROUNDS < 1 || ROUNDS > 63) ROUNDS = 9;
    int counting = !strcmp(mode, "count");
    if (counting) parse_events(getenv("PC_EVENTS") ? getenv("PC_EVENTS") : "instructions,cycles,branches");
    static const size_t sizes[] = {8, 16, 32, 64, 96, 127, 128, 144, 256, 1024, 4096};
    double nul[2][MAXP][MAXE];
    if (counting) {
        setup(64, 0); count_row(NFN, 64, 0, nul[0]);
        setup(64, 4); count_row(NFN, 64, 1, nul[1]);
        printf("N null:");
        for (int p = 0; p < NPASS; p++)
            for (int e = 0; e < NEV[p]; e++) printf(" %s=%.2f/%.2f", EVN[p][e], nul[0][p][e], nul[1][p][e]);
        printf("\n");
    } else {
        setup(64, 0);
        printf("N null: ns=%.3f\n", time_row(NFN, 64, 0));
    }
    for (int k = 0; k < NFN; k++)
        for (int si = 0; si < (int)(sizeof sizes / sizeof sizes[0]) + 1; si++)
            for (int kind = 0; kind < 4; kind++) {
                size_t len = si < 11 ? sizes[si] : 0;
                int mix = si == 11;
                int kd = mix ? 4 + (kind & 1) : kind;
                if (mix && kind > 1) continue;
                if (!mix && kind == 3 && len <= 8) continue;
                setup(len, kd);
                printf("R %s %s %zu %s", VARIANT, FNS[k].name, len, KIND[kd]);
                if (counting) {
                    double out[MAXP][MAXE];
                    count_row(k, len, mix, out);
                    for (int p = 0; p < NPASS; p++)
                        for (int e = 0; e < NEV[p]; e++)
                            printf(" %s=%.2f", EVN[p][e], out[p][e] - nul[mix][p][e]);
                } else {
                    printf(" ns=%.3f", time_row(k, len, mix));
                }
                printf("\n");
            }
    return 0;
}
