/* Class nonshared: the `atexit` member of libc_nonshared.a (DL-LIBC-NONSHARED, D-27.28).
 *
 * `atexit` is not in libc.so.6's ABI: a program or shared object linked through the installed
 * `libc.so` script takes this member from the archive, so the function is linked into the
 * CALLER's module, hidden (spec/behaviour/dynlink.md, "The link-time archive"). It registers the
 * handler with the libc.so.6 function that owns the exit list, tagged with the module's identity:
 * the value of the module's `__dso_handle` (the compiler's start files define it, hidden). That
 * tag is what lets `__cxa_finalize`, called when the module is unloaded, run the handler then.
 * The member holds no state of its own. */

extern void *__dso_handle __attribute__((visibility("hidden")));
extern int __cxa_atexit(void (*)(void *), void *, void *);

__attribute__((visibility("hidden"))) int atexit(void (*f)(void)) {
    return __cxa_atexit((void (*)(void *))f, (void *)0, __dso_handle);
}
