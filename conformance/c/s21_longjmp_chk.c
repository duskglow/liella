/* Spec §21.1: __longjmp_chk aborts when the jump would land below the current stack pointer
 * (into a frame that has returned). */
/* EXPECT-SIGNAL: 6 */
/* EXPECT-STDERR: *** longjmp causes uninitialized stack frame ***: terminated */
#include <setjmp.h>
#include "expect_crash.h"

extern void __longjmp_chk(jmp_buf env, int val) __attribute__((noreturn));

static jmp_buf env;

static __attribute__((noinline)) int deeper(void)
{
    volatile char pad[256];
    pad[0] = 1;
    return setjmp(env) + pad[0];
}

int main(void)
{
    deeper(); /* env now names a frame that has returned, below main's */
    expect_crash();
    __longjmp_chk(env, 1);
    return 1;
}
