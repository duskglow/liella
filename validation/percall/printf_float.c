/* PRINTF-FLOAT: float conversions in the shapes real programs use (gawk-printf formats i/7 with "%.3f" for
 * i < 1e6 through snprintf; %g and the default %f are the other common shapes). Arguments vary per call so
 * every rounding position is exercised; checked calls take their value through a volatile. */
#include "percall.h"

CASE(snprintf_f3_div7, 1000000, 1) { /* gawk-printf: "%.3f" of i / 7 */
    char b[64];
    for (long i = 0; i < n; i++) pc_sink += snprintf(b, sizeof b, "%.3f", (double)(i & 0xfffff) / 7);
    PC_CHECKING { double volatile v = 142857.0 / 7; snprintf(b, sizeof b, "%.3f", v); pc_check("%s", b); }
}
CASE(snprintf_f_default, 100000, 1) { /* "%f" (precision 6) of a mid-range value */
    char b[64];
    for (long i = 0; i < n; i++) pc_sink += snprintf(b, sizeof b, "%f", (double)(i & 0xfffff) * 0.37);
    PC_CHECKING { double volatile v = 1234.5678; snprintf(b, sizeof b, "%f", v); pc_check("%s", b); }
}
CASE(snprintf_f2_small, 100000, 1) { /* "%.2f" of values below 1 (money/percent shapes) */
    char b[64];
    for (long i = 0; i < n; i++) pc_sink += snprintf(b, sizeof b, "%.2f", (double)(i & 0xffff) / 65536.0);
    PC_CHECKING { double volatile v = 0.125; snprintf(b, sizeof b, "%.2f", v); pc_check("%s", b); }
}
CASE(snprintf_g_div7, 100000, 1) { /* "%g" (awk's default OFMT/CONVFMT is "%.6g") of i / 7 */
    char b[64];
    for (long i = 0; i < n; i++) pc_sink += snprintf(b, sizeof b, "%.6g", (double)(i & 0xfffff) / 7);
    PC_CHECKING { double volatile v = 142857.0 / 7; snprintf(b, sizeof b, "%.6g", v); pc_check("%s", b); }
}
/* PRINTF-G: the other %g shapes awk produces. */
CASE(snprintf_g_default, 100000, 1) { /* "%g" (precision 6) of mid-range values with short digit strings */
    char b[64];
    for (long i = 0; i < n; i++) pc_sink += snprintf(b, sizeof b, "%g", (double)(i & 0xfffff) * 0.25);
    PC_CHECKING { double volatile v = 1234.75; snprintf(b, sizeof b, "%g", v); pc_check("%s", b); }
}
CASE(snprintf_g6_sum, 100000, 1) { /* gawk-sum: "%.6g" of a large running sum (e-style, carries) */
    char b[64];
    for (long i = 0; i < n; i++) pc_sink += snprintf(b, sizeof b, "%.6g", (double)(i & 0xfffff) * 1234567.891 + 0.5);
    PC_CHECKING { double volatile v = 999999.5; snprintf(b, sizeof b, "%.6g", v); pc_check("%s", b); }
}
CASE(snprintf_g6_tiny, 100000, 1) { /* "%.6g" of values below 1e-4 (e-style with a negative exponent) */
    char b[64];
    for (long i = 0; i < n; i++) pc_sink += snprintf(b, sizeof b, "%.6g", (double)((i & 0xfffff) + 1) * 1e-9 / 3);
    PC_CHECKING { double volatile v = 0.0000123456789; snprintf(b, sizeof b, "%.6g", v); pc_check("%s", b); }
}
