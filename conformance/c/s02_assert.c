/* Spec §2.4: a failed assertion prints prog: file:line: function: Assertion `expr' failed.
 * (glibc 2.39's exact text, ERRATA E8) and aborts. */
/* EXPECT-SIGNAL: 6 */
/* EXPECT-STDERR: s02_assert: */
/* EXPECT-STDERR: s02_assert.c:12: main: Assertion `x == 1' failed. */
#undef NDEBUG
#include <assert.h>
#include "expect_crash.h" /* the assertion stays on line 12 */
int main(void)
{
    volatile int x = 0;
    expect_crash(); assert(x == 1);
    return 0;
}
