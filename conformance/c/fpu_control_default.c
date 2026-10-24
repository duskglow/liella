/* `__fpu_control` (spec §10.6, D-10.5), a program that does not define it: the variable reads
 * 0x037f (`_FPU_DEFAULT`) and so does the x87 control word at `main`, linked statically or
 * dynamically. crates/boundary/tests/s10_math_symbols.rs also runs this program dynamically, on
 * glibc and on Liella's libc.so.6. */
#include <fpu_control.h>
#include <stdio.h>

int main(void)
{
    fpu_control_t cw;
    _FPU_GETCW(cw);
    if (__fpu_control != 0x037f) {
        fprintf(stderr, "__fpu_control = %#x, want 0x37f\n", (unsigned)__fpu_control);
        return 1;
    }
    if (cw != 0x037f) {
        fprintf(stderr, "x87 control word at main = %#x, want 0x37f\n", (unsigned)cw);
        return 2;
    }
    return 0;
}
