/* NEXT-REGRESS: the string formats listing programs print on a stream (ls -l "%s %*s ", tar -tv
 * "%s %s/%s %*s %-*s"), plus "%s\n" and the short numeric formats gawk's printf hands to the libc. */
#include "percall.h"

CASE(plist_ls, 1000000, 1) { /* ls -l: mode string, then a right-aligned column */
    FILE *f = fopen("/dev/null", "w");
    for (long i = 0; i < n; i++) pc_sink += fprintf(f, "%s %*s ", "-rw-r--r--", 3, "1");
    PC_CHECKING pc_check("%d", fprintf(f, "%s %*s ", "-rw-r--r--", 3, "1"));
}
CASE(plist_tar, 1000000, 1) { /* tar -tv: mode owner/group size date */
    FILE *f = fopen("/dev/null", "w");
    for (long i = 0; i < n; i++)
        pc_sink += fprintf(f, "%s %s/%s %*s %-*s", "-rw-r--r--", "root", "root", 8, "1234", 16, "2024-01-01 00:00");
    PC_CHECKING pc_check("%d", fprintf(f, "%s %s/%s %*s %-*s", "-rw-r--r--", "root", "root", 8, "1234", 16, "2024-01-01 00:00"));
}
CASE(plist_s_nl, 1000000, 1) { /* a name line */
    FILE *f = fopen("/dev/null", "w");
    for (long i = 0; i < n; i++) pc_sink += fprintf(f, "%s\n", "words.txt");
    PC_CHECKING pc_check("%d", fprintf(f, "%s\n", "words.txt"));
}
CASE(plist_star_s, 1000000, 1) { /* one star-width string alone */
    FILE *f = fopen("/dev/null", "w");
    for (long i = 0; i < n; i++) pc_sink += fprintf(f, "%*s", 8, "1234");
    PC_CHECKING pc_check("%d", fprintf(f, "%*s", 8, "1234"));
}
CASE(plist_d, 1000000, 1) { /* gawk-style short integer */
    char b[64];
    for (long i = 0; i < n; i++) pc_sink += snprintf(b, sizeof b, "%d", (int)i);
    PC_CHECKING pc_check("%d", snprintf(b, sizeof b, "%d", 12345));
}
CASE(plist_3f, 1000000, 1) { /* gawk-style %.3f */
    char b[64];
    for (long i = 0; i < n; i++) pc_sink += snprintf(b, sizeof b, "%.3f", (double)i / 7);
    PC_CHECKING pc_check("%d", snprintf(b, sizeof b, "%.3f", 10.0 / 7));
}
/* GAWK-PRINTF: the calls gawk 5.4 (printf.c) makes per `printf "%d %s %x %.3f\n"` record: %d -> snprintf
 * "%.0f" on the double value; %.3f -> sprintf builds "%*.*f" from "*.*%c", then snprintf with width 0,
 * precision 3 (%x and %s are formatted by gawk itself). */
CASE(plist_g_0f, 1000000, 1) { /* format_integer_digits: "%.0f" of an integral double */
    char b[32];
    for (long i = 0; i < n; i++) pc_sink += snprintf(b, sizeof b, "%.0f", (double)i);
    PC_CHECKING pc_check("%d", snprintf(b, sizeof b, "%.0f", 123456.0));
}
CASE(plist_g_cpc, 1000000, 1) { /* format_float builds its format: sprintf(cp, "*.*%c", 'f') */
    char b[32];
    volatile char conv = 'f'; /* not a literal: gcc would fold the call */
    for (long i = 0; i < n; i++) pc_sink += sprintf(b, "*.*%c", conv);
    PC_CHECKING pc_check("%d", sprintf(b, "*.*%c", conv));
}
CASE(plist_g_ss3f, 1000000, 1) { /* format_float: snprintf "%*.*f" width 0, precision 3 */
    char b[100];
    for (long i = 0; i < n; i++) pc_sink += snprintf(b, sizeof b, "%*.*f", 0, 3, (double)i / 7);
    PC_CHECKING pc_check("%d", snprintf(b, sizeof b, "%*.*f", 0, 3, 10.0 / 7));
}
