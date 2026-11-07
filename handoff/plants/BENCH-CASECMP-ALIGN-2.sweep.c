/* BENCH-CASECMP-ALIGN-2: strcasecmp/strncasecmp instructions per call over operand page offsets.
 * a and b live in separate 3-page blocks; A<oa>_B<ob> places a at page offset oa of page 0 (oa may be
 * >= 4096: page 1), b likewise. 256 bytes equal after folding (b has odd letters upper-cased), NUL at 256. */
#include "percall.h"
#include <strings.h>

static _Alignas(4096) char pa[3][4096];
static _Alignas(4096) char pb[3][4096];
static int (*volatile scase)(const char *, const char *) = strcasecmp;
static int (*volatile sncase)(const char *, const char *, size_t) = strncasecmp;

static void fill(char *a, char *b, int len) {
    memset(pa, 'q', sizeof pa);
    memset(pb, 'q', sizeof pb);
    for (int i = 0; i < len; i++) {
        a[i] = "abcdefghijklmnopqrstuvwxyz0123456789 .-_"[i % 40];
        b[i] = (i & 1) && a[i] >= 'a' && a[i] <= 'z' ? (char)(a[i] - 32) : a[i];
    }
    a[len] = 0;
    b[len] = 0;
}

#define SW(nm, oa, ob, len) \
    CASE_LOCALE(nm, 0, 1, "C") { \
        char *a = &pa[0][0] + (oa), *b = &pb[0][0] + (ob); \
        fill(a, b, len); \
        for (long i = 0; i < n; i++) pc_sink += scase(a, b); \
        PC_CHECKING { pc_check("%d", PC_SIGN(scase(a, b))); } \
    }
#define SN(nm, oa, ob, len, bound) \
    CASE_LOCALE(nm, 0, 1, "C") { \
        char *a = &pa[0][0] + (oa), *b = &pb[0][0] + (ob); \
        fill(a, b, len); \
        for (long i = 0; i < n; i++) pc_sink += sncase(a, b, bound); \
        PC_CHECKING { pc_check("%d", PC_SIGN(sncase(a, b, bound))); } \
    }

SW(s256_a000_b000, 0x000, 0x000, 256)
SW(s256_a000_b800, 0x000, 0x800, 256)
SW(s256_a001_b001, 0x001, 0x001, 256)
SW(s256_a001_b013, 0x001, 0x013, 256)
SW(s256_a007_b020, 0x007, 0x020, 256)
SW(s256_a010_b013, 0x010, 0x013, 256)
SW(s256_a01f_b03f, 0x01f, 0x03f, 256)
SW(s256_a020_b000, 0x020, 0x000, 256)
SW(s256_a03f_b001, 0x03f, 0x001, 256)
SW(s256_a040_b840, 0x040, 0x840, 256)
SW(s256_a000_bf00, 0x000, 0xf00, 256)
SW(s256_a013_bf00, 0x013, 0xf00, 256)
SW(s256_a013_bf80, 0x013, 0xf80, 256)
SW(s256_a013_bfc8, 0x013, 0xfc8, 256)
SW(s256_a013_bfe9, 0x013, 0xfe9, 256)
SW(s256_a013_bff7, 0x013, 0xff7, 256)
SW(s256_afd0_bfe8, 0xfd0, 0xfe8, 256)
SW(s256_aff0_b013, 0xff0, 0x013, 256)
SW(s256_aff0_b005, 0xff0, 0x005, 256)
SW(s256_affc_bff1, 0xffc, 0xff1, 256)
SW(s16_a000_b800, 0x000, 0x800, 16)
SW(s16_a013_b021, 0x013, 0x021, 16)
SW(s16_aff8_b013, 0xff8, 0x013, 16)
SN(n64_a000_b800, 0x000, 0x800, 256, 64)
SN(n64_a013_b021, 0x013, 0x021, 256, 64)
SN(n64_a001_b001, 0x001, 0x001, 256, 64)
SN(n64_a013_bfe0, 0x013, 0xfe0, 256, 64)
SN(n64_affa_b013, 0xffa, 0x013, 256, 64)
