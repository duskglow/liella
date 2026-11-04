/* handle.c - the two helper objects of handles.c, built with -DNUM=1 and -DNUM=2. Both define f.
 * Helper 1 also asks for the next f after itself. -DPLANT_NEXT_SELF (helper 1 only) makes that
 * lookup start at helper 1 itself instead (the shape of a "next" lookup that includes the
 * caller's own object): handles.c must then fail. */
#define _GNU_SOURCE
#include <dlfcn.h>

int f(void) { return NUM; }

#if NUM == 1
static void *next(const char *name)
{
#ifdef PLANT_NEXT_SELF
    void *self = dlopen("libhandle1.so", RTLD_NOW | RTLD_NOLOAD);
    return self ? dlsym(self, name) : 0;
#else
    return dlsym(RTLD_NEXT, name);
#endif
}
void *handle1_next_f(void) { return next("f"); }
void *handle1_next_missing(void) { return next("handles_no_such_symbol"); }
#endif
