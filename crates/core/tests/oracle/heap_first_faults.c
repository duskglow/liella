/* The heap's first allocation (HEAP-FIRST-USE, D-3.19): the minor faults of a process's first
 * malloc(16) (the thread cache, the registries, the first segment's metadata, the block's slab
 * page). Prints "minflt <delta>" on stderr. */
#include <stdio.h>
#include <stdlib.h>
#include <sys/resource.h>

static long minflt(void) {
    struct rusage u;
    if (getrusage(RUSAGE_SELF, &u) != 0)
        return -1;
    return u.ru_minflt;
}

int main(void) {
    long before = minflt();
    if (minflt() < 0)
        return 1;
    void *volatile p = malloc(16);
    long after = minflt();
    if (p == NULL || before < 0 || after < 0)
        return 1;
    free(p);
    fprintf(stderr, "minflt %ld\n", after - before);
    return 0;
}
