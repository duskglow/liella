/* PERF-FWRITE-4K: fwrite of a 4096-byte block to /dev/null through an 8192-byte full buffer */
#include "percall.h"
#include <stdio.h>
#include <string.h>

static char f4_buf[8192], f4_blk[4096];

CASE(fwrite_4k, 200000, 1) {
    FILE *f = fopen("/dev/null", "w");
    setvbuf(f, f4_buf, _IOFBF, sizeof f4_buf);
    memset(f4_blk, 'x', sizeof f4_blk);
    for (long i = 0; i < n; i++) pc_sink += fwrite(f4_blk, 1, sizeof f4_blk, f);
    PC_CHECKING pc_check("%zu", fwrite(f4_blk, 1, sizeof f4_blk, f));
}

#include <unistd.h>
static void *(*volatile f4_memcpy)(void *, const void *, size_t) = memcpy;
static char f4_dst[8192];

CASE(memcpy_4k_ref, 200000, 1) {
    memset(f4_blk, 'x', sizeof f4_blk);
    for (long i = 0; i < n; i++) { f4_memcpy(f4_dst + ((i & 1) << 12), f4_blk, 4096); pc_sink += f4_dst[0]; }
    PC_CHECKING pc_check("%d", (int)f4_dst[4095]);
}
CASE(write_4k_ref, 200000, 1) {
    for (long i = 0; i < n; i++) pc_sink += (unsigned long)write(1 + 0 * i, f4_blk, 0);
    PC_CHECKING pc_check("%d", 0);
}

static char f4_big[1 << 16];
CASE(fwrite_4k_win, 200000, 1) { /* 64 KiB buffer: the window copy, one flush in fifteen */
    FILE *f = fopen("/dev/null", "w");
    setvbuf(f, f4_big, _IOFBF, sizeof f4_big);
    for (long i = 0; i < n; i++) pc_sink += fwrite(f4_blk, 1, sizeof f4_blk, f);
    PC_CHECKING pc_check("%zu", fwrite(f4_blk, 1, sizeof f4_blk, f));
}
CASE(fwrite_4k_thru, 200000, 1) { /* 4 KiB buffer: written through */
    FILE *f = fopen("/dev/null", "w");
    setvbuf(f, f4_buf, _IOFBF, 4096);
    for (long i = 0; i < n; i++) pc_sink += fwrite(f4_blk, 1, sizeof f4_blk, f);
    PC_CHECKING pc_check("%zu", fwrite(f4_blk, 1, sizeof f4_blk, f));
}
CASE(memcpy_4088_ref, 200000, 1) {
    for (long i = 0; i < n; i++) { f4_memcpy(f4_dst + 8, f4_blk, 4088); pc_sink += f4_dst[9]; }
    PC_CHECKING pc_check("%d", (int)f4_dst[4095]);
}
