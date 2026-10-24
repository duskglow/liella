/* strdup family per-call counts by length (STRDUP-FAST-2). Run: validation/wl_percall.sh --spec validation/percall/strdup.c <wt> */
#include "percall.h"
#define SD(NAME, LEN, CALLS) \
CASE(NAME, CALLS, 1) { \
    char buf[LEN + 1]; for (int k = 0; k < LEN; k++) buf[k] = 'a' + k % 26; buf[LEN] = 0; \
    const char *volatile s = buf; \
    for (long i = 0; i < n; i++) { char *p = strdup(s); pc_sink += p[0]; free(p); } \
    PC_CHECKING { char *p = strdup(s); pc_check("%s", p); free(p); } \
}
#define SN(NAME, LEN, LIM, CALLS) \
CASE(NAME, CALLS, 1) { \
    char buf[LEN + 1]; for (int k = 0; k < LEN; k++) buf[k] = 'a' + k % 26; buf[LEN] = 0; \
    const char *volatile s = buf; \
    for (long i = 0; i < n; i++) { char *p = strndup(s, LIM); pc_sink += p[0]; free(p); } \
    PC_CHECKING { char *p = strndup(s, LIM); pc_check("%s", p); free(p); } \
}
SD(strdup_0, 0, 1000)
SD(strdup_4, 4, 1000)
SD(strdup_10, 10, 1000)
SD(strdup_16, 16, 1000)
SD(strdup_17, 17, 1000)
SD(strdup_40, 40, 1000)
SD(strdup_200, 200, 1000)
SN(strndup_0_lim0, 0, 0, 1000)
SN(strndup_4_lim100, 4, 100, 1000)
SN(strndup_40_lim100, 40, 100, 1000)
SN(strndup_200_lim100, 200, 100, 1000)
