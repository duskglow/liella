/* wl_percall spec (MEMCMP-EQ-24-32): instructions per call of memcmp, bcmp and __memcmpeq at the folded
 * sizes 8-64 and the first sizes above, Liella vs glibc. "eq": the regions are equal over all n bytes;
 * "hd": they differ at byte 3 (inside the 8-byte head). Misaligned pointers (a + 1, b + 5). Calls go
 * through a volatile function pointer so the compiler cannot expand a constant-size compare inline.
 *   bash validation/wl_percall.sh --spec validation/percall/memcmp_eq32.c <dir|rev> */
#include "percall.h"

typedef int (*pc_cmp_fn)(const void *, const void *, size_t);
extern int __memcmpeq(const void *, const void *, size_t);
static pc_cmp_fn volatile pc_memcmp = memcmp;
static pc_cmp_fn volatile pc_bcmp = bcmp;
static pc_cmp_fn volatile pc_meq = __memcmpeq;
static unsigned char pc_a[256] __attribute__((aligned(64)));
static unsigned char pc_b[256] __attribute__((aligned(64)));

static void pc_run(pc_cmp_fn volatile *fp, size_t len, int head, int sign, long n) {
    memset(pc_a, 'x', sizeof pc_a);
    memset(pc_b, 'x', sizeof pc_b);
    if (head) pc_b[5 + 3] = 'y';
    pc_cmp_fn f = *fp;
    const unsigned char *volatile ap = pc_a + 1;
    const unsigned char *volatile bp = pc_b + 5;
    volatile size_t vn = len;
    const unsigned char *a = ap, *b = bp;
    size_t m = vn;
    for (long i = 0; i < n; i++) pc_sink += f(a, b, m);
    PC_CHECKING {
        int r = f(ap, bp, vn);
        pc_check("%d", sign ? PC_SIGN(r) : r != 0);
    }
}

#define PC_BAND(fn, len)                                                    \
    CASE(fn##_eq_##len, 0, 1) { (void)dir; pc_run(&pc_##fn, len, 0, sizeof(#fn) == 7, n); } \
    CASE(fn##_hd_##len, 0, 1) { (void)dir; pc_run(&pc_##fn, len, 1, sizeof(#fn) == 7, n); }
#define PC_SIZES(fn)                                                        \
    PC_BAND(fn, 8) PC_BAND(fn, 12) PC_BAND(fn, 16) PC_BAND(fn, 17) PC_BAND(fn, 24) PC_BAND(fn, 32) \
    PC_BAND(fn, 33) PC_BAND(fn, 48) PC_BAND(fn, 64) PC_BAND(fn, 96) PC_BAND(fn, 128)

PC_SIZES(memcmp)
PC_SIZES(bcmp)
PC_SIZES(meq)
