/* `__fpu_control` (spec §10.6, D-10.5, #812), a program that defines its own (0x027f: 53-bit
 * precision). As glibc 2.39 (probe-settled): the program links either way; linked dynamically,
 * the x87 control word at `main` is the program's value; linked statically, the definition is
 * ignored and the word stays 0x037f. A constructor's assignment comes too late either way.
 * conformance/run.sh links it statically; crates/boundary/tests/s10_math_symbols.rs also runs it
 * dynamically, on glibc and on Liella's libc.so.6. */
#include <fpu_control.h>
#include <stdio.h>
#include <sys/auxv.h>

fpu_control_t __fpu_control = 0x027f;

__attribute__((constructor)) static void late(void)
{
    __fpu_control = 0x007f;
}

int main(void)
{
    fpu_control_t cw;
    _FPU_GETCW(cw);
    int dynamic = getauxval(AT_BASE) != 0; /* a program interpreter ran */
    unsigned want = dynamic ? 0x027f : 0x037f;
    if (__fpu_control != 0x007f) {
        fprintf(stderr, "__fpu_control = %#x, want the constructor's 0x7f\n", (unsigned)__fpu_control);
        return 1;
    }
    if (cw != want) {
        fprintf(stderr, "x87 control word at main = %#x, want %#x (%s)\n", (unsigned)cw, want, dynamic ? "dynamic" : "static");
        return 2;
    }
    return 0;
}
