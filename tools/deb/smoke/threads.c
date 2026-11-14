/* Smoke program for the reference packages: four threads add to a shared counter under a mutex,
 * each also keeping a thread-local count; the totals must match. */
#include <pthread.h>
#include <stdio.h>

#define THREADS 4
#define ROUNDS 10000

static pthread_mutex_t lock = PTHREAD_MUTEX_INITIALIZER;
static long total;
static _Thread_local long mine;

static void *work(void *arg) {
    (void)arg;
    for (int i = 0; i < ROUNDS; i++) {
        pthread_mutex_lock(&lock);
        total++;
        pthread_mutex_unlock(&lock);
        mine++;
    }
    return (void *)mine;
}

int main(void) {
    pthread_t t[THREADS];
    long sum = 0;
    for (int i = 0; i < THREADS; i++) {
        if (pthread_create(&t[i], NULL, work, NULL) != 0) {
            puts("threads: pthread_create failed");
            return 1;
        }
    }
    for (int i = 0; i < THREADS; i++) {
        void *r;
        pthread_join(t[i], &r);
        sum += (long)r;
    }
    printf("threads: total %ld, thread-local sum %ld\n", total, sum);
    return total == THREADS * ROUNDS && sum == total ? 0 : 1;
}
