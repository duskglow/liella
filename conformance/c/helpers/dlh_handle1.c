/* Helper 1 of s27_dlsym_handles (LINK-HELPERS: dlh_handle1 dlh_handle2, in this load order).
 * Both helpers define f, returning their number. This one also asks for the next f after itself
 * (RTLD_NEXT from a shared object) and for a name nobody defines. */
#define _GNU_SOURCE
#include <dlfcn.h>

int f(void) { return 1; }

void *handle1_next_f(void) { return dlsym(RTLD_NEXT, "f"); }
void *handle1_next_missing(void) { return dlsym(RTLD_NEXT, "handles_no_such_symbol"); }
