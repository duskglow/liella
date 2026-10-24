/* Spec §3.3 mcheck_pedantic (D-3.5 as decided in phase 13): every operation of the malloc
 * family first checks every live block, so a damaged block is reported by the next call even
 * when that call does not involve it. */
#include <mcheck.h>
#include <stdlib.h>

#define CHECK(clause, cond) do { if (!(cond)) return __LINE__; } while (0)

static volatile int calls;
static volatile int last = -9;

static void abortfn(enum mcheck_status s)
{
    calls++;
    last = (int)s;
}

int main(void)
{
    CHECK("s3_3_c_2", mcheck_pedantic(abortfn) == 0);
    CHECK("s3_3_c_2", mcheck(abortfn) == -1);
    char *volatile a = malloc(10);
    char t = a[10];
    a[10] = (char)(t ^ 0x5a);
    char *volatile b = malloc(20);
    CHECK("s3_3_c_2", b != NULL && calls == 1 && last == MCHECK_TAIL);
    a[10] = t;
    free(a);
    free(b);
    CHECK("s3_3_c_2", calls == 1);
    return 0;
}
