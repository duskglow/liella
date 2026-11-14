/* validation/smoke/repro/regcomp_nesting.c: AQOURS-SMOKE finding 1 (spec section 14.9, regcomp).
   Compiles an extended regular expression of DEPTH nested groups around one character,
   "((...(a)...))", and matches it against "xay". Prints the regcomp and regexec results.

   Build and run against both C libraries (from the tree root, after smoke.sh setup):
     gcc -O1 validation/smoke/repro/regcomp_nesting.c -o /tmp/nest-ref && /tmp/nest-ref 300
     LIELLA_SMOKE_SYSROOT=<work>/sysroot LIELLA_SMOKE_LDSO=target/liella/ld-linux-x86-64.so.2 \
       <work>/bin/liella-cc -O1 validation/smoke/repro/regcomp_nesting.c -o /tmp/nest-liella && /tmp/nest-liella 300

   Observed (2026-11-13, host C library glibc 2.39): depth 255 compiles and matches on both; depth 256 to
   1000: the reference compiles and matches (regcomp 0, regexec 0), Liella refuses the pattern with
   REG_ESPACE (12; crates/core/src/pattern/regex/parse.rs MAX_DEPTH = 256). */
#include <regex.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

int main(int argc, char **argv) {
    size_t d = argc > 1 ? strtoul(argv[1], NULL, 10) : 300;
    if (d > 1000)
        d = 1000;
    char *p = malloc(2 * d + 2);
    if (!p)
        return 3;
    memset(p, '(', d);
    p[d] = 'a';
    memset(p + d + 1, ')', d);
    p[2 * d + 1] = 0;
    regex_t re;
    int rc = regcomp(&re, p, REG_EXTENDED | REG_NOSUB);
    int m = -1;
    if (rc == 0) {
        m = regexec(&re, "xay", 0, NULL, 0);
        regfree(&re);
    }
    printf("depth %zu regcomp %d regexec %d\n", d, rc, m);
    free(p);
    return 0;
}
