/* PERF-QSORT-OFFSET-EXP: qsort with a plain C comparator on a 4-byte key, the cbench_unmeasured rows
 * (qsort_c_cmp_1024, _e4, _e8, _e64) as per-call cases: one sort of N elements of SIZE bytes, the master array
 * copied back before each sort (the copy is counted, about N * SIZE / 32 instructions). Divide a row by the
 * comparisons per sort (qs_cmps_<case>, printed in check mode) for "per comparison"; the comparator is
 * 7 instructions or so on both sides. The _256 cases are short enough to single-step with wl_trace.sh. */
#include "percall.h"
#include <stdint.h>

static long qs_ncmp;
static int qs_cmp(const void *a, const void *b) {
    int x = *(const int *)a, y = *(const int *)b;
    return (x > y) - (x < y);
}
static int qs_cmp_counting(const void *a, const void *b) {
    qs_ncmp++;
    return qs_cmp(a, b);
}

static unsigned char qs_master[1024 * 64], qs_work[1024 * 64];

static void qs_fill(size_t n, size_t size) {
    uint32_t s = 12345;
    memset(qs_master, 0, n * size);
    for (size_t i = 0; i < n; i++) {
        s = s * 1103515245u + 12345u;
        int k = (int)(s >> 8);
        memcpy(qs_master + i * size, &k, sizeof k);
    }
}

static int qs_sorted(size_t n, size_t size) {
    for (size_t i = 1; i < n; i++)
        if (qs_cmp(qs_work + (i - 1) * size, qs_work + i * size) > 0) return 0;
    return 1;
}

static void qs_run(long n, size_t cnt, size_t size) {
    qs_fill(cnt, size);
    for (long i = 0; i < n; i++) {
        memcpy(qs_work, qs_master, cnt * size);
        qsort(qs_work, cnt, size, qs_cmp);
        pc_sink += qs_work[0];
    }
    PC_CHECKING {
        memcpy(qs_work, qs_master, cnt * size);
        qs_ncmp = 0;
        qsort(qs_work, cnt, size, qs_cmp_counting);
        /* The comparison count is printed to stderr: it may differ between the two sides by design. */
        fprintf(stderr, "qsort %zu x %zu: %ld comparisons\n", cnt, size, qs_ncmp);
        pc_check("%d", qs_sorted(cnt, size));
    }
}

CASE(qsort_e4_1024, 0, 1) { qs_run(n, 1024, 4); }
CASE(qsort_e8_1024, 0, 1) { qs_run(n, 1024, 8); }
CASE(qsort_e64_1024, 0, 1) { qs_run(n, 1024, 64); }
CASE(qsort_e4_256, 0, 1) { qs_run(n, 256, 4); }
CASE(qsort_e64_256, 0, 1) { qs_run(n, 256, 64); }

/* The comparator alone, as many calls as one 1,024-element sort makes (about 8,900): the floor both sides share. */
static int (*volatile qs_cmp_ptr)(const void *, const void *) = qs_cmp;
CASE(qsort_cmp_only_8900, 0, 1) {
    qs_fill(1024, 4);
    for (long i = 0; i < n; i++) {
        int acc = 0;
        for (int j = 0; j < 8900; j++) acc += qs_cmp_ptr(qs_master + ((j * 4) & 4092), qs_master + ((j * 4 + 4) & 4092));
        pc_sink += acc;
    }
    PC_CHECKING pc_check("%d", 1);
}
