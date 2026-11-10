/* A static C program whose only thread function is pthread_once
 * (crates/core/tests/cxx_throw_products.rs, STATIC-CXX-THROW-PTHREAD-ONCE): it links against
 * libc.a alone and its routine runs exactly once. Prints "runs=<n> rc=<a>,<b>". */
#include <pthread.h>
#include <stdio.h>

static int runs;
static void init(void) { runs++; }

int main(void)
{
    static pthread_once_t once = PTHREAD_ONCE_INIT;
    int a = pthread_once(&once, init);
    int b = pthread_once(&once, init);
    printf("runs=%d rc=%d,%d\n", runs, a, b);
    return runs == 1 && a == 0 && b == 0 ? 0 : 1;
}
