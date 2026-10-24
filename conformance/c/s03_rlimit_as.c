/* T14 (spec §3.1-3.2; milestone 5): under a small address-space limit (RLIMIT_AS of 20 MiB, set
 * by prlimit before the program starts) the heap still allocates what fits: its registries are
 * sized to the limit, and below four segments (64 MiB) no 16 MiB segment is taken: small blocks
 * get page mappings of their own. Above it, a segment is mapped without the alignment slack when
 * the slack does not fit. Then, with the heap
 * exhausted, the standard streams still print (T22). Each failing CHECK exits with its line
 * number. */
/* RUN-WRAP: prlimit --as=20971520 */
/* EXPECT-STDERR: still reporting with an exhausted heap: */
#define _GNU_SOURCE
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/resource.h>

#define CHECK(cond) do { if (!(cond)) return __LINE__; } while (0)

static void *blocks[1 << 16];

int main(void)
{
    struct rlimit r;
    CHECK(getrlimit(RLIMIT_AS, &r) == 0 && r.rlim_cur == 20971520);
    char *small = malloc(100);
    CHECK(small != NULL);
    memset(small, 1, 100);
    char *big = malloc(1 << 20);
    CHECK(big != NULL);
    memset(big, 2, 1 << 20);
    free(big);
    /* 1 KiB blocks until the limit: several MiB of them. */
    size_t n = 0;
    while (n < sizeof blocks / sizeof blocks[0]) {
        void *p = malloc(1024);
        if (!p)
            break;
        memset(p, 3, 1024);
        blocks[n++] = p;
    }
    CHECK(errno == ENOMEM || n == sizeof blocks / sizeof blocks[0]);
    CHECK(n >= 1024);
    /* Exhausted: the streams still work (T22). */
    fprintf(stderr, "still reporting with an exhausted heap: %zu blocks\n", n);
    for (size_t i = 0; i < n; i++)
        free(blocks[i]);
    void *again = malloc(1024);
    CHECK(again != NULL);
    free(again);
    free(small);
    return 0;
}
