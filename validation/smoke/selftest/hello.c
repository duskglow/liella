/* validation/smoke/selftest/hello.c: the smoke harness's own check program (smoke.sh selftest).
   Prints "hello <sqrt(argv[1])>" computed by libm, from a second thread, so the link brings in libm and
   the thread functions through liella-cc's -lm / -lpthread handling. */
#include <math.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>

static void *worker(void *arg) {
    double x = strtod((const char *)arg, NULL);
    printf("hello %f\n", sqrt(x));
    return NULL;
}

int main(int argc, char **argv) {
    pthread_t t;
    if (argc < 2 || pthread_create(&t, NULL, worker, argv[1]) != 0)
        return 2;
    return pthread_join(t, NULL) != 0;
}
