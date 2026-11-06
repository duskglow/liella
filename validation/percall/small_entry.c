/* wl_percall PARITY-SMALL-ENTRY set: the fixed entry cost of very small calls (cbench_hot / cbench_locale rows
 * ctype_b_loc, isalpha_c, nl_langinfo_en_us, nl_langinfo_l_en_us, strnlen_16, strnlen_16_mix). Every call goes
 * through a volatile function pointer so gcc can neither hoist a const getter out of the loop nor fold a call; the
 * indirect call costs the same on both sides. Typical-call counts are 0 (no workload census for these rows). */
#include "percall.h"
#include <ctype.h>

extern const unsigned short **__ctype_b_loc(void);

CASE(ctype_b_loc, 0, 1) {
    const unsigned short **(*volatile f)(void) = __ctype_b_loc;
    for (long i = 0; i < n; i++) pc_sink += (*f())['a'];
    PC_CHECKING pc_check("%d", ((*f())['a'] & _ISalpha) != 0);
}
CASE(isalpha_c, 0, 1) {
    int (*volatile f)(int) = isalpha;
    for (long i = 0; i < n; i++) pc_sink += f('a' + (int)(i & 15));
    PC_CHECKING pc_check("%d %d", f('a') != 0, f('1') != 0);
}
CASE(nl_langinfo_mon, 0, 1) {
    char *(*volatile f)(nl_item) = nl_langinfo;
    for (long i = 0; i < n; i++) pc_sink += f(MON_1 + (int)(i % 12))[0];
    PC_CHECKING pc_check("%s %s %s", f(MON_1), f(CODESET), f(D_T_FMT));
}
CASE(nl_langinfo_l_mon, 0, 1) {
    char *(*volatile f)(nl_item, locale_t) = nl_langinfo_l;
    locale_t loc = newlocale(LC_ALL_MASK, "en_US.UTF-8", (locale_t)0);
    if (loc == (locale_t)0) { fprintf(stderr, "small_entry: newlocale(en_US.UTF-8) failed\n"); exit(4); }
    for (long i = 0; i < n; i++) pc_sink += f(MON_1 + (int)(i % 12), loc)[0];
    PC_CHECKING pc_check("%s %s %s", f(MON_1, loc), f(CODESET, loc), f(D_T_FMT, loc));
    freelocale(loc);
}
CASE(nl_langinfo_l_codeset, 0, 1) {
    char *(*volatile f)(nl_item, locale_t) = nl_langinfo_l;
    locale_t loc = newlocale(LC_ALL_MASK, "en_US.UTF-8", (locale_t)0);
    if (loc == (locale_t)0) { fprintf(stderr, "small_entry: newlocale(en_US.UTF-8) failed\n"); exit(4); }
    for (long i = 0; i < n; i++) pc_sink += f(CODESET, loc)[0];
    PC_CHECKING pc_check("%s", f(CODESET, loc));
    freelocale(loc);
}
CASE(strnlen_16, 0, 1) {
    static char buf[64] = "abcdefghijklmnop";
    size_t (*volatile f)(const char *, size_t) = strnlen;
    const char *volatile p = buf;
    for (long i = 0; i < n; i++) pc_sink += f(p, 32);
    PC_CHECKING pc_check("%zu %zu %zu", f(p, 32), f(p, 16), f(p, 3));
}
CASE(strnlen_16_mix, 0, 1) {
    static char buf[64] = "abcdefghijklmnop";
    size_t (*volatile f)(const char *, size_t) = strnlen;
    const char *volatile p = buf;
    for (long i = 0; i < n; i++) pc_sink += f(p + (i & 7), 8 + (size_t)(i & 31));
    PC_CHECKING pc_check("%zu %zu", f(p + 3, 8), f(p + 7, 39));
}
