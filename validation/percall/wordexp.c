/* wl_percall wordexp set (WORDEXP-PROFILE, plan X-5): instructions per wordexp()+wordfree() call, Liella vs glibc.
 * Run: validation/wl_percall.sh --spec validation/percall/wordexp.c .
 * Every case uses WRDE_NOCMD (no shell runs). The cases are the six rows of bench/c/cbench_opt.c plus probes that
 * separate where the cost goes: a one-word call (the fixed cost of a call), a larger word count (cost per word), one
 * long word (cost per byte), and the same variable lookup with a large environment (cost that grows with data the
 * call need not touch). Typical-call counts are rough orders of magnitude for a program that expands a handful of
 * option strings (a shell-like tool, a build driver); wordexp is not a hot call in any measured workload. */
#include "percall.h"
#include <wordexp.h>

static long pc_words(const char *text, int flags)
{
    wordexp_t we;
    memset(&we, 0, sizeof we);
    if (wordexp(text, &we, flags) != 0)
        return -1;
    long n = (long)we.we_wordc;
    if (n > 0)
        n = n * 1000 + (long)strlen(we.we_wordv[n - 1]);
    wordfree(&we);
    return n;
}

#define WE_CASE(nm, calls, text)                                                         \
    CASE(nm, calls, 1) {                                                                 \
        const char *volatile t = text;                                                   \
        setenv("HOME", "/home/pc", 1);                                                   \
        setenv("WORDS", "p q r s", 1);                                                   \
        unsetenv("UNSET_PC");                                                            \
        for (long i = 0; i < n; i++) pc_sink += pc_words(t, WRDE_NOCMD);                 \
        PC_CHECKING pc_check("%ld", pc_words(t, WRDE_NOCMD));                            \
    }

WE_CASE(we_one_word, 10, "abc")
WE_CASE(we_plain_8, 10, "cc -O2 -Wall -o prog main.c util.c -lm")
WE_CASE(we_tilde_var, 10, "~/a $HOME b")
WE_CASE(we_quoted, 10, "'a b' \"c $HOME\" d\\ e")
WE_CASE(we_split_var, 10, "$WORDS")
WE_CASE(we_param_forms, 10, "${HOME:-/} ${UNSET_PC:-x} ${#HOME}")
WE_CASE(we_arith, 10, "x $((3*(4+5)-7)) y")
WE_CASE(we_arith_only, 10, "$((1+2))")
WE_CASE(we_arith_long, 10, "$((1+2*3-4/2+5%3-(6<<1)+(7>>1)+(8&3)+(9|4)+(10^6)))")
WE_CASE(we_plain_32, 10,
        "a1 b2 c3 d4 e5 f6 g7 h8 i9 j10 k11 l12 m13 n14 o15 p16 q17 r18 s19 t20 u21 v22 w23 x24 y25 z26 "
        "a27 b28 c29 d30 e31 f32")
WE_CASE(we_long_word, 10,
        "abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz"
        "abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz")

/* The same "$HOME" lookup with 2,000 extra environment entries added after the first sixty: the difference to
 * we_home_small is the cost that grows with the environment. */
CASE(we_home_small, 10, 1) {
    const char *volatile t = "$HOME";
    setenv("HOME", "/home/pc", 1);
    for (long i = 0; i < n; i++) pc_sink += pc_words(t, WRDE_NOCMD);
    PC_CHECKING pc_check("%ld", pc_words(t, WRDE_NOCMD));
}
CASE(we_home_bigenv, 10, 1) {
    const char *volatile t = "$HOME";
    char name[32];
    for (int k = 0; k < 2000; k++) {
        snprintf(name, sizeof name, "PC_FILLER_%d", k);
        setenv(name, "some filler value for the environment", 1);
    }
    setenv("HOME", "/home/pc", 1);
    for (long i = 0; i < n; i++) pc_sink += pc_words(t, WRDE_NOCMD);
    PC_CHECKING pc_check("%ld", pc_words(t, WRDE_NOCMD));
}
