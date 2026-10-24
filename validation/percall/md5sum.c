/* MD5SUM-PAR: the per-call libc work of md5sum's steady state (fread of 32 KiB blocks) and its digest output. */
#include "percall.h"

static char md5_buf[32768 + 72] __attribute__((aligned(64)));

CASE(fread_32k, 20000, 1) { /* md5_stream: fread(buf + sum, 1, BLOCKSIZE - sum, stream) of a regular stream */
    FILE *f = fopen("/dev/zero", "r");
    for (long i = 0; i < n; i++) pc_sink += fread(md5_buf, 1, 32768, f);
    PC_CHECKING pc_check("%zu %d", fread(md5_buf, 1, 32768, f), md5_buf[100]);
}
CASE(fread_32k_feof, 20000, 1) { /* the same plus the feof/ferror checks md5_stream does per block */
    FILE *f = fopen("/dev/zero", "r");
    for (long i = 0; i < n; i++) { pc_sink += fread(md5_buf, 1, 32768, f); pc_sink += feof(f) + ferror(f); }
    PC_CHECKING pc_check("%d %d", feof(f), ferror(f));
}
CASE(fread_small, 1000000, 1) { /* a small fread served from the buffer */
    FILE *f = fopen("/dev/zero", "r");
    for (long i = 0; i < n; i++) pc_sink += fread(md5_buf, 1, 64, f);
    PC_CHECKING pc_check("%zu", fread(md5_buf, 1, 64, f));
}
CASE(printf_hex16, 100000, 1) { /* a digest printed byte by byte with %02x, then the name */
    FILE *f = fopen("/dev/null", "w");
    for (long i = 0; i < n; i++) {
        for (int k = 0; k < 16; k++) pc_sink += fprintf(f, "%02x", (unsigned)(i + k) & 0xff);
        pc_sink += fprintf(f, "  %s\n", "words.txt");
    }
    PC_CHECKING pc_check("%d", fprintf(f, "%02x", 0xab));
}
