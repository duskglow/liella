/* wl_percall spec (TIMING-COV-MEMCMP-NONAVX2): memcmp / bcmp / __memcmpeq rows of the brief, the shapes of
 * handoff/timing-coverage-1106/tc1106.c (bench/c/cbench_memref.c rows of the same name). bcmp and __memcmpeq
 * report equality only, so their check prints r != 0 (glibc's nonzero value is not specified); memcmp prints the sign.
 * Run it with and without GLIBC_TUNABLES=glibc.cpu.hwcaps=-AVX2 (the SSE2 and AVX2 builds). */
#include "percall.h"

typedef int (*cmp_fn)(const void *, const void *, size_t);
extern int __memcmpeq(const void *, const void *, size_t);
static cmp_fn volatile f_memcmp = memcmp, f_bcmp = bcmp, f_meq = __memcmpeq;
static unsigned char A[4160] __attribute__((aligned(64))), B[4160] __attribute__((aligned(64)));

/* kind: 0 equal, 1 first byte differs, 2 last byte differs, 3 byte 8 differs; eqonly: print r != 0 */
static void cmp_run(cmp_fn volatile *fp, size_t len, int kind, int eqonly, long n) {
    memset(A, 0x5A, sizeof A); memset(B, 0x5A, sizeof B);
    if (kind == 1) B[0] = 0x5B;
    if (kind == 2) B[len - 1] = 0x5B;
    if (kind == 3) B[8] = 0x5B;
    cmp_fn f = *fp;
    const unsigned char *volatile ap = A, *volatile bp = B;
    for (long i = 0; i < n; i++) pc_sink += f(ap, bp, len);
    PC_CHECKING { int r = f(ap, bp, len); pc_check("%d", eqonly ? r != 0 : PC_SIGN(r)); }
}
CASE(memcmp_diff8_64, 0, 1) { (void)dir; cmp_run(&f_memcmp, 64, 3, 0, n); }
CASE(memcmp_diff8_96, 0, 1) { (void)dir; cmp_run(&f_memcmp, 96, 3, 0, n); }
CASE(memcmp_diff8_127, 0, 1) { (void)dir; cmp_run(&f_memcmp, 127, 3, 0, n); }
CASE(memcmp_head_128, 0, 1) { (void)dir; cmp_run(&f_memcmp, 128, 1, 0, n); }
CASE(memcmp_head_1k, 0, 1) { (void)dir; cmp_run(&f_memcmp, 1024, 1, 0, n); }
CASE(memcmp_eq_64, 0, 1) { (void)dir; cmp_run(&f_memcmp, 64, 0, 0, n); }
CASE(memcmp_eq_128, 0, 1) { (void)dir; cmp_run(&f_memcmp, 128, 0, 0, n); }
CASE(memcmp_tail_128, 0, 1) { (void)dir; cmp_run(&f_memcmp, 128, 2, 0, n); }
CASE(memcmp_256, 0, 1) { (void)dir; cmp_run(&f_memcmp, 256, 0, 0, n); }
CASE(memcmp_1k, 0, 1) { (void)dir; cmp_run(&f_memcmp, 1024, 0, 0, n); }
CASE(memcmp_4k, 0, 1) { (void)dir; cmp_run(&f_memcmp, 4096, 0, 0, n); }
CASE(bcmp_head_128, 0, 1) { (void)dir; cmp_run(&f_bcmp, 128, 1, 1, n); }
CASE(bcmp_head_1k, 0, 1) { (void)dir; cmp_run(&f_bcmp, 1024, 1, 1, n); }
CASE(bcmp_eq_64, 0, 1) { (void)dir; cmp_run(&f_bcmp, 64, 0, 1, n); }
CASE(bcmp_eq_128, 0, 1) { (void)dir; cmp_run(&f_bcmp, 128, 0, 1, n); }
CASE(bcmp_eq_1k, 0, 1) { (void)dir; cmp_run(&f_bcmp, 1024, 0, 1, n); }
CASE(bcmp_tail_64, 0, 1) { (void)dir; cmp_run(&f_bcmp, 64, 2, 1, n); }
CASE(bcmp_eq_4k, 0, 1) { (void)dir; cmp_run(&f_bcmp, 4096, 0, 1, n); }
CASE(meq_head_128, 0, 1) { (void)dir; cmp_run(&f_meq, 128, 1, 1, n); }
CASE(meq_head_1k, 0, 1) { (void)dir; cmp_run(&f_meq, 1024, 1, 1, n); }
CASE(meq_eq_32, 0, 1) { (void)dir; cmp_run(&f_meq, 32, 0, 1, n); }
CASE(meq_eq_64, 0, 1) { (void)dir; cmp_run(&f_meq, 64, 0, 1, n); }
CASE(meq_eq_128, 0, 1) { (void)dir; cmp_run(&f_meq, 128, 0, 1, n); }
CASE(meq_eq_1k, 0, 1) { (void)dir; cmp_run(&f_meq, 1024, 0, 1, n); }
CASE(meq_tail_32, 0, 1) { (void)dir; cmp_run(&f_meq, 32, 2, 1, n); }
CASE(meq_tail_64, 0, 1) { (void)dir; cmp_run(&f_meq, 64, 2, 1, n); }
CASE(meq_tail_128, 0, 1) { (void)dir; cmp_run(&f_meq, 128, 2, 1, n); }
CASE(meq_diff8_100, 0, 1) { (void)dir; cmp_run(&f_meq, 100, 3, 1, n); }
