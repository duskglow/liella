/* wl_percall spec (PERF-MEMSET-4K-3): instructions per call of memset from 1 KiB to 64 KiB, Liella vs glibc.
 * "a": a 64-byte-aligned destination; "o": the destination at +1 (unaligned head). The call goes through a
 * volatile function pointer so the compiler cannot expand a constant-size fill inline.
 *   bash validation/wl_percall.sh --spec validation/percall/memset_sizes.c <dir|rev> */
#include "percall.h"

static void *(*volatile pc_ms)(void *, int, size_t) = memset;
static unsigned char pc_buf[65536 + 128] __attribute__((aligned(64)));

static void pc_fill(size_t off, size_t len, long n) {
    unsigned char *volatile dp = pc_buf + off;
    volatile size_t vn = len;
    unsigned char *d = dp;
    size_t m = vn;
    for (long i = 0; i < n; i++) { pc_ms(d, (int)(i & 7), m); pc_sink += d[100]; }
    PC_CHECKING {
        pc_ms(dp, 0x5A, vn);
        pc_check("%d %d %d", dp[0], dp[vn - 1], dp[vn]);
    }
}

#define PC_SIZE(len)                                                          \
    CASE(memset_a_##len, 0, 1) { (void)dir; pc_fill(0, len, n); }            \
    CASE(memset_o_##len, 0, 1) { (void)dir; pc_fill(1, len, n); }

PC_SIZE(257) PC_SIZE(1024) PC_SIZE(1279) PC_SIZE(1280) PC_SIZE(2048) PC_SIZE(4096) PC_SIZE(8192)
PC_SIZE(16384) PC_SIZE(65536)
