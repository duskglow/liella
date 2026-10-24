/* STRSTR-CYCLES: strstr on 4 KiB haystacks, written to split the time between the candidate scan (looking for
 * places where the needle could start) and the verify step (comparing the needle at a candidate). Run with
 * --event cycles:u as well as the default instructions:u (cycles are not deterministic: read the sd columns).
 *   strstr_hit_4k       text-like lines (getline-sized records joined), needle found at the end
 *   strstr_scan_4k      'a' * 4080 + needle: almost no false candidates, so the cost is the scan alone
 *   strstr_verify_4k    "needlf" repeated + needle: each repeat matches 5 bytes and then fails, so verify dominates
 *   strstr_miss_4k      text-like lines, needle absent (full scan, then NULL)
 *   strstr_hit_short    40-byte line, needle at byte 30 (the getline-loop size) */
#include "percall.h"

#define HAY 4096
static char hay_text[HAY], hay_scan[HAY], hay_verify[HAY], hay_short[64];
static const char needle_buf[] = "needle";
static const char *volatile needle_v = needle_buf;
static const char *volatile hay_v;

static void fill_text(char *h, int with_needle) {
    static const char line[] = "the quick brown fox jumps over the lazy dog; 12345 lines of log text\n";
    size_t len = sizeof line - 1, end = HAY - 1 - (with_needle ? 6 : 0), i = 0;
    while (i < end) { size_t k = end - i < len ? end - i : len; memcpy(h + i, line, k); i += k; }
    if (with_needle) memcpy(h + end, "needle", 6);
    h[HAY - 1] = 0;
}

static long run(const char *h, long n) {
    hay_v = h;
    for (long i = 0; i < n; i++) { const char *const volatile *hp = &hay_v; pc_sink += (long)(strstr(*hp, needle_v) != 0); }
    const char *r = strstr(hay_v, needle_v);
    return r ? (long)(r - h) : -1;
}

CASE(strstr_hit_4k, 0, 1) {
    fill_text(hay_text, 1);
    long at = run(hay_text, n);
    PC_CHECKING pc_check("%ld", at);
}
CASE(strstr_scan_4k, 0, 1) {
    memset(hay_scan, 'a', HAY - 7); memcpy(hay_scan + HAY - 7, "needle", 7);
    long at = run(hay_scan, n);
    PC_CHECKING pc_check("%ld", at);
}
CASE(strstr_verify_4k, 0, 1) {
    size_t i = 0;
    while (i + 6 <= HAY - 7) { memcpy(hay_verify + i, "needlf", 6); i += 6; }
    memset(hay_verify + i, 'x', HAY - 7 - i); memcpy(hay_verify + HAY - 7, "needle", 7);
    long at = run(hay_verify, n);
    PC_CHECKING pc_check("%ld", at);
}
CASE(strstr_miss_4k, 0, 1) {
    fill_text(hay_text, 0);
    long at = run(hay_text, n);
    PC_CHECKING pc_check("%ld", at);
}
CASE(strstr_hit_short, 0, 1) {
    memcpy(hay_short, "lorem ipsum dolor sit amet ab needle xy", 40);
    long at = run(hay_short, n);
    PC_CHECKING pc_check("%ld", at);
}
