/* TB-FOREIGN-WORDS-LOW (D-X.60): the paths whose foreign-block test must cost the runtime's own
 * threads nothing (ruling #4835): malloc/free (the flags word) and __errno_location (one cmpb). */
#include "percall.h"
#include <errno.h>
#include <stdlib.h>

CASE(malloc_free, 1000000, 1) {
    for (long i = 0; i < n; i++) { void *volatile p = malloc(24 + (i & 127)); free(p); }
    PC_CHECKING { void *volatile p = malloc(24 + 5); pc_check("%d %d", p != NULL, (int)((unsigned long)p & 15)); free(p); }
}
CASE(calloc_free, 100000, 1) {
    for (long i = 0; i < n; i++) { void *volatile p = calloc(1, 40 + (i & 63)); free(p); }
    PC_CHECKING { char *volatile p = calloc(1, 40); pc_check("%d", p != NULL && p[39] == 0); free(p); }
}
/* Through a volatile pointer: `errno` alone lets the compiler take the address once per loop
 * (the declaration is const), which measures the loop and not the call. */
CASE(errno_location, 1000000, 1) {
    int *(*volatile loc)(void) = __errno_location;
    for (long i = 0; i < n; i++) { int *e = loc(); *e = (int)i; }
    PC_CHECKING { errno = 7; pc_check("%d", errno); }
}
