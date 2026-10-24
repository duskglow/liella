/* Regex search cases in sed's real shapes (REGEX-SED) and the back-reference rows (REGEX-BACKREF), with a
 * DETERMINISTIC self-check (PERCALL-REGEX-CHECK). The check folds only the result of ONE fresh call: the return
 * code and rm_so/rm_eo of groups 0..re_nsub, over a fixed subject. The regmatch_t array is set to -1 before the
 * call (slots past re_nsub are left untouched by glibc and would print stack garbage), and the call is made inside
 * PC_CHECKING itself - never from the loop's leftovers, which do not exist when the harness runs n = 0. */
#define _GNU_SOURCE
#include "percall.h"
#include <regex.h>
#include <stddef.h>
#include <string.h>
static __typeof__(regexec) *volatile P_regexec = regexec;
static __typeof__(re_search) *volatile P_re_search = re_search;
static const char SUBJ[] = "tcqehwaevb 885775 /usr/lib/tcqehwaevb/1.so";
static const char *volatile subj_br = "xx bob@bob yy";
static const char *volatile subj_long = "aaaa bcd@bce bob bobby@bobby zz";

/* One fresh regexec over m[nm] pre-filled with -1; prints "r so,eo ..." for groups 0..min(re_nsub, nm-1). */
static void check_regexec(const regex_t *re, const char *s, int nm) {
    regmatch_t m[8];
    for (int k = 0; k < 8; k++) { m[k].rm_so = -1; m[k].rm_eo = -1; }
    int r = P_regexec(re, s, (size_t)nm, m, 0);
    pc_check("r=%d", r);
    if (r == 0)
        for (int k = 0; k < nm && k <= (int)re->re_nsub; k++) pc_check("g%d=%ld,%ld", k, (long)m[k].rm_so, (long)m[k].rm_eo);
}

/* sed -E 's/([a-z]+) ([0-9]+)/\2:\1/' : a match at 0 with two groups. */
CASE(regexec_sed_groups, 1000000, 1) {
    static regex_t re; static int ok;
    if (!ok) { regcomp(&re, "([a-z]+) ([0-9]+)", REG_EXTENDED); ok = 1; }
    regmatch_t m[3]; int r = 0;
    for (long i = 0; i < n; i++) r = P_regexec(&re, SUBJ, 3, m, 0);
    pc_sink += r;
    PC_CHECKING check_regexec(&re, SUBJ, 3);
}
/* sed 's/\.so$/.a/' : an anchored literal tail. */
CASE(regexec_sed_tail, 1000000, 1) {
    static regex_t re; static int ok;
    if (!ok) { regcomp(&re, "\\.so$", REG_EXTENDED); ok = 1; }
    regmatch_t m[1]; int r = 0;
    for (long i = 0; i < n; i++) r = P_regexec(&re, SUBJ, 1, m, 0);
    pc_sink += r;
    PC_CHECKING check_regexec(&re, SUBJ, 1);
}
/* The GNU interface sed itself uses (re_search with registers). */
CASE(re_search_sed_groups, 1000000, 1) {
    static struct re_pattern_buffer b; static struct re_registers regs; static int ok;
    if (!ok) { re_syntax_options = RE_SYNTAX_POSIX_EXTENDED; re_compile_pattern("([a-z]+) ([0-9]+)", 17, &b); ok = 1; }
    int len = (int)strlen(SUBJ); int r = 0;
    for (long i = 0; i < n; i++) r = P_re_search(&b, SUBJ, len, 0, len, &regs);
    pc_sink += r;
    PC_CHECKING {
        r = P_re_search(&b, SUBJ, len, 0, len, &regs);
        pc_check("r=%d", r);
        if (r >= 0)
            for (int k = 0; k <= (int)b.re_nsub; k++) pc_check("g%d=%ld,%ld", k, (long)regs.start[k], (long)regs.end[k]);
    }
}

#define BR_BODY(pat, subj) \
    static regex_t re; static int ok; \
    if (!ok) { if (regcomp(&re, pat, 0) != 0) abort(); ok = 1; } \
    regmatch_t m[4]; \
    for (long i = 0; i < n; i++) { if (P_regexec(&re, subj, 2, m, 0) != 0) abort(); } \
    PC_CHECKING check_regexec(&re, subj, 2);
CASE_LOCALE(rx_backref_c, 20000, 1, "C") { BR_BODY("\\(b[a-z]*\\)@\\1", subj_br) }
CASE(rx_backref, 20000, 1) { BR_BODY("\\(b[a-z]*\\)@\\1", subj_br) }
CASE_LOCALE(rx_backref_long_c, 20000, 1, "C") { BR_BODY("\\(b[a-z]*\\)@\\1", subj_long) }
