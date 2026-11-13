/* Class nonshared: the `at_quick_exit` member of libc_nonshared.a (DL-LIBC-NONSHARED, D-27.28).
 *
 * Like `atexit` (atexit.c): linked into the caller's module, hidden, it registers the handler
 * with libc.so.6's `__cxa_at_quick_exit` (GLIBC_2.10) tagged with the module's `__dso_handle`,
 * so unloading the module forgets the handler. The callee is named by its symbol explicitly:
 * a program built against Liella must need only versions the reference exports. */

extern void *__dso_handle __attribute__((visibility("hidden")));
extern int liella_nonshared_quick_target(void (*)(void), void *) __asm__("__cxa_at_quick_exit");

__attribute__((visibility("hidden"))) int at_quick_exit(void (*f)(void)) {
    return liella_nonshared_quick_target(f, __dso_handle);
}
