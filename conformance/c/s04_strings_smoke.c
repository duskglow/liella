/* C conformance test template (implementation-plan.md §4.3).
 *
 * Name files sNN_<area>_<topic>.c. Each program checks one or more spec clauses through the
 * real C interface and exits 0 on success. Every CHECK names the clause ID it verifies
 * (spec/clauses.tsv), so failures trace straight back to the spec.
 *
 * This file is the first test to run once phase 4 links C programs against Liella. It checks
 * clauses of spec §4.1 and §4.8 that only the C calling convention can exercise. */
#include <string.h>

#define CHECK(clause, cond) do { if (!(cond)) return __LINE__; } while (0)

int main(void)
{
    char buf[8];

    /* §4.8: strlen counts the bytes before the terminator. */
    CHECK("s4_8_c_0", strlen("abc") == 3);

    /* §4.1: memmove copies correctly under overlap. */
    memcpy(buf, "abcdef", 7);
    memmove(buf + 1, buf, 5);
    CHECK("s4_1_c_2", memcmp(buf, "aabcde", 7) == 0); /* "abcdef" moved up by one: a a b c d e, then the NUL */

    return 0;
}
