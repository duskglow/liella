/* regcomp + regfree of ordinary patterns (REGCOMP-DEEP-NESTING): the parser and the compiler run once per
 * call, so these rows show what the pattern compiler costs for the shapes programs compile (sed and grep
 * expressions, configuration keys, addresses). The check folds the regcomp result and re_nsub of one fresh
 * compile. */
#define _GNU_SOURCE
#include "percall.h"
#include <regex.h>
#include <stddef.h>
static __typeof__(regcomp) *volatile P_regcomp = regcomp;

#define COMPILE_BODY(pat, fl)                                                          \
    for (long i = 0; i < n; i++) {                                                      \
        regex_t re;                                                                     \
        int r = P_regcomp(&re, pat, fl);                                                \
        pc_sink += r;                                                                   \
        if (r == 0) regfree(&re);                                                       \
    }                                                                                   \
    PC_CHECKING {                                                                       \
        regex_t re;                                                                     \
        int r = P_regcomp(&re, pat, fl);                                                \
        pc_check("r=%d", r);                                                            \
        if (r == 0) { pc_check("nsub=%zu", re.re_nsub); regfree(&re); }                 \
    }

/* sed -E 's/([a-z]+) ([0-9]+)/\2:\1/' */
CASE(regcomp_sed_groups, 100000, 1) { COMPILE_BODY("([a-z]+) ([0-9]+)", REG_EXTENDED) }
/* A configuration line, key = value, in a BRE with groups. */
CASE(regcomp_kv_bre, 100000, 1) { COMPILE_BODY("^\\([a-z0-9_]*\\) = \\(.*\\)$", 0) }
/* An address with an alternation of three literals. */
CASE(regcomp_email_alt, 100000, 1) { COMPILE_BODY("([a-z]+)@([a-z]+)\\.(com|org|net)", REG_EXTENDED) }
/* A dotted quad: an interval over a group. */
CASE(regcomp_ipv4, 100000, 1) { COMPILE_BODY("^([0-9]{1,3}\\.){3}[0-9]{1,3}$", REG_EXTENDED) }
/* A literal, REG_NOSUB (grep -F like). */
CASE(regcomp_literal_nosub, 100000, 1) { COMPILE_BODY("\\.so$", REG_EXTENDED | REG_NOSUB) }
