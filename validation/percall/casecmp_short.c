/* Per-call counts of short ASCII case-insensitive compares in the C locale (TIMING-COV-STRCMP-SHORT-AVX2):
 * strcasecmp of 16 bytes equal up to case, strncasecmp with n = 64 over text equal up to case. */
#include <strings.h>
#include "percall.h"

CASE_LOCALE(strcasecmp_c_16, 0, 1, "C") {
    (void)dir; const char *volatile a = "Hello World 1234", *volatile b = "hELLO wORLD 1234";
    for (long i = 0; i < n; i++) pc_sink += strcasecmp(a, b);
    PC_CHECKING pc_check("%d", PC_SIGN(strcasecmp(a, b)));
}

CASE_LOCALE(strncasecmp_c_64, 0, 1, "C") {
    (void)dir; static char ca[257], cb[257];
    for (int i = 0; i < 256; i++) {
        ca[i] = "abcdefghijklmnopqrstuvwxyz0123456789 .-_"[i % 40];
        cb[i] = (i & 1) && ca[i] >= 'a' && ca[i] <= 'z' ? (char)(ca[i] - 32) : ca[i];
    }
    const char *volatile a = ca, *volatile b = cb;
    for (long i = 0; i < n; i++) pc_sink += strncasecmp(a, b, 64);
    PC_CHECKING pc_check("%d", PC_SIGN(strncasecmp(a, b, 64)));
}
