/* Spec §22.4: mutexes in a process that has one thread, then across its first thread creation
 * (MUTEX-2: the only thread takes and releases plain mutexes without atomic instructions until
 * the first pthread_create). Every type keeps its rules while single-threaded; a mutex taken
 * single-threaded and still held when the first thread starts excludes it and wakes it at the
 * unlock; contention after that loses no update; a fork child of a single-threaded process
 * keeps the rules. */
#define _GNU_SOURCE
#include <errno.h>
#include <pthread.h>
#include <sys/wait.h>
#include <unistd.h>

static pthread_mutex_t held = PTHREAD_MUTEX_INITIALIZER;
static volatile int got_held;
static void *take_held(void *arg)
{
    (void)arg;
    if (pthread_mutex_lock(&held) != 0)
        return (void *)1;
    got_held = 1;
    pthread_mutex_unlock(&held);
    return 0;
}

static pthread_mutex_t counter_lock = PTHREAD_MUTEX_INITIALIZER;
static long counter;
#define ROUNDS 200000
static void *count_up(void *arg)
{
    (void)arg;
    for (int i = 0; i < ROUNDS; i++) {
        pthread_mutex_lock(&counter_lock);
        /* A read, a pause and a write: a lost exclusion loses updates. */
        long v = counter;
        __asm__ volatile("" ::: "memory");
        counter = v + 1;
        pthread_mutex_unlock(&counter_lock);
    }
    return 0;
}

static int rules(void)
{
    pthread_mutex_t n = PTHREAD_MUTEX_INITIALIZER;
    if (pthread_mutex_lock(&n) != 0 || pthread_mutex_trylock(&n) != EBUSY || pthread_mutex_unlock(&n) != 0)
        return 1;
    if (pthread_mutex_trylock(&n) != 0 || pthread_mutex_unlock(&n) != 0 || pthread_mutex_destroy(&n) != 0)
        return 2;
    pthread_mutex_t r = PTHREAD_RECURSIVE_MUTEX_INITIALIZER_NP;
    for (int i = 0; i < 3; i++)
        if (pthread_mutex_lock(&r) != 0)
            return 3;
    if (pthread_mutex_trylock(&r) != 0 || pthread_mutex_destroy(&r) != EBUSY)
        return 4;
    for (int i = 0; i < 4; i++)
        if (pthread_mutex_unlock(&r) != 0)
            return 5;
    if (pthread_mutex_unlock(&r) != EPERM || pthread_mutex_destroy(&r) != 0)
        return 6;
    pthread_mutex_t e = PTHREAD_ERRORCHECK_MUTEX_INITIALIZER_NP;
    if (pthread_mutex_unlock(&e) != EPERM || pthread_mutex_lock(&e) != 0 || pthread_mutex_lock(&e) != EDEADLK)
        return 7;
    if (pthread_mutex_trylock(&e) != EBUSY || pthread_mutex_unlock(&e) != 0 || pthread_mutex_unlock(&e) != EPERM)
        return 8;
    pthread_mutex_t a = PTHREAD_ADAPTIVE_MUTEX_INITIALIZER_NP;
    if (pthread_mutex_lock(&a) != 0 || pthread_mutex_trylock(&a) != EBUSY || pthread_mutex_unlock(&a) != 0 || pthread_mutex_destroy(&a) != 0)
        return 9;
    return 0;
}

int main(void)
{
    alarm(120);
    int rc = rules();
    if (rc != 0)
        return rc;

    /* A fork child of the single-threaded process: same rules; a mutex the parent held at the
     * fork is held in the child by the same (only) thread. */
    if (pthread_mutex_lock(&held) != 0)
        return 20;
    pid_t pid = fork();
    if (pid == 0)
        _exit(rules() != 0 || pthread_mutex_trylock(&held) != EBUSY || pthread_mutex_unlock(&held) != 0 ? 1 : 0);
    int st;
    if (pid < 0 || waitpid(pid, &st, 0) != pid || !WIFEXITED(st) || WEXITSTATUS(st) != 0)
        return 21;

    /* Still held from the single-threaded phase: the first thread must wait for it, and the
     * unlock (now multi-threaded) must wake it. */
    pthread_t t;
    if (pthread_create(&t, NULL, take_held, NULL) != 0)
        return 22;
    usleep(100000);
    if (got_held)
        return 23;
    if (pthread_mutex_unlock(&held) != 0)
        return 24;
    void *res;
    if (pthread_join(t, &res) != 0 || res != 0 || !got_held)
        return 25;

    /* Contention after the first creation. */
    pthread_t w[4];
    for (int i = 0; i < 4; i++)
        if (pthread_create(&w[i], NULL, count_up, NULL) != 0)
            return 26;
    count_up(NULL);
    for (int i = 0; i < 4; i++)
        pthread_join(w[i], NULL);
    if (counter != 5L * ROUNDS)
        return 27;
    return rules() != 0 ? 28 : 0;
}
