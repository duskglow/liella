/* Spec §2.4: assert_perror(e) with e != 0 prints the error's text (§2.2) and aborts. */
/* EXPECT-SIGNAL: 6 */
/* EXPECT-STDERR: s02_assert_perror.c: */
/* EXPECT-STDERR: main: Unexpected error: No such file or directory. */
#define _GNU_SOURCE
#undef NDEBUG
#include <assert.h>
#include <errno.h>
#include "expect_crash.h"

int main(void)
{
    assert_perror(0);
    expect_crash();
    assert_perror(ENOENT);
    return 0;
}
