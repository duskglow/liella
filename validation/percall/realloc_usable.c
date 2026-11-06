/* wl_percall cases for realloc that stays within the block's usable size (PERF-REALLOC-USABLE-2). */
#include "percall.h"

CASE(realloc_within_usable, 1000000, 1) { /* malloc 17, realloc to 24 (same class), store, free */
    for (long i = 0; i < n; i++) {
        char *volatile p = malloc(17); p = realloc(p, 24); p[23] = 1; free(p);
    }
    PC_CHECKING { char *volatile p = malloc(17); p[0] = 5; p = realloc(p, 24); pc_check("%d", p != NULL && p[0] == 5); free(p); }
}
CASE(malloc_free_17, 1000000, 1) { /* the control: malloc 17 and free */
    for (long i = 0; i < n; i++) {
        char *volatile p = malloc(17); p[0] = 1; free(p);
    }
    PC_CHECKING { char *volatile p = malloc(17); p[0] = 5; pc_check("%d", p != NULL && p[0] == 5); free(p); }
}
