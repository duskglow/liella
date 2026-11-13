/* localtime_r / mktime per-call counts on a loaded zone: the zone selection's lock-free path (time_compat::select,
 * the TSC gate) runs once per call (HOTTEXT-TIME-CLUSTER-STABLE). Run: validation/wl_percall.sh --spec
 * validation/percall/time_zone.c <wt> */
#include "percall.h"
#include <stdlib.h>
#include <time.h>
#define LT(NAME, ZONE, CALLS) \
CASE(NAME, CALLS, 1) { \
    setenv("TZ", ZONE, 1); tzset(); \
    volatile time_t t0 = 1700000000; struct tm tm; \
    for (long i = 0; i < n; i++) { time_t t = t0 + i * 3601; localtime_r(&t, &tm); pc_sink += tm.tm_hour; } \
    PC_CHECKING { time_t t = t0; localtime_r(&t, &tm); pc_check("%d %d %d", tm.tm_hour, tm.tm_isdst, (int)tm.tm_gmtoff); } \
}
#define MK(NAME, ZONE, CALLS) \
CASE(NAME, CALLS, 1) { \
    setenv("TZ", ZONE, 1); tzset(); \
    struct tm base = { .tm_year = 123, .tm_mon = 10, .tm_mday = 14, .tm_hour = 12, .tm_isdst = -1 }; \
    for (long i = 0; i < n; i++) { struct tm tm = base; tm.tm_hour += (int)(i % 5000); pc_sink += (long)mktime(&tm); } \
    PC_CHECKING { struct tm tm = base; pc_check("%ld %d", (long)mktime(&tm), tm.tm_isdst); } \
}
LT(localtime_r_new_york, "America/New_York", 1000)
LT(localtime_r_utc_posix, "UTC0", 1000)
MK(mktime_new_york, "America/New_York", 1000)
MK(mktime_utc_posix, "UTC0", 1000)
