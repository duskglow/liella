/* Spec §1.2 contract clause 4: abort ends the process with SIGABRT; handlers registered with
 * atexit do not run. */
/* EXPECT-SIGNAL: 6 */
#include <stdlib.h>
#include "expect_crash.h"

static void never(void) { _Exit(70); }

int main(void)
{
    if (atexit(never) != 0)
        return 1;
    expect_crash();
    abort();
}
