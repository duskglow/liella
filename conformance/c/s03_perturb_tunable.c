/* Spec §3.2 M_PERTURB, §1.8: the tunable glibc.malloc.perturb sets it and overrides
 * MALLOC_PERTURB_. */
/* RUN-ENV: GLIBC_TUNABLES=glibc.malloc.perturb=165 MALLOC_PERTURB_=1 */
#include <stdlib.h>

#define CHECK(clause, cond) do { if (!(cond)) return __LINE__; } while (0)

int main(void)
{
    unsigned char *volatile p = malloc(64);
    for (int i = 0; i < 64; i++)
        CHECK("s3_2_c_1", p[i] == 0x5a);
    free(p);
    return 0;
}
