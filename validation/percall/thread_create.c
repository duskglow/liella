/* DL-TLS-THREADS (thread_create_join), DL-TLS-THREADS-COST: pthread_create + pthread_join in a static program (the path the loader TLS check sits on). */
#include "percall.h"
#include <pthread.h>

static void *nothing(void *a) { return a; }

CASE(thread_create_join, 1000, 1) {
    (void)dir;
    for (long i = 0; i < n; i++) {
        pthread_t t;
        if (pthread_create(&t, 0, nothing, 0) != 0 || pthread_join(t, 0) != 0) abort();
    }
}
