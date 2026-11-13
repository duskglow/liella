/* Class nonshared: the `__stack_chk_fail_local` member of libc_nonshared.a (DL-LIBC-NONSHARED,
 * D-27.28).
 *
 * A module-local entry for the stack-protector failure path: code compiled to call
 * `__stack_chk_fail_local` (a direct call, no PLT or GOT) reaches libc.so.6's `__stack_chk_fail`
 * (GLIBC_2.4) through this hidden member. This file is built without the stack protector. */

extern void __stack_chk_fail(void) __attribute__((noreturn));

__attribute__((visibility("hidden"), noreturn)) void __stack_chk_fail_local(void) {
    __stack_chk_fail();
}
