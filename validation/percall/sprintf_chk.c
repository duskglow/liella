/* sprintf_chk.c (SPRINTF-CHK-LS): the __sprintf_chk shapes real programs use (ls -l nlink column, gawk's
 * "*.*%c"), plus the plain sprintf twins, through volatile pointers so gcc cannot fold them. */
#include "percall.h"

extern int __sprintf_chk(char *, int, size_t, const char *, ...);
static int (*volatile chk_fn)(char *, int, size_t, const char *, ...) = __sprintf_chk;
static int (*volatile spr_fn)(char *, const char *, ...) = sprintf;

CASE(chk_ls, 1000000, 1) { /* ls -l: sprintf(p, "%s %*s ", modebuf, nlink_width, nlink) */
    char b[64];
    for (long i = 0; i < n; i++) pc_sink += chk_fn(b, 1, sizeof b, "%s %*s ", "-rw-r--r--", 2, (i & 1) ? "1" : "12");
    PC_CHECKING pc_check("%d", chk_fn(b, 1, sizeof b, "%s %*s ", "-rw-r--r--", 2, "1"));
}
CASE(spr_ls, 1000000, 1) { /* the same, plain sprintf */
    char b[64];
    for (long i = 0; i < n; i++) pc_sink += spr_fn(b, "%s %*s ", "-rw-r--r--", 2, (i & 1) ? "1" : "12");
    PC_CHECKING pc_check("%d", spr_fn(b, "%s %*s ", "-rw-r--r--", 2, "1"));
}
CASE(chk_cpc, 1000000, 1) { /* gawk format_float: sprintf(cp, "*.*%c", 'f'), fortified */
    char b[32];
    volatile char conv = 'f';
    for (long i = 0; i < n; i++) pc_sink += chk_fn(b, 1, sizeof b, "*.*%c", conv);
    PC_CHECKING pc_check("%d", chk_fn(b, 1, sizeof b, "*.*%c", conv));
}
CASE(chk_lu, 1000000, 1) { /* a short unsigned long column */
    char b[32];
    for (long i = 0; i < n; i++) pc_sink += chk_fn(b, 1, sizeof b, "%lu", (unsigned long)(i & 1023));
    PC_CHECKING pc_check("%d", chk_fn(b, 1, sizeof b, "%lu", 1023UL));
}
