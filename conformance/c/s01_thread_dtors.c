/* Spec §1.2 contract clause 6: __cxa_thread_atexit_impl destructors of the exiting thread run
 * at exit, most recent first, before the atexit handlers (C++ requires thread_local objects to
 * be destroyed before static ones; ERRATA E9). */
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

extern int __cxa_thread_atexit_impl(void (*fn)(void *), void *obj, void *dso);
/* The object's DSO handle, as a C++ compiler passes it (Itanium C++ ABI §3.3.5; the start files
 * define it): a dynamically linked library needs it to find the object that registers. Weak:
 * Liella's static link has no crtbegin, and there the handle is NULL. */
extern void *__dso_handle __attribute__((weak, visibility("hidden")));

static char order[8];
static int n;
static char t1 = '1', t2 = '2';

static void tdtor(void *obj) { order[n++] = *(char *)obj; }
static void a(void)
{
    order[n++] = 'a';
    order[n] = 0;
    _exit(strcmp(order, "21a") == 0 ? 0 : 90 + n);
}

int main(void)
{
    if (atexit(a) != 0)
        return 1;
    if (__cxa_thread_atexit_impl(tdtor, &t1, &__dso_handle) != 0 || __cxa_thread_atexit_impl(tdtor, &t2, &__dso_handle) != 0)
        return 2;
    exit(0);
}
