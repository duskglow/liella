/* Heap metadata page faults (HEAP-META-DENSE, D-3.15): after one warm-up allocation (the thread
 * cache, the first segment, its first slab), the first block of each of 14 further size classes
 * is allocated, never written, and freed again by the same thread. Only the heap's own metadata
 * is touched, so the minor-fault delta counts metadata pages (a single-threaded free must not
 * read the remote bitmaps' pages). Prints "minflt <delta>" on stderr. */
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
    static const size_t sizes[] = {32, 48, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384, 448};
    void *volatile keep[sizeof sizes / sizeof sizes[0]];
    void *volatile warm = malloc(16);
    if (warm == NULL)
        return 1;
    long before = minflt();
    for (size_t i = 0; i < sizeof sizes / sizeof sizes[0]; i++) {
        keep[i] = malloc(sizes[i]);
        if (keep[i] == NULL)
            return 1;
    }
    for (size_t i = 0; i < sizeof sizes / sizeof sizes[0]; i++)
        free(keep[i]);
    long after = minflt();
    free(warm);
    if (before < 0 || after < 0)
        return 1;
    fprintf(stderr, "minflt %ld\n", after - before);
    return 0;
}
