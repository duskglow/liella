/* Spec §3.3 mcheck(NULL) (D-3.5 as decided in phase 13): the default handler prints mcheck(3)'s
 * message for the problem and aborts. */
/* EXPECT-SIGNAL: 6 */
/* EXPECT-STDERR: memory clobbered past end of allocated block */
#include <mcheck.h>
#include <stdlib.h>
#include "expect_crash.h"

int main(void)
{
    if (mcheck(NULL) != 0)
        return 1;
    char *volatile p = malloc(10);
    p[10] = (char)(p[10] ^ 0x5a);
    expect_crash();
    free(p);
    return 2;
}
