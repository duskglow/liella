/* Spec §3.2 M_PERTURB through MALLOC_PERTURB_ (mallopt(3)): new blocks other than calloc's are
 * filled with the complement of the value's low byte; mallopt(M_PERTURB, v) changes it. */
/* RUN-ENV: MALLOC_PERTURB_=165 */
#include <malloc.h>
#include <stdlib.h>
#include <string.h>

#define CHECK(clause, cond) do { if (!(cond)) return __LINE__; } while (0)

static int all(const unsigned char *p, size_t n, unsigned char v)
{
    for (size_t i = 0; i < n; i++)
        if (p[i] != v)
            return 0;
    return 1;
}

int main(void)
{
    unsigned char *volatile p = malloc(200);
    CHECK("s3_2_c_1", all(p, 200, 0x5a));
    unsigned char *volatile z = calloc(1, 200);
    CHECK("s3_2_c_1", all(z, 200, 0));
    unsigned char *volatile big = malloc(300000);
    CHECK("s3_2_c_1", all(big, 300000, 0x5a));
    p = realloc(p, 400);
    CHECK("s3_2_c_1", all(p + 200, 200, 0x5a));
    CHECK("s3_2_c_1", mallopt(M_PERTURB, 0x1ff) == 1);
    unsigned char *volatile q = malloc(100);
    CHECK("s3_2_c_1", all(q, 100, 0x00)); /* the complement of 0xff */
    free(p);
    free(z);
    free(big);
    free(q);
    return 0;
}
