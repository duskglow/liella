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
