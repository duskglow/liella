/* Spec §2.5: an overwritten stack canary is detected on return: the fixed message, then
 * SIGABRT. The overflow is deliberate; the corrupted frame never returns. */
/* EXPECT-SIGNAL: 6 */
/* EXPECT-STDERR: *** stack smashing detected ***: terminated */

#include "expect_crash.h"

static void __attribute__((noinline)) smash(volatile char *p)
{
    for (int i = 0; i < 64; i++)
        p[i] = 'A';
}

static int __attribute__((noinline)) victim(void)
{
    char buf[8];
    smash(buf);
    return buf[0];
}

int main(void)
{
    expect_crash();
    return victim();
}
