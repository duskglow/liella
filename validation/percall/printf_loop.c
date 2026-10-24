/* PRINTF-LAYERS: the calls coreutils printf makes for `printf "%s %d %x\n" a $i $i` (the printf-loop
 * workload): one conversion per call with the intmax_t modifier, and the `%s` of a short word. */
#include "percall.h"
#include <stdint.h>

CASE(printf_jd, 1000000, 1) { /* one %jd (coreutils printf's %d directive) */
    FILE *f = fopen("/dev/null", "w");
    for (long i = 0; i < n; i++) pc_sink += fprintf(f, "%jd", (intmax_t)(i % 20000));
    PC_CHECKING { intmax_t volatile v = 12345; pc_check("%d", fprintf(f, "%jd", v)); }
}
CASE(printf_jx, 1000000, 1) { /* one %jx (coreutils printf's %x directive) */
    FILE *f = fopen("/dev/null", "w");
    for (long i = 0; i < n; i++) pc_sink += fprintf(f, "%jx", (uintmax_t)(i % 20000));
    PC_CHECKING { uintmax_t volatile v = 0x3039; pc_check("%d", fprintf(f, "%jx", v)); }
}
CASE(printf_s, 1000000, 1) { /* one %s of a one-byte word */
    FILE *f = fopen("/dev/null", "w");
    for (long i = 0; i < n; i++) pc_sink += fprintf(f, "%s", "a");
    PC_CHECKING pc_check("%d", fprintf(f, "%s", "a"));
}
