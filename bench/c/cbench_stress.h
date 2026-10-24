/* The `threads` stress profile of `cargo xtask bench --stress` (STRESS-ENV-AUDIT-2). With
 * CBENCH_STRESS_THREADS=<n> in the environment, a constructor starts <n> idle threads before main, the way a
 * program with a thread pool runs: each allocates once (so an allocator that keeps per-thread state has it)
 * and then sleeps for good with every signal blocked, so process-directed signals still reach the main
 * thread. The rows then run in a multi-threaded process instead of a single-threaded one. Unset (the fixed
 * environment): one getenv at start-up and nothing else. Included by cbench_rows.h, so every C benchmark
 * that filters rows honours it on the Liella and the glibc side alike. */
#ifndef CBENCH_STRESS_H
#define CBENCH_STRESS_H
#include <pthread.h>
#include <signal.h>
#include <stdlib.h>
#include <unistd.h>

static void *cb_stress_idle(void *arg)
{
    (void)arg;
    free(malloc(64));
    for (;;)
        pause();
    return NULL;
}

__attribute__((constructor)) static void cb_stress_threads(void)
{
    const char *v = getenv("CBENCH_STRESS_THREADS");
    int n = v ? atoi(v) : 0;
    if (n <= 0)
        return;
    sigset_t all, old;
    sigfillset(&all);
    pthread_sigmask(SIG_SETMASK, &all, &old);
    for (int k = 0; k < n; k++) {
        pthread_t t;
        if (pthread_create(&t, NULL, cb_stress_idle, NULL) != 0 || pthread_detach(t) != 0) {
            (void)!write(2, "cbench: CBENCH_STRESS_THREADS: pthread_create failed", 52);
            _exit(95);
        }
    }
    pthread_sigmask(SIG_SETMASK, &old, NULL);
}
#endif
