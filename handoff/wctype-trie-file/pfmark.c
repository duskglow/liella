/* WCTYPE-TRIE-FILE-5 diagnostic: the first localtime_r's page faults, delimited by two
 * marker pages so `perf record -e page-faults -d` shows which addresses the call touches. */
#include <stdio.h>
#include <stdlib.h>
#include <time.h>
#include <sys/resource.h>

static char mark_a[3 * 4096] __attribute__((aligned(4096)));
static char mark_b[3 * 4096] __attribute__((aligned(4096)));

static long minflt(void) {
    struct rusage u;
    getrusage(RUSAGE_SELF, &u);
    return u.ru_minflt;
}

int main(void) {
    setenv("TZ", "America/New_York", 1);
    time_t when = 1759579200;
    struct tm r;
    long f0 = minflt();
    *(volatile char *)&mark_a[4096] = 1;
    long n = localtime_r(&when, &r) != NULL;
    *(volatile char *)&mark_b[4096] = 1;
    long f1 = minflt();
    fprintf(stderr, "minflt %ld (incl. 1 marker) a=%p b=%p\n", n == 1 && r.tm_isdst == 1 ? f1 - f0 : -1,
            (void *)&mark_a[4096], (void *)&mark_b[4096]);
    return 0;
}
