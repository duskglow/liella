/* Class nonshared: the `pthread_atfork` member of libc_nonshared.a (DL-LIBC-NONSHARED, D-27.28).
 *
 * libc.so.6 exports `pthread_atfork` only as a compat version (no default), so a new link binds
 * an unversioned reference to this member: `__pthread_atfork` (hidden) with `pthread_atfork` a
 * weak hidden alias, as the probes list the archive's symbols. It registers the three handlers
 * with libc.so.6's `__register_atfork` (GLIBC_2.3.2) tagged with the caller module's
 * `__dso_handle`, so unloading the module can drop them. */

extern void *__dso_handle __attribute__((visibility("hidden")));
extern int __register_atfork(void (*)(void), void (*)(void), void (*)(void), void *);

__attribute__((visibility("hidden"))) int __pthread_atfork(void (*prepare)(void), void (*parent)(void), void (*child)(void)) {
    return __register_atfork(prepare, parent, child, __dso_handle);
}

extern int pthread_atfork(void (*)(void), void (*)(void), void (*)(void))
    __attribute__((weak, alias("__pthread_atfork"), visibility("hidden")));
