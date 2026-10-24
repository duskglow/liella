/* Spec §2.5 (C0.5): a fortified copy that would overflow its destination calls __chk_fail:
 * the fixed message, then SIGABRT. */
/* RUN-ARGS: this-argument-is-too-long */
/* EXPECT-SIGNAL: 6 */
/* EXPECT-STDERR: *** buffer overflow detected ***: terminated */
#undef _FORTIFY_SOURCE
#define _FORTIFY_SOURCE 2
#include <string.h>
#include "expect_crash.h"

int main(int argc, char **argv)
{
    char buf[8];
    if (argc < 2)
        return 1;
    expect_crash();
    strcpy(buf, argv[1]);
    return buf[0];
}
