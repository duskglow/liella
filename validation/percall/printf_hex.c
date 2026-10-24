/* PRINTF-HEX: short hex conversions on a stream (md5sum prints its digest as 16 x "%02x"; spec from MD5SUM-PAR). */
#include "percall.h"

CASE(printf_hex16, 100000, 1) { /* a digest printed byte by byte with %02x, then the name */
    FILE *f = fopen("/dev/null", "w");
    for (long i = 0; i < n; i++) {
        for (int k = 0; k < 16; k++) pc_sink += fprintf(f, "%02x", (unsigned)(i + k) & 0xff);
        pc_sink += fprintf(f, "  %s\n", "words.txt");
    }
    PC_CHECKING pc_check("%d", fprintf(f, "%02x", 0xab));
}
CASE(printf_02x, 1000000, 1) { /* one %02x conversion alone */
    FILE *f = fopen("/dev/null", "w");
    for (long i = 0; i < n; i++) pc_sink += fprintf(f, "%02x", (unsigned)i & 0xff);
    PC_CHECKING { unsigned volatile v = 0x0c; pc_check("%d", fprintf(f, "%02x", v)); }
}
CASE(printf_name_line, 1000000, 1) { /* the "  %s\n" tail md5sum prints after the digest */
    FILE *f = fopen("/dev/null", "w");
    for (long i = 0; i < n; i++) pc_sink += fprintf(f, "  %s\n", "words.txt");
    PC_CHECKING pc_check("%d", fprintf(f, "  %s\n", "words.txt"));
}
