/* T14 (spec §3.1; owner requirement 2026-10-05; D-3.14): a process started under a soft
 * address-space limit of 20 MiB (hard limit unlimited, set by prlimit) fills the heap to the
 * limit, raises its soft limit, and then allocates far past the earlier limit: the heap's
 * registries grow by generations instead of keeping the sizes chosen at the first allocation.
 * Each failing CHECK exits with its line number. */
/* RUN-WRAP: prlimit --as=20971520:unlimited */
#define _GNU_SOURCE
#include <errno.h>
#include <stdlib.h>
#include <string.h>
#include <sys/resource.h>

#define CHECK(cond) do { if (!(cond)) return __LINE__; } while (0)

#define MAX (1 << 17)
static void *blocks[MAX];

int main(void)
{
    struct rlimit r;
    CHECK(getrlimit(RLIMIT_AS, &r) == 0 && r.rlim_cur == 20971520 && r.rlim_max == RLIM_INFINITY);
    size_t n = 0;
    while (n < MAX) {
        void *p = malloc(1024);
        if (!p)
            break;
        memset(p, 1, 1024);
        blocks[n++] = p;
    }
    CHECK(n > 1000 && n < MAX);
    size_t under = n;
    r.rlim_cur = RLIM_INFINITY;
    CHECK(setrlimit(RLIMIT_AS, &r) == 0);
    /* 100,000 more blocks (about 100 MiB): five times what the earlier limit allowed. */
    for (int i = 0; i < 100000 && n < MAX; i++) {
        void *p = malloc(1024);
        CHECK(p != NULL);
        memset(p, 2, 1024);
        blocks[n++] = p;
    }
    void *big = malloc(64 << 20);
    CHECK(big != NULL);
    memset(big, 3, 64 << 20);
    free(big);
    for (size_t i = 0; i < n; i++)
        free(blocks[i]);
    CHECK(n >= under + 100000);
    return 0;
}
