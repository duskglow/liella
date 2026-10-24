/* argv[1] = charset. stdin lines: "D <hex bytes>" decode to code points, "E <hex code point>"
 * encode. Prints one line each: code points / bytes in hex, or "ERR", or "SKIP" when the
 * charset is unknown to iconv. */
#include <errno.h>
#include <iconv.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

int main(int argc, char **argv) {
    if (argc < 2) return 2;
    iconv_t dec = iconv_open("UTF-32LE", argv[1]);
    iconv_t enc = iconv_open(argv[1], "UTF-32LE");
    char line[256];
    while (fgets(line, sizeof line, stdin)) {
        if (dec == (iconv_t)-1 || enc == (iconv_t)-1) { puts("SKIP"); continue; }
        char kind = line[0];
        unsigned char in[16]; size_t n = 0;
        if (kind == 'D') {
            for (char *p = line + 2; p[0] && p[1] && p[0] != '\n'; p += 2) { unsigned v; sscanf(p, "%2x", &v); in[n++] = (unsigned char)v; }
            unsigned char out[64]; char *ip = (char *)in, *op = (char *)out; size_t il = n, ol = sizeof out;
            iconv(dec, NULL, NULL, NULL, NULL);
            size_t r = iconv(dec, &ip, &il, &op, &ol);
            if (r == (size_t)-1 || il != 0) { puts("ERR"); continue; }
            /* Flush: some modules hold a base letter back for a combining mark that may follow. */
            iconv(dec, NULL, NULL, &op, &ol);
            for (unsigned char *q = out; q < (unsigned char *)op; q += 4) printf("%s%x", q == out ? "" : " ", q[0] | q[1] << 8 | q[2] << 16 | (unsigned)q[3] << 24);
            putchar('\n');
        } else {
            unsigned cp = (unsigned)strtoul(line + 2, NULL, 16);
            unsigned char u[4] = { cp & 255, cp >> 8 & 255, cp >> 16 & 255, cp >> 24 };
            unsigned char out[16]; char *ip = (char *)u, *op = (char *)out; size_t il = 4, ol = sizeof out;
            iconv(enc, NULL, NULL, NULL, NULL);
            size_t r = iconv(enc, &ip, &il, &op, &ol);
            if (r == (size_t)-1) { puts("ERR"); continue; }
            /* Flush any shift state. */
            iconv(enc, NULL, NULL, &op, &ol);
            for (unsigned char *q = out; q < (unsigned char *)op; q++) printf("%02x", *q);
            putchar('\n');
        }
    }
    return 0;
}
