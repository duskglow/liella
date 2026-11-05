/* PARITY-ROWS-1105N probe: nl_langinfo, isalpha, __ctype_b_loc, strnlen (16 bytes) per-call instructions. */
#include "percall.h"
#include <ctype.h>
#include <langinfo.h>
#include <string.h>

CASE(nl_langinfo_mon, 10000000, 1) {
    for (long i = 0; i < n; i++) pc_sink += nl_langinfo((nl_item)(ABMON_1 + (i & 7)))[0];
    PC_CHECKING pc_check("%c", nl_langinfo(ABMON_1)[0]);
}
CASE(isalpha_c, 10000000, 1) {
    for (long i = 0; i < n; i++) pc_sink += isalpha((int)(i & 127));
    PC_CHECKING pc_check("%d", isalpha('q') != 0);
}
CASE(ctype_b_loc, 20000000, 1) {
    const unsigned short **(*volatile fn)(void) = __ctype_b_loc;
    for (long i = 0; i < n; i++) pc_sink += fn() != 0;
    PC_CHECKING pc_check("%d", fn() != 0);
}
CASE(strnlen_16, 10000000, 1) {
    static char buf[64] = "0123456789abcdef0123456789abcdef";
    size_t volatile lim = 16;
    for (long i = 0; i < n; i++) pc_sink += (long)strnlen(buf, lim);
    PC_CHECKING pc_check("%zu", strnlen(buf, lim));
}
