/* Spec §24.5: the fortified gethostname (__gethostname_chk) with a size beyond the object
 * ends the process (__chk_fail: SIGABRT), as glibc's does. */
/* EXPECT-SIGNAL: 6 */
/* EXPECT-STDERR: buffer overflow detected */
#include <stddef.h>
#include "expect_crash.h"

int __gethostname_chk(char *name, size_t len, size_t buflen);

int main(void)
{
    char b[8];
    expect_crash();
    __gethostname_chk(b, 64, sizeof b);
    return 0;
}
