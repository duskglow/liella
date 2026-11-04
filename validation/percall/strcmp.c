/* strcmp per call (PERF-STRCMP-MIX): equal strings of 16 and 64 bytes at 64 start alignments (both strings
 * chosen from a volatile table so the call is never folded), and short names that differ early. */
#include "percall.h"

#define SL 192
static char pool_a[64 * SL + 64], pool_b[64 * SL + 64];
static volatile unsigned char ia[64], ib[64];
static const char *const nm[8] = {"alpha.c", "alpha.h", "beta_long_name.txt", "gamma", "delta.o", "epsilon.rs", "zeta", "eta.S"};
static char nbuf_a[8][32], nbuf_b[8][32];

static void sc_fill(size_t n)
{
    unsigned s = 12345u;
    for (int i = 0; i < 64; i++) {
        s = s * 1103515245u + 12345u;
        ia[i] = (s >> 16) & 63;
        s = s * 1103515245u + 12345u;
        ib[i] = (s >> 16) & 63;
    }
    for (int o = 0; o < 64; o++) {
        for (size_t k = 0; k < n; k++) { pool_a[o * SL + o + k] = 'x'; pool_b[o * SL + o + k] = 'x'; }
        pool_a[o * SL + o + n] = 0;
        pool_b[o * SL + o + n] = 0;
    }
}

CASE(strcmp_16_mix, 0, 1) {
    sc_fill(16);
    long acc = 0;
    for (long i = 0; i < n; i++) acc += strcmp(pool_a + ia[i & 63] * SL + ia[i & 63], pool_b + ib[(i * 7) & 63] * SL + ib[(i * 7) & 63]);
    pc_sink += acc;
    PC_CHECKING { pc_check("%d", strcmp(pool_a + ia[1] * SL + ia[1], pool_b + ib[2] * SL + ib[2])); }
}

CASE(strcmp_64_mix, 0, 1) {
    sc_fill(64);
    long acc = 0;
    for (long i = 0; i < n; i++) acc += strcmp(pool_a + ia[i & 63] * SL + ia[i & 63], pool_b + ib[(i * 7) & 63] * SL + ib[(i * 7) & 63]);
    pc_sink += acc;
    PC_CHECKING { pc_check("%d", strcmp(pool_a + ia[1] * SL + ia[1], pool_b + ib[2] * SL + ib[2])); }
}

CASE(strcmp_names, 0, 1) {
    for (int i = 0; i < 8; i++) { strcpy(nbuf_a[i], nm[i]); strcpy(nbuf_b[i], nm[(i * 3 + 1) & 7]); }
    long acc = 0;
    for (long i = 0; i < n; i++) acc += strcmp(nbuf_a[i & 7], nbuf_b[(i * 5 + 3) & 7]) > 0;
    pc_sink += acc;
    PC_CHECKING { pc_check("%d", PC_SIGN(strcmp(nbuf_a[(long)ia[0] & 7], nbuf_b[(long)ib[0] & 7]))); }
}
